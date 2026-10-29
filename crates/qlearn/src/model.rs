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

// No `Eq`: a score holds a fraction.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
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
        self.page_with_content(id).is_some()
    }

    /// The id and the title of a page with content.
    pub fn name_of(&self, id: &str) -> Option<PageName> {
        self.page_with_content(id).map(|page| PageName {
            id: page.id.clone(),
            title: page.title.clone(),
        })
    }

    fn page_with_content(&self, id: &str) -> Option<&PageRef> {
        self.parts
            .iter()
            .flat_map(|part| &part.pages)
            .find(|page| page.id == id && page.state != PageState::Planned)
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

    /// Record that the agent added a quiz to a page with content.
    ///
    /// The new quiz has no answer yet, so a page that was answered or
    /// debriefed is taught again.
    pub fn push_quiz(&mut self, id: &str, now: OffsetDateTime) {
        self.updated = stamp(now);

        let held = self
            .parts
            .iter_mut()
            .flat_map(|part| part.pages.iter_mut())
            .find(|page| page.id == id);
        if let Some(page) = held
            && matches!(page.state, PageState::Answered | PageState::Debriefed)
        {
            page.state = PageState::Taught;
        }
    }

    /// Record that the learner answered every quiz of a page.
    ///
    /// Only a taught page moves: a page with no content has no quiz, and a
    /// debriefed page went back to taught when it got a new quiz.
    pub fn answer_page(&mut self, id: &str, now: OffsetDateTime) {
        let held = self
            .parts
            .iter_mut()
            .flat_map(|part| part.pages.iter_mut())
            .find(|page| page.id == id);
        if let Some(page) = held
            && page.state == PageState::Taught
        {
            page.state = PageState::Answered;
            self.updated = stamp(now);
        }
    }

    /// Record that the agent wrote the debrief of every quiz of a page.
    ///
    /// Only an answered page moves: a debrief waits for the answer of its
    /// quiz, so a page with a quiz left to answer is still taught.
    pub fn debrief_page(&mut self, id: &str, now: OffsetDateTime) {
        if let Some(page) = self.page_mut(id)
            && page.state == PageState::Answered
        {
            page.state = PageState::Debriefed;
            self.updated = stamp(now);
        }
    }

    /// Set the score of a page, as its quizzes give it.
    pub fn score_page(&mut self, id: &str, score: Option<Score>, now: OffsetDateTime) {
        if let Some(page) = self.page_mut(id)
            && page.score != score
        {
            page.score = score;
            self.updated = stamp(now);
        }
    }

    fn page_mut(&mut self, id: &str) -> Option<&mut PageRef> {
        self.parts
            .iter_mut()
            .flat_map(|part| part.pages.iter_mut())
            .find(|page| page.id == id)
    }

    /// Whether the syllabus holds a page, with content or not.
    pub fn holds(&self, id: &str) -> bool {
        self.parts
            .iter()
            .flat_map(|part| &part.pages)
            .any(|page| page.id == id)
    }
}

/// What `PATCH /api/course` changes. A field left out stays as it is.
///
/// Every field goes through `present`: a plain `Option` reads `null` as a
/// field left out, and the rest of the change would pass without a word.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CourseChange {
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub title: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub goal: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub profile: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub quiz_mode: Option<QuizMode>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub status: Option<CourseStatus>,
    /// `null` drops the follow-up.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub follow_up: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub author: Option<AgentAuthor>,
}

/// The author that a body of the agent may name. The route already knows
/// the writer, so the field changes nothing, but `learner` on a route of
/// the agent is a mistake.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentAuthor {
    Agent,
}

/// A field that the body holds. `null` passes only when `T` takes it.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

impl Course {
    /// Apply a change of the agent, or refuse it whole with the reason.
    pub fn change(&mut self, change: CourseChange, now: OffsetDateTime) -> Result<(), String> {
        // A call that changes nothing is a mistake of the agent, and an
        // answer of success would hide it.
        let named = CourseChange {
            author: None,
            ..change.clone()
        };
        if named == CourseChange::default() {
            return Err(
                "the change names no field: give title, goal, profile, quizMode, status or followUp"
                    .to_owned(),
            );
        }
        if change.title.as_deref().is_some_and(|t| t.trim().is_empty()) {
            return Err("the title is empty".to_owned());
        }
        // Only the creation of a course makes it new.
        if change.status == Some(CourseStatus::New) {
            return Err("status takes running or finished, not new".to_owned());
        }
        if let Some(Some(date)) = &change.follow_up
            && !is_date(date)
        {
            return Err(format!(
                "followUp takes a date YYYY-MM-DD or null, not {date:?}"
            ));
        }

        if let Some(title) = change.title {
            self.title = title.trim().to_owned();
        }
        if let Some(goal) = change.goal {
            self.goal = goal;
        }
        if let Some(profile) = change.profile {
            self.profile = profile;
        }
        if let Some(mode) = change.quiz_mode {
            self.quiz_mode = mode;
        }
        if let Some(status) = change.status {
            self.status = status;
        }
        if let Some(follow_up) = change.follow_up {
            self.follow_up = follow_up;
        }
        self.updated = stamp(now);
        Ok(())
    }
}

