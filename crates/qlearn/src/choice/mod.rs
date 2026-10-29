//! The choices that the agent asks. See section 7 of the design.
//!
//! A choice takes the place of a dialog of the terminal: the level and the
//! quiz mode at the start, the next step at a checkpoint, the follow-up at
//! the end. The agent writes it in JSON, and the server checks all of it
//! before it stores anything, as it does for a quiz. A choice is never
//! graded: the learner answers it, and the agent reads the answer.
//!
//! The limits are those of a dialog of a terminal: 1 to 4 questions, 2 to
//! 4 options each, and free text beside the options.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::json::{array, object, shape, text, unknown};
use crate::model::{
    self, ChoosingOption, ChoosingQuestion, ChosenAnswer, ChosenLabels, Course, PageKind,
};
use crate::quiz::{Refusal, count_of};

/// How many questions a choice holds.
pub const QUESTIONS: std::ops::RangeInclusive<usize> = 1..=4;

/// How many options a question holds.
pub const OPTIONS: std::ops::RangeInclusive<usize> = 2..=4;

/// A choice as `choices.json` holds it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoredChoosing {
    /// See [`id`].
    pub id: String,
    /// None: the choice shows at the top of the syllabus column.
    pub page: Option<String>,
    pub questions: Vec<ChoosingQuestion>,
    /// When the agent asked it, in RFC 3339.
    pub at: String,
    /// What the learner sent, once sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered: Option<Answered>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Answered {
    /// One per question, in order, each pick list in increasing order.
    pub answers: Vec<ChosenAnswer>,
    /// RFC 3339.
    pub at: String,
}

impl StoredChoosing {
    /// The choice as the page sees it.
    pub fn view(&self) -> model::Choosing {
        model::Choosing {
            id: self.id.clone(),
            page: self.page.clone(),
            questions: self.questions.clone(),
            answers: self.answered.as_ref().map(|sent| sent.answers.clone()),
        }
    }

    /// Take the answer of the learner, at the stamp `at`.
    ///
    /// A choice is answered once: the agent may act on the first answer
    /// before a second one comes.
    pub fn answer(&mut self, answers: &[ChosenAnswer], at: &str) -> Result<(), Refusal> {
        if self.answered.is_some() {
            return Err(Refusal::Conflict(format!(
                "{} was answered already, and a choice is answered once",
                self.id
            )));
        }

        let mut faults = Vec::new();
        if answers.len() != self.questions.len() {
            faults.push(format!(
                "answers: holds {}, and the choice has {}. Answer each question once",
                count_of(answers.len(), "answer"),
                count_of(self.questions.len(), "question")
            ));
        }
        let mut kept = Vec::new();
        for (n, (answer, question)) in answers.iter().zip(&self.questions).enumerate() {
            let path = format!("answers[{n}]");
            let other = answer
                .other
                .as_deref()
                .map(str::trim)
                .filter(|other| !other.is_empty())
                .map(str::to_owned);
            let mut picked = answer.picked.clone();
            picked.sort_unstable();
            let count = question.options.len();
            for index in picked.iter().filter(|index| **index >= count) {
                faults.push(format!(
                    "{path}.picked: {index} is out of range. The question has {}, so a pick \
                     takes 0 to {}",
                    count_of(count, "option"),
                    count - 1
                ));
            }
            if picked.windows(2).any(|pair| pair[0] == pair[1]) {
                faults.push(format!("{path}.picked: picks one option twice"));
            }
            if !question.multi && picked.len() > 1 {
                faults.push(format!(
                    "{path}.picked: holds {}, and the question takes one",
                    count_of(picked.len(), "pick")
                ));
            }
            if picked.is_empty() && other.is_none() {
                faults.push(format!(
                    "{path}: empty. Pick an option, or write an answer of your own"
                ));
            }
            kept.push(ChosenAnswer { picked, other });
        }
        if !faults.is_empty() {
            return Err(Refusal::BadRequest(faults.join("\n")));
        }

        self.answered = Some(Answered {
            answers: kept,
            at: at.to_owned(),
        });
        Ok(())
    }

    /// What the agent needs once the learner answered: the questions as it
    /// wrote them, and the labels of the picks.
    pub fn detail(&self) -> model::Detail {
        let answers = self.answered.as_ref().map_or(&[][..], |sent| &sent.answers);
        model::Detail::Chosen {
            questions: self.questions.clone(),
            answers: answers
                .iter()
                .zip(&self.questions)
                .map(|(answer, question)| ChosenLabels {
                    header: question.header.clone(),
                    picked: answer.picked.clone(),
                    labels: answer
                        .picked
                        .iter()
                        .filter_map(|at| question.options.get(*at))
                        .map(|option| option.label.clone())
                        .collect(),
                    other: answer.other.clone(),
                })
                .collect(),
        }
    }

