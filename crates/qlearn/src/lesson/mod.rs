//! The parser of a page.
//!
//! The agent writes a page in Markdown. `pulldown-cmark` reads it, and this
//! module splits it into blocks, checks each one, and writes the HTML of the
//! text. See section 4 of the design.
//!
//! Every tag of the output is written here, from a short list. HTML in the
//! source comes out as text, so a page never holds a script or a style that
//! the agent typed.

pub mod chart;
pub mod excerpt;
pub mod image;
pub mod steps;

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use sha2::{Digest, Sha256};

use crate::git::exec::Git;
use crate::highlight::Highlighter;
use crate::model::{Block, BlockSource, Line, Marker, Span};
pub use image::Notes;

/// A page that passed every check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    /// The hidden line of an excerpt is None here.
    pub blocks: Vec<Block>,
    /// The text of the first heading. It titles a page that the syllabus
    /// does not hold.
    pub title: Option<String>,
    /// The hidden line of each excerpt that has one, by the id of its block.
    /// It stays on the server until the page may show it.
    pub hidden: BTreeMap<String, Line>,
    /// The bytes of each image, by its name in `media/`. The store writes
    /// them with the page, never before the page passes every check.
    pub media: BTreeMap<String, Vec<u8>>,
}

/// Where a source shows: the lesson of a page, the debrief of its quiz
/// `n`, from 1, or the cheatsheet of the course.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Within {
    Lesson,
    Debrief(usize),
    Cheatsheet,
}

/// One error of a source, at its line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault {
    /// The line in the source, from 1.
    pub line: usize,
    /// The kind of the block at fault, such as `excerpt`.
    pub kind: &'static str,
    pub message: String,
}

impl Fault {
    /// The line of section 4.7 of the design: `<file>:<line>: <kind>: <message>`.
    pub fn display(&self, file: &str) -> String {
        format!("{file}:{}: {}: {}", self.line, self.kind, self.message)
    }
}

/// The fences that open a block of their own, by info string, with the
/// kind of that block. Any other fence is code in a text block.
const FENCES: [(&str, &str); 4] = [
    ("excerpt", "excerpt"),
    ("mermaid", "diagram"),
    ("chart", "chart"),
    ("steps", "steps"),
];

/// The most lines that a diagram holds. Mermaid draws a larger one too
/// small to read, and the server cannot run Mermaid to check it.
pub const DIAGRAM_LINES: usize = 200;

/// One event of the parser, with the bytes of the source that it covers.
type Item<'a> = (Event<'a>, Range<usize>);

/// Split a source into blocks, read its excerpts in the repository, or say
/// every error that it holds.
///
/// Nothing is stored before this returns `Ok`, so a page with one error
/// leaves the course as it was. `git` is None outside a repository, where
/// each excerpt and each image of the repository is an error. `notes` are
/// the notes of the course, where an image may come from too.
pub async fn parse(
    source: &str,
    git: Option<&Git>,
    notes: &Notes,
    highlighter: &Highlighter,
) -> Result<Parsed, Vec<Fault>> {
    parse_with(source, Within::Lesson, git, notes, highlighter).await
}

/// Parse the source of a debrief, as a page (section 5.4 of the design).
///
/// A debrief hides no line. It shows once its quiz is answered, so the
/// learner would never answer for the line. A hidden line of the lesson in
/// it waits for the page, in `PageFile::page`. `n` is the number of the
/// quiz, which the ids of the blocks take.
pub async fn parse_debrief(
    source: &str,
    n: usize,
    git: Option<&Git>,
    notes: &Notes,
    highlighter: &Highlighter,
) -> Result<Parsed, Vec<Fault>> {
    parse_with(source, Within::Debrief(n), git, notes, highlighter).await
}

/// Parse the source of the cheatsheet, as a page (section 9 of the design).
///
/// A cheatsheet hides no line: the learner keeps it to read, and it has no
/// quiz to answer for the line.
pub async fn parse_cheatsheet(
    source: &str,
    git: Option<&Git>,
    notes: &Notes,
    highlighter: &Highlighter,
) -> Result<Parsed, Vec<Fault>> {
    parse_with(source, Within::Cheatsheet, git, notes, highlighter).await
}

async fn parse_with(
    source: &str,
    within: Within,
    git: Option<&Git>,
    notes: &Notes,
    highlighter: &Highlighter,
) -> Result<Parsed, Vec<Fault>> {
    let place = Place {
        slug: &notes.slug,
        outside: git.is_none(),
    };
    let Split {
        pieces,
        title,
        mut faults,
        ..
    } = split(source, highlighter, within, place);

    // Every excerpt is read, even on a page already refused, so the agent
    // gets every error in one pass.
    let mut revs = excerpt::Revs::new();
    let mut blocks = Vec::new();
    let mut hidden = BTreeMap::new();
    let mut media = BTreeMap::new();
    for piece in pieces {
        match piece {
            Piece::Block(block) => blocks.push(block),
            Piece::Image(id, spec) => {
                let read = match (spec.in_store(), git) {
                    (true, _) => image::read_notes(id, &spec, notes),
                    (false, Some(git)) => image::read(id, &spec, git, &mut revs).await,
                    // Outside a repository, the split refused it already.
                    (false, None) => continue,
                };
                match read {
                    Ok((block, bytes)) => {
                        if let Block::Image { media: name, .. } = &block {
                            media.insert(name.clone(), bytes);
                        }
                        blocks.push(block);
                    }
                    Err(fault) => faults.push(fault),
                }
            }
            Piece::Excerpt(id, spec) => {
                // Outside a repository, the split refused every excerpt.
                let Some(git) = git else {
                    continue;
                };
                let refusal = match within {
                    Within::Lesson => None,
                    Within::Debrief(_) => {
                        Some("a debrief hides no line: its quiz is answered already. Take hide out")
                    }
                    Within::Cheatsheet => Some(
                        "a cheatsheet hides no line: the learner keeps it to read, and answers \
                         no quiz on it. Take hide out",
                    ),
                };
                if let (Some(line), Some(message)) = (spec.hides_at(), refusal) {
                    faults.push(Fault {
                        line,
                        kind: "excerpt",
                        message: message.to_owned(),
                    });
                }
                match excerpt::read(&spec, git, highlighter, &mut revs).await {
                    Ok((excerpt, line)) => {
                        if let Some(line) = line {
                            hidden.insert(id.clone(), line);
                        }
                        blocks.push(Block::Excerpt { id, excerpt });
                    }
                    Err(fault) => faults.push(fault),
                }
            }
        }
    }

    done(blocks, title, hidden, media, faults)
}

/// The page, or every fault in the order of the lines.
fn done(
    blocks: Vec<Block>,
    title: Option<String>,
    hidden: BTreeMap<String, Line>,
    media: BTreeMap<String, Vec<u8>>,
    mut faults: Vec<Fault>,
) -> Result<Parsed, Vec<Fault>> {
    match faults.is_empty() {
        true => Ok(Parsed {
            blocks,
            title,
            hidden,
            media,
        }),
        false => {
            // Stable: the errors of one line keep their order.
            faults.sort_by_key(|fault| fault.line);
            Err(faults)
        }
    }
}

/// The course of a source, which the error of an image names, and whether
/// a repository holds it.
#[derive(Clone, Copy)]
struct Place<'a> {
    slug: &'a str,
    outside: bool,
}

impl Place<'_> {
    /// For a split whose faults are dropped: no error names it.
    const ANY: Place<'static> = Place {
        slug: "",
        outside: false,
    };
}

/// A block as the split finds it. An excerpt and an image wait for their
/// read.
enum Piece {
    Block(Block),
    Excerpt(String, excerpt::Spec),
    Image(String, image::Spec),
}

/// The blocks of a source as the agent wrote them, in order: a thread
/// hands the agent the source of its block. A block that fails a check has
/// no id, so it is left out.
pub fn sources(source: &str, within: Within) -> Vec<BlockSource> {
    split(source, Highlighter::shared(), within, Place::ANY).sources
}

/// A source cut into blocks that each read alone, for an export.
pub struct Cut {
    /// The Markdown of each block by id, from the start of its first line.
    pub blocks: HashMap<String, String>,
    /// The link reference definitions that lie outside every block. They
    /// give no event, so no block holds them, yet a block may name them.
    pub definitions: Vec<String>,
}

/// The blocks of a source as Markdown that reads alone, by id.
pub fn cut(source: &str, within: Within) -> Cut {
    let split = split(source, Highlighter::shared(), within, Place::ANY);
    let blocks = split
        .sources
        .into_iter()
        .zip(&split.spans)
        .map(|(block, span)| (block.id, source[span.clone()].to_owned()))
        .collect();

    let parser = Parser::new_ext(source, OPTIONS);
    let mut outside: Vec<Range<usize>> = parser
        .reference_definitions()
        .iter()
        .map(|(_, definition)| definition.span.clone())
        .filter(|at| {
            !split
                .spans
                .iter()
                .any(|span| span.start <= at.start && at.end <= span.end)
        })
        .collect();
    // The definitions sit in a map, with no order.
    outside.sort_by_key(|at| at.start);
    let definitions = outside
        .into_iter()
        .map(|at| source[at].trim().to_owned())
        .collect();

    Cut {
        blocks,
        definitions,
    }
}

/// The HTML of the Markdown of a comment. The HTML of the source is
/// escaped and a fence is colored, as in a text block.
pub fn markdown(source: &str) -> String {
    let items: Vec<Item> = Parser::new_ext(source, OPTIONS)
        .into_offset_iter()
        .collect();
    Writer::new(Highlighter::shared()).node(&items)
}

