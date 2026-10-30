//! The cheatsheet as Markdown, for a wiki. See section 9 of the design,
//! and the entry "Holes found by the Markdown export" of `stack.md` for
//! the form of each block.
//!
//! The Markdown is GitHub-flavored, and comes from the stored blocks: an
//! excerpt keeps the lines and the commit of the write, and a line that a
//! page still hides stays hidden. A text, a callout and the body of a step
//! are stored as HTML only, so their Markdown comes from the source of the
//! agent, found by the id of the block. That split reads no repository, so
//! no `rev` moves.

use anyhow::{Result, anyhow};

use crate::lesson::{self, Within};
use crate::model::{Block, Chart, Cheatsheet, CodeRange, Excerpt, Grade, Marker, Revisit, Steps};

/// What a line that a page still hides shows in place of its text. It
/// gives no length away, as the blank of the page.
pub const HIDDEN: &str = "▒▒▒ hidden line ▒▒▒";

/// The cheatsheet, then Map of the code and To revisit, as one Markdown
/// document. `source` is the source that the agent wrote with the blocks.
pub fn render(sheet: &Cheatsheet, source: &str) -> Result<String> {
    let cut = lesson::cut(source, Within::Cheatsheet);
    let source_of =
        |block: &Block| {
            cut.blocks
            .get(block.id()).map(String::as_str).ok_or_else(|| {
            // The source and the blocks are written together, so a block
            // with no source is a file changed by hand.
            anyhow!(
                "the source of the cheatsheet holds no block {}. Ask the agent to write the \
                 cheatsheet again with `qlearn cheatsheet`",
                block.id()
            )
        })
        };

    let mut parts = Vec::new();
    for block in &sheet.blocks {
        parts.push(match block {
            Block::Text { .. } => source_of(block)?.to_owned(),
            Block::Callout { marker, .. } => callout(*marker, source_of(block)?),
            Block::Excerpt { excerpt, .. } => excerpt_of(excerpt),
            Block::Diagram { source, .. } => fence("mermaid", source),
            Block::Chart { chart, .. } => chart_of(chart),
            Block::Steps { steps, .. } => {
                let bodies = lesson::steps::bodies(source_of(block)?)
                    .ok_or_else(|| anyhow!("block {} holds no steps", block.id()))?;
                steps_of(steps, &bodies)
            }
            Block::Image {
                alt,
                path,
                repo,
                commit,
                ..
            } => {
                // An image of the notes has no commit: the page shows its
                // path alone.
                let at = match (repo, commit) {
                    (Some(repo), Some(commit)) => place(path, None, repo, commit),
                    _ => code(path),
                };
                format!("**Image**: {} ({at})", plain(alt))
            }
        });
    }
    // A text or a callout may name a definition that no block holds.
    if !cut.definitions.is_empty() {
        parts.push(cut.definitions.join("\n"));
    }
    parts.push(map_of(&sheet.map));
    parts.push(revisit_of(&sheet.revisit));

    let mut out = parts.join("\n\n");
    out.push('\n');
    Ok(out)
}

/// A quote with the marker of the page as a bold label: every wiki draws a
/// quote, where few know the alerts of GitHub, and none knows a hook.
fn callout(marker: Marker, source: &str) -> String {
    let label = match marker {
        Marker::Hook => "🧠 Hook",
        Marker::Teaser => "🔭 Teaser",
        Marker::Pitfall => "⚠️ Pitfall",
        Marker::Note => "ℹ️ Note",
    };
    // The parser found the marker as the first text of the quote.
    let Some((open, close)) = source
        .find("[!")
        .and_then(|open| Some((open, open + source[open..].find(']')?)))
    else {
        return source.to_owned();
    };
    let rest = &source[close + 1..];
    let space = match rest.starts_with(char::is_whitespace) {
        true => "",
        false => " ",
    };
    format!("{}**{label}.**{space}{rest}", &source[..open])
}

