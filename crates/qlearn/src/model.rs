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
}

/// What `GET /api/session` answers. The syllabus is in `course.parts`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub course: Course,
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
