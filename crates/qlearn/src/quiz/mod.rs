//! The quiz format. See section 5.1 of the design.
//!
//! The agent writes a quiz in JSON, and the server checks all of it before
//! it stores anything. The body is walked by hand, as a syllabus is, so the
//! agent gets every fault at once, each with its place in the body.
//!
//! The quiz is stored with the right option of each choice. The page gets
//! `model::Quiz`, which has no field for it.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::json::{array, object, shape, text, unknown};
use crate::lesson;
use crate::model::{self, Course, PageKind, Point};

/// The most questions with options that a quiz holds.
pub const CHOICES: usize = 4;

/// How many options a question with options holds.
pub const OPTIONS: std::ops::RangeInclusive<usize> = 2..=4;

/// A quiz as `pages/<id>.json` holds it, with the right option of each
/// choice. The labels of its points live in `points.json`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoredQuiz {
    pub open: Option<StoredOpen>,
    pub choices: Vec<StoredChoice>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoredOpen {
    pub prompt: String,
    /// The points that the answer shows. The agent grades each one.
    pub points: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoredChoice {
    pub header: String,
    pub prompt: String,
    /// Inline Markdown, as the agent wrote it.
    pub options: Vec<String>,
    /// The index of the right option. It never leaves the server before
    /// the learner sends the choices.
    pub answer: usize,
    pub point: String,
}

impl StoredQuiz {
    /// The quiz as the page sees it: no answer, and the options in HTML.
    pub fn view(&self) -> model::Quiz {
        model::Quiz {
            open: self.open.as_ref().map(|open| model::OpenQuestion {
                prompt: open.prompt.clone(),
            }),
            choices: self
                .choices
                .iter()
                .map(|choice| model::Choice {
                    header: choice.header.clone(),
                    prompt: choice.prompt.clone(),
                    // The push checked each option, so the text is the
                    // fallback of a renderer that a later release changed.
                    options: choice
                        .options
                        .iter()
                        .map(|option| {
                            lesson::inline(option).unwrap_or_else(|_| lesson::escape(option))
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// Whether the learner answered the quiz. No route takes an answer in
    /// this release, so no quiz is answered yet.
    pub fn answered(&self) -> bool {
        false
    }
}

/// A quiz that passed every check, and the points that it adds to the
/// course.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    pub quiz: StoredQuiz,
    /// The points that the course did not know, with no result yet.
    pub points: Vec<Point>,
}

/// Read a quiz of the agent for the page `page`, or give every fault.
///
/// `known` holds the points of the course: a question may name one of
/// them with no label.
pub fn parse(
    body: &Value,
    page: &str,
    course: &Course,
    known: &[Point],
) -> Result<Parsed, Vec<String>> {
    let mut faults = Vec::new();
    let Some(fields) = object(body, "the quiz", &mut faults) else {
        return Err(faults);
    };
    for (name, value) in fields {
        match name.as_str() {
            "open" | "choices" | "points" => {}
            // The commands of the agent name the writer, as on every route
            // of the agent.
            "author" if value == "agent" => {}
            "author" => faults.push(format!("author: takes \"agent\", not {value}")),
            _ => faults.push(format!(
                "unknown field {name:?}. A quiz takes open, choices and points"
            )),
        }
    }

    let mut named = Named::default();
    let open = read_open(fields.get("open"), &mut named, &mut faults);
    let choices = read_choices(fields, &mut named, &mut faults);
    let entries = read_entries(fields.get("points"), page, course, &mut faults);

    let no_open = fields.get("open").is_none_or(Value::is_null);
    let no_choices = match fields.get("choices") {
        None => true,
        Some(Value::Array(items)) => items.is_empty(),
        Some(_) => false,
    };
    if no_open && no_choices {
        faults.push(
            "the quiz is empty. Give it an open question, choices with options, or both".to_owned(),
        );
    }

    let points = check_points(&named, entries, known, page, &mut faults);

    match faults.is_empty() {
        true => Ok(Parsed {
            quiz: StoredQuiz { open, choices },
            points,
        }),
        false => Err(faults),
    }
}

/// The points that the questions name, each with the place of its first
/// use, in the order of the body.
#[derive(Default)]
struct Named {
    first: Vec<(String, String)>,
    seen: HashSet<String>,
}

impl Named {
    fn add(&mut self, id: &str, path: String) {
        if self.seen.insert(id.to_owned()) {
            self.first.push((id.to_owned(), path));
        }
    }
}

fn read_open(
    value: Option<&Value>,
    named: &mut Named,
    faults: &mut Vec<String>,
) -> Option<StoredOpen> {
    let value = value.filter(|value| !value.is_null())?;
    let fields = object(value, "open", faults)?;
    unknown(
        fields,
        &["prompt", "points"],
        "open",
        "An open question takes prompt and points",
        faults,
    );
    let prompt = prompt(fields, "prompt", "open", faults);

    let mut points = Vec::new();
    if let Some(items) = array(fields, "points", "open", faults) {
        if items.is_empty() {
            faults.push(
                "open.points: empty. Name the point, or the points, that the answer shows"
                    .to_owned(),
            );
        }
        for (at, item) in items.iter().enumerate() {
            let path = format!("open.points[{at}]");
            let Some(id) = point_id(item, &path, faults) else {
                continue;
            };
            if points.contains(&id) {
                faults.push(format!("{path}: the point {id} comes twice"));
                continue;
            }
            named.add(&id, path);
            points.push(id);
        }
    }

    Some(StoredOpen {
        prompt: prompt?,
        points,
    })
}

fn read_choices(
    fields: &Map<String, Value>,
    named: &mut Named,
    faults: &mut Vec<String>,
) -> Vec<StoredChoice> {
    if !fields.contains_key("choices") {
        return Vec::new();
    }
    let Some(items) = array(fields, "choices", "", faults) else {
        return Vec::new();
    };
    if items.len() > CHOICES {
        faults.push(format!(
            "choices: holds {}, and a quiz holds {CHOICES} at most. Move the others to a \
             second quiz",
            count_of(items.len(), "choice")
        ));
    }

    items
        .iter()
        .enumerate()
        .filter_map(|(at, item)| read_choice(item, &format!("choices[{at}]"), named, faults))
        .collect()
}

fn read_choice(
    value: &Value,
    path: &str,
    named: &mut Named,
    faults: &mut Vec<String>,
) -> Option<StoredChoice> {
    let fields = object(value, path, faults)?;
    unknown(
        fields,
        &["header", "prompt", "options", "answer", "point"],
        path,
        "A choice takes header, prompt, options, answer and point",
        faults,
    );
    let header = prompt(fields, "header", path, faults);
    let prompt = prompt(fields, "prompt", path, faults);

    let mut options = Vec::new();
    let mut count = None;
    if let Some(items) = array(fields, "options", path, faults) {
        count = Some(items.len());
        if !OPTIONS.contains(&items.len()) {
            faults.push(format!(
                "{path}.options: holds {}, and a choice takes {} to {} options",
                count_of(items.len(), "option"),
                OPTIONS.start(),
                OPTIONS.end()
            ));
        }
        for (at, item) in items.iter().enumerate() {
            let at = format!("{path}.options[{at}]");
            let Some(option) = item.as_str() else {
                faults.push(format!("{at}: takes a string, not {}", shape(item)));
                continue;
            };
            if let Err(error) = lesson::inline(option) {
                faults.push(format!(
                    "{at}: {error}. An option is inline Markdown on one line. To open it with \
                     a number or a dash, escape it: 1\\. or \\-"
                ));
                continue;
            }
            options.push(option.to_owned());
        }
    }

    let answer = read_answer(fields.get("answer"), count, path, faults);

    let point = match fields.get("point") {
        Some(value) => point_id(value, &format!("{path}.point"), faults),
        None => {
            faults.push(format!(
                "{path}.point: missing. Name the point that the question checks"
            ));
            None
        }
    };
    if let Some(id) = &point {
        named.add(id, format!("{path}.point"));
    }

    Some(StoredChoice {
        header: header?,
        prompt: prompt?,
        options,
        answer: answer?,
        point: point?,
    })
}

/// The index of the right option. `count` is the number of options, when
/// the body gives a list of them.
fn read_answer(
    value: Option<&Value>,
    count: Option<usize>,
    path: &str,
    faults: &mut Vec<String>,
) -> Option<usize> {
    let at = format!("{path}.answer");
    let Some(value) = value else {
        faults.push(format!(
            "{at}: missing. Give the index of the right option, from 0"
        ));
        return None;
    };
    let index = match value {
        Value::Number(number) if number.is_u64() => number.as_u64(),
        // A negative index is out of range, as one past the end is.
        Value::Number(number) if number.is_i64() => None,
        Value::Number(_) => {
            faults.push(format!("{at}: takes an integer, not {value}"));
            return None;
        }
        other => {
            faults.push(format!("{at}: takes an integer, not {}", shape(other)));
            return None;
        }
    };
    let index = index.and_then(|index| usize::try_from(index).ok());

    match (index, count) {
        (Some(index), Some(count)) if index < count => Some(index),
        // No option at all: the fault of the options says it.
        (_, Some(0)) => None,
        (_, Some(count)) => {
            faults.push(format!(
                "{at}: {value} is out of range. The choice has {}, so answer takes 0 to {}",
                count_of(count, "option"),
                count - 1
            ));
            None
        }
        // No list of options to check against: its own fault says why.
        (index, None) => index,
    }
}

/// A new point, as the `points` of the body gives it.
struct Entry {
    id: String,
    path: String,
    /// None when the label is missing, or has a fault of its own.
    label: Option<String>,
    unlabelled: bool,
    lesson: Option<String>,
}

fn read_entries(
    value: Option<&Value>,
    page: &str,
    course: &Course,
    faults: &mut Vec<String>,
) -> Vec<Entry> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Some(fields) = object(value, "points", faults) else {
        return Vec::new();
    };

    let mut entries = Vec::new();
    for (id, value) in fields {
        let path = format!("points.{id}");
        if !is_point_id(id) {
            faults.push(format!("points: {}", not_a_point_id(id)));
            continue;
        }
        let Some(fields) = object(value, &path, faults) else {
            continue;
        };
        unknown(
            fields,
            &["label", "lesson"],
            &path,
            "A point takes label and lesson",
            faults,
        );
        // A point that the course knows needs no label: the check of a
        // missing one waits for the points of the course.
        let label = match fields.get("label") {
            Some(_) => prompt(fields, "label", &path, faults),
            None => None,
        };
        let lesson = match fields.get("lesson") {
            Some(_) => text(fields, "lesson", &path, faults)
                .filter(|lesson| check_lesson(lesson, &path, page, course, faults)),
            None => None,
        };
        entries.push(Entry {
            id: id.clone(),
            path,
            unlabelled: !fields.contains_key("label"),
            label,
            lesson,
        });
    }
    entries
}

/// Whether `lesson` names a page that the syllabus holds.
fn check_lesson(
    lesson: &str,
    path: &str,
    page: &str,
    course: &Course,
    faults: &mut Vec<String>,
) -> bool {
    if PageKind::of_id(lesson).is_none() {
        faults.push(format!(
            "{path}.lesson: {lesson:?} is not a page id. Name the page that taught the point, \
             such as 3. The page of the quiz, {page}, is the default"
        ));
        return false;
    }
    if !course.holds(lesson) {
        faults.push(format!(
            "{path}.lesson: the syllabus holds no page {lesson}. Name the page that taught \
             the point"
        ));
        return false;
    }
    true
}

/// Match the points that the questions name with the entries of the body
/// and the points of the course. Answers the new points.
fn check_points(
    named: &Named,
    entries: Vec<Entry>,
    known: &[Point],
    page: &str,
    faults: &mut Vec<String>,
) -> Vec<Point> {
    let find = |id: &str| known.iter().find(|point| point.id == id);

    for (id, path) in &named.first {
        let given = entries.iter().any(|entry| entry.id == *id);
        if !given && find(id).is_none() {
            faults.push(format!(
                "{path}: the course does not know the point {id}. Give it a label in points"
            ));
        }
    }

    let mut new = Vec::new();
    for entry in entries {
        if !named.seen.contains(&entry.id) {
            faults.push(format!(
                "{}: no question of the quiz names this point. Name it in a question, or \
                 leave it out",
                entry.path
            ));
            continue;
        }
        if let Some(point) = find(&entry.id) {
            // The same entry again changes nothing. Another one would
            // change the label or the lesson of the results it holds.
            let same = entry
                .label
                .as_deref()
                .is_none_or(|label| label == point.label)
                && entry
                    .lesson
                    .as_deref()
                    .is_none_or(|lesson| lesson == point.lesson);
            if !same {
                faults.push(format!(
                    "{}: the course knows this point already, as {:?} from page {}. Leave it \
                     out of points",
                    entry.path, point.label, point.lesson
                ));
            }
            continue;
        }
        if entry.unlabelled {
            faults.push(format!(
                "{}.label: missing. A new point needs a label, such as \"How a split vote \
                 ends\"",
                entry.path
            ));
        }
        if let Some(label) = entry.label {
            new.push(Point {
                id: entry.id,
                label,
                lesson: entry.lesson.unwrap_or_else(|| page.to_owned()),
                results: Vec::new(),
            });
        }
    }
    new
}

/// A string field that holds text, trimmed. A text of spaces is empty.
fn prompt(
    fields: &Map<String, Value>,
    name: &str,
    path: &str,
    faults: &mut Vec<String>,
) -> Option<String> {
    let value = text(fields, name, path, faults)?;
    let value = value.trim();
    if value.is_empty() {
        faults.push(format!("{path}.{name}: empty"));
        return None;
    }
    Some(value.to_owned())
}

/// The id of a point, as a question names it.
fn point_id(value: &Value, path: &str, faults: &mut Vec<String>) -> Option<String> {
    let Some(id) = value.as_str() else {
        faults.push(format!("{path}: takes a string, not {}", shape(value)));
        return None;
    };
    if !is_point_id(id) {
        faults.push(format!("{path}: {}", not_a_point_id(id)));
        return None;
    }
    Some(id.to_owned())
}

/// "1 option", "3 options".
fn count_of(count: usize, noun: &str) -> String {
    match count {
        1 => format!("1 {noun}"),
        _ => format!("{count} {noun}s"),
    }
}

/// A point id goes into `--point <id>=ok` on the command line, so it holds
/// no space and no `=`.
fn is_point_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn not_a_point_id(id: &str) -> String {
    format!(
        "{id:?} is not a point id. Use lowercase letters, digits and hyphens, such as \
         split-vote"
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::model::{PageRef, PageState, Part};
    use time::OffsetDateTime;

    /// A course whose syllabus holds pages 1 to 3 and c1, and that knows
    /// the point `random-timeout`, taught by page 2.
    fn course() -> (Course, Vec<Point>) {
        let mut course = Course::new(
            "raft",
            OffsetDateTime::from_unix_timestamp(1_767_225_600).unwrap(),
        );
        let page = |id: &str, state| PageRef {
            id: id.to_owned(),
            kind: PageKind::of_id(id).unwrap(),
            title: format!("Page {id}"),
            state,
            score: None,
        };
        course.parts = vec![Part {
            title: "Why".to_owned(),
            pages: vec![
                page("1", PageState::Taught),
                page("2", PageState::Taught),
                page("3", PageState::Taught),
                page("c1", PageState::Planned),
            ],
        }];
        let known = vec![Point {
            id: "random-timeout".to_owned(),
            label: "Why the election timeout is random".to_owned(),
            lesson: "2".to_owned(),
            results: Vec::new(),
        }];
        (course, known)
    }

    /// What a case gives, in a form that a reader checks at a glance: the
    /// quiz as stored, the new points, and the quiz as the page gets it.
    /// Or each fault, as the agent reads it.
    fn outcome(text: &str) -> String {
        let (course, known) = course();
        let body: Value = serde_json::from_str(text).expect("a case of the corpus is JSON");

        match parse(&body, "3", &course, &known) {
            Ok(parsed) => format!(
                "stored:\n{}\n\nnew points:\n{}\n\nthe page gets:\n{}\n",
                serde_json::to_string_pretty(&parsed.quiz).unwrap(),
                serde_json::to_string_pretty(&parsed.points).unwrap(),
                serde_json::to_string_pretty(&parsed.quiz.view()).unwrap(),
            ),
            Err(faults) => format!("refused\n{}\n", faults.join("\n")),
        }
    }

    /// Every shape and every refusal of a quiz is a case of the corpus.
    #[test]
    fn the_corpus_of_quizzes() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/quizzes");
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        files.sort();
        assert!(!files.is_empty(), "no case in {}", dir.display());

        for path in files {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).unwrap();
            let out = outcome(&text);

            // A case named refused-* must be refused, and no other.
            assert_eq!(
                out.starts_with("refused"),
                name.starts_with("refused-"),
                "{name}: {out}"
            );
            insta::with_settings!({ description => text.clone(), omit_expression => true }, {
                insta::assert_snapshot!(name, out);
            });
        }
    }

    /// The page must never get the right option before the learner sends
    /// the choices: the view has no field for it, whatever the quiz holds.
    #[test]
    fn the_view_of_a_quiz_carries_no_answer() {
        let (course, known) = course();
        let body = serde_json::json!({
            "choices": [{ "header": "Vote", "prompt": "Who wins?",
                          "options": ["S1", "S2"], "answer": 1, "point": "random-timeout" }],
        });

        let parsed = parse(&body, "3", &course, &known).unwrap();
        let view = serde_json::to_value(parsed.quiz.view()).unwrap();

        assert_eq!(parsed.quiz.choices[0].answer, 1);
        assert!(!view.to_string().contains("answer"), "{view}");
        assert!(parsed.points.is_empty(), "the course knows the point");
    }
}
