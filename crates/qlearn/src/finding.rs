//! The findings of a course. See section 3 of the design.
//!
//! A course on real code finds real problems: a race, a check that is
//! missing, a comment that says the wrong thing. A finding records one, for
//! the learner to check with the team. The agent writes it with
//! `qlearn finding`, and writes it again by its id to change it, such as
//! once the team confirms it. The learner reads the findings and edits
//! none.
//!
//! `course.json` keeps the body in Markdown, as the agent wrote it: the
//! page gets the HTML of the server, and the agent gets the Markdown back.

use serde_json::Value;

use crate::json::{line, object, shape, text};
use crate::model::Finding;

/// The longest id. It names the finding in a command, so it stays short.
pub const ID_MOST: usize = 40;

/// The longest title. It shows on one line of the syllabus column.
pub const TITLE_MOST: usize = 120;

/// The finding that the agent sent, as the store keeps it, or every
/// fault at once, each with its field. The id is in the path.
pub fn check(id: &str, body: &Value) -> Result<Finding, Vec<String>> {
    let mut faults = Vec::new();
    if !is_id(id) {
        faults.push(format!(
            "id: {id:?} is not a finding id. Use lowercase words of letters and digits, \
             joined by single hyphens, {ID_MOST} characters at most, such as seed-race"
        ));
    }
    let Some(fields) = object(body, "the finding", &mut faults) else {
        return Err(faults);
    };
    for (name, value) in fields {
        match name.as_str() {
            "title" | "body" | "checked" => {}
            // The commands of the agent name the writer, as on every route
            // of the agent.
            "author" if value == "agent" => {}
            "author" => faults.push(format!("author: takes \"agent\", not {value}")),
            _ => faults.push(format!(
                "unknown field {name:?}. A finding takes title, body and checked"
            )),
        }
    }
    let title = line(
        fields,
        "title",
        "",
        TITLE_MOST,
        "a line of the findings",
        &mut faults,
    );
    let body = text(fields, "body", "", &mut faults);
    // A finding is written whole, so one written again with no `checked`
    // is to check again.
    let checked = match fields.get("checked") {
        None => Some(false),
        Some(Value::Bool(checked)) => Some(*checked),
        Some(other) => {
            faults.push(format!("checked: takes a boolean, not {}", shape(other)));
            None
        }
    };

    match (title, body, checked) {
        (Some(title), Some(body), Some(checked)) if faults.is_empty() => Ok(Finding {
            id: id.to_owned(),
            title,
            body: body.trim().to_owned(),
            checked,
        }),
        _ => Err(faults),
    }
}

/// Whether `id` names a finding: `[a-z0-9]` words joined by single
/// hyphens. A hyphen at an end or twice in a row is most often a typo of
/// the agent, which would write a second finding.
fn is_id(id: &str) -> bool {
    id.len() <= ID_MOST
        && id.split('-').all(|word| {
            !word.is_empty()
                && word
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// Put `finding` in `findings`: in place of the one with its id, so the
/// list keeps the order of their first write, or at the end.
pub fn write(findings: &mut Vec<Finding>, finding: Finding) {
    match findings.iter_mut().find(|held| held.id == finding.id) {
        Some(held) => *held = finding,
        None => findings.push(finding),
    }
}

/// The finding as the page sees it: the body in the HTML of the server,
/// with the HTML of the source escaped as in a page.
pub fn view(finding: &Finding) -> Finding {
    Finding {
        body: crate::lesson::markdown(&finding.body),
        ..finding.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sent(title: &str, body: &str, checked: bool) -> Value {
        serde_json::json!({ "title": title, "body": body, "checked": checked })
    }

    #[test]
    fn an_id_is_lowercase_words_joined_by_hyphens() {
        for id in ["vote-race", "a", "f1", "2-timers", &"a".repeat(ID_MOST)] {
            assert!(is_id(id), "{id}");
        }
        for id in [
            "",
            "-race",
            "race-",
            "vote--race",
            "Vote",
            "vote_race",
            "vote race",
            "../race",
            "é",
            &"a".repeat(ID_MOST + 1),
        ] {
            assert!(!is_id(id), "{id:?}");
        }
    }

    #[test]
    fn a_finding_is_trimmed_and_keeps_its_markdown() {
        let finding = check("vote-race", &sent("  A race  ", "\n`vote()` <b>\n\n", true)).unwrap();

        assert_eq!(
            finding,
            Finding {
                id: "vote-race".to_owned(),
                title: "A race".to_owned(),
                body: "`vote()` <b>".to_owned(),
                checked: true,
            }
        );
    }

    #[test]
    fn every_fault_comes_at_once() {
        let faults = check("Bad id", &sent(" \n", "", false)).unwrap_err();
        assert_eq!(faults.len(), 2, "{faults:?}");
        assert!(faults[0].starts_with("id: \"Bad id\" is not a finding id"));
        assert_eq!(faults[1], "title: empty");

        let long = "a".repeat(TITLE_MOST + 1);
        let faults = check("race", &sent(&long, "", false)).unwrap_err();
        assert_eq!(
            faults,
            [format!(
                "title: holds {} characters, and {TITLE_MOST} at most fit on a line of the \
                 findings",
                TITLE_MOST + 1
            )]
        );
        let faults = check("race", &sent("Two\nlines", "", false)).unwrap_err();
        assert!(faults[0].contains("line break"), "{faults:?}");

        // A field of the wrong shape does not hide the others.
        let body = serde_json::json!({ "title": 1, "checked": "yes", "author": "learner",
                                       "state": "open" });
        let mut faults = check("Bad id", &body).unwrap_err();
        assert!(
            faults
                .remove(0)
                .starts_with("id: \"Bad id\" is not a finding id")
        );
        faults.sort();
        assert_eq!(
            faults,
            [
                "author: takes \"agent\", not \"learner\"",
                "body: missing",
                "checked: takes a boolean, not a string",
                "title: takes a string, not a number",
                "unknown field \"state\". A finding takes title, body and checked",
            ]
        );
        let faults = check("race", &serde_json::json!([])).unwrap_err();
        assert_eq!(faults, ["the finding: takes an object, not an array"]);
    }

    #[test]
    fn a_finding_written_again_keeps_its_place() {
        let finding = |id: &str, title: &str| Finding {
            id: id.to_owned(),
            title: title.to_owned(),
            body: String::new(),
            checked: false,
        };
        let mut findings = vec![finding("a", "A"), finding("b", "B")];

        write(&mut findings, finding("a", "A again"));
        write(&mut findings, finding("c", "C"));

        assert_eq!(
            findings,
            [
                finding("a", "A again"),
                finding("b", "B"),
                finding("c", "C")
            ]
        );
    }

    #[test]
    fn the_page_gets_the_body_as_html_with_the_html_of_the_source_escaped() {
        let finding = Finding {
            id: "race".to_owned(),
            title: "A <race>".to_owned(),
            body: "Two *timers*.\n\n<script>alert(1)</script>".to_owned(),
            checked: false,
        };

        let view = view(&finding);

        assert_eq!(view.title, "A <race>", "the page escapes the title");
        assert!(view.body.contains("<em>timers</em>"), "{}", view.body);
        assert!(!view.body.contains("<script>"), "{}", view.body);
        assert!(view.body.contains("&lt;script&gt;"), "{}", view.body);
    }
}
