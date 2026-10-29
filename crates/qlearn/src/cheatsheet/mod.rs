//! The cheatsheet of a course. See section 9 of the design.
//!
//! The agent writes its source at the end of the course, in the format of a
//! page. The server adds two sections that it builds from the store at each
//! read: Map of the code and To revisit. They are records, so they are true
//! by construction, and a result that the learner gets after the write
//! shows at the next read.

use std::collections::BTreeSet;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::model::{
    Block, Cheatsheet, CodeRange, Course, Excerpt, Grade, PageName, Point, Revisit,
};
use crate::store::{PageFile, Store};

/// What `cheatsheet.json` holds.
///
/// Each field takes `serde(default)`, so the file of an earlier release
/// still reads.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct CheatsheetFile {
    pub blocks: Vec<Block>,
}

/// The cheatsheet as the page gets it, from the blocks of the agent. None:
/// the agent has not written it.
///
/// Before the write, every list is empty and nothing more is read: the page
/// asks at each load, long before the end of the course.
pub fn build(store: &Store, course: &Course, sheet: Option<CheatsheetFile>) -> Result<Cheatsheet> {
    let Some(sheet) = sheet else {
        return Ok(Cheatsheet {
            written: false,
            blocks: Vec::new(),
            map: Vec::new(),
            revisit: Vec::new(),
        });
    };

    let pages = pages_of(store, course)?;
    let mut blocks = sheet.blocks;
    // The agent may show the hidden line of a page whose quiz waits, as a
    // debrief may.
    for (_, file) in pages.iter().filter(|(_, file)| !file.answered()) {
        file.hide_in(&mut blocks);
    }
    let shown: Vec<_> = pages
        .iter()
        .map(|(name, file)| (name.clone(), excerpts(file)))
        .collect();

    Ok(Cheatsheet {
        written: true,
        blocks,
        map: map(&shown),
        revisit: revisit(course, store.load_points(&course.slug)?),
    })
}

/// The pages with content, in the order of the syllabus, each with its
/// file.
///
/// A file that cannot be read fails the read: a map with a page left out
/// would read as a page that showed no code.
fn pages_of(store: &Store, course: &Course) -> Result<Vec<(PageName, PageFile)>> {
    let mut pages = Vec::new();
    for page in course.parts.iter().flat_map(|part| &part.pages) {
        let Some(name) = course.name_of(&page.id) else {
            continue;
        };
        // `course.json` is written last in a push, so a page that it backs
        // has its file. One missing was removed by hand.
        let file = store.load_page(&course.slug, &name.id)?.ok_or_else(|| {
            anyhow!(
                "{} is missing, though course.json says that the agent pushed page {}",
                store
                    .course_dir(&course.slug)
                    .join("pages")
                    .join(format!("{}.json", name.id))
                    .display(),
                name.id
            )
        })?;
        pages.push((name, file));
    }
    Ok(pages)
}

/// The excerpts of a page: those of its lesson, then those of the debriefs
/// of its quizzes.
fn excerpts(file: &PageFile) -> Vec<Excerpt> {
    let debriefs = file
        .quizzes
        .iter()
        .filter_map(|quiz| quiz.debrief.as_ref())
        .flat_map(|debrief| &debrief.blocks);
    file.blocks
        .iter()
        .chain(debriefs)
        .filter_map(|block| match block {
            Block::Excerpt { excerpt, .. } => Some(excerpt.clone()),
            _ => None,
        })
        .collect()
}

/// The lines `from` to `to` of a file, and the rank of the page that showed
/// them in the syllabus.
type Shown = (u32, u32, usize);