/// What `split` finds in a source.
struct Split {
    pieces: Vec<Piece>,
    title: Option<String>,
    /// The errors that need no git call.
    faults: Vec<Fault>,
    sources: Vec<BlockSource>,
    spans: Vec<Range<usize>>,
}

/// The pieces of a page, its title, and the errors that need no git call.
/// Outside a repository, each excerpt and each image of the repository is
/// an error of its own, with no check of its keys, since the fix is to take
/// it out. The error of an image names the notes of the course of `place`,
/// where an image works.
fn split(source: &str, highlighter: &Highlighter, within: Within, place: Place) -> Split {
    let items: Vec<Item> = Parser::new_ext(source, OPTIONS)
        .into_offset_iter()
        .collect();
    let starts = line_starts(source);
    let line_of = |byte: usize| starts.partition_point(|start| *start <= byte);

    let mut page = Page {
        source,
        within,
        blocks: Vec::new(),
        open: None,
        seen: HashMap::new(),
        sources: Vec::new(),
        spans: Vec::new(),
    };
    let mut title = None;
    let mut faults = Vec::new();

    let mut at = 0;
    while at < items.len() {
        let end = matching_end(&items, at);
        let node = &items[at..=end];
        let range = node[0].1.clone();

        // A fence of a block kind inside a list or a quote would show code
        // that the server never checked. A callout there would show its
        // marker as text.
        for (offset, (event, range)) in node.iter().enumerate().skip(1) {
            if let Some(kind) = fence_kind(event) {
                faults.push(nested(line_of(range.start), kind));
            }
            // The marker is in the first events of a quote: a scan to the
            // end of each nested quote would make deep nesting quadratic.
            if let Event::Start(Tag::BlockQuote(_)) = event
                && let Some(opening) = opening(source, &node[offset..])
            {
                faults.push(nested_callout(line_of(range.start), &opening.word));
            }
            // An image alone in a list item, a quote or a table cell would
            // read as a block that the server never read. An image in a
            // line of text stays text.
            if let Event::Start(Tag::Paragraph | Tag::Item | Tag::TableCell) = event
                && holds_one_image(node, offset)
            {
                faults.push(nested_image(line_of(range.start)));
            }
        }

        match role(source, node) {
            Role::Heading => {
                // A heading starts a block, so a thread can name a section.
                page.close();
                if title.is_none() {
                    title = Some(plain_text(node));
                }
                page.text(node, highlighter);
            }
            Role::Text => page.text(node, highlighter),
            Role::Callout(opening) => {
                // A callout is a block of its own, as a heading starts one.
                page.close();
                let line = line_of(range.start);
                match Marker::of_word(&opening.word) {
                    None => faults.push(unknown_marker(line, &opening.word)),
                    Some(marker) => {
                        let body = callout_body(node, &opening);
                        // The marker shares its paragraph with the next
                        // line, so the scan above cannot see an image
                        // alone after it.
                        if body.first().is_some_and(|(_, at)| *at == node[1].1)
                            && holds_one_image(&body, 0)
                        {
                            faults.push(nested_image(line));
                        }
                        match body.is_empty() {
                            true => faults.push(empty_callout(line)),
                            false => {
                                let html = Writer::new(highlighter).node(&body);
                                let id = page.id("callout", range.clone());
                                page.blocks
                                    .push(Piece::Block(Block::Callout { id, marker, html }));
                            }
                        }
                    }
                }
            }
            Role::Excerpt if place.outside => {
                page.close();
                faults.push(excerpt::outside(line_of(range.start)));
            }
            Role::Excerpt => {
                page.close();
                let fence = line_of(range.start);
                let body = fence_body(node);
                match excerpt::spec(&body, fence) {
                    Ok(spec) => {
                        let id = page.id("excerpt", range.clone());
                        page.blocks.push(Piece::Excerpt(id, spec));
                    }
                    Err(found) => faults.extend(found),
                }
            }
            Role::Diagram => {
                page.close();
                let line = line_of(range.start);
                let body = fence_body(node);
                match check_diagram(&body, line) {
                    Ok(()) => {
                        let id = page.id("diagram", range.clone());
                        page.blocks
                            .push(Piece::Block(Block::Diagram { id, source: body }));
                    }
                    Err(fault) => faults.push(fault),
                }
            }
            Role::Chart => {
                page.close();
                let line = line_of(range.start);
                let body = fence_body(node);
                match chart::parse(&body, line) {
                    Ok(chart) => {
                        let id = page.id("chart", range.clone());
                        page.blocks.push(Piece::Block(Block::Chart { id, chart }));
                    }
                    Err(found) => faults.extend(found),
                }
            }
            Role::Steps => {
                page.close();
                let line = line_of(range.start);
                let body = fence_body(node);
                match steps::parse(&body, line) {
                    Ok(steps) => {
                        let id = page.id("steps", range.clone());
                        page.blocks.push(Piece::Block(Block::Steps { id, steps }));
                    }
                    Err(found) => faults.extend(found),
                }
            }
            Role::Image => {
                page.close();
                let line = line_of(range.start);
                let Event::Start(Tag::Image { dest_url, .. }) = &node[1].0 else {
                    unreachable!("the role checked the image");
                };
                if place.outside && !image::names_the_store(dest_url) {
                    faults.push(image::outside(line, place.slug));
                } else {
                    let alt = plain_text(&node[2..node.len() - 2]);
                    match image::spec(&alt, dest_url, line, place.slug) {
                        Ok(spec) => {
                            let id = page.id("image", range.clone());
                            page.blocks.push(Piece::Image(id, spec));
                        }
                        Err(found) => faults.extend(found),
                    }
                }
            }
            Role::Refused(kind) => {
                page.close();
                faults.push(refused(line_of(range.start), kind));
            }
        }
        at = end + 1;
    }
    page.close();

    Split {
        pieces: page.blocks,
        title,
        faults,
        sources: page.sources,
        spans: page.spans,
    }
}

/// The Markdown that a page takes.
const OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_TASKLISTS);

/// The HTML of inline Markdown, such as an option of a choice, with no
/// paragraph around it. The HTML of the source is escaped, as in a block.
///
/// The error names what the text holds in place of one line: a list, a
/// heading or a line break would break the line of an option.
pub fn inline(source: &str) -> Result<String, String> {
    let items: Vec<Item> = Parser::new_ext(source, OPTIONS)
        .into_offset_iter()
        .collect();
    let Some((first, _)) = items.first() else {
        return Err("is empty".to_owned());
    };
    let end = matching_end(&items, 0);
    let alone = end == items.len() - 1;
    let html = matches!(first, Event::Start(Tag::HtmlBlock)) && alone;
    if !html && !matches!(first, Event::Start(Tag::Paragraph)) {
        return Err(format!("holds {}", block_name(first)));
    }
    if !alone {
        return Err("holds more than one paragraph".to_owned());
    }
    // A paragraph may run over several lines, and a hard break shows one.
    if source.trim().contains(['\n', '\r']) {
        return Err("holds more than one line".to_owned());
    }
    // A line that opens with a tag is a block of HTML. Its tags come out
    // as text anyway, so it is the text that the agent wrote.
    if html {
        return Ok(escape(source.trim()));
    }

    Ok(Writer::new(Highlighter::shared()).node(&items[1..end]))
}

/// A block of Markdown as an error names it.
fn block_name(event: &Event) -> &'static str {
    match event {
        Event::Start(Tag::Heading { .. }) => "a heading",
        Event::Start(Tag::List(_)) => "a list",
        Event::Start(Tag::BlockQuote(_)) => "a quote",
        Event::Start(Tag::CodeBlock(_)) => "a fence of code",
        Event::Start(Tag::Table(_)) => "a table",
        Event::Start(Tag::HtmlBlock) => "a block of HTML",
        Event::Rule => "a rule",
        _ => "a block",
    }
}

/// The id of a block: the first 12 hexadecimal characters of the SHA-256
/// of its kind and its source. An unchanged block keeps its id when the
/// agent pushes the page again, so its threads stay on it. A later copy of
/// the same block hashes its rank too, so two copies never share threads.
/// A block of a debrief hashes the number of its quiz too: the page places
/// a thread by the id of its block, and a debrief may repeat the lesson.
/// A block of the cheatsheet hashes its place too, as it may repeat a
/// lesson.
pub fn block_id(kind: &str, source: &str, rank: usize, within: Within) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update(b"\n");
    hasher.update(source.as_bytes());
    if rank > 1 {
        hasher.update(format!("\n{rank}").as_bytes());
    }
    match within {
        Within::Lesson => {}
        Within::Debrief(n) => hasher.update(format!("\ndebrief {n}").as_bytes()),
        Within::Cheatsheet => hasher.update(b"\ncheatsheet"),
    }

    hex::encode(&hasher.finalize()[..6])
}

fn refused(line: usize, kind: &'static str) -> Fault {
    Fault {
        line,
        kind,
        message: format!("this release of qlearn does not support {kind} blocks yet"),
    }
}

/// A fence of a block kind inside a list or a quote.
fn nested(line: usize, kind: &'static str) -> Fault {
    let block = match kind {
        "excerpt" => "an excerpt",
        "diagram" => "a diagram",
        "chart" => "a chart",
        "steps" => "a steps block",
        _ => return refused(line, kind),
    };
    Fault {
        line,
        kind,
        message: format!(
            "{block} cannot sit inside a list or a quote: put it at the top level of the page"
        ),
    }
}

