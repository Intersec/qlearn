//! The parser of a page.
//!
//! The agent writes a page in Markdown. `pulldown-cmark` reads it, and this
//! module splits it into blocks, checks each one, and writes the HTML of the
//! text. See section 4 of the design.
//!
//! Every tag of the output is written here, from a short list. HTML in the
//! source comes out as text, so a page never holds a script or a style that
//! the agent typed.

use std::collections::HashMap;
use std::ops::Range;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use sha2::{Digest, Sha256};

use crate::highlight::Highlighter;
use crate::model::{Block, Span};

/// A page that passed every check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    pub blocks: Vec<Block>,
    /// The text of the first heading. It titles a page that the syllabus
    /// does not hold.
    pub title: Option<String>,
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

/// The markers of a callout, on the first line of a quote.
const CALLOUTS: [&str; 4] = ["hook", "teaser", "pitfall", "note"];

/// One event of the parser, with the bytes of the source that it covers.
type Item<'a> = (Event<'a>, Range<usize>);

/// Split a source into blocks, or say every error that it holds.
///
/// Nothing is stored before this returns `Ok`, so a page with one error
/// leaves the course as it was.
pub fn parse(source: &str, highlighter: &Highlighter) -> Result<Parsed, Vec<Fault>> {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let items: Vec<Item> = Parser::new_ext(source, options)
        .into_offset_iter()
        .collect();
    let starts = line_starts(source);
    let line_of = |byte: usize| starts.partition_point(|start| *start <= byte);

    let mut page = Page {
        source,
        blocks: Vec::new(),
        open: None,
        seen: HashMap::new(),
    };
    let mut title = None;
    let mut faults = Vec::new();

    let mut at = 0;
    while at < items.len() {
        let end = matching_end(&items, at);
        let node = &items[at..=end];
        let range = node[0].1.clone();

        // A fence of a block kind inside a list or a quote would show code
        // that the server never checked.
        for (event, range) in &node[1..] {
            if let Some(kind) = fence_kind(event) {
                faults.push(refused(line_of(range.start), kind));
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
            Role::Refused(kind) => {
                page.close();
                faults.push(refused(line_of(range.start), kind));
            }
        }
        at = end + 1;
    }
    page.close();

    match faults.is_empty() {
        true => Ok(Parsed {
            blocks: page.blocks,
            title,
        }),
        false => Err(faults),
    }
}

/// The id of a block: the first 12 hexadecimal characters of the SHA-256
/// of its kind and its source. An unchanged block keeps its id when the
/// agent pushes the page again, so its threads stay on it. A later copy of
/// the same block hashes its rank too, so two copies never share threads.
pub fn block_id(kind: &str, source: &str, rank: usize) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update(b"\n");
    hasher.update(source.as_bytes());
    if rank > 1 {
        hasher.update(format!("\n{rank}").as_bytes());
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
    /// A block of a kind that this release does not draw yet.
    Refused(&'static str),
}

fn role(source: &str, node: &[Item]) -> Role {
    match &node[0].0 {
        Event::Start(Tag::Heading { .. }) => Role::Heading,
        event @ Event::Start(Tag::CodeBlock(_)) => match fence_kind(event) {
            Some(kind) => Role::Refused(kind),
            None => Role::Text,
        },
        Event::Start(Tag::Paragraph) if is_image_alone(node) => Role::Refused("image"),
        Event::Start(Tag::BlockQuote(_)) if is_callout(&source[node[0].1.clone()]) => {
            Role::Refused("callout")
        }
        _ => Role::Text,
    }
}

/// Whether a paragraph holds one image and nothing else.
fn is_image_alone(paragraph: &[Item]) -> bool {
    let inner = &paragraph[1..paragraph.len() - 1];
    matches!(inner.first(), Some((Event::Start(Tag::Image { .. }), _)))
        && matching_end(inner, 0) == inner.len() - 1
}

/// Whether a quote opens with the marker of a callout, such as `[!hook]`.
fn is_callout(quote: &str) -> bool {
    let first = quote.lines().next().unwrap_or("");
    let first = first.trim_start().trim_start_matches('>').trim_start();
    let Some(marker) = first
        .strip_prefix("[!")
        .and_then(|rest| rest.split_once(']'))
        .map(|(marker, _)| marker)
    else {
        return false;
    };

    CALLOUTS
        .iter()
        .any(|callout| callout.eq_ignore_ascii_case(marker))
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
    blocks: Vec<Block>,
    /// The text block being written: the bytes that it covers, and its HTML.
    open: Option<(Range<usize>, String)>,
    /// How many times each kind and source came, for the rank of a copy.
    seen: HashMap<(&'static str, &'s str), usize>,
}

impl<'s> Page<'s> {
    fn id(&mut self, kind: &'static str, source: &'s str) -> String {
        let rank = self.seen.entry((kind, source)).or_default();
        *rank += 1;
        block_id(kind, source, *rank)
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
        // The blank lines after a block are not part of it: a page pushed
        // again with one more section keeps the id of this one.
        let source = self.source[covered].trim_end();

        let id = self.id("text", source);
        self.blocks.push(Block::Text { id, html });
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

fn escape(text: &str) -> String {
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

    fn parse(source: &str) -> Result<Parsed, Vec<Fault>> {
        super::parse(source, Highlighter::shared())
    }

    fn ids(source: &str) -> Vec<String> {
        parse(source)
            .unwrap()
            .blocks
            .iter()
            .map(|block| block.id().to_owned())
            .collect()
    }

    /// What a case of the corpus gives, in a form that a reader checks at
    /// a glance: each block with its id, or each error as the agent reads it.
    fn outcome(file: &str, source: &str) -> String {
        match parse(source) {
            Ok(parsed) => {
                let mut out = format!("title: {}\n", parsed.title.as_deref().unwrap_or("(none)"));
                for block in &parsed.blocks {
                    let Block::Text { id, html } = block;
                    out.push_str(&format!("\ntext {id}\n{html}\n"));
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
        }
    }

    /// Every shape and every error of a page is a case of the corpus.
    #[test]
    fn the_corpus_of_pages() {
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

            insta::with_settings!({ description => source.clone(), omit_expression => true }, {
                insta::assert_snapshot!(name, outcome(&file, &source));
            });
        }
    }

    #[test]
    fn the_id_is_the_hash_of_the_kind_and_the_source() {
        // printf 'text\n# One' | sha256sum
        assert_eq!(block_id("text", "# One", 1), "5b1eb887548a");
        assert_eq!(ids("# One\n"), ["5b1eb887548a"]);
    }

    #[test]
    fn an_unchanged_block_keeps_its_id_when_the_page_is_pushed_again() {
        let first = "# One\n\nThe first section.\n\n## Two\n\nThe second.\n";
        let second = "# One\n\nThe first section.\n\n## Two\n\nThe second, rewritten.\n\n\
                      ## Three\n\nA new section.\n";

        let before = ids(first);
        let after = ids(second);

        assert_eq!(before, ids(first), "the same source gives the same ids");
        assert_eq!(after.len(), 3);
        assert_eq!(after[0], before[0], "the first section did not change");
        assert_ne!(after[1], before[1], "the second section changed");
    }

    #[test]
    fn two_copies_of_one_block_get_two_ids() {
        let ids = ids("# One\n\n# One\n\n# One\n");

        // printf 'text\n# One' | sha256sum, then with '\n2' and '\n3'
        assert_eq!(ids, ["5b1eb887548a", "300e24ac0238", "7cd404852f2e"]);
    }

    #[test]
    fn the_blank_lines_around_a_block_do_not_change_its_id() {
        assert_eq!(ids("Some text.\n"), ids("\n\nSome text.\n\n\n"));
    }

    #[test]
    fn consecutive_text_forms_one_block_and_a_heading_starts_one() {
        let parsed = parse("One.\n\n- a\n- b\n\nTwo.\n\n# Title\n\nThree.\n").unwrap();

        assert_eq!(parsed.blocks.len(), 2);
        assert_eq!(parsed.title.as_deref(), Some("Title"));
    }

    #[test]
    fn the_title_is_the_text_of_the_first_heading() {
        let parsed = parse("Intro.\n\n## The `term` and *the* leader\n\n# Later\n").unwrap();

        assert_eq!(parsed.title.as_deref(), Some("The term and the leader"));
        assert_eq!(parse("No heading.\n").unwrap().title, None);
    }

    #[test]
    fn a_fence_of_code_inside_a_list_stays_in_the_text() {
        let parsed = parse("- Run it:\n\n  ```sh\n  make check\n  ```\n").unwrap();
        let Block::Text { html, .. } = &parsed.blocks[0];

        assert!(html.contains("<pre class=\"md-code\">"), "{html}");
    }
}
