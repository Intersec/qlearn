//! The syllabus that the agent writes as a whole. See section 3.1 of the
//! design.
//!
//! The body is walked by hand, not read through serde: serde stops at the
//! first fault, and the agent must get every fault at once to fix them in
//! one pass.

use std::collections::HashSet;

use serde_json::Value;

use crate::json::{array, object, text, unknown};
use crate::model::{Course, PageKind, PageRef, PageState, Part};

/// The parts of a new syllabus, or every fault of the body.
///
/// A kept page keeps its state and its score: they come from the store,
/// never from the agent. A new page is planned. A page with content that
/// the body leaves out is a fault, because its answers would be lost.
pub fn plan(body: &Value, course: &Course) -> Result<Vec<Part>, Vec<String>> {
    let mut faults = Vec::new();
    let mut seen = HashSet::new();
    let mut unread = false;
    // A body with no list of parts says nothing of the pages that it drops.
    let Some(parts) = read_parts(body, &mut seen, &mut unread, &mut faults) else {
        return Err(faults);
    };

    let mut planned = Vec::new();
    for part in parts {
        let mut pages = Vec::new();
        for page in part.pages {
            let Some(id) = &page.id else { continue };
            let held = course
                .parts
                .iter()
                .flat_map(|part| &part.pages)
                .find(|held| held.id == *id);
            pages.push(PageRef {
                id: id.clone(),
                kind: page.kind.unwrap_or(PageKind::Lesson),
                title: page.title,
                state: held.map_or(PageState::Planned, |held| held.state),
                score: held.and_then(|held| held.score),
            });
        }
        planned.push(Part {
            title: part.title,
            pages,
        });
    }

    // A page whose id cannot be read may be a kept page: its fault comes
    // alone, and the next pass tells whether a page is dropped.
    let held = course.parts.iter().flat_map(|part| &part.pages);
    for page in held.filter(|_| !unread) {
        if page.state != PageState::Planned && !seen.contains(&page.id) {
            faults.push(format!(
                "page {} ({:?}) has content, and the syllabus drops it. Keep it in the \
                 syllabus: its answers would be lost",
                page.id, page.title
            ));
        }
    }

    match faults.is_empty() {
        true => Ok(planned),
        false => Err(faults),
    }
}

/// A part as the body gives it. A field with a fault reads as empty, so
/// the walk goes on and finds the other faults.
struct PartBody {
    title: String,
    pages: Vec<PageBody>,
}

struct PageBody {
    /// None when the id is missing, is not a page id, or came before.
    id: Option<String>,
    kind: Option<PageKind>,
    title: String,
}

/// `seen` gathers the ids of the pages, so a page given twice is a fault.
/// `unread` is set when the body holds a page, or may hold one, whose id
/// is not a string.
fn read_parts(
    body: &Value,
    seen: &mut HashSet<String>,
    unread: &mut bool,
    faults: &mut Vec<String>,
) -> Option<Vec<PartBody>> {
    let fields = object(body, "the syllabus", faults)?;
    for (name, value) in fields {
        match name.as_str() {
            "parts" => {}
            // The commands of the agent name the writer, as on every route
            // of the agent.
            "author" if value == "agent" => {}
            "author" => faults.push(format!("author: takes \"agent\", not {value}")),
            _ => faults.push(format!(
                "unknown field {name:?}. The syllabus takes parts only"
            )),
        }
    }
    let parts = array(fields, "parts", "", faults)?;

    let several = parts.len() > 1;
    let mut out = Vec::new();
    for (at, part) in parts.iter().enumerate() {
        let path = format!("parts[{at}]");
        let Some(fields) = object(part, &path, faults) else {
            *unread = true;
            continue;
        };
        unknown(
            fields,
            &["title", "pages"],
            &path,
            "A part takes title and pages",
            faults,
        );
        let title = text(fields, "title", &path, faults)
            .map(|title| title.trim().to_owned())
            .unwrap_or_default();
        // One part with no title is the implicit part: the page shows a
        // flat list. Among several parts, a part with no title has no name.
        if title.is_empty() && several && fields.get("title").is_some_and(Value::is_string) {
            faults.push(format!(
                "{path}.title: empty. Give each part a title, or write one part"
            ));
        }
        let pages = match array(fields, "pages", &path, faults) {
            Some(pages) => pages
                .iter()
                .enumerate()
                .filter_map(|(rank, page)| {
                    read_page(page, &format!("{path}.pages[{rank}]"), seen, unread, faults)
                })
                .collect(),
            None => {
                *unread = true;
                Vec::new()
            }
        };
        out.push(PartBody { title, pages });
    }
    Some(out)
}

