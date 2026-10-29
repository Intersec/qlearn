//! The steps block: a state that changes over time, which the learner
//! plays one step at a time.
//!
//! The server checks the whole format, so the page can play every steps
//! block that it gets. See section 4.5 of the design, and the entry "Holes
//! found by the steps" of `stack.md` for the limits.
//!
//! Every fault comes at once, at the line of the fence, with the JSON path
//! of the field, such as `steps[2].rows[0]`.

use std::collections::HashSet;

use serde_json::{Map, Value};

use super::{DIAGRAM_LINES, Fault};
use crate::json::{self, array, at, object, shape, text, unknown};
use crate::model::{Step, Steps};
use crate::quiz::count_of;

const KIND: &str = "steps";

/// Where a text shows, as a fault names it.
const PLACE: &str = "the step view";

/// The most steps of a block. More is a story that the learner loses
/// track of: split it in two blocks, with text between them.
pub const STEPS: usize = 30;

/// The most columns of a table. More do not fit a narrow screen.
pub const COLUMNS: usize = 12;

/// The most rows of a step, so that the table of a step fits a screen and
/// the learner sees each change without a scroll.
pub const ROWS: usize = 40;

/// The most characters of each text, so that it holds on its line of the
/// step view on a narrow screen.
pub const TITLE: usize = 100;
pub const LABEL: usize = 60;
pub const COLUMN: usize = 40;
pub const CELL: usize = 60;

/// The most characters of a body. A step says one thing: the text of the
/// page says the rest.
pub const BODY: usize = 400;

/// Read the JSON of a fence. `fence` is the line of its opening.
pub fn parse(body: &str, fence: usize) -> Result<Steps, Vec<Fault>> {
    let value = super::fence_json(
        body,
        fence,
        KIND,
        "the fence holds no steps: write their JSON inside it",
    )?;

    let mut faults = Vec::new();
    let steps = read(&value, &mut faults);
    match (steps, faults.is_empty()) {
        (Some(steps), true) => Ok(steps),
        _ => Err(faults
            .into_iter()
            .map(|message| Fault {
                line: fence,
                kind: KIND,
                message,
            })
            .collect()),
    }
}

/// The body of each step, in Markdown, from the source of a steps fence
/// that passed the checks. A block holds the bodies as HTML only, and the
/// Markdown export of the cheatsheet needs them as the agent wrote them.
/// None when the source is no such fence.
pub fn bodies(fence: &str) -> Option<Vec<String>> {
    use pulldown_cmark::{Event, Parser};

    let json: String = Parser::new(fence)
        .filter_map(|event| match event {
            Event::Text(text) => Some(text.into_string()),
            _ => None,
        })
        .collect();
    let value: Value = serde_json::from_str(&json).ok()?;
    value
        .get("steps")?
        .as_array()?
        .iter()
        .map(|step| Some(step.get("body")?.as_str()?.trim().to_owned()))
        .collect()
}

fn read(value: &Value, faults: &mut Vec<String>) -> Option<Steps> {
    let fields = object(value, "the steps", faults)?;
    for name in fields.keys() {
        if !["title", "columns", "steps"].contains(&name.as_str()) {
            faults.push(format!(
                "unknown field {name:?}. Steps take title, columns and steps"
            ));
        }
    }

    let title = json::line(fields, "title", "", TITLE, PLACE, faults);
    let columns = read_columns(fields.get("columns"), faults);
    let steps = read_steps(fields, columns.as_deref(), faults);

    // A table needs its head. A block that holds only diagrams has no
    // table. The raw JSON says it, so a fault of a step hides no fault here.
    let has_rows = |item: &Value| item.get("rows").is_some_and(|rows| !rows.is_null());
    if let Some(columns) = &columns
        && columns.is_empty()
        && let Some(Value::Array(items)) = fields.get("steps")
        && items.iter().any(has_rows)
    {
        let found = match fields.get("columns") {
            None | Some(Value::Null) => "missing",
            Some(_) => "empty",
        };
        faults.push(format!(
            "columns: {found}. Name the column of each cell of the rows"
        ));
    }

    Some(Steps {
        title: title?,
        columns: columns?,
        steps: steps?,
    })
}

