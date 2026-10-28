//! The types that cross the wire.
//!
//! The TypeScript shapes of `roadmap/design.md` are the contract. A field
//! here is spelled as there, through `rename_all = "camelCase"`.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// The format of `course.json`. A change owes a bump and a migration: a
/// future qlearn reads the file that this one writes.
pub const VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Course {
    pub version: u32,
    pub slug: String,
    /// The subject, as the learner named it.
    pub title: String,
    /// Markdown.
    pub goal: String,
    /// Markdown: the background and the level of the learner.
    pub profile: String,
    pub quiz_mode: QuizMode,
    pub status: CourseStatus,
    /// A date, YYYY-MM-DD.
    pub follow_up: Option<String>,
    pub parts: Vec<Part>,
    pub findings: Vec<Finding>,
    /// RFC 3339.
    pub created: String,
    pub updated: String,
}

impl Course {
    /// A course that nothing has been written to yet. The slug stands for
    /// the title until the agent sets the real one.
    pub fn new(slug: &str, now: OffsetDateTime) -> Self {
        let now = stamp(now);

        Self {
            version: VERSION,
            slug: slug.to_owned(),
            title: slug.to_owned(),
            goal: String::new(),
            profile: String::new(),
            quiz_mode: QuizMode::Mixed,
            status: CourseStatus::New,
            follow_up: None,
            parts: Vec::new(),
            findings: Vec::new(),
            created: now.clone(),
            updated: now,
        }
    }

    /// Whether the syllabus holds the page with content. `course.json` is
    /// written last in a push, so a page file that it does not back is the
    /// trace of a push that failed.
    pub fn has_content(&self, id: &str) -> bool {
        self.parts
            .iter()
            .flat_map(|part| &part.pages)
            .any(|page| page.id == id && page.state != PageState::Planned)
    }

    /// Record that the agent pushed the blocks of a page.
    ///
    /// A planned page becomes taught. A page that the syllabus does not hold
    /// goes at the end of the last part, so a course with no syllabus still
    /// works. Its title is its first heading, or its id. A later syllabus
    /// moves it.
    pub fn push_page(
        &mut self,
        id: &str,
        kind: PageKind,
        heading: Option<&str>,
        now: OffsetDateTime,
    ) {
        self.updated = stamp(now);

        let held = self
            .parts
            .iter_mut()
            .flat_map(|part| part.pages.iter_mut())
            .find(|page| page.id == id);
        if let Some(page) = held {
            // Blocks pushed again leave the answers of the learner as they are.
            if page.state == PageState::Planned {
                page.state = PageState::Taught;
            }
            return;
        }

        // A course with no parts is one implicit part with no title.
        if self.parts.is_empty() {
            self.parts.push(Part {
                title: String::new(),
                pages: Vec::new(),
            });
        }
        let title = heading.filter(|title| !title.is_empty()).unwrap_or(id);
        if let Some(part) = self.parts.last_mut() {
            part.pages.push(PageRef {
                id: id.to_owned(),
                kind,
                title: title.to_owned(),
                state: PageState::Taught,
                score: None,
            });
        }
    }
}

/// What `GET /api/session` answers. The syllabus is in `course.parts`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub course: Course,
    /// The page that the agent pushed last, or None before the first push.
    /// The page opens on it when the address names no page.
    pub current: Option<String>,
    /// The release and the commit under it, as `qlearn --version` says.
    pub version: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum QuizMode {
    Mixed,
    Current,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CourseStatus {
    New,
    Running,
    Finished,
}

impl CourseStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Running => "running",
            Self::Finished => "finished",
        }
    }
}

/// A part of the syllabus. A course with no parts is one implicit part
/// with no title.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    pub title: String,
    pub pages: Vec<PageRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PageRef {
    /// `1`, `7a`, `c1`, `f1`.
    pub id: String,
    pub kind: PageKind,
    pub title: String,
    /// Comes from the store, never from the agent.
    pub state: PageState,
    pub score: Option<Score>,
}

// `lowercase`, not `camelCase`: the contract spells it `followup`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PageKind {
    Lesson,
    Checkpoint,
    FollowUp,
}

impl PageKind {
    /// The kind that a page id names, or None when the id is not one.
    ///
    /// `[0-9]+[a-z]?` is a lesson, `c[0-9]+` a checkpoint, `f[0-9]+` a
    /// follow-up. An id names a file of the store, so nothing else passes.
    pub fn of_id(id: &str) -> Option<Self> {
        let digits = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());