    /// The rank of the choice in the course, from 1.
    fn rank(&self) -> usize {
        rank_of(&self.id).unwrap_or(0)
    }
}

/// The id of the choice `n` of the course: `choice-<n>`, such as
/// `choice-2`.
///
/// A choice may name no page, so its id holds no page id, as the id of a
/// thread does. Choices are never removed, so the rank stays unique in the
/// course.
pub fn id(n: usize) -> String {
    format!("choice-{n}")
}

/// The rank that a choice id names, or None when it names no choice.
pub fn rank_of(id: &str) -> Option<usize> {
    let n = id.strip_prefix("choice-")?;
    // `+1` and `01` parse too, and name no choice.
    let rank = n
        .parse::<usize>()
        .ok()
        .filter(|rank| rank.to_string() == n)?;
    (rank > 0).then_some(rank)
}

/// The id of the next choice of a course that holds `choices`.
pub fn next_id(choices: &[StoredChoosing]) -> String {
    let last = choices.iter().map(StoredChoosing::rank).max().unwrap_or(0);
    id(last + 1)
}

/// A choice that passed every check, with no id yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    pub page: Option<String>,
    pub questions: Vec<ChoosingQuestion>,
}

/// Read a choice of the agent, or give every fault, each with its place in
/// the body.
///
/// A page that the choice names must have content: the learner sees the
/// choice at the end of that page, and a planned page cannot be opened.
pub fn parse(body: &Value, course: &Course) -> Result<Parsed, Vec<String>> {
    let mut faults = Vec::new();
    let Some(fields) = object(body, "the choice", &mut faults) else {
        return Err(faults);
    };
    for (name, value) in fields {
        match name.as_str() {
            "page" | "questions" => {}
            // The commands of the agent name the writer, as on every route
            // of the agent.
            "author" if value == "agent" => {}
            "author" => faults.push(format!("author: takes \"agent\", not {value}")),
            _ => faults.push(format!(
                "unknown field {name:?}. A choice takes page and questions"
            )),
        }
    }

    let page = read_page(fields.get("page"), course, &mut faults);
    let questions = read_questions(fields, &mut faults);

    match faults.is_empty() {
        true => Ok(Parsed { page, questions }),
        false => Err(faults),
    }
}

fn read_page(value: Option<&Value>, course: &Course, faults: &mut Vec<String>) -> Option<String> {
    let value = value.filter(|value| !value.is_null())?;
    let Some(page) = value.as_str() else {
        faults.push(format!(
            "page: takes a page id or null, not {}",
            shape(value)
        ));
        return None;
    };
    if PageKind::of_id(page).is_none() {
        faults.push(format!(
            "page: {page:?} is not a page id. Name a page of the syllabus, such as c1, or leave \
             page out to ask at the top of the syllabus column"
        ));
        return None;
    }
    if !course.holds(page) {
        faults.push(format!(
            "page: the syllabus holds no page {page}. Name a page of the syllabus, or leave \
             page out to ask at the top of the syllabus column"
        ));
        return None;
    }
    if !course.has_content(page) {
        faults.push(format!(
            "page: page {page} has no content yet, so the learner cannot open it. Push it \
             first with `qlearn page {page}`, or leave page out"
        ));
        return None;
    }
    Some(page.to_owned())
}

fn read_questions(fields: &Map<String, Value>, faults: &mut Vec<String>) -> Vec<ChoosingQuestion> {
    let Some(items) = array(fields, "questions", "", faults) else {
        return Vec::new();
    };
    if !QUESTIONS.contains(&items.len()) {
        faults.push(format!(
            "questions: holds {}, and a choice asks {} to {}",
            count_of(items.len(), "question"),
            QUESTIONS.start(),
            QUESTIONS.end()
        ));
    }
    items
        .iter()
        .enumerate()
        .filter_map(|(at, item)| read_question(item, &format!("questions[{at}]"), faults))
        .collect()
}