/// The checks of a diagram that need no Mermaid: the page draws it, and
/// reports a source that Mermaid refuses.
fn check_diagram(body: &str, line: usize) -> Result<(), Fault> {
    let fault = |message: String| Fault {
        line,
        kind: "diagram",
        message,
    };
    if body.trim().is_empty() {
        return Err(fault(
            "the fence holds no Mermaid source: write the diagram inside it".to_owned(),
        ));
    }
    let lines = body.lines().count();
    if lines > DIAGRAM_LINES {
        return Err(fault(format!(
            "the diagram holds {lines} lines, and a diagram has {DIAGRAM_LINES} at most: \
             split it in two"
        )));
    }
    Ok(())
}

/// The JSON of a fence, such as a chart. `fence` is the line of its
/// opening, and `empty` the fault of a fence with nothing inside.
fn fence_json(
    body: &str,
    fence: usize,
    kind: &'static str,
    empty: &str,
) -> Result<serde_json::Value, Vec<Fault>> {
    let fault = |line: usize, message: String| Fault {
        line,
        kind,
        message,
    };
    if body.trim().is_empty() {
        return Err(vec![fault(fence, empty.to_owned())]);
    }
    serde_json::from_str(body).map_err(|error| {
        // The line of the error in the page, not in the fence.
        let message = error.to_string();
        let message = message
            .split_once(" at line ")
            .map_or(message.as_str(), |(head, _)| head);
        vec![fault(
            fence + error.line(),
            format!("not valid JSON: {message}, at column {}", error.column()),
        )]
    })
}

/// An image alone inside a list, a quote or a table.
fn nested_image(line: usize) -> Fault {
    Fault {
        line,
        kind: "image",
        message: "an image cannot sit inside a list, a quote or a table: put it alone in a \
                  paragraph at the top level of the page"
            .to_owned(),
    }
}

/// A quote in the form of a callout inside a list or a quote.
fn nested_callout(line: usize, word: &str) -> Fault {
    match Marker::of_word(word) {
        Some(_) => Fault {
            line,
            kind: "callout",
            message: "a callout cannot sit inside a list or a quote: put it at the top \
                      level of the page"
                .to_owned(),
        },
        None => unknown_marker(line, word),
    }
}

/// A marker that is no callout. Refused, so that a typo does not turn the
/// callout into a plain quote that shows its marker as text.
fn unknown_marker(line: usize, word: &str) -> Fault {
    Fault {
        line,
        kind: "callout",
        message: format!(
            "[!{word}] is not a callout: write [!hook], [!teaser], [!pitfall] or [!note]"
        ),
    }
}

fn empty_callout(line: usize) -> Fault {
    Fault {
        line,
        kind: "callout",
        message: "the quote holds only its marker: write the text of the callout under it"
            .to_owned(),
    }
}

/// The text inside a fence, without its marks.
fn fence_body(node: &[Item]) -> String {
    node.iter()
        .filter_map(|(event, _)| match event {
            Event::Text(text) => Some(text.as_ref()),
            _ => None,
        })
        .collect()
}

/// The kind of the block that a fence opens, or None for a fence of code.
fn fence_kind(event: &Event) -> Option<&'static str> {
    let Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) = event else {
        return None;
    };
    let word = info.split_whitespace().next().unwrap_or("");

    FENCES
        .iter()
        .find(|(fence, _)| *fence == word)
        .map(|(_, kind)| *kind)
}

/// What a node at the top level of the source is to the page.
enum Role {
    Heading,
    Text,
    Callout(Opening),
    Excerpt,
    Diagram,
    Chart,
    Steps,
    Image,
    /// A block of a kind that this release does not draw yet.
    Refused(&'static str),
}

fn role(source: &str, node: &[Item]) -> Role {
    match &node[0].0 {
        Event::Start(Tag::Heading { .. }) => Role::Heading,
        event @ Event::Start(Tag::CodeBlock(_)) => match fence_kind(event) {
            Some("excerpt") => Role::Excerpt,
            Some("diagram") => Role::Diagram,
            Some("chart") => Role::Chart,
            Some("steps") => Role::Steps,
            Some(kind) => Role::Refused(kind),
            None => Role::Text,
        },
        Event::Start(Tag::Paragraph) if holds_one_image(node, 0) => Role::Image,
        Event::Start(Tag::BlockQuote(_)) => match opening(source, node) {
            Some(opening) => Role::Callout(opening),
            None => Role::Text,
        },
        _ => Role::Text,
    }
}

/// Whether the element that opens at `at` holds one image and nothing
/// else. Only the image is scanned, so a check of each item of a deep list
/// stays linear.
fn holds_one_image(items: &[Item], at: usize) -> bool {
    if !matches!(
        items.get(at + 1),
        Some((Event::Start(Tag::Image { .. }), _))
    ) {
        return false;
    }
    // Right after the image, an end closes the element that holds it.
    let end = matching_end(items, at + 1);
    matches!(items.get(end + 1), Some((Event::End(_), _)))
}

/// The `[!word]` that opens a quote.
///
/// The GFM alerts of `pulldown-cmark` know only their own five kinds, so the
/// marker is read here, from the events of the first paragraph.
struct Opening {
    word: String,
    /// How many text events the marker takes, after `Start(Paragraph)`.
    events: usize,
    /// The text after `]` in the last of those events.
    rest: String,
}

/// The marker of a quote, when its first line opens with `[!word]`. Any
/// word counts, so that the caller can refuse one that is no callout. It
/// reads only the first events, so `quote` may run past the quote's end.
fn opening(source: &str, quote: &[Item]) -> Option<Opening> {
    let (Some((Event::Start(Tag::Paragraph), _)), Some((_, first))) = (quote.get(1), quote.get(2))
    else {
        return None;
    };
    // A quote whose first line is blank is a plain quote.
    if source[quote[0].1.start..first.start].contains('\n') {
        return None;
    }

    // The parser cuts `[!hook]` into `[`, `!hook` and `]`.
    let mut text = String::new();
    let mut events = 0;
    for (event, _) in &quote[2..] {
        let Event::Text(part) = event else {
            break;
        };
        text.push_str(part);
        events += 1;
        if part.contains(']') {
            break;
        }
    }
    let (word, rest) = text.strip_prefix("[!")?.split_once(']')?;
    let is_word = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    // The events hold decoded text: an escape or an entity in the source
    // asks for the brackets as text, as on GitHub.
    let paragraph = quote[1].1.start;
    let is_literal = source[paragraph..]
        .trim_start()
        .starts_with(&format!("[!{word}]"));

    (is_word && is_literal).then(|| Opening {
        word: word.to_owned(),
        events,
        rest: rest.to_owned(),
    })
}

/// The events inside a callout, without its marker. Text after the marker
/// on its line stays: it opens the body.
fn callout_body<'a>(quote: &[Item<'a>], opening: &Opening) -> Vec<Item<'a>> {
    let inner = &quote[1..quote.len() - 1];
    let mut tail = &inner[1 + opening.events..];
    let mut body = vec![inner[0].clone()];

    // The text after `]` often comes in an event of its own.
    let mut rest = (opening.rest.clone(), inner[opening.events].1.clone());
    if rest.0.is_empty()
        && let Some((Event::Text(text), range)) = tail.first()
    {
        rest = (text.to_string(), range.clone());
        tail = &tail[1..];
    }
    let (rest, range) = rest;
    let rest = rest.trim_start();
    if !rest.is_empty() {
        body.push((Event::Text(rest.to_owned().into()), range));
    } else if matches!(tail.first(), Some((Event::SoftBreak | Event::HardBreak, _))) {
        tail = &tail[1..];
    }
    body.extend(tail.iter().cloned());

    // A marker alone in its paragraph leaves the paragraph empty.
    if matches!(body.get(1), Some((Event::End(TagEnd::Paragraph), _))) {
        body.drain(..2);
    }
    body
}

/// The index of the event that closes the one at `at`. An event that opens
/// nothing closes itself.
fn matching_end(items: &[Item], at: usize) -> usize {
    let mut depth = 0usize;
    for (index, (event, _)) in items.iter().enumerate().skip(at) {
        match event {
            Event::Start(_) => depth += 1,
            Event::End(_) => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0 {
            return index;
        }
    }
    items.len() - 1
}

/// The text of a heading, with no markup.
fn plain_text(node: &[Item]) -> String {
    let mut text = String::new();
    for (event, _) in node {
        match event {
            Event::Text(part) | Event::Code(part) => text.push_str(part),
            Event::SoftBreak | Event::HardBreak => text.push(' '),
            _ => {}
        }
    }
    text.trim().to_owned()
}

/// The blocks of a page, as the parse finds them.
struct Page<'s> {
    source: &'s str,
    within: Within,
    blocks: Vec<Piece>,
    /// The text block being written: the bytes that it covers, and its HTML.
    open: Option<(Range<usize>, String)>,
    /// How many times each kind and source came, for the rank of a copy.
    seen: HashMap<(&'static str, &'s str), usize>,
    /// Each block with an id, as the agent wrote it.
    sources: Vec<BlockSource>,
    /// The bytes of each of `sources`, from the start of its first line.
    spans: Vec<Range<usize>>,
}

impl<'s> Page<'s> {
    /// The id of the block that `range` of the source holds.
    fn id(&mut self, kind: &'static str, range: Range<usize>) -> String {
        let whole: &'s str = self.source;
        // The blank lines after a block are not part of it: a page pushed
        // again with one more section keeps the id of this one.
        let source = whole[range.clone()].trim_end();
        // The parser starts a block after its indent, which an indented
        // code block needs.
        let line = whole[..range.start].rfind('\n').map_or(0, |at| at + 1);
        let start = match whole[line..range.start].trim_start_matches([' ', '\t']) {
            "" => line,
            _ => range.start,
        };
        self.spans.push(start..range.start + source.len());
        let rank = self.seen.entry((kind, source)).or_default();
        *rank += 1;
        let id = block_id(kind, source, *rank, self.within);
        self.sources.push(BlockSource {
            id: id.clone(),
            kind: kind.to_owned(),
            source: source.to_owned(),
        });
        id
    }

