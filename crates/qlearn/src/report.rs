//! The course as text, for `qlearn -v`.

use crate::model::{Course, PageKind, PageState};

/// The course and its syllabus, one line per page.
pub fn outline(course: &Course) -> String {
    let mut out = format!(
        "{}  {}  {}\n",
        course.slug,
        course.status.as_str(),
        course.title
    );
    if course.parts.iter().all(|part| part.pages.is_empty()) {
        out.push_str("  no page yet\n");
        return out;
    }

    for part in &course.parts {
        // A course with no parts is one part with no title.
        if !part.title.is_empty() {
            out.push_str(&format!("  {}\n", part.title));
        }
        for page in &part.pages {
            let score = page
                .score
                .map(|score| format!("  {}/{}", score.right, score.total))
                .unwrap_or_default();
            out.push_str(&format!(
                "    {:<4} {:<10} {:<9} {}{score}\n",
                page.id,
                kind(page.kind),
                state(page.state),
                page.title
            ));
        }
    }
    out
}

fn kind(kind: PageKind) -> &'static str {
    match kind {
        PageKind::Lesson => "lesson",
        PageKind::Checkpoint => "checkpoint",
        PageKind::FollowUp => "followup",
    }
}

fn state(state: PageState) -> &'static str {
    match state {
        PageState::Planned => "planned",
        PageState::Taught => "taught",
        PageState::Answered => "answered",
        PageState::Debriefed => "debriefed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PageRef, Part, Score};
    use time::OffsetDateTime;

    fn course() -> Course {
        Course::new("raft", OffsetDateTime::UNIX_EPOCH)
    }

    #[test]
    fn a_new_course_says_that_it_has_no_page() {
        assert_eq!(outline(&course()), "raft  new  raft\n  no page yet\n");
    }

    #[test]
    fn each_page_gives_its_state_and_its_score() {
        let mut course = course();
        course.title = "Raft".to_owned();
        course.parts = vec![Part {
            title: "Why".to_owned(),
            pages: vec![
                PageRef {
                    id: "1".to_owned(),
                    kind: PageKind::Lesson,
                    title: "Two leaders, one term".to_owned(),
                    state: PageState::Answered,
                    score: Some(Score { right: 2, total: 3 }),
                },
                PageRef {
                    id: "c1".to_owned(),
                    kind: PageKind::Checkpoint,
                    title: "Checkpoint".to_owned(),
                    state: PageState::Planned,
                    score: None,
                },
            ],
        }];

        assert_eq!(
            outline(&course),
            "raft  new  Raft\n\
             \x20 Why\n\
             \x20   1    lesson     answered  Two leaders, one term  2/3\n\
             \x20   c1   checkpoint planned   Checkpoint\n"
        );
    }
}