fn read_page(
    page: &Value,
    path: &str,
    seen: &mut HashSet<String>,
    unread: &mut bool,
    faults: &mut Vec<String>,
) -> Option<PageBody> {
    let Some(fields) = object(page, path, faults) else {
        *unread = true;
        return None;
    };
    unknown(
        fields,
        &["id", "kind", "title"],
        path,
        "A page takes id, kind and title. The server keeps its state and its score",
        faults,
    );

    let id = text(fields, "id", path, faults);
    let named = id.as_deref().and_then(|id| {
        let kind = PageKind::of_id(id);
        if kind.is_none() {
            faults.push(format!(
                "{path}.id: {id:?} is not a page id. Use a number with an optional letter, \
                 such as 7 or 7a, or c1 for a checkpoint, or f1 for a follow-up"
            ));
        }
        kind
    });

    let kind = text(fields, "kind", path, faults).and_then(|kind| {
        let read = serde_json::from_value::<PageKind>(Value::String(kind.clone())).ok();
        if read.is_none() {
            faults.push(format!(
                "{path}.kind: {kind:?} is not a kind. Use lesson, checkpoint or followup"
            ));
        }
        read
    });
    if let (Some(kind), Some(named), Some(id)) = (kind, named, &id)
        && kind != named
    {
        faults.push(format!(
            "{path}.kind: page {id} has the kind {}, not {}. A c id takes the kind \
             checkpoint, an f id followup, and any other id lesson",
            spell(named),
            spell(kind)
        ));
    }

    // A title of spaces is an empty title.
    let title = text(fields, "title", path, faults).map(|title| title.trim().to_owned());
    if title.as_deref() == Some("") {
        faults.push(format!("{path}.title: empty. Give the page a title"));
    }

    // A string that is no page id cannot be a kept page: the drop is real.
    *unread |= id.is_none();
    let id = id.filter(|_| named.is_some());
    if let Some(id) = &id
        && !seen.insert(id.clone())
    {
        faults.push(format!("{path}.id: page {id} is in the syllabus twice"));
        return None;
    }

    Some(PageBody {
        id,
        kind: named,
        title: title.unwrap_or_default(),
    })
}