/// The lines in a fence of their language, under the place that they come
/// from.
fn excerpt_of(excerpt: &Excerpt) -> String {
    let lines: Vec<&str> = excerpt
        .lines
        .iter()
        .map(|line| line.as_ref().map_or(HIDDEN, |line| line.text.as_str()))
        .collect();
    let mut head = place(
        &excerpt.path,
        Some((excerpt.from, excerpt.to)),
        &excerpt.repo,
        &excerpt.commit,
    );
    // A fence has no marks, so the head names the lines.
    let mut marked = excerpt.marked.clone();
    marked.sort_unstable();
    marked.dedup();
    match marked.as_slice() {
        [] => {}
        [one] => head.push_str(&format!(", line {one} marked")),
        _ => head.push_str(&format!(", lines {} marked", ranges(&marked))),
    }

    format!("{head}\n\n{}", fence(&excerpt.lang, &lines.join("\n")))
}

/// `path:from-to at <commit>`, and the submodule that holds the file.
fn place(path: &str, lines: Option<(u32, u32)>, repo: &str, commit: &str) -> String {
    let at = match lines {
        Some((from, to)) if from == to => format!("{path}:{from}"),
        Some((from, to)) => format!("{path}:{from}-{to}"),
        None => path.to_owned(),
    };
    // The length that the page shows.
    let short = commit.get(..7).unwrap_or(commit);
    let mut out = format!("{} at {}", code(&at), code(short));
    if !repo.is_empty() {
        out.push_str(&format!(" in submodule {}", code(repo)));
    }
    out
}

