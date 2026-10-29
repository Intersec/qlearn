//! The whole course in one document. See section 8.4 of the design.
//!
//! An agent whose context was reset reads it to resume the course: where
//! it stopped, what the learner answered, and which points to ask again.
//! The server builds it for `GET /api/export`. With no server, `qlearn
//! export` builds it from the store, which nothing writes then.

use anyhow::{Result, anyhow};

use crate::model::{
    Course, Export, ExportChoice, ExportDebrief, ExportOpen, ExportPage, ExportQuiz, ExportThread,
    Point,
};
use crate::quiz::StoredQuiz;
use crate::store::{PageFile, Store};

/// The export of `course`. `points` are those of the course, which the
/// caller read or settled.
///
/// A file of the store that cannot be read fails the whole export: a part
/// of the course left out would read as a part that never happened.
pub fn build(
    store: &Store,
    course: &Course,
    current: Option<String>,
    points: Vec<Point>,
) -> Result<Export> {
    let mut pages = Vec::new();
    let ids = course
        .parts
        .iter()
        .flat_map(|part| &part.pages)
        .filter(|page| course.has_content(&page.id));
    for page in ids {
        let id = &page.id;
        // `course.json` is written last in a push, so a page that it backs
        // has its files. One missing was removed by hand.
        let missing = |name: String| {
            let path = store.course_dir(&course.slug).join("pages").join(name);
            anyhow!(
                "{} is missing, though course.json says that the agent pushed page {id}",
                path.display()
            )
        };
        let file = store
            .load_page(&course.slug, id)?
            .ok_or_else(|| missing(format!("{id}.json")))?;
        let source = store
            .load_source(&course.slug, id)?
            .ok_or_else(|| missing(format!("{id}.md")))?;
        pages.push(page_of(id, source, &file));
    }

    Ok(Export {
        version: crate::version::LONG.to_owned(),
        course: course.clone(),
        current,
        pages,
        choices: store
            .load_choices(&course.slug)?
            .iter()
            .map(crate::choice::StoredChoosing::view)
            .collect(),
        points: crate::quiz::in_order(points),
        notes: store.notes(&course.slug)?,
    })
}

fn page_of(id: &str, source: String, file: &PageFile) -> ExportPage {
    ExportPage {
        id: id.to_owned(),
        source,
        quizzes: file.quizzes.iter().map(quiz_of).collect(),
        threads: file
            .threads()
            .into_iter()
            .map(|(thread, earlier)| {
                // The Markdown as written, as the agent reads it in `ask`.
                let view = thread.view(id, str::to_owned);
                ExportThread {
                    id: view.id,
                    block: thread.block.clone(),
                    earlier,
                    quote: view.quote,
                    comments: view.comments,
                }
            })
            .collect(),
    }
}

fn quiz_of(quiz: &StoredQuiz) -> ExportQuiz {
    // The view of the page decides when the right option shows, so the
    // export cannot give it away sooner.
    let view = quiz.view();
    ExportQuiz {
        open: quiz.open.as_ref().map(|open| ExportOpen {
            prompt: open.prompt.clone(),
            points: open.points.clone(),
            sent: quiz.sent.open.clone(),
        }),
        choices: quiz
            .choices
            .iter()
            .zip(view.choices)
            .map(|(choice, shown)| ExportChoice {
                header: choice.header.clone(),
                prompt: choice.prompt.clone(),
                options: choice.options.clone(),
                point: choice.point.clone(),
                graded: shown.graded,
            })
            .collect(),
        debrief: quiz.debrief.as_ref().map(|debrief| ExportDebrief {
            source: debrief.source.clone(),
            points: debrief.points.clone(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use time::OffsetDateTime;

    use crate::model::{Block, PageKind};
    use crate::store::{PageFile, Store};
    use crate::testutil::{build_repo, commit};

    /// `course.json` says that the agent pushed the page, so a file of the
    /// page that is gone is a part of the course lost, not one that never
    /// happened.
    #[tokio::test]
    async fn a_page_whose_file_is_gone_fails_the_export() {
        let repo = build_repo(&[commit("first").file("a.txt", "a\n")]).await;
        let store = Store::at(repo.path());
        let now = OffsetDateTime::from_unix_timestamp(1_767_225_600).unwrap();
        let mut course = store.open(Some("raft"), now).unwrap();
        let page = PageFile {
            blocks: vec![Block::Text {
                id: "0123456789ab".to_owned(),
                html: "<p>One.</p>".to_owned(),
            }],
            hidden: BTreeMap::new(),
            quizzes: Vec::new(),
            threads: Vec::new(),
        };
        store.save_page("raft", "1", "One.\n", &page).unwrap();
        course.push_page("1", PageKind::Lesson, None, now);
        store.save(&course).unwrap();
        let export = super::build(&store, &course, None, Vec::new()).unwrap();
        assert_eq!(export.pages.len(), 1);

        for name in ["1.md", "1.json"] {
            let path = repo.path().join(".learn/raft/pages").join(name);
            let kept = std::fs::read(&path).unwrap();
            std::fs::remove_file(&path).unwrap();

            let error = super::build(&store, &course, None, Vec::new()).unwrap_err();

            let error = format!("{error:#}");
            assert!(error.contains(&path.display().to_string()), "{error}");
            std::fs::write(&path, kept).unwrap();
        }
    }
}