        if let Some(rest) = id.strip_prefix('c') {
            return digits(rest).then_some(Self::Checkpoint);
        }
        if let Some(rest) = id.strip_prefix('f') {
            return digits(rest).then_some(Self::FollowUp);
        }
        let rest = match id.as_bytes().last() {
            Some(b) if b.is_ascii_lowercase() => &id[..id.len() - 1],
            _ => id,
        };
        digits(rest).then_some(Self::Lesson)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PageState {
    Planned,
    Taught,
    Answered,
    Debriefed,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Score {
    pub right: u32,
    pub total: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub id: String,
    pub title: String,
    /// Markdown.
    pub body: String,
    /// False reads "to check".
    pub checked: bool,
}

/// A block of a page. See section 4.1 of the design.
///
/// The kinds of the later steps join this enum. Until then, the server
/// refuses a page that holds one.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Block {
    /// Paragraphs, lists, tables, and fences of code, up to the next
    /// heading. The HTML is written by the server, from a short list of
    /// tags.
    Text { id: String, html: String },
}

impl Block {
    pub fn id(&self) -> &str {
        match self {
            Self::Text { id, .. } => id,
        }
    }
}

/// What `GET /api/pages/:id` answers. Quizzes and threads join it later.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub id: String,
    pub blocks: Vec<Block>,
}

/// A range of a line of code with its syntax class, in bytes of the line.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    pub start: usize,
    pub end: usize,
    /// One or two CSS classes, for example `tok-keyword tok-control`.
    pub cls: String,
}

/// Who made a write. It names the writer, and grants no permission.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Author {
    Learner,
    Agent,
}

/// What a write changed, or what the learner asks of the agent.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EventKind {
    Course,
    Syllabus,
    Page,
    Quiz,
    Debrief,
    Status,
    Thread,
    Reply,
    Choose,
    Finding,
    Cheatsheet,
    /// The agent waits again after it wrote: its work is over, and a page
    /// that held its reloads catches up once.
    Idle,
    Next,
    Answer,
    Ask,
    Chosen,
    Broken,
}

impl EventKind {
    /// A request needs the agent. `qlearn wait` returns on a request only.
    pub fn is_request(self) -> bool {
        matches!(
            self,
            Self::Next | Self::Answer | Self::Ask | Self::Chosen | Self::Broken
        )
    }
}

/// One write of the server, numbered. See section 8.3 of the design.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub seq: u64,
    pub kind: EventKind,
    pub author: Author,
    pub page: Option<String>,
    /// The quiz, the thread, or the choice.
    pub id: Option<String>,
}

/// What `GET /api/events` answers.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
    pub events: Vec<Event>,
    /// The number to ask after next time.
    pub next: u64,
    /// The events asked for are gone, or this server never made them. The
    /// party reads everything again.
    pub reset: bool,
    /// An agent is present: a `qlearn wait` is open, ended a moment ago,
    /// or handed over a request that the agent works on.
    pub agent: bool,
    /// The agent works on a request that a wait handed over, and has not
    /// waited again since. The page says that the agent prepares the next
    /// page.
    pub working: bool,
    /// An agent was present on this server, and is present no more. The
    /// page asks the learner to start it again.
    pub left: bool,
    /// What the agent says it does, until its next wait.
    pub status: Option<String>,
}