/// Map of the code: the excerpts of each page, in the order of the
/// syllabus, as ranges of lines.
///
/// Excerpts of one file at one commit that overlap or touch make one range,
/// which names every page that showed a part of it. One file at two commits
/// gives a range for each commit: the same numbers may name other lines.
/// The ranges come by path, then by commit in the order that the course
/// first showed it, then by line.
pub fn map(pages: &[(PageName, Vec<Excerpt>)]) -> Vec<CodeRange> {
    let mut files: Vec<(&Excerpt, Vec<Shown>)> = Vec::new();
    for (rank, (_, excerpts)) in pages.iter().enumerate() {
        for excerpt in excerpts {
            let range = (excerpt.from, excerpt.to, rank);
            match files
                .iter_mut()
                .find(|(first, _)| first.path == excerpt.path && first.commit == excerpt.commit)
            {
                Some((_, ranges)) => ranges.push(range),
                None => files.push((excerpt, vec![range])),
            }
        }
    }
    // Stable: the commits of one path keep the order of the course.
    files.sort_by(|(a, _), (b, _)| a.path.cmp(&b.path));

    let mut out = Vec::new();
    for (file, mut ranges) in files {
        ranges.sort_unstable();
        let mut merged: Vec<(u32, u32, BTreeSet<usize>)> = Vec::new();
        for (from, to, rank) in ranges {
            match merged.last_mut() {
                Some((_, end, ranks)) if from <= end.saturating_add(1) => {
                    *end = (*end).max(to);
                    ranks.insert(rank);
                }
                _ => merged.push((from, to, BTreeSet::from([rank]))),
            }
        }
        out.extend(merged.into_iter().map(|(from, to, ranks)| {
            CodeRange {
                path: file.path.clone(),
                repo: file.repo.clone(),
                commit: file.commit.clone(),
                from,
                to,
                lessons: ranks
                    .into_iter()
                    .map(|rank| pages[rank].0.clone())
                    .collect(),
            }
        }));
    }
    out
}