/// Sorted line numbers as `3, 6-8`.
fn ranges(lines: &[u32]) -> String {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for &line in lines {
        match out.last_mut() {
            Some((_, end)) if *end + 1 == line => *end = line,
            _ => out.push((line, line)),
        }
    }
    out.iter()
        .map(|(from, to)| match from == to {
            true => from.to_string(),
            false => format!("{from}-{to}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A table: the x values as rows, a column for each series. A gap of a
/// line chart is an empty cell.
fn chart_of(chart: &Chart) -> String {
    let mut out = format!("**{}**\n\n", plain(&chart.title));
    // The axis of the values, which no head of the table names.
    match chart.y.unit.is_empty() {
        true => out.push_str(&format!("Values: {}.\n\n", plain(&chart.y.label))),
        false => out.push_str(&format!(
            "Values: {}, in {}.\n\n",
            plain(&chart.y.label),
            plain(&chart.y.unit)
        )),
    }

    let mut head = vec![plain(&chart.x.label)];
    head.extend(chart.series.iter().map(|series| plain(&series.name)));
    let rows: Vec<Vec<String>> = chart
        .x
        .values
        .iter()
        .enumerate()
        .map(|(at, x)| {
            let mut row = vec![plain(x)];
            row.extend(chart.series.iter().map(|series| {
                series
                    .values
                    .get(at)
                    .and_then(Option::as_ref)
                    .map_or(String::new(), ToString::to_string)
            }));
            row
        })
        .collect();
    // The numbers align to the right, as in a spreadsheet.
    let mut align = vec!["---"];
    align.extend(chart.series.iter().map(|_| "---:"));
    out.push_str(&table(&head, &align, &rows));
    out
}

/// A numbered list: each step with its label, its body, and its table or
/// its diagram.
fn steps_of(steps: &Steps, bodies: &[String]) -> String {
    let mut out = String::new();
    if !steps.title.is_empty() {
        out.push_str(&format!("**{}**\n\n", plain(&steps.title)));
    }
    let head: Vec<String> = steps.columns.iter().map(|column| plain(column)).collect();
    let align = vec!["---"; head.len()];

    let items: Vec<String> = steps
        .steps
        .iter()
        .enumerate()
        .map(|(at, step)| {
            let number = format!("{}. ", at + 1);
            let body = bodies.get(at).map_or("", String::as_str);
            let mut item = format!("**{}**: {body}", plain(&step.label));
            let state = match (&step.rows, &step.diagram) {
                (Some(rows), _) => {
                    let rows: Vec<Vec<String>> = rows
                        .iter()
                        .map(|row| row.iter().map(|cell| plain(cell)).collect())
                        .collect();
                    Some(table(&head, &align, &rows))
                }
                (None, Some(diagram)) => Some(fence("mermaid", diagram)),
                (None, None) => None,
            };
            if let Some(state) = state {
                item.push_str("\n\n");
                item.push_str(&state);
            }
            // The lines after the first one belong to the item.
            let pad = " ".repeat(number.len());
            let item = item
                .lines()
                .map(|line| match line.is_empty() {
                    true => String::new(),
                    false => format!("{pad}{line}"),
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!("{number}{}", &item[pad.len()..])
        })
        .collect();

    out.push_str(&items.join("\n\n"));
    out
}

fn map_of(map: &[CodeRange]) -> String {
    let mut out = "## Map of the code\n\n".to_owned();
    if map.is_empty() {
        out.push_str("The course showed no code.");
        return out;
    }
    let items: Vec<String> = map
        .iter()
        .map(|range| {
            let lessons: Vec<String> = range
                .lessons
                .iter()
                .map(|page| format!("{}, page {}", plain(&page.title), plain(&page.id)))
                .collect();
            format!(
                "- {} ({})",
                place(
                    &range.path,
                    Some((range.from, range.to)),
                    &range.repo,
                    &range.commit
                ),
                lessons.join("; ")
            )
        })
        .collect();
    out.push_str(&items.join("\n"));
    out
}

fn revisit_of(revisit: &[Revisit]) -> String {
    let mut out = "## To revisit\n\n".to_owned();
    if revisit.is_empty() {
        out.push_str("Nothing to revisit.");
        return out;
    }
    let items: Vec<String> = revisit
        .iter()
        .map(|point| {
            let result = match point.result {
                Grade::Ok => "Right",
                Grade::Partial => "Partial",
                Grade::Missed => "Missed",
            };
            let lesson = match &point.title {
                Some(title) => format!("{}, page {}", plain(title), plain(&point.lesson)),
                None => format!("page {}", plain(&point.lesson)),
            };
            format!("- **{result}**: {} ({lesson})", plain(&point.label))
        })
        .collect();
    out.push_str(&items.join("\n"));
    out
}

/// A table of GitHub. Each cell is Markdown on one line, with `|` escaped.
fn table(head: &[String], align: &[&str], rows: &[Vec<String>]) -> String {
    let line = |cells: &[String]| format!("| {} |", cells.join(" | "));
    let mut out = vec![line(head), format!("| {} |", align.join(" | "))];
    out.extend(rows.iter().map(|row| line(row)));
    out.join("\n")
}

/// A fence that no line of `body` can close: longer than its longest run
/// of backticks.
fn fence(info: &str, body: &str) -> String {
    let ticks = "`".repeat(longest_run(body, '`').max(2) + 1);
    let body = body.trim_end_matches('\n');
    format!("{ticks}{info}\n{body}\n{ticks}")
}

/// A code span that holds `text` whole, whatever backticks it holds.
fn code(text: &str) -> String {
    let ticks = "`".repeat(longest_run(text, '`') + 1);
    let pad = match text.starts_with('`') || text.ends_with('`') {
        true => " ",
        false => "",
    };
    format!("{ticks}{pad}{text}{pad}{ticks}")
}

fn longest_run(text: &str, of: char) -> usize {
    text.split(|c| c != of).map(str::len).max().unwrap_or(0)
}

/// Plain text, such as a title or a cell, as Markdown that shows it as it
/// is: the page draws these as text, never as Markdown.
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|' | '&' | '~' => {
                out.push('\\');
                out.push(c);
            }
            '\n' | '\r' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::exec::Git;
    use crate::highlight::Highlighter;
    use crate::model::PageName;
    use crate::testutil::{PNG, SVG, build_repo, commit};

    /// A cheatsheet with a block of each kind. The line `return` of
    /// `src/raft.c` shows in it, and a page still hides it.
    const SHEET: &str = r#"# Raft in one page

The random timeout breaks the tie. A `|` in code, and <b>raw HTML</b>.

| Name | What |
|---|---|
| `election_timeout` | the wait of a follower |

> [!hook]
> The first follower to wake up wins.

>[!Pitfall] A `base` of 0 divides by zero.
>
> Check it.

```excerpt
path: src/raft.c
lines: 2-4
mark: 2, 3
```

```excerpt
path: vendor/lib/src/lib.c
lines: 2
```

```mermaid
graph LR
  F[Follower] --> C[Candidate]
```

```chart
{
  "kind": "bar",
  "title": "Election time, by timeout range",
  "x": { "label": "Timeout range (ms)", "values": ["150-300", "150-151"] },
  "y": { "label": "Median election time", "unit": "ms" },
  "series": [{ "name": "5 servers", "values": [283, 1520] }]
}
```

```chart
{
  "kind": "line",
  "title": "The term of each server",
  "x": { "label": "Time (s)", "values": [0, 1, 2] },
  "y": { "label": "Term" },
  "series": [
    { "name": "S1", "values": [1, null, 3] },
    { "name": "S|2", "values": [1, 2, 3] }
  ]
}
```

```steps
{
  "title": "From follower to leader",
  "columns": ["Server", "State"],
  "steps": [
    { "label": "Before", "body": "Every server is a *follower*.",
      "rows": [["S1", "follower"], ["S2", "follower"]] },
    { "label": "The vote", "body": "S1 asks S2 for its `vote`.",
      "diagram": "sequenceDiagram\n  S1->>S2: RequestVote\n" }
  ]
}
```

![The states of a server](docs/states.png)

![The magic ring, drawn by the agent](.learn/raft/notes/ring.svg)
"#;

    const RAFT: &str = "/* The election timer. */\n\
                        int election_timeout(int base)\n\
                        {\n\
                        \x20   return base + rand() % base;\n\
                        }\n";

    #[tokio::test]
    async fn the_markdown_holds_every_block_and_both_sections() {
        let lib = build_repo(&[commit("lib").file(
            "src/lib.c",
            "/* A list of the servers. */\nstruct server *servers;\n",
        )])
        .await;
        let top = build_repo(&[commit("raft")
            .file("src/raft.c", RAFT)
            .bytes("docs/states.png", PNG)
            .submodule("vendor/lib", &lib)])
        .await;
        let git = Git::discover(top.path()).await.unwrap();
        let notes = crate::lesson::Notes::of(&crate::store::Store::at(top.path()), "raft");
        std::fs::create_dir_all(&notes.dir).unwrap();
        std::fs::write(notes.dir.join("ring.svg"), SVG).unwrap();
        let mut blocks =
            crate::lesson::parse_cheatsheet(SHEET, Some(&git), &notes, Highlighter::shared())
                .await
                .unwrap()
                .blocks;
        // As `build` does while the quiz of the page that hides it waits.
        let Some(Block::Excerpt { excerpt, .. }) = blocks
            .iter_mut()
            .find(|block| matches!(block, Block::Excerpt { .. }))
        else {
            panic!("{blocks:?}");
        };
        excerpt.lines[2] = None;
        let (top_head, lib_head) = (top.sha("HEAD").await, lib.sha("HEAD").await);
        let page = |id: &str, title: &str| PageName {
            id: id.to_owned(),
            title: title.to_owned(),
        };
        let sheet = Cheatsheet {
            written: true,
            blocks,
            map: vec![
                CodeRange {
                    path: "src/raft.c".to_owned(),
                    repo: String::new(),
                    commit: top_head.clone(),
                    from: 1,
                    to: 4,
                    lessons: vec![page("1", "The timer"), page("c1", "The *vote*")],
                },
                CodeRange {
                    path: "vendor/lib/src/lib.c".to_owned(),
                    repo: "vendor/lib".to_owned(),
                    commit: lib_head.clone(),
                    from: 2,
                    to: 2,
                    lessons: vec![page("2", "The servers")],
                },
            ],
            revisit: vec![
                Revisit {
                    point: "split-vote".to_owned(),
                    label: "How a split vote ends".to_owned(),
                    result: Grade::Missed,
                    lesson: "1".to_owned(),
                    title: Some("The timer".to_owned()),
                },
                Revisit {
                    point: "majority".to_owned(),
                    label: "How many votes win".to_owned(),
                    result: Grade::Partial,
                    lesson: "9".to_owned(),
                    title: None,
                },
            ],
        };

        let text = render(&sheet, SHEET).unwrap();

        assert!(!text.contains("return base"), "the hidden line shows");
        // The superproject names the path of the submodule, which changes
        // at every run.
        let text = text
            .replace(&top_head[..7], "<top>")
            .replace(&lib_head[..7], "<lib>");
        insta::assert_snapshot!("markdown", text);
    }

    #[test]
    fn an_empty_section_says_so() {
        let sheet = Cheatsheet {
            written: true,
            blocks: Vec::new(),
            map: Vec::new(),
            revisit: Vec::new(),
        };

        assert_eq!(
            render(&sheet, "").unwrap(),
            "## Map of the code\n\nThe course showed no code.\n\n\
             ## To revisit\n\nNothing to revisit.\n"
        );
    }

    /// A block whose source is gone was changed by hand: the export fails
    /// rather than drop the block.
    #[test]
    fn a_block_that_the_source_does_not_hold_fails_the_export() {
        let sheet = Cheatsheet {
            written: true,
            blocks: vec![Block::Text {
                id: "0123456789ab".to_owned(),
                html: "<p>One.</p>".to_owned(),
            }],
            map: Vec::new(),
            revisit: Vec::new(),
        };

        let error = render(&sheet, "Two.\n").unwrap_err().to_string();

        assert!(error.contains("0123456789ab"), "{error}");
        assert!(error.contains("qlearn cheatsheet"), "{error}");
    }

    /// The text and diagram blocks of `source`, as the write stores them.
    fn sheet_of(source: &str) -> Cheatsheet {
        let blocks = lesson::sources(source, Within::Cheatsheet)
            .into_iter()
            .map(|block| match block.kind.as_str() {
                "text" => Block::Text {
                    id: block.id,
                    html: String::new(),
                },
                "diagram" => Block::Diagram {
                    id: block.id,
                    source: "graph LR\n  A --> B".to_owned(),
                },
                kind => panic!("{kind}"),
            })
            .collect();
        Cheatsheet {
            written: true,
            blocks,
            map: Vec::new(),
            revisit: Vec::new(),
        }
    }

    /// The parser puts the start of an indented code block after its
    /// indent, which the code block needs.
    #[test]
    fn an_indented_code_block_keeps_its_indent() {
        let source = "```mermaid\ngraph LR\n  A --> B\n```\n\n    int x = 1;\n    int y = 2;\n";

        let text = render(&sheet_of(source), source).unwrap();

        assert!(
            text.contains("```\n\n    int x = 1;\n    int y = 2;\n\n## Map"),
            "{text}"
        );
    }

    /// A definition gives no event, so the end of the source is outside
    /// every block.
    #[test]
    fn a_reference_link_keeps_its_definition() {
        let source = "Read [the paper][raft], then [the thesis][thesis].\n\n\
                      [thesis]: https://example.org/thesis.pdf\n\n\
                      More text.\n\n\
                      ```mermaid\ngraph LR\n  A --> B\n```\n\n\
                      [raft]: https://raft.github.io/raft.pdf \"Raft\"\n";

        let text = render(&sheet_of(source), source).unwrap();

        assert!(
            text.contains("```\n\n[raft]: https://raft.github.io/raft.pdf \"Raft\"\n\n## Map"),
            "{text}"
        );
        assert_eq!(text.matches("[thesis]: ").count(), 1, "{text}");
    }

    #[test]
    fn a_fence_and_a_code_span_hold_their_backticks() {
        assert_eq!(fence("c", "a\n````\nb\n"), "`````c\na\n````\nb\n`````");
        assert_eq!(fence("", "x"), "```\nx\n```");
        assert_eq!(code("a`b"), "``a`b``");
        assert_eq!(code("`a"), "`` `a ``");
        // Outside a table, a backslash shows in a code span.
        assert_eq!(code("a|b"), "`a|b`");
    }

    #[test]
    fn a_callout_puts_its_label_in_place_of_its_marker() {
        assert_eq!(
            callout(Marker::Note, "> [!note]\n> Read it."),
            "> **ℹ️ Note.**\n> Read it."
        );
        assert_eq!(
            callout(Marker::Teaser, ">[!TEASER]Later."),
            ">**🔭 Teaser.** Later."
        );
    }
}