/// The head of the tables. Empty when the fence gives none.
fn read_columns(value: Option<&Value>, faults: &mut Vec<String>) -> Option<Vec<String>> {
    let items = match value {
        None | Some(Value::Null) => return Some(Vec::new()),
        Some(Value::Array(items)) => items,
        Some(other) => {
            faults.push(format!("columns: takes an array, not {}", shape(other)));
            return None;
        }
    };
    if items.len() > COLUMNS {
        faults.push(format!(
            "columns: holds {}, and a step shows {COLUMNS} at most",
            count_of(items.len(), "column")
        ));
    }

    let mut seen = HashSet::new();
    let mut columns = Some(Vec::new());
    for (n, item) in items.iter().enumerate() {
        let path = format!("columns[{n}]");
        let column = match item {
            Value::String(name) => Some(name.trim().to_owned()),
            other => {
                faults.push(format!("{path}: takes a string, not {}", shape(other)));
                None
            }
        }
        .filter(|name| match name.is_empty() {
            true => {
                faults.push(format!("{path}: empty"));
                false
            }
            false => json::short(name, &path, COLUMN, PLACE, faults),
        })
        // A screen reader names a cell by its column.
        .filter(|name| {
            let first = seen.insert(name.clone());
            if !first {
                faults.push(format!("{path}: {name:?} comes twice in columns"));
            }
            first
        });
        match (column, columns.as_mut()) {
            (Some(column), Some(columns)) => columns.push(column),
            _ => columns = None,
        }
    }
    columns
}

fn read_steps(
    fields: &Map<String, Value>,
    columns: Option<&[String]>,
    faults: &mut Vec<String>,
) -> Option<Vec<Step>> {
    let items = array(fields, "steps", "", faults)?;
    if items.is_empty() {
        faults.push("steps: empty. Give at least one step".to_owned());
        return None;
    }
    if items.len() > STEPS {
        faults.push(format!(
            "steps: holds {} steps, and a steps block plays {STEPS} at most. Split it in two",
            items.len()
        ));
    }

    let mut steps = Some(Vec::new());
    for (n, item) in items.iter().enumerate() {
        let read = read_step(item, &format!("steps[{n}]"), columns, faults);
        match (read, steps.as_mut()) {
            (Some(step), Some(steps)) => steps.push(step),
            _ => steps = None,
        }
    }
    steps
}

fn read_step(
    value: &Value,
    path: &str,
    columns: Option<&[String]>,
    faults: &mut Vec<String>,
) -> Option<Step> {
    let fields = object(value, path, faults)?;
    unknown(
        fields,
        &["label", "body", "rows", "diagram"],
        path,
        "A step takes label, body, and rows or diagram",
        faults,
    );
    let label = json::line(fields, "label", path, LABEL, PLACE, faults);
    let html = read_body(fields, path, faults);

    let given = |name: &str| fields.get(name).is_some_and(|value| !value.is_null());
    let (has_rows, has_diagram) = (given("rows"), given("diagram"));
    let one = match (has_rows, has_diagram) {
        (false, false) => {
            faults.push(format!(
                "{path}: holds neither rows nor a diagram. Give one of them"
            ));
            false
        }
        (true, true) => {
            faults.push(format!(
                "{path}: holds both rows and a diagram. Give one of them"
            ));
            false
        }
        _ => true,
    };
    // Both are read, so that every fault of the step comes at once.
    let rows = has_rows.then(|| read_rows(&fields["rows"], &at(path, "rows"), columns, faults));
    let diagram = has_diagram.then(|| read_diagram(fields, path, faults));

    one.then_some(())?;
    Some(Step {
        label: label?,
        html: html?,
        // A field left out stays None, and a field at fault drops the step.
        rows: rows.map_or(Some(None), |found| found.map(Some))?,
        diagram: diagram.map_or(Some(None), |found| found.map(Some))?,
    })
}

/// The body as HTML: inline Markdown on one line, with the HTML of the
/// source escaped, as an option of a choice.
fn read_body(fields: &Map<String, Value>, path: &str, faults: &mut Vec<String>) -> Option<String> {
    let at = at(path, "body");
    let source = text(fields, "body", path, faults)?;
    if source.trim().is_empty() {
        faults.push(format!("{at}: empty. Say what happens at this step"));
        return None;
    }
    let html = match super::inline(&source) {
        Ok(html) => html,
        Err(error) => {
            faults.push(format!(
                "{at}: {error}. A body is inline Markdown on one line: put a list or a \
                 table in the text of the page"
            ));
            return None;
        }
    };
    let count = source.trim().chars().count();
    if count > BODY {
        faults.push(format!(
            "{at}: holds {count} characters, and a body holds {BODY} at most. Say the rest \
             in the text of the page"
        ));
        return None;
    }
    Some(html)
}