fn spell(kind: PageKind) -> &'static str {
    match kind {
        PageKind::Lesson => "lesson",
        PageKind::Checkpoint => "checkpoint",
        PageKind::FollowUp => "followup",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Score;
    use serde_json::json;
    use time::OffsetDateTime;

    fn page(id: &str, title: &str, state: PageState) -> PageRef {
        PageRef {
            id: id.to_owned(),
            kind: PageKind::of_id(id).unwrap(),
            title: title.to_owned(),
            state,
            score: None,
        }
    }

    /// A course whose page 1 has content and a score, and whose page 2 is
    /// planned.
    fn course() -> Course {
        let mut course = Course::new(
            "raft",
            OffsetDateTime::from_unix_timestamp(1_767_225_600).unwrap(),
        );
        let mut one = page("1", "Two leaders", PageState::Answered);
        one.score = Some(Score {
            right: 2.0,
            total: 3,
        });
        course.parts = vec![Part {
            title: "Why".to_owned(),
            pages: vec![one, page("2", "The vote", PageState::Planned)],
        }];
        course
    }

    fn faults(body: Value) -> Vec<String> {
        plan(&body, &course()).unwrap_err()
    }

    #[test]
    fn a_kept_page_keeps_its_state_and_its_score_and_a_new_page_is_planned() {
        let body = json!({ "parts": [
            { "title": "How", "pages": [
                { "id": "c1", "kind": "checkpoint", "title": "Checkpoint" },
                { "id": "1", "kind": "lesson", "title": "  Two leaders, one term " },
            ] },
        ] });

        let parts = plan(&body, &course()).unwrap();

        assert_eq!(
            serde_json::to_value(parts).unwrap(),
            json!([{ "title": "How", "pages": [
                { "id": "c1", "kind": "checkpoint", "title": "Checkpoint", "state": "planned",
                  "score": null },
                { "id": "1", "kind": "lesson", "title": "Two leaders, one term",
                  "state": "answered", "score": { "right": 2, "total": 3 } },
            ] }])
        );
    }

    #[test]
    fn a_planned_page_may_be_dropped() {
        let body = json!({ "parts": [
            { "title": "", "pages": [{ "id": "1", "kind": "lesson", "title": "Two leaders" }] },
        ], "author": "agent" });

        let parts = plan(&body, &course()).unwrap();

        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].title, "");
        assert_eq!(parts[0].pages.len(), 1);
    }

    #[test]
    fn a_dropped_page_with_content_is_refused_by_its_id_and_its_title() {
        let body = json!({ "parts": [
            { "title": "Why", "pages": [{ "id": "2", "kind": "lesson", "title": "The vote" }] },
        ] });

        assert_eq!(
            faults(body),
            [
                "page 1 (\"Two leaders\") has content, and the syllabus drops it. Keep it in the \
              syllabus: its answers would be lost"
            ]
        );
    }

    #[test]
    fn every_fault_comes_at_once_each_with_its_place() {
        let body = json!({
            "parts": [
                { "title": "Why", "pages": [
                    { "id": "1", "kind": "lesson", "title": "Two leaders", "state": "taught" },
                    { "id": "7ab", "kind": "lesson", "title": "Bad id" },
                    { "id": " 7", "kind": "lesson", "title": "A space" },
                    { "id": "c1", "kind": "lesson", "title": "Wrong kind" },
                    { "id": "f1", "kind": "quiz", "title": "Bad kind" },
                    { "id": "3", "kind": "lesson", "title": "  " },
                    { "id": "1", "kind": "lesson", "title": "Twice" },
                    { "kind": "lesson", "title": "No id" },
                    { "id": 4, "kind": "lesson", "title": "A number" },
                    "4",
                ] },
                { "title": "", "pages": [], "intro": "x" },
                { "title": "How" },
            ],
            "slug": "other",
            "author": "learner",
        });

        let faults = faults(body);

        insta::assert_snapshot!(faults.join("\n"), @r#"
        author: takes "agent", not "learner"
        unknown field "slug". The syllabus takes parts only
        parts[0].pages[0]: unknown field "state". A page takes id, kind and title. The server keeps its state and its score
        parts[0].pages[1].id: "7ab" is not a page id. Use a number with an optional letter, such as 7 or 7a, or c1 for a checkpoint, or f1 for a follow-up
        parts[0].pages[2].id: " 7" is not a page id. Use a number with an optional letter, such as 7 or 7a, or c1 for a checkpoint, or f1 for a follow-up
        parts[0].pages[3].kind: page c1 has the kind checkpoint, not lesson. A c id takes the kind checkpoint, an f id followup, and any other id lesson
        parts[0].pages[4].kind: "quiz" is not a kind. Use lesson, checkpoint or followup
        parts[0].pages[5].title: empty. Give the page a title
        parts[0].pages[6].id: page 1 is in the syllabus twice
        parts[0].pages[7].id: missing
        parts[0].pages[8].id: takes a string, not a number
        parts[0].pages[9]: takes an object, not a string
        parts[1]: unknown field "intro". A part takes title and pages
        parts[1].title: empty. Give each part a title, or write one part
        parts[2].pages: missing
        "#);
    }

    /// A page that the body names but whose id cannot be read may be the
    /// kept page: the fault of its shape is the only fault.
    #[test]
    fn a_page_whose_id_cannot_be_read_is_not_called_dropped() {
        let one = json!({ "id": 1, "kind": "lesson", "title": "Two leaders" });
        assert_eq!(
            faults(json!({ "parts": [{ "title": "", "pages": [one] }] })),
            ["parts[0].pages[0].id: takes a string, not a number"]
        );
        assert_eq!(
            faults(
                json!({ "parts": [{ "title": "", "pages": [{ "kind": "lesson",
                "title": "Two leaders" }] }] })
            ),
            ["parts[0].pages[0].id: missing"]
        );
        assert_eq!(
            faults(json!({ "parts": ["Why"] })),
            ["parts[0]: takes an object, not a string"]
        );
        assert_eq!(
            faults(json!({ "parts": [{ "title": "", "pages": {} }] })),
            ["parts[0].pages: takes an array, not an object"]
        );
    }

    #[test]
    fn a_body_that_is_not_a_syllabus_is_refused_with_its_shape() {
        assert_eq!(
            faults(json!([])),
            ["the syllabus: takes an object, not an array"]
        );
        assert_eq!(faults(json!({})), ["parts: missing"]);
        assert_eq!(
            faults(json!({ "parts": {} })),
            ["parts: takes an array, not an object"]
        );
        assert_eq!(
            faults(json!({ "parts": [{ "title": null, "pages": [] }] })),
            [
                "parts[0].title: takes a string, not null",
                "page 1 (\"Two leaders\") has content, and the syllabus drops it. Keep it in \
                 the syllabus: its answers would be lost",
            ]
        );
    }
}