/// To revisit: every point whose last result is missed or partial, in the
/// order of section 5.3 of the design, with the title of its lesson.
pub fn revisit(course: &Course, points: Vec<Point>) -> Vec<Revisit> {
    let title = |id: &str| {
        course
            .parts
            .iter()
            .flat_map(|part| &part.pages)
            .find(|page| page.id == id)
            .map(|page| page.title.clone())
    };

    crate::quiz::in_order(points)
        .into_iter()
        .filter_map(|point| {
            let result = point.results.last()?.result;
            (result != Grade::Ok).then(|| Revisit {
                title: title(&point.lesson),
                point: point.id,
                label: point.label,
                result,
                lesson: point.lesson,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Graded, PageKind, PageRef, PageState, Part, PointResult};
    use time::OffsetDateTime;

    fn name(id: &str) -> PageName {
        PageName {
            id: id.to_owned(),
            title: format!("Page {id}"),
        }
    }

    fn excerpt(path: &str, commit: &str, from: u32, to: u32) -> Excerpt {
        Excerpt {
            path: path.to_owned(),
            repo: String::new(),
            commit: commit.to_owned(),
            from,
            to,
            lang: "c".to_owned(),
            lines: Vec::new(),
            marked: Vec::new(),
        }
    }

    /// Each range as `path@commit:from-to` with the ids of its pages.
    fn shown(map: &[CodeRange]) -> Vec<String> {
        map.iter()
            .map(|range| {
                let ids: Vec<_> = range.lessons.iter().map(|page| page.id.as_str()).collect();
                format!(
                    "{}@{}:{}-{} {}",
                    range.path,
                    range.commit,
                    range.from,
                    range.to,
                    ids.join(",")
                )
            })
            .collect()
    }

    #[test]
    fn excerpts_that_overlap_or_touch_make_one_range() {
        let pages = [
            (
                name("1"),
                vec![
                    excerpt("src/raft.c", "aaa", 10, 20),
                    excerpt("src/raft.c", "aaa", 40, 45),
                ],
            ),
            // Overlaps 10-20, then touches 40-45.
            (
                name("2"),
                vec![
                    excerpt("src/raft.c", "aaa", 15, 25),
                    excerpt("src/raft.c", "aaa", 46, 50),
                ],
            ),
            // A gap of one line keeps two ranges.
            (name("3"), vec![excerpt("src/raft.c", "aaa", 27, 30)]),
            // Inside a range, and shown twice by one page.
            (
                name("4"),
                vec![
                    excerpt("src/raft.c", "aaa", 12, 14),
                    excerpt("src/raft.c", "aaa", 12, 14),
                ],
            ),
        ];

        assert_eq!(
            shown(&map(&pages)),
            [
                "src/raft.c@aaa:10-25 1,2,4",
                "src/raft.c@aaa:27-30 3",
                "src/raft.c@aaa:40-50 1,2",
            ]
        );
    }

    #[test]
    fn one_path_at_two_commits_gives_a_range_for_each_commit() {
        let pages = [
            (name("1"), vec![excerpt("src/raft.c", "new", 1, 6)]),
            (name("2"), vec![excerpt("src/raft.c", "old", 4, 9)]),
            (name("3"), vec![excerpt("src/raft.c", "new", 7, 8)]),
        ];

        // The commit that the course showed first comes first.
        assert_eq!(
            shown(&map(&pages)),
            ["src/raft.c@new:1-8 1,3", "src/raft.c@old:4-9 2"]
        );
    }

    #[test]
    fn the_map_comes_by_path_and_names_each_page_in_the_order_of_the_syllabus() {
        let pages = [
            (name("2"), vec![excerpt("src/vote.c", "aaa", 5, 9)]),
            (name("1"), vec![excerpt("src/a/timer.c", "aaa", 1, 3)]),
            (name("c1"), vec![excerpt("src/vote.c", "aaa", 1, 4)]),
            (name("7"), Vec::new()),
        ];

        let map = map(&pages);

        assert_eq!(
            shown(&map),
            ["src/a/timer.c@aaa:1-3 1", "src/vote.c@aaa:1-9 2,c1"]
        );
        assert_eq!(map[1].lessons, [name("2"), name("c1")]);
        assert!(super::map(&[]).is_empty());
    }

    fn point(id: &str, lesson: &str, results: &[(&str, Grade)]) -> Point {
        Point {
            id: id.to_owned(),
            label: format!("The point {id}"),
            lesson: lesson.to_owned(),
            results: results
                .iter()
                .map(|(at, result)| PointResult {
                    at: (*at).to_owned(),
                    result: *result,
                    how: Graded::Open,
                })
                .collect(),
        }
    }

    #[test]
    fn to_revisit_holds_every_missed_or_partial_point_and_no_other() {
        let mut course = Course::new("raft", OffsetDateTime::UNIX_EPOCH);
        course.parts = vec![Part {
            title: String::new(),
            pages: vec![PageRef {
                id: "1".to_owned(),
                kind: PageKind::Lesson,
                title: "The timer".to_owned(),
                state: PageState::Debriefed,
                score: None,
            }],
        }];
        let points = vec![
            point("right", "1", &[("2026-01-01T00:00:00Z", Grade::Ok)]),
            point("half", "1", &[("2026-01-03T00:00:00Z", Grade::Partial)]),
            point(
                "learnt",
                "1",
                &[
                    ("2026-01-01T00:00:00Z", Grade::Missed),
                    ("2026-01-02T00:00:00Z", Grade::Ok),
                ],
            ),
            point(
                "lost",
                "3",
                &[
                    ("2026-01-01T00:00:00Z", Grade::Ok),
                    ("2026-01-02T00:00:00Z", Grade::Missed),
                ],
            ),
            point("untried", "1", &[]),
        ];

        let revisit = revisit(&course, points);

        assert_eq!(
            revisit,
            [
                Revisit {
                    point: "lost".to_owned(),
                    label: "The point lost".to_owned(),
                    result: Grade::Missed,
                    lesson: "3".to_owned(),
                    title: None,
                },
                Revisit {
                    point: "half".to_owned(),
                    label: "The point half".to_owned(),
                    result: Grade::Partial,
                    lesson: "1".to_owned(),
                    title: Some("The timer".to_owned()),
                },
            ]
        );
    }
}