/// Whether `text` is a day of the calendar, as YYYY-MM-DD.
fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    let shape = bytes.len() == 10
        && bytes.iter().enumerate().all(|(at, b)| match at {
            4 | 7 => *b == b'-',
            _ => b.is_ascii_digit(),
        });
    if !shape {
        return false;
    }
    let (Ok(year), Ok(month), Ok(day)) = (
        text[..4].parse::<i32>(),
        text[5..7].parse::<u8>(),
        text[8..].parse::<u8>(),
    ) else {
        return false;
    };
    time::Month::try_from(month)
        .is_ok_and(|month| time::Date::from_calendar_date(year, month, day).is_ok())
}

/// The body of `PUT /api/status`. An empty text clears the status.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StatusText {
    pub text: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub author: Option<AgentAuthor>,
}

/// What `PUT /api/status` answers: the status as the page shows it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub status: Option<String>,
}

/// What `GET /api/session` answers. The syllabus is in `course.parts`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub course: Course,
    /// The page that the agent pushed last, or None before the first push.
    /// The page opens on it when the address names no page.
    pub current: Option<String>,
    /// The release and the commit under it, as `qlearn --version` says.
    pub version: String,
    /// The choices that name no page, open or answered, in the order that
    /// the agent asked them. The page shows them at the top of the syllabus
    /// column, and finds an open one again after a reload.
    // A server of an earlier release sends none.
    #[serde(default)]
    pub choices: Vec<Choosing>,
    /// Why `choices.json` cannot be read, or None. The choices then show
    /// nowhere, and only the routes of the choices refuse.
    #[serde(default)]
    pub choices_error: Option<String>,
    /// The id of this server, as its events carry it.
    // A server of an earlier release sends none.
    #[serde(default)]
    pub server: String,
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
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    pub title: String,
    pub pages: Vec<PageRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
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

/// The score of a page. See section 5.3 of the design.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Score {
    /// In steps of 0.5: a partial result counts half. `course.json` of an
    /// earlier release holds an integer, which reads as well.
    #[serde(serialize_with = "whole_or_half")]
    pub right: f64,
    pub total: u32,
}

/// A whole number goes out as an integer, as an earlier release wrote it,
/// so a score with no half reads `2/3` and not `2.0/3`.
fn whole_or_half<S: serde::Serializer>(right: &f64, serializer: S) -> Result<S::Ok, S::Error> {
    // A score counts at most a few hundred questions, far inside u32.
    if right.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(right) {
        return serializer.serialize_u32(*right as u32);
    }
    serializer.serialize_f64(*right)
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
    /// A quote that opens with a marker, drawn as a box. The HTML holds no
    /// marker: the page draws it itself.
    Callout {
        id: String,
        marker: Marker,
        html: String,
    },
    /// Lines of the repository, read by the server at the push.
    Excerpt { id: String, excerpt: Excerpt },
    /// A Mermaid source, as the agent wrote it. The page draws it: the
    /// server cannot run Mermaid.
    Diagram { id: String, source: String },
    /// A bar or a line chart, checked by the server. The page draws it.
    Chart { id: String, chart: Chart },
}

impl Block {
    pub fn id(&self) -> &str {
        match self {
            Self::Text { id, .. }
            | Self::Callout { id, .. }
            | Self::Excerpt { id, .. }
            | Self::Diagram { id, .. }
            | Self::Chart { id, .. } => id,
        }
    }

    /// The kind, as the contract spells it.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Text { .. } => "text",
            Self::Callout { .. } => "callout",
            Self::Excerpt { .. } => "excerpt",
            Self::Diagram { .. } => "diagram",
            Self::Chart { .. } => "chart",
        }
    }
}

/// A chart, as the server checked it. See section 4.4 of the design.
///
/// Every text is plain text: the page escapes it when it draws it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Chart {
    pub kind: ChartKind,
    pub title: String,
    pub x: ChartX,
    pub y: ChartY,
    /// 1 to 8, each with one value per x value.
    pub series: Vec<ChartSeries>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ChartKind {
    Bar,
    Line,
}