fn read_rows(
    value: &Value,
    path: &str,
    columns: Option<&[String]>,
    faults: &mut Vec<String>,
) -> Option<Vec<Vec<String>>> {
    let Value::Array(items) = value else {
        faults.push(format!("{path}: takes an array, not {}", shape(value)));
        return None;
    };
    if items.is_empty() {
        faults.push(format!(
            "{path}: empty. Give at least one row, or a diagram in place of rows"
        ));
        return None;
    }
    if items.len() > ROWS {
        faults.push(format!(
            "{path}: holds {} rows, and a step shows {ROWS} at most",
            items.len()
        ));
    }

    let mut rows = Some(Vec::new());
    for (r, item) in items.iter().enumerate() {
        let row = read_row(item, &format!("{path}[{r}]"), columns, faults);
        match (row, rows.as_mut()) {
            (Some(row), Some(rows)) => rows.push(row),
            _ => rows = None,
        }
    }
    rows
}

fn read_row(
    value: &Value,
    path: &str,
    columns: Option<&[String]>,
    faults: &mut Vec<String>,
) -> Option<Vec<String>> {
    let Value::Array(items) = value else {
        faults.push(format!(
            "{path}: takes an array of cells, not {}",
            shape(value)
        ));
        return None;
    };
    // An empty head is a fault of its own, said once for the block.
    let mut row = match columns {
        Some(columns) if !columns.is_empty() && items.len() != columns.len() => {
            faults.push(format!(
                "{path}: holds {}, and columns holds {}. Give one cell for each column",
                count_of(items.len(), "cell"),
                columns.len()
            ));
            None
        }
        _ => Some(Vec::new()),
    };
    for (c, item) in items.iter().enumerate() {
        let path = format!("{path}[{c}]");
        // An empty cell is a value too, such as no vote yet.
        let cell = match item {
            Value::String(text) => Some(text.trim().to_owned()),
            Value::Number(number) => Some(number.to_string()),
            other => {
                faults.push(format!(
                    "{path}: takes a string or a number, not {}",
                    shape(other)
                ));
                None
            }
        }
        .filter(|cell| json::short(cell, &path, CELL, PLACE, faults));
        match (cell, row.as_mut()) {
            (Some(cell), Some(row)) => row.push(cell),
            _ => row = None,
        }
    }
    row
}