fn stamp(time: OffsetDateTime) -> String {
    // Only a year outside 0..=9999 fails, which no clock gives.
    time.format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn at_new_year() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_767_225_600).unwrap()
    }

    #[test]
    fn a_new_course_has_the_shape_of_the_contract() {
        let course = Course::new("raft", at_new_year());

        assert_eq!(
            serde_json::to_value(&course).unwrap(),
            json!({
                "version": 1,
                "slug": "raft",
                "title": "raft",
                "goal": "",
                "profile": "",
                "quizMode": "mixed",
                "status": "new",
                "followUp": null,
                "parts": [],
                "findings": [],
                "created": "2026-01-01T00:00:00Z",
                "updated": "2026-01-01T00:00:00Z",
            })
        );
    }

    #[test]
    fn a_page_id_names_its_kind() {
        for (id, kind) in [
            ("1", PageKind::Lesson),
            ("12", PageKind::Lesson),
            ("7a", PageKind::Lesson),
            ("c1", PageKind::Checkpoint),
            ("f10", PageKind::FollowUp),
        ] {
            assert_eq!(PageKind::of_id(id), Some(kind), "{id}");
        }
        for id in [
            "", "a", "c", "f", "7ab", "7A", "c1a", "x1", "1-2", "../1", "1.md", " 1", "١",
        ] {
            assert_eq!(PageKind::of_id(id), None, "{id:?}");
        }
    }

    fn page(id: &str, state: PageState) -> PageRef {
        PageRef {
            id: id.to_owned(),
            kind: PageKind::of_id(id).unwrap(),
            title: format!("Page {id}"),
            state,
            score: None,
        }
    }

    #[test]
    fn a_page_of_the_syllabus_becomes_taught_and_keeps_its_title() {
        let mut course = Course::new("raft", at_new_year());
        course.parts = vec![Part {
            title: "Why".to_owned(),
            pages: vec![
                page("1", PageState::Planned),
                page("2", PageState::Answered),
            ],
        }];
        let later = OffsetDateTime::from_unix_timestamp(1_767_225_660).unwrap();

        course.push_page("1", PageKind::Lesson, Some("A heading"), later);
        course.push_page("2", PageKind::Lesson, None, later);

        assert_eq!(
            course.parts[0].pages,
            [page("1", PageState::Taught), page("2", PageState::Answered)]
        );
        assert_eq!(course.updated, "2026-01-01T00:01:00Z");
    }

    #[test]
    fn only_a_page_past_planned_has_content() {
        let mut course = Course::new("raft", at_new_year());
        course.parts = vec![Part {
            title: "Why".to_owned(),
            pages: vec![page("1", PageState::Planned), page("2", PageState::Taught)],
        }];

        assert!(!course.has_content("1"));
        assert!(course.has_content("2"));
        assert!(!course.has_content("3"));
    }

    #[test]
    fn a_page_outside_the_syllabus_goes_at_the_end_of_the_last_part() {
        let mut course = Course::new("raft", at_new_year());
        course.parts = vec![
            Part {
                title: "Why".to_owned(),
                pages: vec![page("1", PageState::Taught)],
            },
            Part {
                title: "How".to_owned(),
                pages: vec![page("2", PageState::Planned)],
            },
        ];

        course.push_page(
            "c1",
            PageKind::Checkpoint,
            Some("Checkpoint one"),
            at_new_year(),
        );
        course.push_page("f1", PageKind::FollowUp, None, at_new_year());
        course.push_page("7a", PageKind::Lesson, Some(""), at_new_year());

        assert_eq!(course.parts[0].pages, [page("1", PageState::Taught)]);
        assert_eq!(
            serde_json::to_value(&course.parts[1].pages).unwrap(),
            json!([
                { "id": "2", "kind": "lesson", "title": "Page 2", "state": "planned", "score": null },
                { "id": "c1", "kind": "checkpoint", "title": "Checkpoint one",
                  "state": "taught", "score": null },
                { "id": "f1", "kind": "followup", "title": "f1", "state": "taught", "score": null },
                { "id": "7a", "kind": "lesson", "title": "7a", "state": "taught", "score": null },
            ])
        );
    }

    #[test]
    fn a_page_pushed_to_a_course_with_no_parts_makes_the_implicit_part() {
        let mut course = Course::new("raft", at_new_year());

        course.push_page("1", PageKind::Lesson, Some("Two leaders"), at_new_year());
        course.push_page("1", PageKind::Lesson, Some("Renamed"), at_new_year());

        assert_eq!(
            serde_json::to_value(&course.parts).unwrap(),
            json!([{ "title": "", "pages": [
                { "id": "1", "kind": "lesson", "title": "Two leaders", "state": "taught",
                  "score": null },
            ] }])
        );
    }

    #[test]
    fn a_text_block_has_the_shape_of_the_contract() {
        let block = Block::Text {
            id: "0123456789ab".to_owned(),
            html: "<p>One.</p>".to_owned(),
        };

        assert_eq!(
            serde_json::to_value(&block).unwrap(),
            json!({ "id": "0123456789ab", "kind": "text", "html": "<p>One.</p>" })
        );
    }

    #[test]
    fn a_syllabus_has_the_shape_of_the_contract() {
        let part = Part {
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
                    id: "f1".to_owned(),
                    kind: PageKind::FollowUp,
                    title: "A week later".to_owned(),
                    state: PageState::Planned,
                    score: None,
                },
            ],
        };

        assert_eq!(
            serde_json::to_value(&part).unwrap(),
            json!({
                "title": "Why",
                "pages": [
                    { "id": "1", "kind": "lesson", "title": "Two leaders, one term",
                      "state": "answered", "score": { "right": 2, "total": 3 } },
                    { "id": "f1", "kind": "followup", "title": "A week later",
                      "state": "planned", "score": null },
                ],
            })
        );
    }
}