/// The categories of the chart, in the order that the page draws them.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChartX {
    pub label: String,
    /// A number that the agent wrote comes as its text.
    pub values: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChartY {
    pub label: String,
    /// "" when the values have no unit, such as a count.
    pub unit: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChartSeries {
    pub name: String,
    /// One per x value. None, a gap, only in a line chart. A Number keeps
    /// `283` an integer on the wire, where an f64 would write `283.0`.
    pub values: Vec<Option<serde_json::Number>>,
}

/// The marker of a callout: `[!hook]`, `[!teaser]`, `[!pitfall]` or
/// `[!note]` on the first line of a quote.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Marker {
    /// A short way to remember.
    Hook,
    /// A link to a later lesson.
    Teaser,
    /// A trap of the code.
    Pitfall,
    Note,
}

impl Marker {
    pub const ALL: [Self; 4] = [Self::Hook, Self::Teaser, Self::Pitfall, Self::Note];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hook => "hook",
            Self::Teaser => "teaser",
            Self::Pitfall => "pitfall",
            Self::Note => "note",
        }
    }

    /// The marker that a word names. Case does not count, as in the alerts
    /// of GitHub.
    pub fn of_word(word: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|marker| marker.as_str().eq_ignore_ascii_case(word))
    }
}

/// Lines of a file of the repository. See section 4.2 of the design.
///
/// The commit is the one that the rev named at the push, so the excerpt
/// shows the code as the lesson taught it, even after the code changes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Excerpt {
    /// From the top level directory of the worktree.
    pub path: String,
    /// "" for the main repository, or the path of the submodule.
    pub repo: String,
    /// The full hash, in the repository that holds the file.
    pub commit: String,
    pub from: u32,
    pub to: u32,
    /// The token that named the grammar, such as `c`, or "" for plain text.
    pub lang: String,
    /// None: the hidden line, until the page may show it.
    pub lines: Vec<Option<Line>>,
    pub marked: Vec<u32>,
}

/// A line of code with its syntax classes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    pub text: String,
    pub spans: Vec<Span>,
}

/// What `GET /api/pages/:id` answers.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub id: String,
    pub blocks: Vec<Block>,
    /// In the order that the agent added them. Quiz `n` is at `n - 1`.
    pub quizzes: Vec<Quiz>,
    /// In the order that they opened. A thread whose block a later push
    /// took away comes last.
    pub threads: Vec<Thread>,
    /// The choices that the agent asked at the end of this page, open or
    /// answered, in the order that it asked them.
    pub choices: Vec<Choosing>,
}

/// A choice that the agent asks the learner, in place of a dialog of its
/// terminal. See section 7 of the design. A choice is never graded.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Choosing {
    /// `choice-<n>`, such as `choice-2`.
    pub id: String,
    /// The page at whose end the choice shows, or None for the top of the
    /// syllabus column.
    pub page: Option<String>,
    pub questions: Vec<ChoosingQuestion>,
    /// None until the learner answers. Then one answer per question, in
    /// order.
    pub answers: Option<Vec<ChosenAnswer>>,
}

/// A question of a choice. Every text is plain text.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChoosingQuestion {
    /// A short name of the question.
    pub header: String,
    pub prompt: String,
    pub options: Vec<ChoosingOption>,
    /// The learner may pick several options.
    pub multi: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChoosingOption {
    pub label: String,
    /// "" when the agent gave none.
    pub description: String,
}

/// The answer of the learner to one question of a choice.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChosenAnswer {
    /// The options that the learner picked, from 0, in order. Empty when the
    /// free text answers alone.
    pub picked: Vec<usize>,
    /// The free text. Empty or left out: none.
    #[serde(default)]
    pub other: Option<String>,
}

/// The body of `POST /api/choices/:id`: one answer per question of the
/// choice, in order.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChoiceSent {
    pub answers: Vec<ChosenAnswer>,
}

/// What `POST /api/choices` answers.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceId {
    pub id: String,
}

/// The answer to a question of a choice, as the agent reads it in the
/// detail of `chosen`: the labels with the indices, so it needs no second
/// command.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChosenLabels {
    pub header: String,
    pub picked: Vec<usize>,
    /// The label of each picked option, in the order of `picked`.
    pub labels: Vec<String>,
    pub other: Option<String>,
}

/// A question of the learner, or a word of the agent, and the comments
/// that answer it. See section 6 of the design.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    /// `<page>-<n>`, such as `3-2`: the page, and the rank of the thread in
    /// it, from 1.
    pub id: String,
    pub page: String,
    /// None: about the whole page.
    pub block: Option<String>,
    /// The text of the block that the learner selected.
    pub quote: Option<String>,
    pub comments: Vec<Comment>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub author: Author,
    /// The page gets the HTML that the server writes from the Markdown, as
    /// for an option of a choice. The agent gets the Markdown.
    pub body: String,
    /// RFC 3339.
    pub at: String,
}

/// The body of `POST /api/pages/:id/threads`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadOpened {
    /// None: about the whole page.
    #[serde(default)]
    pub block: Option<String>,
    /// Empty or left out: none.
    #[serde(default)]
    pub quote: Option<String>,
    /// Markdown.
    pub body: String,
    /// Left out: the learner.
    #[serde(default)]
    pub author: Option<Author>,
}