fn read_question(value: &Value, path: &str, faults: &mut Vec<String>) -> Option<ChoosingQuestion> {
    let fields = object(value, path, faults)?;
    unknown(
        fields,
        &["header", "prompt", "options", "multi"],
        path,
        "A question takes header, prompt, options and multi",
        faults,
    );
    let header = filled(fields, "header", path, true, faults);
    let prompt = filled(fields, "prompt", path, false, faults);

    let multi = match fields.get("multi") {
        None => Some(false),
        Some(Value::Bool(multi)) => Some(*multi),
        Some(other) => {
            faults.push(format!(
                "{path}.multi: takes true or false, not {}",
                shape(other)
            ));
            None
        }
    };

    let mut options = Vec::new();
    let mut whole = true;
    if let Some(items) = array(fields, "options", path, faults) {
        if !OPTIONS.contains(&items.len()) {
            faults.push(format!(
                "{path}.options: holds {}, and a question takes {} to {}",
                count_of(items.len(), "option"),
                OPTIONS.start(),
                OPTIONS.end()
            ));
        }
        let mut labels = HashSet::new();
        for (at, item) in items.iter().enumerate() {
            let at = format!("{path}.options[{at}]");
            match read_option(item, &at, faults) {
                // The agent reads the answer by its labels, so two of them
                // must differ.
                Some(option) if !labels.insert(option.label.clone()) => {
                    faults.push(format!(
                        "{at}.label: {:?} comes twice in the question",
                        option.label
                    ));
                    whole = false;
                }
                Some(option) => options.push(option),
                None => whole = false,
            }
        }
    }

    whole.then_some(())?;
    Some(ChoosingQuestion {
        header: header?,
        prompt: prompt?,
        options,
        multi: multi?,
    })
}

fn read_option(value: &Value, path: &str, faults: &mut Vec<String>) -> Option<ChoosingOption> {
    let fields = object(value, path, faults)?;
    unknown(
        fields,
        &["label", "description"],
        path,
        "An option takes label and description",
        faults,
    );
    let label = filled(fields, "label", path, true, faults);
    let description = match fields.get("description") {
        None | Some(Value::Null) => Some(String::new()),
        Some(_) => text(fields, "description", path, faults).map(|text| text.trim().to_owned()),
    };
    Some(ChoosingOption {
        label: label?,
        description: description?,
    })
}