    /// Add a node to the open text block, or open one.
    fn text(&mut self, node: &[Item], highlighter: &Highlighter) {
        let range = node[0].1.clone();
        let html = Writer::new(highlighter).node(node);

        match self.open.as_mut() {
            Some((covered, out)) => {
                covered.end = covered.end.max(range.end);
                out.push('\n');
                out.push_str(&html);
            }
            None => self.open = Some((range, html)),
        }
    }

    fn close(&mut self) {
        let Some((covered, html)) = self.open.take() else {
            return;
        };
        let id = self.id("text", covered);
        self.blocks.push(Piece::Block(Block::Text { id, html }));
    }
}

/// The HTML of one node of the source.
struct Writer<'h> {
    out: String,
    /// For each open link, whether it was written as an `<a>`.
    links: Vec<bool>,
    aligns: Vec<Alignment>,
    cell: usize,
    head: bool,
    /// A fence or an indented block being read: its language and its code.
    code: Option<(String, String)>,
    highlighter: &'h Highlighter,
}

impl<'h> Writer<'h> {
    fn new(highlighter: &'h Highlighter) -> Self {
        Self {
            out: String::new(),
            links: Vec::new(),
            aligns: Vec::new(),
            cell: 0,
            head: false,
            code: None,
            highlighter,
        }
    }

    fn node(mut self, node: &[Item]) -> String {
        for (event, _) in node {
            self.event(event);
        }
        self.out
    }

    fn event(&mut self, event: &Event) {
        if let Some((_, code)) = self.code.as_mut() {
            match event {
                Event::Text(text) => code.push_str(text),
                Event::End(TagEnd::CodeBlock) => self.code_block(),
                _ => {}
            }
            return;
        }

        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(*tag),
            Event::Text(text) => self.out.push_str(&escape(text)),
            Event::Code(text) => self.out.push_str(&format!("<code>{}</code>", escape(text))),
            // The agent never puts a tag of its own on the page.
            Event::Html(text) | Event::InlineHtml(text) => self.out.push_str(&escape(text)),
            Event::InlineMath(text) | Event::DisplayMath(text) => {
                self.out.push_str(&format!("<code>{}</code>", escape(text)))
            }
            Event::FootnoteReference(label) => self
                .out
                .push_str(&format!("<sup>[{}]</sup>", escape(label))),
            Event::SoftBreak => self.out.push('\n'),
            Event::HardBreak => self.out.push_str("<br>"),
            Event::TaskListMarker(done) => {
                let mark = if *done { "☑" } else { "☐" };
                self.out
                    .push_str(&format!("<span class=\"md-task\">{mark}</span> "))
            }
            Event::Rule => self.out.push_str("<hr>"),
        }
    }

    fn start(&mut self, tag: &Tag) {
        let html = match tag {
            Tag::Paragraph => "<p>".to_owned(),
            Tag::Heading { level, .. } => format!("<{}>", heading(*level)),
            Tag::BlockQuote(_) => "<blockquote>".to_owned(),
            Tag::CodeBlock(kind) => {
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_owned()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((language, String::new()));
                return;
            }
            Tag::HtmlBlock => "<pre class=\"md-html\">".to_owned(),
            Tag::List(Some(first)) => format!("<ol start=\"{first}\">"),
            Tag::List(None) => "<ul>".to_owned(),
            Tag::Item => "<li>".to_owned(),
            Tag::FootnoteDefinition(label) => {
                format!("<div class=\"md-footnote\"><sup>[{}]</sup>", escape(label))
            }
            Tag::DefinitionList => "<dl>".to_owned(),
            Tag::DefinitionListTitle => "<dt>".to_owned(),
            Tag::DefinitionListDefinition => "<dd>".to_owned(),
            Tag::Table(aligns) => {
                self.aligns = aligns.clone();
                "<table>".to_owned()
            }
            Tag::TableHead => {
                self.head = true;
                self.cell = 0;
                "<thead><tr>".to_owned()
            }
            Tag::TableRow => {
                self.cell = 0;
                "<tr>".to_owned()
            }
            Tag::TableCell => {
                let name = if self.head { "th" } else { "td" };
                let align = match self.aligns.get(self.cell) {
                    Some(Alignment::Left) => " class=\"md-left\"",
                    Some(Alignment::Center) => " class=\"md-center\"",
                    Some(Alignment::Right) => " class=\"md-right\"",
                    _ => "",
                };
                self.cell += 1;
                format!("<{name}{align}>")
            }
            Tag::Emphasis => "<em>".to_owned(),
            Tag::Strong => "<strong>".to_owned(),
            Tag::Strikethrough => "<del>".to_owned(),
            Tag::Superscript => "<sup>".to_owned(),
            Tag::Subscript => "<sub>".to_owned(),
            Tag::Link { dest_url, .. } => self.link(dest_url, ""),
            // An image inside a line of text is not a block. It loads
            // nothing: a page never reaches the network on its own.
            Tag::Image { dest_url, .. } => self.link(dest_url, " class=\"md-image\""),
            Tag::MetadataBlock(_) => String::new(),
        };
        self.out.push_str(&html);
    }

    /// Only a link that leaves the page for the web opens, in a new tab.
    /// Any other target has nothing to open in a course.
    fn link(&mut self, url: &str, class: &str) -> String {
        let open = opens(url);
        self.links.push(open);
        match open {
            true => format!(
                "<a{class} href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\">",
                escape(url)
            ),
            false => match class.is_empty() {
                true => format!("<span class=\"md-link\" title=\"{}\">", escape(url)),
                false => format!("<span{class} title=\"{}\">", escape(url)),
            },
        }
    }

    fn end(&mut self, tag: TagEnd) {
        let html = match tag {
            TagEnd::Paragraph => "</p>",
            TagEnd::Heading(level) => {
                self.out.push_str(&format!("</{}>", heading(level)));
                return;
            }
            TagEnd::BlockQuote(_) => "</blockquote>",
            TagEnd::CodeBlock => "",
            TagEnd::HtmlBlock => "</pre>",
            TagEnd::List(true) => "</ol>",
            TagEnd::List(false) => "</ul>",
            TagEnd::Item => "</li>",
            TagEnd::FootnoteDefinition => "</div>",
            TagEnd::DefinitionList => "</dl>",
            TagEnd::DefinitionListTitle => "</dt>",
            TagEnd::DefinitionListDefinition => "</dd>",
            TagEnd::Table => "</tbody></table>",
            TagEnd::TableHead => {
                self.head = false;
                "</tr></thead><tbody>"
            }
            TagEnd::TableRow => "</tr>",
            TagEnd::TableCell => match self.head {
                true => "</th>",
                false => "</td>",
            },
            TagEnd::Emphasis => "</em>",
            TagEnd::Strong => "</strong>",
            TagEnd::Strikethrough => "</del>",
            TagEnd::Superscript => "</sup>",
            TagEnd::Subscript => "</sub>",
            TagEnd::Link | TagEnd::Image => match self.links.pop() {
                Some(true) => "</a>",
                _ => "</span>",
            },
            TagEnd::MetadataBlock(_) => "",
        };
        self.out.push_str(html);
    }

    /// A fence of code, colored with the language of its info string. The
    /// agent typed it, so it carries no path and no commit.
    fn code_block(&mut self) {
        let Some((language, code)) = self.code.take() else {
            return;
        };
        let spans = match language.is_empty() {
            true => Vec::new(),
            false => self.highlighter.spans(&code, &language),
        };

        self.out.push_str("<pre class=\"md-code\"><code>");
        for (number, line) in code.lines().enumerate() {
            if number > 0 {
                self.out.push('\n');
            }
            let line_spans = spans.get(number).map_or(&[][..], Vec::as_slice);
            self.out.push_str(&painted(line, line_spans));
        }
        self.out.push_str("</code></pre>");
    }
}

fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(at, _)| at + 1))
        .collect()
}

/// One line of code, with its syntax classes.
fn painted(line: &str, spans: &[Span]) -> String {
    let mut html = String::new();
    let mut last = 0;

    for span in spans {
        let (Some(before), Some(inside)) =
            (line.get(last..span.start), line.get(span.start..span.end))
        else {
            continue;
        };
        html.push_str(&escape(before));
        html.push_str(&format!(
            "<span class=\"{}\">{}</span>",
            escape(&span.cls),
            escape(inside)
        ));
        last = span.end;
    }
    html.push_str(&escape(line.get(last..).unwrap_or_default()));
    html
}

fn opens(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
}

fn heading(level: HeadingLevel) -> &'static str {
    match level {
        HeadingLevel::H1 => "h1",
        HeadingLevel::H2 => "h2",
        HeadingLevel::H3 => "h3",
        HeadingLevel::H4 => "h4",
        HeadingLevel::H5 => "h5",
        HeadingLevel::H6 => "h6",
    }
}

/// Text as HTML that shows it, tags and all.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::model::ImageOrigin;
    use crate::testutil::{PNG, Repo, SVG, build_repo, commit};

    /// The code that the excerpts of the corpus read. A comment opens above
    /// line 10, so the colors of an excerpt that starts inside it show
    /// whether the parse ran from the first line.
    const RAFT: &str = "\
#include <stdlib.h>

#include \"log.h\"
#include \"raft.h\"

/* The election timer.
 *
 * A follower that hears no leader for a while starts an election. The
 * timeout has a random part, so two followers rarely start at once.
 */