/// The body of `POST /api/threads/:id`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplySent {
    /// Markdown.
    pub body: String,
    /// Left out: the learner.
    #[serde(default)]
    pub author: Option<Author>,
}

/// A quiz as the page sees it. See section 5 of the design.
///
/// The right option of a choice sits in `Choice::graded`, which stays None
/// until the learner sends the choices: the page must not get it before.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Quiz {
    pub open: Option<OpenQuestion>,
    pub choices: Vec<Choice>,
    /// None until the agent writes it, which it does once the learner
    /// answered the quiz.
    pub debrief: Option<Debrief>,
}

/// What the agent says of the answers to a quiz. See section 5.4 of the
/// design.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Debrief {
    /// The blocks of its source, as the blocks of a page. No line is hidden.
    pub blocks: Vec<Block>,
    /// The grade of each point of the open question, in its order. Empty
    /// for a quiz with no open question.
    pub points: Vec<GradedPoint>,
}

/// A point of an open question, with the grade that the agent gave it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GradedPoint {
    pub id: String,
    pub label: String,
    pub result: Grade,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenQuestion {
    /// Plain text.
    pub prompt: String,
    /// The open answer of the learner, once sent.
    pub sent: Option<String>,
}

/// A question with options, of which one is right.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    /// Plain text: a short name of the question.
    pub header: String,
    /// Plain text.
    pub prompt: String,
    /// The HTML of each option, written by the server from the inline
    /// Markdown of the agent.
    pub options: Vec<String>,
    /// None until the learner sends the choices. The right option is in
    /// here, so it reaches the page only then.
    pub graded: Option<GradedChoice>,
}

/// A choice once the learner has sent the choices of its quiz.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GradedChoice {
    /// The option that the learner picked, from 0.
    pub picked: usize,
    /// The right option, from 0.
    pub answer: usize,
    pub ok: bool,
    /// The text of the "Other" box, or None when the learner left it empty.
    pub other: Option<String>,
}

/// The body of `POST /api/pages/:id/quizzes/:n/open`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenSent {
    pub text: String,
}

/// The body of `POST /api/pages/:id/quizzes/:n/choices`: one pick for each
/// choice of the quiz, in order.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChoicesSent {
    pub choices: Vec<Pick>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Pick {
    /// The option that the learner picked, from 0.
    pub option: usize,
    /// The text of the "Other" box. Empty or left out: none.
    #[serde(default)]
    pub other: Option<String>,
}

/// What `POST /api/pages/:id/quizzes` answers: the number of the new quiz
/// in its page, from 1.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuizNumber {
    pub page: String,
    pub n: usize,
}

/// A point that a quiz checks, with its results. See section 5.3 of the
/// design.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub id: String,
    pub label: String,
    /// The page that taught the point.
    pub lesson: String,
    pub results: Vec<PointResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PointResult {
    /// RFC 3339.
    pub at: String,
    pub result: Grade,
    pub how: Graded,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Grade {
    Ok,
    Partial,
    Missed,
}

/// What gave a result: a choice that the server graded, or an open answer
/// that the agent graded.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Graded {
    Choice,
    Open,
}

/// A range of a line of code with its syntax class.
///
/// The offsets count bytes inside the server. A line of an excerpt leaves
/// it in UTF-16 code units, which is what a browser slices with.
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
    /// The learner sent a part of a quiz. The other tabs of the page read
    /// it again. Only the last part makes an `answer` request.
    Answered,
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
    /// What the agent needs to answer a request. Only the view of the
    /// agent carries it: the page holds all of it already.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Detail>,
}

/// The detail of a request, so that the agent needs no second command to
/// answer it. See the table of section 8.3 of the design. The kind of the
/// event says which request it is, so the detail carries no tag.
///
/// The requests of the later steps add their own variant.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Detail {
    /// `next`: the page that the learner left.
    Next { left: PageName },
    /// `broken`: the block that the page cannot draw, and why. The event
    /// names the page.
    Broken { block: BlockSource, error: String },
    /// `answer`: a quiz that the learner answered in full, with the
    /// answers. The event names the page.
    Answer {
        /// The number of the quiz in its page, from 1.
        quiz: usize,
        open: Option<OpenAnswer>,
        choices: Vec<ChoiceAnswer>,
    },
    /// `ask`: a thread that the learner opened or answered. The comments
    /// are in Markdown, as written. The event names the page and the
    /// thread.
    Ask {
        thread: Thread,
        /// The block as the agent wrote it when the thread opened, or None
        /// for a thread about the whole page.
        block: Option<BlockSource>,
        /// The block is gone: a later push of the page took it away.
        earlier: bool,
    },
    /// `chosen`: a choice that the learner answered, with its questions as
    /// the agent wrote them. The event names the page and the choice.
    Chosen {
        questions: Vec<ChoosingQuestion>,
        /// One per question, in order.
        answers: Vec<ChosenLabels>,
    },
}

