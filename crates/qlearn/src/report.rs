//! The course as text, for the learner: `qlearn -v`, `qlearn export` and
//! `qlearn delete`.

use crate::model::{Course, Export, Grade, PageKind, PageState};
use crate::store::Removal;

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

/// What `qlearn export` prints: the course and its syllabus, then the
/// points to revisit, in the order of section 5.3 of the design.
pub fn summary(export: &Export) -> String {
    let mut out = outline(&export.course);
    let revisit: Vec<_> = export
        .points
        .iter()
        .filter_map(|point| {
            let last = point.results.last()?;
            (last.result != Grade::Ok).then_some((point, last.result))
        })
        .collect();

    out.push('\n');
    if revisit.is_empty() {
        out.push_str("Nothing to revisit.\n");
        return out;
    }
    out.push_str("To revisit\n");
    for (point, result) in revisit {
        out.push_str(&format!(
            "  {}  (page {}, {})\n",
            point.label,
            point.lesson,
            grade(result)
        ));
    }
    out
}

/// What `qlearn delete` names before it asks: what the learner loses.
pub fn removal(removal: &Removal) -> String {
    let course = match &removal.title {
        Some(title) => format!("the course {title:?}"),
        None => format!(
            "the course {}, whose course.json this qlearn cannot read",
            removal.slug
        ),
    };
    let mut out = format!("This removes {}/, {course}:\n", removal.dir.display());
    out.push_str(&format!(
        "  {}, with the answers of the learner\n",
        crate::quiz::count_of(removal.pages, "page")
    ));
    out.push_str(match removal.cheatsheet {
        true => "  the cheatsheet\n",
        false => "  no cheatsheet\n",
    });
    if removal.notes > 0 {
        out.push_str(&format!(
            "  {} of the agent\n",
            crate::quiz::count_of(removal.notes, "note")
        ));
    }
    out
}

fn grade(grade: Grade) -> &'static str {
    match grade {
        Grade::Ok => "ok",
        Grade::Partial => "partial",
        Grade::Missed => "missed",
    }
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
    use crate::model::{Graded, PageRef, Part, Point, PointResult, Score};
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
                    score: Some(Score {
                        right: 1.5,
                        total: 3,
                    }),
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
             \x20   1    lesson     answered  Two leaders, one term  1.5/3\n\
             \x20   c1   checkpoint planned   Checkpoint\n"
        );
    }

    fn point(id: &str, lesson: &str, results: &[Grade]) -> Point {
        Point {
            id: id.to_owned(),
            label: format!("The {id}"),
            lesson: lesson.to_owned(),
            results: results
                .iter()
                .map(|result| PointResult {
                    at: "2026-01-01T00:00:00Z".to_owned(),
                    result: *result,
                    how: Graded::Choice,
                })
                .collect(),
        }
    }

    fn export(points: Vec<Point>) -> Export {
        Export {
            version: "0.1.0".to_owned(),
            course: course(),
            current: None,
            pages: Vec::new(),
            choices: Vec::new(),
            points,
            notes: Vec::new(),
        }
    }

    #[test]
    fn the_summary_ends_with_the_points_to_revisit_in_their_order() {
        let points = vec![
            point("split-vote", "3", &[Grade::Missed]),
            point("majority", "2", &[Grade::Ok, Grade::Partial]),
            point("timeout", "1", &[Grade::Missed, Grade::Ok]),
            point("leader", "1", &[]),
        ];

        assert_eq!(
            summary(&export(points)),
            "raft  new  raft\n  no page yet\n\n\
             To revisit\n\
             \x20 The split-vote  (page 3, missed)\n\
             \x20 The majority  (page 2, partial)\n"
        );
        assert_eq!(
            summary(&export(vec![point("timeout", "1", &[Grade::Ok])])),
            "raft  new  raft\n  no page yet\n\nNothing to revisit.\n"
        );
    }

    #[test]
    fn a_removal_names_the_directory_the_pages_the_cheatsheet_and_the_notes() {
        let mut removal = Removal {
            slug: "raft".to_owned(),
            dir: "/work/.learn/raft".into(),
            title: Some("Raft elections".to_owned()),
            pages: 3,
            cheatsheet: true,
            notes: 2,
        };
        assert_eq!(
            super::removal(&removal),
            "This removes /work/.learn/raft/, the course \"Raft elections\":\n\
             \x20 3 pages, with the answers of the learner\n\
             \x20 the cheatsheet\n\
             \x20 2 notes of the agent\n"
        );

        removal.title = None;
        removal.pages = 1;
        removal.cheatsheet = false;
        removal.notes = 0;
        assert_eq!(
            super::removal(&removal),
            "This removes /work/.learn/raft/, the course raft, whose course.json this \
             qlearn cannot read:\n\
             \x20 1 page, with the answers of the learner\n\
             \x20 no cheatsheet\n"
        );
    }
}