int election_timeout(int base)
{
    int timeout = base + rand() % base;

    log_debug(\"next election in %d ms\", timeout);
    return timeout;
}

/* A server votes once per term, for the first candidate that asks. */
bool grant_vote(struct server *s, int term, int candidate)
{
    if (term < s->term) {
        return false;
    }
    if (term > s->term) {
        s->term = term;
        s->voted_for = -1;
    }
    if (s->voted_for != -1 && s->voted_for != candidate) {
        return false;
    }
    s->voted_for = candidate;
    return true;
}
";

    /// An SVG as a drawing program exports it: its doctype declares the
    /// entities of its namespaces.
    const EDITOR_SVG: &str = "<?xml version=\"1.0\"?>\n<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG \
                              1.1//EN\" \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\" [\n  \
                              <!ENTITY ns_extend \"http://ns.example.com/Extensibility/1.0/\">\n]>\n\
                              <svg width=\"10\" height=\"10\"></svg>\n";

    /// Text in UTF-16, little endian, with its byte order mark.
    fn utf16(text: &str) -> Vec<u8> {
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        bytes
    }

    /// The repository of the corpus: `src/raft.c`, a file of no known
    /// language, a file outside ASCII, images under `docs/`, and `lib` as
    /// the submodule `vendor/lib`. The branch `stale` names a commit of `lib` that the
    /// submodule never fetched.
    struct Corpus {
        top: Repo,
        git: Git,
        /// The notes of the course `raft`, under the top level of `top`.
        notes: Notes,
        /// The full hashes that a snapshot names, and the name it gives
        /// them: the path of the submodule changes the hash of `top` at
        /// every run.
        labels: Vec<(String, &'static str)>,
        _lib: Repo,
    }

    async fn corpus() -> Corpus {
        let lib = build_repo(&[commit("lib")
            .file(
                "src/lib.c",
                "/* A list of the servers. */\nstruct server *servers;\nint count;\n",
            )
            .file("docs/logo.svg", SVG)])
        .await;
        // One byte over the limit: the size is read before the bytes.
        let mut big = PNG.to_vec();
        big.resize(image::MAX_BYTES as usize + 1, 0);
        let top = build_repo(&[commit("raft")
            .file("src/raft.c", RAFT)
            .file("src/terms.xyz", "term 1: S1 leads\nterm 2: S2 leads\n")
            .file(
                "src/names.c",
                "/* Les élections. */\nchar *chef = \"Ægir\"; /* the leader */\n",
            )
            .bytes("docs/flow.png", PNG)
            .file("docs/states.svg", SVG)
            .file("docs/fake.png", SVG)
            .file("docs/editor.svg", EDITOR_SVG)
            .bytes("docs/wide.svg", &utf16(SVG))
            .bytes("docs/my figure.png", PNG)
            .file("docs/notes.gif", "GIF is the name, not the content.\n")
            .bytes("docs/big.png", &big)
            .submodule("vendor/lib", &lib)])
        .await;
        let head = top.sha("HEAD").await;

        lib.commit(&commit("two").file("src/lib.c", "int two;\n"))
            .await;
        let unfetched = lib.sha("HEAD").await;
        top.git(&["checkout", "--quiet", "-b", "stale"]).await;
        top.commit(&commit("bump lib").gitlink("vendor/lib", &unfetched))
            .await;
        let stale = top.sha("HEAD").await;
        top.git(&["checkout", "--quiet", "main"]).await;

        let labels = vec![
            (head, "<top HEAD>"),
            (stale, "<top stale>"),
            (lib.sha("HEAD~1").await, "<lib HEAD>"),
            (unfetched, "<lib unfetched>"),
        ];
        let git = Git::discover(top.path()).await.unwrap();
        let notes = notes_in(top.path());
        Corpus {
            top,
            git,
            notes,
            labels,
            _lib: lib,
        }
    }

    /// The notes of the course `raft` under `dir`, with a file for each
    /// case of the corpus, the notes of the course `bully`, and a file of
    /// the store outside the notes.
    fn notes_in(dir: &Path) -> Notes {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::symlink;

        let notes = Notes::of(&crate::store::Store::at(dir), "raft");
        let at = |path: &str| notes.dir.join(path);
        let write = |path: &Path, bytes: &[u8]| {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        };
        write(&at("ring.svg"), SVG.as_bytes());
        write(&at("my ring.svg"), SVG.as_bytes());
        write(&at("figures/pixels.png"), PNG);
        write(&at("fake.png"), SVG.as_bytes());
        write(&at("editor.svg"), EDITOR_SVG.as_bytes());
        write(&at("wide.svg"), &utf16(SVG));
        write(&at("text.gif"), b"GIF is the name, not the content.\n");
        // One byte over the limit, with no byte written: the size comes
        // from the metadata.
        std::fs::File::create(at("big.png"))
            .unwrap()
            .set_len(image::MAX_BYTES + 1)
            .unwrap();
        std::fs::create_dir(at("folder.svg")).unwrap();
        let pipe = std::ffi::CString::new(at("pipe.svg").as_os_str().as_bytes()).unwrap();
        // SAFETY: the path is a C string that lives across the call.
        assert_eq!(unsafe { libc::mkfifo(pipe.as_ptr(), 0o600) }, 0);
        symlink("ring.svg", at("linked.svg")).unwrap();
        let elsewhere = dir.join("elsewhere");
        write(&elsewhere.join("away.svg"), SVG.as_bytes());
        symlink(elsewhere.join("away.svg"), at("away.svg")).unwrap();
        symlink(&elsewhere, at("out")).unwrap();
        write(&dir.join(".learn/bully/notes/ring.svg"), SVG.as_bytes());
        write(&dir.join(".learn/raft/ring.svg"), SVG.as_bytes());
        notes
    }

    impl Corpus {
        async fn parse(&self, source: &str) -> Result<Parsed, Vec<Fault>> {
            super::parse(source, Some(&self.git), &self.notes, Highlighter::shared()).await
        }

        /// A text with each known hash, whole or cut short, by its name.
        fn label(&self, text: &str) -> String {
            let mut text = text.to_owned();
            for (hash, name) in &self.labels {
                text = text.replace(hash.as_str(), name);
                text = text.replace(&hash[..12], name);
            }
            text
        }
    }

    /// A repository for the tests that read no excerpt.
    async fn plain() -> Corpus {
        let top = build_repo(&[commit("first").file("a.c", "int a;\n")]).await;
        let git = Git::discover(top.path()).await.unwrap();
        let notes = notes_in(top.path());
        Corpus {
            _lib: build_repo(&[]).await,
            top,
            git,
            notes,
            labels: Vec::new(),
        }
    }

    async fn ids(source: &str) -> Vec<String> {
        plain()
            .await
            .parse(source)
            .await
            .unwrap()
            .blocks
            .iter()
            .map(|block| block.id().to_owned())
            .collect()
    }

    /// What a case of the corpus gives, in a form that a reader checks at
    /// a glance: each block with its id, or each error as the agent reads it.
    async fn outcome(corpus: &Corpus, file: &str, source: &str) -> String {
        render(corpus, file, corpus.parse(source).await)
    }

    fn render(corpus: &Corpus, file: &str, parsed: Result<Parsed, Vec<Fault>>) -> String {
        let out = match parsed {
            Ok(parsed) => {
                let mut out = format!("title: {}\n", parsed.title.as_deref().unwrap_or("(none)"));
                for block in &parsed.blocks {
                    match block {
                        Block::Text { id, html } => out.push_str(&format!("\ntext {id}\n{html}\n")),
                        Block::Callout { id, marker, html } => {
                            out.push_str(&format!("\ncallout {id} {}\n{html}\n", marker.as_str()))
                        }
                        Block::Excerpt { id, excerpt } => {
                            out.push_str(&excerpt_outcome(id, excerpt, parsed.hidden.get(id)))
                        }
                        Block::Diagram { id, source } => {
                            out.push_str(&format!("\ndiagram {id}\n{source}"))
                        }
                        Block::Chart { id, chart } => out.push_str(&format!(
                            "\nchart {id}\n{}\n",
                            serde_json::to_string_pretty(chart).unwrap()
                        )),
                        Block::Steps { id, steps } => out.push_str(&format!(
                            "\nsteps {id}\n{}\n",
                            serde_json::to_string_pretty(steps).unwrap()
                        )),
                        Block::Image {
                            id,
                            alt,
                            media,
                            path,
                            origin,
                            repo,
                            commit,
                            format,
                            width,
                            height,
                        } => {
                            let place = match (origin, repo, commit) {
                                (ImageOrigin::Repository, Some(repo), Some(commit)) => {
                                    format!("in {repo:?} at {commit}")
                                }
                                _ => format!("{origin:?}, repo {repo:?}, commit {commit:?}"),
                            };
                            out.push_str(&format!(
                                "\nimage {id}\n{path} {place}, {}, {width:?} x {height:?}\n\
                                 alt: {alt:?}\nmedia: {media}, {} bytes\n",
                                format.mime(),
                                parsed.media[media].len()
                            ))
                        }
                    }
                }
                out
            }
            Err(faults) => {
                let mut out = "refused\n".to_owned();
                for fault in faults {
                    out.push_str(&fault.display(file));
                    out.push('\n');
                }
                out
            }
        };
        corpus.label(&out)
    }

    /// An excerpt as a reader checks it: where it comes from, then each
    /// line with its number, `>` on a marked line, and its colors.
    fn excerpt_outcome(id: &str, excerpt: &crate::model::Excerpt, hidden: Option<&Line>) -> String {
        let lines = match excerpt.from == excerpt.to {
            true => excerpt.from.to_string(),
            false => format!("{}-{}", excerpt.from, excerpt.to),
        };
        let mut out = format!(
            "\nexcerpt {id}\n{}:{lines} in {:?} at {}, lang {:?}\n",
            excerpt.path, excerpt.repo, excerpt.commit, excerpt.lang
        );
        for (number, line) in (excerpt.from..).zip(&excerpt.lines) {
            let mark = match excerpt.marked.contains(&number) {
                true => '>',
                false => ' ',
            };
            let text = match line {
                Some(line) => painted(&line.text, &in_bytes(line)),
                None => "(hidden)".to_owned(),
            };
            out.push_str(&format!("{number:>4} {mark} {text}\n"));
        }
        if let Some(line) = hidden {
            out.push_str(&format!("hidden on the server: {:?}\n", line.text));
        }
        out
    }

    /// The spans of an excerpt line in bytes again, for `painted`: they
    /// leave the server in UTF-16 units.
    fn in_bytes(line: &Line) -> Vec<Span> {
        let byte = |utf16: usize| {
            let mut units = 0;
            for (at, ch) in line.text.char_indices() {
                if units >= utf16 {
                    return at;
                }
                units += ch.len_utf16();
            }
            line.text.len()
        };
        line.spans
            .iter()
            .map(|span| Span {
                start: byte(span.start),
                end: byte(span.end),
                cls: span.cls.clone(),
            })
            .collect()
    }

    /// Every shape and every error of a page is a case of the corpus.
    #[tokio::test]
    async fn the_corpus_of_pages() {
        let corpus = corpus().await;
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/pages");
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
            .collect();
        files.sort();
        assert!(!files.is_empty(), "no case in {}", dir.display());

        for path in files {
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let source = std::fs::read_to_string(&path).unwrap();
            let out = outcome(&corpus, &file, &source).await;

            insta::with_settings!({ description => source.clone(), omit_expression => true }, {
                insta::assert_snapshot!(name, out);
            });
        }
    }

    /// Outside a repository, every block but the excerpt and the image
    /// passes, and each excerpt and image is an error of its own.
    #[tokio::test]
    async fn the_corpus_of_pages_outside_a_repository() {
        let corpus = plain().await;
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/outside");
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
            .collect();
        files.sort();
        assert!(!files.is_empty(), "no case in {}", dir.display());

        for path in files {
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let source = std::fs::read_to_string(&path).unwrap();
            let parsed = super::parse(&source, None, &corpus.notes, Highlighter::shared()).await;
            let out = render(&corpus, &file, parsed);

            insta::with_settings!({ description => source.clone(), omit_expression => true }, {
                insta::assert_snapshot!(format!("outside-{name}"), out);
            });
        }
    }

    /// A debrief and a cheatsheet read the repository as a page does, so
    /// outside one they refuse an excerpt and an image of the repository
    /// too. An image of the notes works.
    #[tokio::test]
    async fn a_debrief_and_a_cheatsheet_outside_a_repository_show_no_code() {
        let corpus = plain().await;
        let notes = &corpus.notes;
        let source = "# Recap\n\n```excerpt\npath: src/raft.c\nlines: 1-3\n```\n\n\
                      ![The states](docs/states.svg)\n\n```c\nint term;\n```\n\n\
                      ![The ring](.learn/raft/notes/ring.svg)\n";

        let debrief = parse_debrief(source, 1, None, notes, Highlighter::shared())
            .await
            .unwrap_err();
        let sheet = parse_cheatsheet(source, None, notes, Highlighter::shared())
            .await
            .unwrap_err();

        for faults in [debrief, sheet] {
            let lines: Vec<_> = faults
                .iter()
                .map(|fault| (fault.line, fault.kind))
                .collect();
            assert_eq!(lines, [(3, "excerpt"), (8, "image")], "{faults:?}");
        }
        let kept = "# Recap\n\n```c\nint term;\n```\n\n![The ring](.learn/raft/notes/ring.svg)\n";
        let debrief = parse_debrief(kept, 1, None, notes, Highlighter::shared())
            .await
            .unwrap();
        let sheet = parse_cheatsheet(kept, None, notes, Highlighter::shared())
            .await
            .unwrap();
        for parsed in [debrief, sheet] {
            let kinds: Vec<_> = parsed.blocks.iter().map(Block::kind).collect();
            assert_eq!(kinds, ["text", "image"]);
            assert_eq!(parsed.media.len(), 1);
        }
    }

    /// A debrief and a cheatsheet in a repository take an image of the
    /// notes too, and refuse one of another course.
    #[tokio::test]
    async fn a_debrief_and_a_cheatsheet_take_an_image_of_the_notes() {
        let corpus = plain().await;
        let (git, notes) = (Some(&corpus.git), &corpus.notes);
        let source = "# Recap\n\n![The ring](.learn/raft/notes/ring.svg)\n\n\
                      ![Another ring](.learn/bully/notes/ring.svg)\n";

        let debrief = parse_debrief(source, 1, git, notes, Highlighter::shared())
            .await
            .unwrap_err();
        let sheet = parse_cheatsheet(source, git, notes, Highlighter::shared())
            .await
            .unwrap_err();

        for faults in [debrief, sheet] {
            let lines: Vec<_> = faults.iter().map(|fault| fault.line).collect();
            assert_eq!(lines, [5], "{faults:?}");
        }
        let kept = source.replace("bully", "raft");
        let sheet = parse_cheatsheet(&kept, git, notes, Highlighter::shared())
            .await
            .unwrap();
        let Some(Block::Image {
            origin,
            repo,
            commit,
            ..
        }) = sheet.blocks.get(1)
        else {
            panic!("{:?}", sheet.blocks);
        };
        assert_eq!((*origin, repo, commit), (ImageOrigin::Notes, &None, &None));
    }

    /// A link in place of `notes/` leads out of the notes, as a link
    /// inside them does: a page would show any image of its target.
    #[tokio::test]
    async fn a_link_in_place_of_the_notes_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(elsewhere.join("private")).unwrap();
        std::fs::write(elsewhere.join("private/away.svg"), SVG).unwrap();
        let notes = Notes::of(&crate::store::Store::at(dir.path()), "raft");
        std::fs::create_dir_all(notes.dir.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &notes.dir).unwrap();
        let source = "# T\n\n![A file out of the course](.learn/raft/notes/private/away.svg)\n";

        let faults = super::parse(source, None, &notes, Highlighter::shared())
            .await
            .unwrap_err();

        let found: Vec<_> = faults
            .iter()
            .map(|f| (f.line, f.message.as_str()))
            .collect();
        assert_eq!(
            found,
            [(
                3,
                ".learn/raft/notes/ is a symbolic link: put a directory in its place, and the \
                 image in it"
            )]
        );
    }

    #[tokio::test]
    async fn the_id_is_the_hash_of_the_kind_and_the_source() {
        // printf 'text\n# One' | sha256sum
        assert_eq!(block_id("text", "# One", 1, Within::Lesson), "5b1eb887548a");
        assert_eq!(ids("# One\n").await, ["5b1eb887548a"]);
    }

    #[test]
    fn the_id_of_a_block_of_a_debrief_takes_its_quiz() {
        // printf 'text\n# One\ndebrief 2' | sha256sum
        assert_eq!(
            block_id("text", "# One", 1, Within::Debrief(2)),
            "3043bfce5167"
        );
        let found = sources("# One\n", Within::Debrief(2));
        assert_eq!(found[0].id, "3043bfce5167");
        assert_ne!(
            block_id("text", "# One", 1, Within::Debrief(1)),
            block_id("text", "# One", 1, Within::Debrief(2))
        );
    }

    #[tokio::test]
    async fn the_id_of_an_excerpt_is_the_hash_of_its_fence() {
        let fence = "```excerpt\npath: a.c\nlines: 1\n```";
        let source = format!("{fence}\n\n{fence}\n");

        // printf 'excerpt\n```excerpt\npath: a.c\nlines: 1\n```' | sha256sum
        assert_eq!(
            block_id("excerpt", fence, 1, Within::Lesson),
            "f97986e2c189"
        );
        assert_eq!(
            ids(&source).await,
            [
                block_id("excerpt", fence, 1, Within::Lesson),
                block_id("excerpt", fence, 2, Within::Lesson)
            ]
        );
    }

    #[tokio::test]
    async fn an_unchanged_block_keeps_its_id_when_the_page_is_pushed_again() {
        let first = "# One\n\nThe first section.\n\n## Two\n\nThe second.\n";
        let second = "# One\n\nThe first section.\n\n## Two\n\nThe second, rewritten.\n\n\
                      ## Three\n\nA new section.\n";

        let before = ids(first).await;
        let after = ids(second).await;

        assert_eq!(
            before,
            ids(first).await,
            "the same source gives the same ids"
        );
        assert_eq!(after.len(), 3);
        assert_eq!(after[0], before[0], "the first section did not change");
        assert_ne!(after[1], before[1], "the second section changed");
    }

    #[tokio::test]
    async fn two_copies_of_one_block_get_two_ids() {
        let ids = ids("# One\n\n# One\n\n# One\n").await;

        // printf 'text\n# One' | sha256sum, then with '\n2' and '\n3'
        assert_eq!(ids, ["5b1eb887548a", "300e24ac0238", "7cd404852f2e"]);
    }

    #[tokio::test]
    async fn the_blank_lines_around_a_block_do_not_change_its_id() {
        assert_eq!(ids("Some text.\n").await, ids("\n\nSome text.\n\n\n").await);
    }

    #[tokio::test]
    async fn consecutive_text_forms_one_block_and_a_heading_starts_one() {
        let parsed = plain()
            .await
            .parse("One.\n\n- a\n- b\n\nTwo.\n\n# Title\n\nThree.\n")
            .await
            .unwrap();

        assert_eq!(parsed.blocks.len(), 2);
        assert_eq!(parsed.title.as_deref(), Some("Title"));
    }

    #[tokio::test]
    async fn an_excerpt_closes_the_text_block_before_it() {
        let parsed = plain()
            .await
            .parse("One.\n\n```excerpt\npath: a.c\nlines: 1\n```\nTwo.\n")
            .await
            .unwrap();

        let kinds: Vec<_> = parsed.blocks.iter().map(Block::kind).collect();
        assert_eq!(kinds, ["text", "excerpt", "text"]);
    }

    #[tokio::test]
    async fn a_callout_closes_the_text_block_before_it() {
        let parsed = plain()
            .await
            .parse("One.\n\n> [!hook]\n> A term.\n\nTwo.\n")
            .await
            .unwrap();

        let shape: Vec<_> = parsed
            .blocks
            .iter()
            .map(|block| match block {
                Block::Text { html, .. } => format!("text {html}"),
                Block::Callout { marker, html, .. } => format!("{} {html}", marker.as_str()),
                other => other.kind().to_owned(),
            })
            .collect();
        assert_eq!(
            shape,
            [
                "text <p>One.</p>",
                "hook <p>A term.</p>",
                "text <p>Two.</p>"
            ]
        );
    }

    #[tokio::test]
    async fn the_id_of_a_callout_is_the_hash_of_its_quote() {
        let quote = "> [!note]\n> A term.";
        let source = format!("{quote}\n\n{quote}\n");

        assert_eq!(
            ids(&source).await,
            [
                block_id("callout", quote, 1, Within::Lesson),
                block_id("callout", quote, 2, Within::Lesson)
            ]
        );
    }

    #[tokio::test]
    async fn the_id_of_a_diagram_is_the_hash_of_its_fence() {
        let fence = "```mermaid\ngraph LR\n  A --> B\n```";
        let source = format!("One.\n\n{fence}\n\n{fence}\n");

        let parsed = plain().await.parse(&source).await.unwrap();

        let shape: Vec<_> = parsed
            .blocks
            .iter()
            .map(|block| (block.kind(), block.id().to_owned()))
            .collect();
        assert_eq!(
            shape[1..],
            [
                ("diagram", block_id("diagram", fence, 1, Within::Lesson)),
                ("diagram", block_id("diagram", fence, 2, Within::Lesson)),
            ]
        );
        assert_eq!(
            parsed.blocks[1],
            Block::Diagram {
                id: block_id("diagram", fence, 1, Within::Lesson),
                source: "graph LR\n  A --> B\n".to_owned(),
            }
        );
    }

    #[tokio::test]
    async fn the_id_of_a_chart_is_the_hash_of_its_fence() {
        let fence = "```chart\n{ \"kind\": \"bar\", \"title\": \"Votes\", \
                     \"x\": { \"label\": \"Term\", \"values\": [\"1\"] }, \
                     \"y\": { \"label\": \"Votes\" }, \
                     \"series\": [{ \"name\": \"S1\", \"values\": [3] }] }\n```";
        let source = format!("{fence}\n\n{fence}\n");

        assert_eq!(
            ids(&source).await,
            [
                block_id("chart", fence, 1, Within::Lesson),
                block_id("chart", fence, 2, Within::Lesson)
            ]
        );
    }

    /// A debrief is a page source: it takes a chart, and refuses a wrong
    /// one with the same faults.
    #[tokio::test]
    async fn a_debrief_takes_a_chart() {
        let corpus = plain().await;
        let chart = "```chart\n{ \"kind\": \"line\", \"title\": \"Votes\", \
                     \"x\": { \"label\": \"Term\", \"values\": [1, 2] }, \
                     \"y\": { \"label\": \"Votes\" }, \
                     \"series\": [{ \"name\": \"S1\", \"values\": [3, null] }] }\n```\n";

        let parsed = parse_debrief(
            chart,
            1,
            Some(&corpus.git),
            &corpus.notes,
            Highlighter::shared(),
        )
        .await
        .unwrap();
        let refused = parse_debrief(
            &chart.replace("[3, null]", "[3]"),
            1,
            Some(&corpus.git),
            &corpus.notes,
            Highlighter::shared(),
        )
        .await
        .unwrap_err();

        assert_eq!(parsed.blocks[0].kind(), "chart");
        assert_eq!(
            refused[0].display("debrief.md"),
            "debrief.md:1: chart: series[0].values: holds 1 value, and x holds 2. Give one \
             value for each x value"
        );
    }

    /// A debrief is a page source: it takes steps, and refuses wrong ones
    /// with the same faults.
    #[tokio::test]
    async fn a_debrief_takes_steps() {
        let corpus = plain().await;
        let fence = "```steps\n{ \"title\": \"A vote\", \"columns\": [\"Server\", \"Vote\"], \
                     \"steps\": [{ \"label\": \"t = 0\", \"body\": \"S2 votes.\", \
                     \"rows\": [[\"S2\", \"S1\"]] }] }\n```";

        let parsed = parse_debrief(
            fence,
            1,
            Some(&corpus.git),
            &corpus.notes,
            Highlighter::shared(),
        )
        .await
        .unwrap();
        let refused = parse_debrief(
            &fence.replace("[\"S2\", \"S1\"]", "[\"S2\"]"),
            1,
            Some(&corpus.git),
            &corpus.notes,
            Highlighter::shared(),
        )
        .await
        .unwrap_err();

        assert_eq!(parsed.blocks[0].kind(), "steps");
        assert_eq!(
            parsed.blocks[0].id(),
            block_id("steps", fence, 1, Within::Debrief(1))
        );
        assert_eq!(
            refused[0].display("debrief.md"),
            "debrief.md:1: steps: steps[0].rows[0]: holds 1 cell, and columns holds 2. Give \
             one cell for each column"
        );
    }

    #[tokio::test]
    async fn a_diagram_of_the_largest_size_passes_and_one_line_more_does_not() {
        let fence = |lines: usize| {
            let body: String = (0..lines).map(|n| format!("  A{n} --> B{n}\n")).collect();
            format!("# Big\n\n```mermaid\ngraph LR\n{body}```\n")
        };
        let corpus = plain().await;

        assert!(corpus.parse(&fence(DIAGRAM_LINES - 1)).await.is_ok());
        let faults = corpus.parse(&fence(DIAGRAM_LINES)).await.unwrap_err();
        assert_eq!(
            faults,
            [Fault {
                line: 3,
                kind: "diagram",
                message: "the diagram holds 201 lines, and a diagram has 200 at most: \
                          split it in two"
                    .to_owned(),
            }]
        );
    }

    #[test]
    fn inline_markdown_has_no_paragraph_and_no_html_of_the_source() {
        assert_eq!(
            inline("Call `grant_vote()` *once*").unwrap(),
            "Call <code>grant_vote()</code> <em>once</em>"
        );
        assert_eq!(
            inline("<b onclick=\"x\">a</b>").unwrap(),
            "&lt;b onclick=&quot;x&quot;&gt;a&lt;/b&gt;"
        );
        assert_eq!(inline(r"1\. The first").unwrap(), "1. The first");
        assert_eq!(
            inline("<script>alert(1)</script> now").unwrap(),
            "&lt;script&gt;alert(1)&lt;/script&gt; now"
        );
    }

    #[test]
    fn inline_markdown_refuses_a_block() {
        for (source, error) in [
            ("", "is empty"),
            ("  \n", "is empty"),
            ("- a", "holds a list"),
            ("1. The first", "holds a list"),
            ("# A title", "holds a heading"),
            ("> a", "holds a quote"),
            ("```\ncode\n```", "holds a fence of code"),
            ("---", "holds a rule"),
            ("One.\n\nTwo.", "holds more than one paragraph"),
            ("first line\nsecond line", "holds more than one line"),
            ("b  \nc", "holds more than one line"),
            ("b\\\nc", "holds more than one line"),
            ("a\r\nb", "holds more than one line"),
            ("<div>a</div>\n<div>b</div>", "holds more than one line"),
        ] {
            assert_eq!(inline(source), Err(error.to_owned()), "{source:?}");
        }
    }

    /// A thread hands the agent the source of its block, so each source
    /// must carry the id that the page gives the block.
    #[tokio::test]
    async fn each_block_has_its_source_under_its_id() {
        let corpus = plain().await;
        let source = "# Two leaders\n\nOne term.\n\n\
                      > [!hook]\n> One vote per term.\n\n\
                      ```mermaid\ngraph LR\n  A --> B\n```\n\n\
                      ## The vote\n\nOne term.\n\n\
                      > [!hook]\n> One vote per term.\n";
        let parsed = corpus.parse(source).await.unwrap();

        let found = sources(source, Within::Lesson);

        let ids: Vec<&str> = parsed.blocks.iter().map(Block::id).collect();
        assert_eq!(found.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ids);
        let kinds: Vec<&str> = found.iter().map(|s| s.kind.as_str()).collect();
        assert_eq!(kinds, ["text", "callout", "diagram", "text", "callout"]);
        assert_eq!(found[0].source, "# Two leaders\n\nOne term.");
        assert_eq!(found[2].source, "```mermaid\ngraph LR\n  A --> B\n```");
        // Two copies of one callout: two ids, one source.
        assert_ne!(found[1].id, found[4].id);
        assert_eq!(found[1].source, found[4].source);
    }

    #[test]
    fn a_comment_is_markdown_with_its_html_escaped() {
        let html = markdown("Why `rand()`?\n\n<script>alert(1)</script>\n\n```c\nint a;\n```\n");

        assert!(
            html.starts_with("<p>Why <code>rand()</code>?</p>"),
            "{html}"
        );
        assert!(html.contains("&lt;script&gt;"), "{html}");
        assert!(!html.contains("<script"), "{html}");
        assert!(
            html.contains("<pre class=\"md-code\"><code><span class=\"tok-"),
            "{html}"
        );
    }

    #[test]
    fn deep_quotes_in_a_list_parse_in_linear_time() {
        // Each nested quote once scanned to its own end: this page took
        // seconds, and a page of the largest size took minutes.
        let source = format!("- {}x\n", "> ".repeat(40_000));
        let start = std::time::Instant::now();

        let found = split(&source, Highlighter::shared(), Within::Lesson, Place::ANY);

        assert_eq!((found.pieces.len(), found.faults.len()), (1, 0));
        let spent = start.elapsed();
        assert!(spent < std::time::Duration::from_secs(2), "{spent:?}");
    }

    #[tokio::test]
    async fn the_title_is_the_text_of_the_first_heading() {
        let corpus = plain().await;
        let parsed = corpus
            .parse("Intro.\n\n## The `term` and *the* leader\n\n# Later\n")
            .await
            .unwrap();

        assert_eq!(parsed.title.as_deref(), Some("The term and the leader"));
        assert_eq!(corpus.parse("No heading.\n").await.unwrap().title, None);
    }

    #[tokio::test]
    async fn a_fence_of_code_inside_a_list_stays_in_the_text() {
        let parsed = plain()
            .await
            .parse("- Run it:\n\n  ```sh\n  make check\n  ```\n")
            .await
            .unwrap();
        let Block::Text { html, .. } = &parsed.blocks[0] else {
            panic!("{:?}", parsed.blocks[0]);
        };

        assert!(html.contains("<pre class=\"md-code\">"), "{html}");
    }

    /// The commit is stored with the lines, so a page pushed before a
    /// change of the code shows the code as it was.
    #[tokio::test]
    async fn an_excerpt_names_the_commit_of_the_push() {
        let corpus = corpus().await;
        let source = "```excerpt\npath: src/raft.c\nlines: 13\n```\n";
        let before = corpus.parse(source).await.unwrap();

        corpus
            .top
            .commit(&commit("faster").file("src/raft.c", "int x;\n"))
            .await;
        let after = corpus.parse(source).await.unwrap_err();

        let Block::Excerpt { excerpt, .. } = &before.blocks[0] else {
            panic!("{:?}", before.blocks[0]);
        };
        assert_eq!(excerpt.commit, corpus.labels[0].0);
        assert_eq!(
            excerpt.lines[0].as_ref().unwrap().text,
            "    int timeout = base + rand() % base;"
        );
        assert_eq!(
            after[0].message,
            "src/raft.c has 1 line at HEAD, the excerpt asks for 13"
        );
    }

    /// An image is read at HEAD in the object database: a change in the
    /// worktree that is not committed does not reach the page.
    #[tokio::test]
    async fn an_image_is_read_at_head_and_not_in_the_worktree() {
        let corpus = corpus().await;
        std::fs::write(corpus.top.path().join("docs/flow.png"), "not an image\n").unwrap();
        std::fs::write(corpus.top.path().join("docs/new.png"), PNG).unwrap();

        let parsed = corpus
            .parse("![The flow](docs/flow.png)\n\n![A new one](docs/new.png)\n")
            .await
            .unwrap_err();

        assert_eq!(
            corpus.label(&parsed[0].display("1.md")),
            "1.md:3: image: docs/new.png is not a file at commit <top HEAD>"
        );
        let parsed = corpus.parse("![The flow](docs/flow.png)\n").await.unwrap();
        assert_eq!(
            parsed.media.into_values().collect::<Vec<_>>(),
            [PNG.to_vec()]
        );
    }

    /// An image in a submodule whose commit was never fetched gets the
    /// error of an excerpt, with the command that fetches it.
    #[tokio::test]
    async fn an_image_in_a_submodule_whose_commit_is_missing_names_the_submodule() {
        let corpus = corpus().await;
        corpus.top.git(&["checkout", "--quiet", "stale"]).await;

        let faults = corpus
            .parse("![The logo](vendor/lib/docs/logo.svg)\n")
            .await
            .unwrap_err();

        assert_eq!(faults.len(), 1, "{faults:?}");
        let message = corpus.label(&faults[0].display("1.md"));
        assert!(
            message.starts_with(
                "1.md:1: image: the submodule vendor/lib does not hold commit <lib unfetched>. \
                 Run git --git-dir "
            ),
            "{message}"
        );
    }

    /// A link destination is a URL, so a space in a name comes as `%20`,
    /// as GitHub writes it. The path between `<` and `>` takes it raw.
    #[tokio::test]
    async fn an_image_path_is_percent_decoded() {
        let corpus = corpus().await;

        let parsed = corpus
            .parse("![A figure](docs/my%20figure.png)\n\n![A figure](<docs/my figure.png>)\n")
            .await
            .unwrap();

        let paths: Vec<_> = parsed
            .blocks
            .iter()
            .map(|block| match block {
                Block::Image { path, .. } => path.as_str(),
                block => panic!("{block:?}"),
            })
            .collect();
        assert_eq!(paths, ["docs/my figure.png", "docs/my figure.png"]);
    }

    /// The same image twice is two blocks and one file of `media/`.
    #[tokio::test]
    async fn the_same_image_twice_is_two_blocks_and_one_media() {
        let corpus = corpus().await;
        let image = "![The flow](docs/flow.png)";

        let parsed = corpus
            .parse(&format!("{image}\n\n{image}\n"))
            .await
            .unwrap();

        assert_eq!(
            parsed.blocks.iter().map(Block::id).collect::<Vec<_>>(),
            [
                block_id("image", image, 1, Within::Lesson),
                block_id("image", image, 2, Within::Lesson)
            ]
        );
        assert_eq!(parsed.media.len(), 1);
        assert_eq!(
            sources(&format!("{image}\n"), Within::Lesson)[0].kind,
            "image"
        );
    }

    #[test]
    fn an_image_check_of_a_deep_list_runs_in_linear_time() {
        let source = format!("{}![a](b.png)\n", "- ".repeat(40_000));
        let start = std::time::Instant::now();

        let found = split(&source, Highlighter::shared(), Within::Lesson, Place::ANY);

        assert_eq!(found.faults.len(), 1);
        let spent = start.elapsed();
        assert!(spent < std::time::Duration::from_secs(2), "{spent:?}");
    }

    /// A debrief shows once its quiz is answered: a hidden line in it
    /// would never wait for an answer. The other faults come with it.
    #[tokio::test]
    async fn a_debrief_hides_no_line() {
        let corpus = corpus().await;
        let source = "# Almost\n\n```excerpt\npath: src/raft.c\nlines: 12-14\nmark: 13\nhide: 13\n\
                      ```\n\n```excerpt\npath: src/raft.c\nlines: 900\n```\n";

        let faults = parse_debrief(
            source,
            1,
            Some(&corpus.git),
            &corpus.notes,
            Highlighter::shared(),
        )
        .await
        .unwrap_err();
        let shown = parse_debrief(
            &source.replace("hide: 13\n", ""),
            1,
            Some(&corpus.git),
            &corpus.notes,
            Highlighter::shared(),
        )
        .await;

        let lines: Vec<_> = faults
            .iter()
            .map(|fault| fault.display("debrief.md"))
            .collect();
        assert_eq!(
            lines,
            [
                "debrief.md:7: excerpt: a debrief hides no line: its quiz is answered already. \
                 Take hide out",
                "debrief.md:12: excerpt: src/raft.c has 34 lines at HEAD, the excerpt asks for 900",
            ]
        );
        assert!(
            corpus.parse(source).await.is_err(),
            "the read fails in a page too"
        );
        let faults = shown.unwrap_err();
        assert_eq!(faults.len(), 1, "{faults:?}");
    }

    /// The learner keeps the cheatsheet to read, and answers no quiz on
    /// it: a hidden line is refused, with the other faults. Its blocks take
    /// ids of their own, as a cheatsheet may repeat a lesson.
    #[tokio::test]
    async fn a_cheatsheet_hides_no_line() {
        let corpus = corpus().await;
        let source = "# Raft\n\n```excerpt\npath: src/raft.c\nlines: 12-14\nhide: 13\n```\n\n\
                      > [!tip]\n> A typo.\n";

        let faults = parse_cheatsheet(
            source,
            Some(&corpus.git),
            &corpus.notes,
            Highlighter::shared(),
        )
        .await
        .unwrap_err();
        let parsed = parse_cheatsheet(
            "# Raft\n",
            Some(&corpus.git),
            &corpus.notes,
            Highlighter::shared(),
        )
        .await
        .unwrap();

        let lines: Vec<_> = faults
            .iter()
            .map(|fault| fault.display("cheatsheet.md"))
            .collect();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(
            lines[0],
            "cheatsheet.md:6: excerpt: a cheatsheet hides no line: the learner keeps it to \
             read, and answers no quiz on it. Take hide out"
        );
        assert!(
            lines[1].starts_with("cheatsheet.md:9: callout: "),
            "{lines:?}"
        );
        assert_eq!(
            parsed.blocks[0].id(),
            block_id("text", "# Raft", 1, Within::Cheatsheet)
        );
        assert_ne!(
            block_id("text", "# Raft", 1, Within::Cheatsheet),
            block_id("text", "# Raft", 1, Within::Lesson)
        );
    }
}