/// The open question of a quiz, with the answer of the learner. The agent
/// grades each point in the debrief.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenAnswer {
    pub prompt: String,
    pub points: Vec<String>,
    pub text: String,
}

/// A choice of a quiz, with the pick of the learner and its grade.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceAnswer {
    pub header: String,
    pub prompt: String,
    /// Inline Markdown, as the agent wrote it.
    pub options: Vec<String>,
    pub picked: usize,
    pub answer: usize,
    pub ok: bool,
    pub other: Option<String>,
    pub point: String,
}

/// A block as the agent wrote it, so that it finds the block in its file.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BlockSource {
    pub id: String,
    pub kind: String,
    pub source: String,
}

/// The body of `POST /api/pages/:id/broken`: a block that the page cannot
/// draw, and the error of the page.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Broken {
    pub block: String,
    pub error: String,
}

/// A page as the agent names it to the learner.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PageName {
    pub id: String,
    pub title: String,
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
    /// waited again since.
    pub working: bool,
    /// The work holds a Next request. The page says that the agent
    /// prepares the next page.
    pub preparing: bool,
    /// An agent was present on this server, and is present no more. The
    /// page asks the learner to start it again.
    pub left: bool,
    /// What the agent says it does, until its next wait.
    pub status: Option<String>,
    /// The id of this server, drawn at its start. A new server numbers its
    /// events from 1 again, so a party that sees another id reads
    /// everything again, whatever `next` says.
    // A server of an earlier release sends none, as in its session.
    #[serde(default)]
    pub server: String,
}

/// What `GET /api/export` answers: the whole course, so that an agent that
/// lost its context resumes it. See section 8.4 of the design.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Export {
    /// The release and the commit under it, as `qlearn --version` says.
    pub version: String,
    /// The syllabus holds the state and the score of each page. Its
    /// `version` is the format of the store.
    pub course: Course,
    /// The page that the agent pushed last, or None before the first push.
    pub current: Option<String>,
    /// The pages with content, in the order of the syllabus.
    pub pages: Vec<ExportPage>,
    /// Every choice, open or answered, in the order that the agent asked
    /// them.
    pub choices: Vec<Choosing>,
    /// In the order of section 5.3 of the design.
    pub points: Vec<Point>,
    /// The files under `notes/`, relative to it. Their content stays
    /// there: the agent reads them with its own tools.
    pub notes: Vec<String>,
}

/// A page as the agent wrote it, with what the learner did on it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportPage {
    pub id: String,
    /// The source as the agent pushed it last.
    pub source: String,
    /// In the order that the agent added them. Quiz `n` is at `n - 1`.
    pub quizzes: Vec<ExportQuiz>,
    /// In the order that the page shows them: a thread whose block is gone
    /// comes last.
    pub threads: Vec<ExportThread>,
}