/// A string field that holds text, trimmed. A text of spaces is empty. A
/// `short` text, a header or a label, holds one line.
fn filled(
    fields: &Map<String, Value>,
    name: &str,
    path: &str,
    short: bool,
    faults: &mut Vec<String>,
) -> Option<String> {
    let value = text(fields, name, path, faults)?;
    let value = value.trim();
    if value.is_empty() {
        faults.push(format!("{path}.{name}: empty"));
        return None;
    }
    if short && value.contains('\n') {
        faults.push(format!(
            "{path}.{name}: holds a line break. Keep it on one line, and put the rest in the \
             prompt or the description"
        ));
        return None;
    }
    Some(value.to_owned())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use time::OffsetDateTime;

    use super::*;
    use crate::model::{PageRef, PageState, Part};

    /// A course whose syllabus holds pages 1 and c1 with content, and page
    /// 2 still planned.
    fn course() -> Course {
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
                page("c1", PageState::Answered),
                page("2", PageState::Planned),
            ],
        }];
        course
    }

    /// What a case gives, in a form that a reader checks at a glance: the
    /// choice as the page gets it, or each fault, as the agent reads it.
    fn outcome(text: &str) -> String {
        let body: Value = serde_json::from_str(text).expect("a case of the corpus is JSON");

        match parse(&body, &course()) {
            Ok(parsed) => {
                let stored = StoredChoosing {
                    id: id(1),
                    page: parsed.page,
                    questions: parsed.questions,
                    at: "2026-01-01T00:00:00Z".to_owned(),
                    answered: None,
                };
                format!(
                    "the page gets:\n{}\n",
                    serde_json::to_string_pretty(&stored.view()).unwrap()
                )
            }
            Err(faults) => format!("refused\n{}\n", faults.join("\n")),
        }
    }

    /// Every shape and every refusal of a choice is a case of the corpus.
    #[test]
    fn the_corpus_of_choices() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/choices");
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

    #[test]
    fn a_choice_id_names_its_rank() {
        assert_eq!(id(3), "choice-3");
        for (id, rank) in [("choice-1", 1), ("choice-12", 12)] {
            assert_eq!(rank_of(id), Some(rank), "{id}");
        }
        for id in [
            "",
            "choice",
            "choice-",
            "choice-0",
            "choice-01",
            "choice-+1",
            "1",
            "c1",
            "choice-1a",
            "../choice-1",
        ] {
            assert_eq!(rank_of(id), None, "{id:?}");
        }
    }

    fn asked(n: usize, questions: Vec<ChoosingQuestion>) -> StoredChoosing {
        StoredChoosing {
            id: id(n),
            page: None,
            questions,
            at: "2026-01-01T00:00:00Z".to_owned(),
            answered: None,
        }
    }

    #[test]
    fn the_next_choice_takes_the_rank_after_the_last() {
        assert_eq!(next_id(&[]), "choice-1");
        assert_eq!(
            next_id(&[asked(1, Vec::new()), asked(4, Vec::new())]),
            "choice-5"
        );
    }

    fn question(multi: bool) -> ChoosingQuestion {
        ChoosingQuestion {
            header: "Level".to_owned(),
            prompt: "How well do you know Raft?".to_owned(),
            options: ["New to it", "Read the paper", "Wrote one"]
                .into_iter()
                .map(|label| ChoosingOption {
                    label: label.to_owned(),
                    description: String::new(),
                })
                .collect(),
            multi,
        }
    }

    fn answer(picked: &[usize], other: Option<&str>) -> ChosenAnswer {
        ChosenAnswer {
            picked: picked.to_vec(),
            other: other.map(str::to_owned),
        }
    }

    #[test]
    fn an_answer_gives_the_agent_the_labels_of_the_picks() {
        let mut choice = asked(1, vec![question(false), question(true), question(false)]);

        choice
            .answer(
                &[
                    answer(&[1], Some("  ")),
                    answer(&[2, 0], None),
                    answer(&[], Some(" A little, at work ")),
                ],
                "2026-01-01T00:01:00Z",
            )
            .unwrap();

        let model::Detail::Chosen { answers, .. } = choice.detail() else {
            panic!("{:?}", choice.detail());
        };
        assert_eq!(
            answers,
            [
                ChosenLabels {
                    header: "Level".to_owned(),
                    picked: vec![1],
                    labels: vec!["Read the paper".to_owned()],
                    other: None,
                },
                ChosenLabels {
                    header: "Level".to_owned(),
                    picked: vec![0, 2],
                    labels: vec!["New to it".to_owned(), "Wrote one".to_owned()],
                    other: None,
                },
                ChosenLabels {
                    header: "Level".to_owned(),
                    picked: Vec::new(),
                    labels: Vec::new(),
                    other: Some("A little, at work".to_owned()),
                },
            ]
        );
        assert_eq!(
            choice.view().answers.unwrap()[1],
            answer(&[0, 2], None),
            "the picks go in order"
        );
    }

    #[test]
    fn an_answer_that_does_not_fit_gets_every_fault_and_takes_nothing() {
        let mut choice = asked(2, vec![question(false), question(true)]);

        let refused = choice.answer(
            &[
                answer(&[0, 1], None),
                answer(&[3, 1, 1], None),
                answer(&[], Some(" ")),
            ],
            "2026-01-01T00:01:00Z",
        );

        assert_eq!(
            refused,
            Err(Refusal::BadRequest(
                "answers: holds 3 answers, and the choice has 2 questions. Answer each question \
                 once\n\
                 answers[0].picked: holds 2 picks, and the question takes one\n\
                 answers[1].picked: 3 is out of range. The question has 3 options, so a pick \
                 takes 0 to 2\n\
                 answers[1].picked: picks one option twice"
                    .to_owned()
            ))
        );
        assert_eq!(choice.answered, None);

        for answers in [
            vec![],
            vec![answer(&[0], None)],
            vec![answer(&[], None), answer(&[0], None)],
        ] {
            assert!(
                matches!(
                    choice.answer(&answers, "2026-01-01T00:01:00Z"),
                    Err(Refusal::BadRequest(_))
                ),
                "{answers:?}"
            );
        }
        assert_eq!(choice.answered, None);
    }

    #[test]
    fn a_choice_is_answered_once() {
        let mut choice = asked(1, vec![question(false)]);
        choice
            .answer(&[answer(&[0], None)], "2026-01-01T00:01:00Z")
            .unwrap();

        assert!(matches!(
            choice.answer(&[answer(&[1], None)], "2026-01-01T00:02:00Z"),
            Err(Refusal::Conflict(_))
        ));
        assert_eq!(choice.view().answers, Some(vec![answer(&[0], None)]));
    }
}