/// The checks of a diagram that need no Mermaid, as for a `mermaid` fence.
fn read_diagram(
    fields: &Map<String, Value>,
    path: &str,
    faults: &mut Vec<String>,
) -> Option<String> {
    let at = at(path, "diagram");
    let source = text(fields, "diagram", path, faults)?;
    if source.trim().is_empty() {
        faults.push(format!("{at}: empty. Write the Mermaid source of the step"));
        return None;
    }
    let lines = source.lines().count();
    if lines > DIAGRAM_LINES {
        faults.push(format!(
            "{at}: holds {lines} lines, and a diagram has {DIAGRAM_LINES} at most: split it \
             in two"
        ));
        return None;
    }
    Some(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Markdown of each body, as the agent wrote it, for the export of
    /// the cheatsheet.
    #[test]
    fn the_bodies_come_from_the_source_of_the_fence() {
        let fence = "~~~steps\n{ \"steps\": [\n  { \"label\": \"a\", \"body\": \" S1 *wakes*. \" },\n  \
                     { \"label\": \"b\", \"body\": \"`vote` <b>\" } ] }\n~~~";

        assert_eq!(
            bodies(fence),
            Some(vec!["S1 *wakes*.".to_owned(), "`vote` <b>".to_owned()])
        );
        assert_eq!(bodies("Some text."), None);
    }

    fn faults(body: &str) -> Vec<String> {
        parse(body, 10)
            .unwrap_err()
            .into_iter()
            .map(|fault| fault.display("lesson.md"))
            .collect()
    }

    /// A block of `count` steps, each with `rows` rows of `columns` cells.
    fn block(count: usize, columns: usize, rows: usize) -> String {
        let head: Vec<String> = (0..columns).map(|n| format!("\"c{n}\"")).collect();
        let row = format!("[{}]", vec!["\"1\""; columns].join(","));
        let step = format!(
            r#"{{ "label": "t", "body": "b", "rows": [{}] }}"#,
            vec![row; rows].join(",")
        );
        format!(
            r#"{{ "title": "T", "columns": [{}], "steps": [{}] }}"#,
            head.join(","),
            vec![step; count].join(",")
        )
    }

    /// The agent copies the example of the procedure, so it must pass.
    #[test]
    fn the_example_of_the_procedure_passes() {
        let (_, rest) = crate::agent::SKILL.split_once("```steps\n").unwrap();
        let (body, _) = rest.split_once("```").unwrap();

        let steps = parse(body, 1).unwrap();
        assert!(steps.steps.len() > 1);
    }

    #[test]
    fn a_number_in_a_cell_comes_as_its_text() {
        let steps = parse(
            r#"{ "title": "T", "columns": ["Term", "Leader"],
                 "steps": [{ "label": "t", "body": "b", "rows": [[6, ""], [7.5, " S1 "]] }] }"#,
            1,
        )
        .unwrap();

        assert_eq!(
            steps.steps[0].rows,
            Some(vec![
                vec!["6".to_owned(), String::new()],
                vec!["7.5".to_owned(), "S1".to_owned()]
            ])
        );
    }

    #[test]
    fn the_largest_block_passes_and_one_more_of_each_does_not() {
        assert!(parse(&block(STEPS, COLUMNS, ROWS), 1).is_ok());

        assert_eq!(
            faults(&block(STEPS + 1, 1, 1)),
            [
                "lesson.md:10: steps: steps: holds 31 steps, and a steps block plays 30 at \
                 most. Split it in two"
            ]
        );
        assert_eq!(
            faults(&block(1, COLUMNS + 1, 1)),
            ["lesson.md:10: steps: columns: holds 13 columns, and a step shows 12 at most"]
        );
        assert_eq!(
            faults(&block(1, 1, ROWS + 1)),
            ["lesson.md:10: steps: steps[0].rows: holds 41 rows, and a step shows 40 at most"]
        );
    }

    #[test]
    fn a_diagram_of_the_largest_size_passes_and_one_line_more_does_not() {
        let fence = |lines: usize| {
            let source = "A --> B\\n".repeat(lines);
            format!(
                r#"{{ "title": "T", "steps": [{{ "label": "t", "body": "b",
                   "diagram": "{source}" }}] }}"#
            )
        };

        assert!(parse(&fence(DIAGRAM_LINES), 1).is_ok());
        assert_eq!(
            faults(&fence(DIAGRAM_LINES + 1)),
            [
                "lesson.md:10: steps: steps[0].diagram: holds 201 lines, and a diagram has \
                 200 at most: split it in two"
            ]
        );
    }

    #[test]
    fn a_missing_head_comes_with_the_faults_of_another_step() {
        assert_eq!(
            faults(
                r#"{ "title": "T", "steps": [
                     { "label": "t", "body": "b", "rows": [["S1", 6]] },
                     { "label": "", "body": "b", "rows": [["S1", 6]] } ] }"#
            ),
            [
                "lesson.md:10: steps: steps[1].label: empty",
                "lesson.md:10: steps: columns: missing. Name the column of each cell of the rows",
            ]
        );
    }

    #[test]
    fn an_empty_body_asks_for_a_text_and_not_for_a_move() {
        assert_eq!(
            faults(
                r#"{ "title": "T", "steps": [{ "label": "t", "body": " ", "diagram": "graph LR" }] }"#
            ),
            ["lesson.md:10: steps: steps[0].body: empty. Say what happens at this step"]
        );
    }

    #[test]
    fn a_body_of_the_largest_size_passes_and_one_character_more_does_not() {
        let body = |count: usize| {
            format!(
                r#"{{ "title": "T", "steps": [{{ "label": "t", "body": "{}",
                   "diagram": "graph LR" }}] }}"#,
                "a".repeat(count)
            )
        };

        assert!(parse(&body(BODY), 1).is_ok());
        assert_eq!(
            faults(&body(BODY + 1)),
            [
                "lesson.md:10: steps: steps[0].body: holds 401 characters, and a body holds \
                 400 at most. Say the rest in the text of the page"
            ]
        );
    }
}