/// A quiz as the agent wrote it, with the answers of the learner.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportQuiz {
    pub open: Option<ExportOpen>,
    pub choices: Vec<ExportChoice>,
    pub debrief: Option<ExportDebrief>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportOpen {
    pub prompt: String,
    pub points: Vec<String>,
    /// The open answer of the learner, once sent.
    pub sent: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportChoice {
    pub header: String,
    pub prompt: String,
    /// Inline Markdown, as the agent wrote it.
    pub options: Vec<String>,
    pub point: String,
    /// None until the learner sends the choices. The right option is in
    /// here, as in the page: the page may read this route too.
    pub graded: Option<GradedChoice>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportDebrief {
    /// The source as the agent wrote it last.
    pub source: String,
    /// The grade of each point of the open question.
    pub points: Vec<GradedPoint>,
}

/// A thread as the agent reads it in the detail of `ask`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportThread {
    pub id: String,
    /// The block as the agent wrote it when the thread opened, or None for
    /// a thread about the whole page.
    pub block: Option<BlockSource>,
    /// The block is gone: a later push of the page took it away.
    pub earlier: bool,
    pub quote: Option<String>,
    /// In Markdown, as written.
    pub comments: Vec<Comment>,
}

/// A time as the store writes it: RFC 3339.
pub fn stamp(time: OffsetDateTime) -> String {
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

    /// A server of an earlier release sends no id. The batch still reads,
    /// with the empty id that its session gave, so a wait goes on.
    #[test]
    fn a_batch_without_a_server_id_reads_with_an_empty_one() {
        let batch: Batch = serde_json::from_value(json!({
            "events": [],
            "next": 3,
            "reset": false,
            "agent": false,
            "working": false,
            "preparing": false,
            "left": false,
            "status": null,
        }))
        .unwrap();

        assert_eq!(batch.server, "");
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
    fn a_quiz_takes_an_answered_page_back_to_taught() {
        let mut course = Course::new("raft", at_new_year());
        course.parts = vec![Part {
            title: "Why".to_owned(),
            pages: vec![
                page("1", PageState::Taught),
                page("2", PageState::Answered),
                page("3", PageState::Debriefed),
            ],
        }];
        let later = OffsetDateTime::from_unix_timestamp(1_767_225_660).unwrap();

        for id in ["1", "2", "3"] {
            course.push_quiz(id, later);
        }

        assert_eq!(
            course.parts[0].pages,
            [
                page("1", PageState::Taught),
                page("2", PageState::Taught),
                page("3", PageState::Taught),
            ]
        );
        assert_eq!(course.updated, "2026-01-01T00:01:00Z");
    }

    /// The page gets no answer before the learner sends the choices, so
    /// the wire shape of a quiz with no answer has no key for one.
    #[test]
    fn a_quiz_has_the_shape_of_the_page_and_no_answer() {
        let quiz = Quiz {
            open: Some(OpenQuestion {
                prompt: "Why?".to_owned(),
                sent: None,
            }),
            choices: vec![Choice {
                header: "Split vote".to_owned(),
                prompt: "What next?".to_owned(),
                options: vec!["<em>A</em>".to_owned(), "B".to_owned()],
                graded: None,
            }],
            debrief: None,
        };

        assert_eq!(
            serde_json::to_value(&quiz).unwrap(),
            json!({
                "open": { "prompt": "Why?", "sent": null },
                "choices": [{ "header": "Split vote", "prompt": "What next?",
                              "options": ["<em>A</em>", "B"], "graded": null }],
                "debrief": null,
            })
        );
    }

    #[test]
    fn a_graded_choice_has_the_shape_of_the_page() {
        let graded = GradedChoice {
            picked: 0,
            answer: 1,
            ok: false,
            other: Some("A new term?".to_owned()),
        };

        assert_eq!(
            serde_json::to_value(&graded).unwrap(),
            json!({ "picked": 0, "answer": 1, "ok": false, "other": "A new term?" })
        );
    }

    #[test]
    fn only_a_taught_page_becomes_answered() {
        let mut course = Course::new("raft", at_new_year());
        course.parts = vec![Part {
            title: "Why".to_owned(),
            pages: vec![
                page("1", PageState::Taught),
                page("2", PageState::Planned),
                page("3", PageState::Debriefed),
            ],
        }];
        let later = OffsetDateTime::from_unix_timestamp(1_767_225_660).unwrap();

        for id in ["1", "2", "3", "4"] {
            course.answer_page(id, later);
        }

        assert_eq!(
            course.parts[0].pages,
            [
                page("1", PageState::Answered),
                page("2", PageState::Planned),
                page("3", PageState::Debriefed),
            ]
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
    fn a_follow_up_takes_a_day_of_the_calendar() {
        for date in ["2026-11-04", "2028-02-29", "0001-01-01"] {
            assert!(is_date(date), "{date}");
        }
        for date in [
            "",
            "2026-11-4",
            "2026/11/04",
            "2026-13-01",
            "2026-00-01",
            "2026-02-29",
            "2026-04-31",
            "2026-11-04T00:00:00Z",
            "+026-11-04",
            "２026-11-04",
        ] {
            assert!(!is_date(date), "{date:?}");
        }
    }

    #[test]
    fn a_follow_up_left_out_differs_from_a_null_one() {
        let change: CourseChange = serde_json::from_str(r#"{"followUp":null}"#).unwrap();
        assert_eq!(change.follow_up, Some(None));
        let change: CourseChange = serde_json::from_str(r#"{"title":"Raft"}"#).unwrap();
        assert_eq!(change.follow_up, None);
    }

    #[test]
    fn a_null_is_refused_but_for_the_follow_up() {
        for body in [
            r#"{"title":null}"#,
            r#"{"goal":"x","title":null}"#,
            r#"{"goal":null}"#,
            r#"{"profile":null}"#,
            r#"{"quizMode":null}"#,
            r#"{"status":null}"#,
            r#"{"author":null}"#,
        ] {
            assert!(
                serde_json::from_str::<CourseChange>(body).is_err(),
                "{body}"
            );
        }
    }

    #[test]
    fn a_refused_change_leaves_the_course_as_it_was() {
        let mut course = Course::new("raft", at_new_year());
        let before = course.clone();

        let refused = CourseChange {
            goal: Some("Read the election".to_owned()),
            follow_up: Some(Some("2026-02-30".to_owned())),
            ..CourseChange::default()
        };
        assert!(course.change(refused, at_new_year()).is_err());

        assert_eq!(course, before);
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
    fn a_callout_block_has_the_shape_of_the_contract() {
        let block = Block::Callout {
            id: "0123456789ab".to_owned(),
            marker: Marker::Pitfall,
            html: "<p>Two leaders.</p>".to_owned(),
        };

        assert_eq!(
            serde_json::to_value(&block).unwrap(),
            json!({ "id": "0123456789ab", "kind": "callout", "marker": "pitfall",
                    "html": "<p>Two leaders.</p>" })
        );
        for marker in Marker::ALL {
            assert_eq!(
                serde_json::to_value(marker).unwrap(),
                json!(marker.as_str())
            );
        }
    }

    #[test]
    fn a_diagram_block_has_the_shape_of_the_contract() {
        let block = Block::Diagram {
            id: "0123456789ab".to_owned(),
            source: "graph LR\n  A --> B\n".to_owned(),
        };

        assert_eq!(
            serde_json::to_value(&block).unwrap(),
            json!({ "id": "0123456789ab", "kind": "diagram", "source": "graph LR\n  A --> B\n" })
        );
        assert_eq!(block.kind(), "diagram");
    }

    #[test]
    fn a_chart_block_has_the_shape_of_the_contract() {
        let block = Block::Chart {
            id: "0123456789ab".to_owned(),
            chart: Chart {
                kind: ChartKind::Line,
                title: "Terms".to_owned(),
                x: ChartX {
                    label: "Time (s)".to_owned(),
                    values: vec!["0".to_owned(), "1".to_owned()],
                },
                y: ChartY {
                    label: "Term".to_owned(),
                    unit: String::new(),
                },
                series: vec![ChartSeries {
                    name: "S1".to_owned(),
                    values: vec![Some(serde_json::Number::from(1)), None],
                }],
            },
        };

        assert_eq!(
            serde_json::to_value(&block).unwrap(),
            json!({ "id": "0123456789ab", "kind": "chart", "chart": {
                "kind": "line", "title": "Terms",
                "x": { "label": "Time (s)", "values": ["0", "1"] },
                "y": { "label": "Term", "unit": "" },
                "series": [{ "name": "S1", "values": [1, null] }] } })
        );
        assert_eq!(block.kind(), "chart");
    }

    #[test]
    fn a_debrief_has_the_shape_of_the_page() {
        let debrief = Debrief {
            blocks: vec![Block::Text {
                id: "0123456789ab".to_owned(),
                html: "<p>Almost.</p>".to_owned(),
            }],
            points: vec![GradedPoint {
                id: "random-timeout".to_owned(),
                label: "Why the timeout is random".to_owned(),
                result: Grade::Partial,
            }],
        };

        assert_eq!(
            serde_json::to_value(&debrief).unwrap(),
            json!({
                "blocks": [{ "id": "0123456789ab", "kind": "text", "html": "<p>Almost.</p>" }],
                "points": [{ "id": "random-timeout", "label": "Why the timeout is random",
                             "result": "partial" }],
            })
        );
    }

    /// A partial result counts half, and a whole score keeps the integer
    /// of an earlier release.
    #[test]
    fn a_score_goes_in_steps_of_a_half() {
        let score = |right| Score { right, total: 3 };

        assert_eq!(
            serde_json::to_value(score(1.5)).unwrap(),
            json!({ "right": 1.5, "total": 3 })
        );
        assert_eq!(
            serde_json::to_string(&score(2.0)).unwrap(),
            r#"{"right":2,"total":3}"#
        );
        assert_eq!(
            serde_json::from_str::<Score>(r#"{"right":0.5,"total":1}"#).unwrap(),
            Score {
                right: 0.5,
                total: 1
            }
        );
    }

    /// The score of an earlier release is an integer. The format stays at
    /// version 1, so its `course.json` must read as it is.
    #[test]
    fn a_course_of_an_earlier_release_with_an_integer_score_reads() {
        let text = r#"{
            "version": 1, "slug": "raft", "title": "Raft", "goal": "", "profile": "",
            "quizMode": "mixed", "status": "running", "followUp": null,
            "parts": [{ "title": "", "pages": [
                { "id": "1", "kind": "lesson", "title": "Two leaders", "state": "answered",
                  "score": { "right": 2, "total": 3 } }
            ] }],
            "findings": [], "created": "2026-01-01T00:00:00Z", "updated": "2026-01-01T00:00:00Z"
        }"#;

        let course: Course = serde_json::from_str(text).unwrap();

        assert_eq!(
            course.parts[0].pages[0].score,
            Some(Score {
                right: 2.0,
                total: 3
            })
        );
        let back = serde_json::to_value(&course).unwrap();
        assert_eq!(
            back["parts"][0]["pages"][0]["score"],
            json!({ "right": 2, "total": 3 })
        );
    }

    #[test]
    fn only_an_answered_page_becomes_debriefed() {
        let mut course = Course::new("raft", at_new_year());
        course.parts = vec![Part {
            title: "Why".to_owned(),
            pages: vec![
                page("1", PageState::Answered),
                page("2", PageState::Taught),
                page("3", PageState::Planned),
            ],
        }];
        let later = OffsetDateTime::from_unix_timestamp(1_767_225_660).unwrap();

        for id in ["1", "2", "3", "4"] {
            course.debrief_page(id, later);
        }

        assert_eq!(
            course.parts[0].pages,
            [
                page("1", PageState::Debriefed),
                page("2", PageState::Taught),
                page("3", PageState::Planned),
            ]
        );
        assert_eq!(course.updated, "2026-01-01T00:01:00Z");

        // A quiz pushed after the debrief asks for an answer again.
        course.push_quiz("1", later);
        assert_eq!(course.parts[0].pages[0].state, PageState::Taught);
    }

    #[test]
    fn a_score_changes_the_course_only_when_it_differs() {
        let mut course = Course::new("raft", at_new_year());
        course.parts = vec![Part {
            title: "Why".to_owned(),
            pages: vec![page("1", PageState::Answered)],
        }];
        let later = OffsetDateTime::from_unix_timestamp(1_767_225_660).unwrap();

        course.score_page("1", None, later);
        assert_eq!(course.updated, "2026-01-01T00:00:00Z");
        let half = Some(Score {
            right: 1.5,
            total: 3,
        });
        course.score_page("1", half, later);

        assert_eq!(course.parts[0].pages[0].score, half);
        assert_eq!(course.updated, "2026-01-01T00:01:00Z");
    }

    /// The detail carries no tag, so the wait of the agent must still read
    /// each kind back as itself.
    #[test]
    fn each_detail_reads_back_as_itself() {
        let details = [
            Detail::Next {
                left: PageName {
                    id: "1".to_owned(),
                    title: "Two leaders".to_owned(),
                },
            },
            Detail::Broken {
                block: BlockSource {
                    id: "0123456789ab".to_owned(),
                    kind: "diagram".to_owned(),
                    source: "graph LR\n".to_owned(),
                },
                error: "Parse error on line 1".to_owned(),
            },
            Detail::Answer {
                quiz: 2,
                open: Some(OpenAnswer {
                    prompt: "Why?".to_owned(),
                    points: vec!["random-timeout".to_owned()],
                    text: "So that two followers rarely wake up at once.".to_owned(),
                }),
                choices: vec![ChoiceAnswer {
                    header: "Split vote".to_owned(),
                    prompt: "What next?".to_owned(),
                    options: vec!["A *new* term".to_owned(), "The higher id".to_owned()],
                    picked: 1,
                    answer: 0,
                    ok: false,
                    other: None,
                    point: "split-vote".to_owned(),
                }],
            },
            Detail::Answer {
                quiz: 1,
                open: None,
                choices: Vec::new(),
            },
            Detail::Ask {
                thread: Thread {
                    id: "1-2".to_owned(),
                    page: "1".to_owned(),
                    block: Some("0123456789ab".to_owned()),
                    quote: Some("rand()".to_owned()),
                    comments: vec![Comment {
                        author: Author::Learner,
                        body: "Why *random*?".to_owned(),
                        at: "2026-01-01T00:00:00Z".to_owned(),
                    }],
                },
                block: Some(BlockSource {
                    id: "0123456789ab".to_owned(),
                    kind: "text".to_owned(),
                    source: "One term.".to_owned(),
                }),
                earlier: true,
            },
            Detail::Ask {
                thread: Thread {
                    id: "1-1".to_owned(),
                    page: "1".to_owned(),
                    block: None,
                    quote: None,
                    comments: Vec::new(),
                },
                block: None,
                earlier: false,
            },
            Detail::Chosen {
                questions: vec![ChoosingQuestion {
                    header: "Next step".to_owned(),
                    prompt: "Part 1 is done. What next?".to_owned(),
                    options: vec![
                        ChoosingOption {
                            label: "Part 2".to_owned(),
                            description: "The code tour".to_owned(),
                        },
                        ChoosingOption {
                            label: "A break".to_owned(),
                            description: String::new(),
                        },
                    ],
                    multi: false,
                }],
                answers: vec![ChosenLabels {
                    header: "Next step".to_owned(),
                    picked: vec![1],
                    labels: vec!["A break".to_owned()],
                    other: None,
                }],
            },
            Detail::Chosen {
                questions: Vec::new(),
                answers: Vec::new(),
            },
        ];

        for detail in details {
            let text = serde_json::to_string(&detail).unwrap();
            assert_eq!(serde_json::from_str::<Detail>(&text).unwrap(), detail);
        }
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
                    score: Some(Score {
                        right: 2.0,
                        total: 3,
                    }),
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
