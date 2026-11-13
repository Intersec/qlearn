//! The HTTP routes.
//!
//! Every answer but an image and the Markdown of the cheatsheet is JSON,
//! and every error carries the same shape, so the page and the commands
//! of the agent never have to guess what came back.

pub mod auth;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection, StringRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post, put};
use axum::{Json, Router, middleware};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::sync::RwLock;

use crate::choice::{self, StoredChoosing};
use crate::events::{self, Events};
use crate::highlight::Highlighter;
use crate::lesson::{self, Fault, Notes};
use crate::model::{
    self, AgentStatus, Author, Batch, Block, BlockSource, Broken, Cheatsheet, ChoiceId, ChoiceSent,
    ChoicesSent, Choosing, Course, CourseChange, Detail, Event, EventKind, Export, Finding, Graded,
    GradedPoint, OpenSent, Page, PageKind, Point, PointResult, QuizNumber, ReplySent, Session,
    StatusText, Thread, ThreadOpened,
};
use crate::quiz::{Refusal, StoredDebrief};
use crate::root::Root;
use crate::store::{self, PageFile, Store};
use crate::thread::{self, StoredComment, StoredThread};

#[derive(Clone)]
pub struct AppState {
    pub token: Arc<String>,
    /// The port of this server, which names its cookie.
    pub port: u16,
    /// The course of this server. The server is the only writer of the
    /// store, so the copy in memory is the course.
    pub course: Arc<RwLock<Course>>,
    /// The page that the agent pushed last. It changes under the write
    /// lock of the course.
    pub current: Arc<Mutex<Option<String>>>,
    pub store: Arc<Store>,
    /// The directory of the course. Its repository, when it has one, is
    /// where the excerpts are read.
    pub root: Root,
    /// The notes of the course, where a page may find an image. A server
    /// serves one course, so they never move.
    pub notes: Arc<Notes>,
    pub events: Arc<Events>,
    /// The broken blocks that the agent heard of, by page and block, with
    /// the request. A push of the page forgets its own.
    pub broken: Arc<Mutex<HashMap<(String, String), Event>>>,
}

impl AppState {
    /// `current` is the page that the agent pushed last, as the store says.
    pub fn new(
        root: Root,
        store: Store,
        course: Course,
        current: Option<String>,
        token: String,
        port: u16,
    ) -> Self {
        let notes = Notes::of(&store, &course.slug);
        Self {
            token: Arc::new(token),
            port,
            notes: Arc::new(notes),
            course: Arc::new(RwLock::new(course)),
            current: Arc::new(Mutex::new(current)),
            store: Arc::new(store),
            root,
            events: Arc::new(Events::default()),
            broken: Arc::default(),
        }
    }

    fn current(&self) -> Option<String> {
        self.current
            .lock()
            .expect("the current page is poisoned")
            .clone()
    }
}

/// The whole application: the routes, the interface, and the token guard.
///
/// The interface is inside the guard, not beside it. The first page load is
/// what carries the token and takes the cookie back, so a page served
/// outside the guard would never get one, and every API call would fail.
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/api/session", get(session))
        .route("/api/events", get(wait_events))
        .route("/api/course", patch(change_course))
        .route("/api/status", put(set_status))
        .route("/api/syllabus", put(write_syllabus))
        .route("/api/pages/{id}", get(page).put(push_page))
        .route("/api/pages/{id}/quizzes", post(push_quiz))
        .route("/api/pages/{id}/quizzes/{n}/open", post(send_open))
        .route("/api/pages/{id}/quizzes/{n}/choices", post(send_choices))
        .route("/api/pages/{id}/quizzes/{n}/debrief", put(write_debrief))
        .route("/api/points", get(points))
        .route("/api/export", get(export))
        .route("/api/pages/{id}/next", post(next))
        .route("/api/pages/{id}/broken", post(broken))
        .route("/api/pages/{id}/threads", post(open_thread))
        .route("/api/threads/{id}", post(reply))
        .route("/api/choices", post(choose))
        .route("/api/choices/{id}", post(answer_choice))
        .route("/api/findings/", put(write_unnamed_finding))
        .route("/api/findings/{id}", put(write_finding))
        .route("/api/media/{name}", get(media))
        .route("/api/cheatsheet", get(cheatsheet).put(write_cheatsheet))
        .route("/api/cheatsheet.md", get(cheatsheet_markdown))
        .method_not_allowed_fallback(method_not_allowed)
        .fallback(interface)
        .layer(middleware::from_fn_with_state(state.clone(), auth::guard))
        .layer(middleware::from_fn(timed))
        .with_state(state)
}

/// Time the whole request, so a trace shows what the browser waited for.
///
/// It sits outside the token guard: a request that fails the guard is worth
/// a line too.
async fn timed(request: axum::extract::Request, next: middleware::Next) -> Response {
    let started = crate::trace::start();
    let what = started.map(|_| format!("{} {}", request.method(), without_token(request.uri())));
    let response = next.run(request).await;

    crate::trace::since(started, || {
        format!(
            "{} -> {}",
            what.unwrap_or_default(),
            response.status().as_u16()
        )
    });
    response
}

/// The address that a trace may print. The token never belongs in a log.
fn without_token(uri: &axum::http::Uri) -> String {
    let path = uri.path();
    let Some(query) = uri.query() else {
        return path.to_owned();
    };

    let kept: Vec<&str> = query.split('&').filter(|p| !p.starts_with("t=")).collect();
    match kept.is_empty() {
        true => path.to_owned(),
        false => format!("{path}?{}", kept.join("&")),
    }
}

/// What the interface may load: its own files, and the images of a `data:`
/// address. A lesson loads nothing from the network (section 4.1 of the
/// design), and Mermaid fetches the image of a shape, or the `url()` of a
/// style, before DOMPurify sees its SVG. Mermaid writes its styles inline.
const POLICY: &str = "default-src 'self'; img-src 'self' data:; \
                      style-src 'self' 'unsafe-inline'; object-src 'none'; base-uri 'none'";

async fn interface(uri: axum::http::Uri) -> Response {
    match crate::assets::get(uri.path()) {
        Some((body, mime)) => (
            StatusCode::OK,
            [
                (axum::http::header::CONTENT_TYPE, mime),
                (
                    axum::http::header::CONTENT_SECURITY_POLICY,
                    POLICY.to_owned(),
                ),
            ],
            axum::body::Body::from(body),
        )
            .into_response(),
        // A route that does not exist lands here too, and it owes the
        // error shape of the API.
        None => ApiError::not_found(format!("{} names nothing", uri.path())).into_response(),
    }
}

// axum answers a wrong method with an empty body, which no client can read.
async fn method_not_allowed(method: axum::http::Method, uri: axum::http::Uri) -> ApiError {
    ApiError::method_not_allowed(format!("{} does not take {method}", uri.path()))
}

async fn session(State(state): State<AppState>) -> Json<Session> {
    let course = state.course.read().await;
    let (choices, choices_error) = match state.store.load_choices(&course.slug) {
        Ok(choices) => (choices, None),
        Err(error) => (Vec::new(), Some(format!("{error:#}"))),
    };
    // A choice on a page comes with its page.
    let choices = choices
        .iter()
        .filter(|choice| choice.page.is_none())
        .map(StoredChoosing::view)
        .collect();

    // The page draws the body of a finding with no Markdown parser.
    let mut course = course.clone();
    course.findings = course.findings.iter().map(crate::finding::view).collect();

    Json(Session {
        course,
        current: state.current(),
        version: crate::version::LONG.to_owned(),
        choices,
        choices_error,
        server: state.events.server().to_owned(),
    })
}

/// The choices to show with a page. Every command of the agent probes the
/// session, and a quiz holds answers of the learner, so an unreadable
/// `choices.json` refuses only the routes of the choices. The session says
/// why.
fn shown_choices(state: &AppState, slug: &str) -> Vec<StoredChoosing> {
    state.store.load_choices(slug).unwrap_or_default()
}

/// The blocks of a page. A page with no content yet names nothing.
async fn page(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<Page>, ApiError> {
    let Path(id) = path.map_err(bad_path)?;
    let no_content = || ApiError::not_found(format!("page {id} has no content yet"));
    if PageKind::of_id(&id).is_none() {
        return Err(no_content());
    }

    // Held while the file is read, so a read never sees a push halfway
    // through.
    let course = state.course.read().await;
    // `course.json` is written last, so a file that it does not back is
    // the trace of a push that failed.
    if !course.has_content(&id) {
        return Err(no_content());
    }
    let Some(file) = state.store.load_page(&course.slug, &id)? else {
        return Err(no_content());
    };
    let choices = shown_choices(&state, &course.slug);

    Ok(Json(file.page(&id, &choices)))
}

/// Push the blocks of a page. The body is the source.
///
/// The whole page is checked before anything is stored, and a refused page
/// gets every error at once, so the agent fixes them in one pass.
async fn push_page(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
    body: Result<String, StringRejection>,
) -> Result<Json<Page>, ApiError> {
    let Path(id) = path.map_err(bad_path)?;
    let kind = store::check_page_id(&id).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let source = body.map_err(|rejection| ApiError {
        status: rejection.status(),
        code: "badRequest",
        message: rejection.body_text(),
    })?;

    let started = crate::trace::start();
    let parsed = lesson::parse(
        &source,
        state.root.git(),
        &state.notes,
        Highlighter::shared(),
    )
    .await
    .map_err(|faults| ApiError::invalid_page(&id, &faults))?;
    crate::trace::since(started, || {
        format!("parse page {id}, {} bytes", source.len())
    });

    let mut course = state.course.write().await;
    // The copy in memory changes only once every file is written, and a
    // write that fails puts the page back, so a push that answers an error
    // leaves the course and the page as they were.
    let mut changed = course.clone();
    changed.push_page(
        &id,
        kind,
        parsed.title.as_deref(),
        OffsetDateTime::now_utc(),
    );
    // The quizzes and the threads stay with the page: they hold answers of
    // the learner. A thread whose block is gone moves to the end of the
    // page. A file that the course does not back is the trace of a failed
    // push.
    let kept = match course.has_content(&id) {
        true => state.store.load_page(&changed.slug, &id)?,
        false => None,
    }
    .unwrap_or_default();
    let choices = shown_choices(&state, &changed.slug);
    let file = PageFile {
        blocks: parsed.blocks,
        hidden: parsed.hidden,
        quizzes: kept.quizzes,
        threads: kept.threads,
    };
    // The images first: a page file never names an image that is not
    // there.
    let media = state.store.save_media(&changed.slug, &parsed.media)?;
    let undo = match state.store.save_page(&changed.slug, &id, &source, &file) {
        Ok(undo) => media.and(undo),
        Err(error) => return Err(media.after(error).into()),
    };
    if let Err(error) = state.store.save(&changed) {
        return Err(undo.after(error).into());
    }
    *course = changed;
    *state.current.lock().expect("the current page is poisoned") = Some(id.clone());
    // A new version of the page: a block that is still broken is news.
    state
        .broken
        .lock()
        .expect("the broken blocks are poisoned")
        .retain(|(page, _), _| *page != id);
    // Under the lock, so the events come in the order of the writes.
    state
        .events
        .push(EventKind::Page, Author::Agent, Some(&id), None);
    drop(course);

    Ok(Json(file.page(&id, &choices)))
}

/// Add a quiz to a page with content. Answers its number in the page.
///
/// A refused quiz gets every fault at once, so the agent fixes them in one
/// pass. The right options stay in the store: the page gets the quiz
/// without them.
async fn push_quiz(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<QuizNumber>, ApiError> {
    let Path(id) = path.map_err(bad_path)?;
    store::check_page_id(&id).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let Json(body) = body.map_err(bad_json)?;

    let mut course = state.course.write().await;
    // A quiz sits under its lesson, so the lesson comes first.
    let no_content = || {
        ApiError::not_found(format!(
            "page {id} has no content yet. Push the page first, with `qlearn page {id}`"
        ))
    };
    if !course.has_content(&id) {
        return Err(no_content());
    }
    let mut file = state
        .store
        .load_page(&course.slug, &id)?
        .ok_or_else(no_content)?;
    let mut points = state.store.load_points(&course.slug)?;
    let parsed = crate::quiz::parse(&body, &id, &course, &points)
        .map_err(|faults| ApiError::invalid_quiz(&faults))?;

    // The copy in memory changes only once every file is written, and a
    // write that fails puts the others back, so a quiz that answers an
    // error leaves the course and the page as they were.
    let mut changed = course.clone();
    changed.push_quiz(&id, OffsetDateTime::now_utc());
    file.quizzes.push(parsed.quiz);
    let n = file.quizzes.len();
    points.extend(parsed.points);
    // The page file goes last: the page serves a quiz once that file holds
    // it. A server killed before leaves points that no quiz names and a
    // page back to taught, so the agent sends the same quiz again and it
    // lands once.
    let mut undo = state.store.save_points(&changed.slug, &points)?;
    match state.store.save_undoable(&changed) {
        Ok(later) => undo = undo.and(later),
        Err(error) => return Err(undo.after(error).into()),
    }
    if let Err(error) = state.store.save_blocks(&changed.slug, &id, &file) {
        return Err(undo.after(error).into());
    }
    *course = changed;
    // Under the lock, so the events come in the order of the writes.
    state.events.push(
        EventKind::Quiz,
        Author::Agent,
        Some(&id),
        Some(&n.to_string()),
    );
    drop(course);

    Ok(Json(QuizNumber { page: id, n }))
}

/// The learner sends the open answer of a quiz. Answers the page.
async fn send_open(
    State(state): State<AppState>,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Result<Json<OpenSent>, JsonRejection>,
) -> Result<Json<Page>, ApiError> {
    let Path((id, n)) = path.map_err(bad_path)?;
    let Json(body) = body.map_err(bad_json)?;

    send(&state, &id, &n, Part::Open(body.text)).await
}

/// The learner sends the choices of a quiz. The server grades them, and
/// answers the page with the right options.
async fn send_choices(
    State(state): State<AppState>,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Result<Json<ChoicesSent>, JsonRejection>,
) -> Result<Json<Page>, ApiError> {
    let Path((id, n)) = path.map_err(bad_path)?;
    let Json(body) = body.map_err(bad_json)?;

    send(&state, &id, &n, Part::Choices(body.choices)).await
}

/// A part of the answer to a quiz.
enum Part {
    Open(String),
    Choices(Vec<model::Pick>),
}

/// Store a part of the answer to the quiz `n` of a page.
///
/// A choice adds its result to its point at once. Once the learner has
/// sent the last part of the quiz, the agent gets an `answer` request, and
/// the page is answered when no quiz of it is left without its answers.
async fn send(state: &AppState, id: &str, n: &str, part: Part) -> Result<Json<Page>, ApiError> {
    // Held for the whole change, so two tabs cannot send one part twice.
    let mut course = state.course.write().await;
    // The page answers a quiz that it shows, so a page with no content is
    // a request that went astray.
    let no_content = || ApiError::not_found(format!("page {id} has no content yet"));
    if PageKind::of_id(id).is_none() || !course.has_content(id) {
        return Err(no_content());
    }
    let mut file = state
        .store
        .load_page(&course.slug, id)?
        .ok_or_else(no_content)?;
    let at = quiz_number(&file, id, n)?;
    let choices = shown_choices(state, &course.slug);
    let quiz = &mut file.quizzes[at - 1];

    let now = OffsetDateTime::now_utc();
    let mut points = None;
    match part {
        Part::Open(text) => quiz.send_open(&text).map_err(refused)?,
        Part::Choices(picks) => {
            let stamp = model::stamp(now);
            let grades = quiz.send_choices(&picks, &stamp).map_err(refused)?;
            let mut known = state.store.load_points(&course.slug)?;
            add_results(&mut known, grades, &stamp)?;
            points = Some(known);
        }
    }
    let done = quiz.answered().then(|| quiz.detail(at));

    // The copy in memory changes only once every file is written, and a
    // write that fails puts the others back, so an answer that gets an
    // error leaves the store as it was and the learner sends it again.
    let mut changed = course.clone();
    if file.answered() {
        changed.answer_page(id, now);
    }
    changed.score_page(id, file.score(), now);
    // The page file goes first: it holds the answer, and the other files
    // follow from it. A server killed before them writes them at its next
    // start, in `settle`, so the learner never sends a part twice.
    let mut undo = state.store.save_blocks(&changed.slug, id, &file)?;
    if let Some(points) = &points {
        match state.store.save_points(&changed.slug, points) {
            Ok(later) => undo = undo.and(later),
            Err(error) => return Err(undo.after(error).into()),
        }
    }
    if changed != *course
        && let Err(error) = state.store.save_undoable(&changed)
    {
        return Err(undo.after(error).into());
    }
    *course = changed;
    // Under the lock, so the events come in the order of the writes. The
    // other tabs of the page show each part, and only the last one needs
    // the agent.
    let n = at.to_string();
    state
        .events
        .push(EventKind::Answered, Author::Learner, Some(id), Some(&n));
    if let Some(detail) = done {
        state
            .events
            .request(EventKind::Answer, Some(id), Some(&n), detail);
    }
    drop(course);

    Ok(Json(file.page(id, &choices)))
}

/// The number of the quiz `n` of a page, from 1, as the address gives it.
fn quiz_number(file: &PageFile, id: &str, n: &str) -> Result<usize, ApiError> {
    let count = file.quizzes.len();
    // `+1` and `01` parse too, and name no quiz in the address.
    n.parse::<usize>()
        .ok()
        .filter(|at| (1..=count).contains(at) && at.to_string() == n)
        .ok_or_else(|| ApiError::not_found(format!("page {id} holds no quiz {n}")))
}

/// The name that the faults of the source of a debrief give it. The
/// command of the agent puts the name of its own file in its place.
pub const DEBRIEF_FILE: &str = "debrief.md";

/// The page file of a page with content, and the number of its quiz `n`,
/// once the learner answered that quiz.
fn answered_quiz(
    store: &Store,
    course: &Course,
    id: &str,
    n: &str,
) -> Result<(PageFile, usize), ApiError> {
    let no_content = || ApiError::not_found(format!("page {id} has no content yet"));
    if PageKind::of_id(id).is_none() || !course.has_content(id) {
        return Err(no_content());
    }
    let file = store.load_page(&course.slug, id)?.ok_or_else(no_content)?;
    let at = quiz_number(&file, id, n)?;
    // A debrief comments on the answers and grades the open one, so it waits for them.
    if !file.quizzes[at - 1].answered() {
        return Err(ApiError::conflict(format!(
            "the learner has not answered quiz {at} of page {id} yet. Write its debrief once \
             the answer reaches your wait"
        )));
    }
    Ok((file, at))
}

/// Write the debrief of the quiz `n` of a page, with the grade of each
/// point of its open question. Answers the page.
///
/// The source is a page source, checked as a push of a page is, and a
/// refused debrief gets every fault at once. A debrief written again
/// replaces the first one and the results that it recorded, so the agent
/// can fix a diagram that the page cannot draw.
async fn write_debrief(
    State(state): State<AppState>,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<Page>, ApiError> {
    let Path((id, n)) = path.map_err(bad_path)?;
    let Json(body) = body.map_err(bad_json)?;

    // Checked before the parse, which reads the repository, so a debrief
    // of no quiz gets that answer first.
    let (quiz, at) = {
        let course = state.course.read().await;
        let (file, at) = answered_quiz(&state.store, &course, &id, &n)?;
        (file.quizzes[at - 1].clone(), at)
    };
    let read = crate::quiz::read_debrief(&body, &quiz);
    let parsed = match &read.source {
        Some(source) => Some(
            lesson::parse_debrief(
                source,
                at,
                state.root.git(),
                &state.notes,
                Highlighter::shared(),
            )
            .await,
        ),
        None => None,
    };
    let mut faults: Vec<String> = match &parsed {
        Some(Err(faults)) => faults
            .iter()
            .map(|fault| fault.display(DEBRIEF_FILE))
            .collect(),
        _ => Vec::new(),
    };
    faults.extend(read.faults);
    let (Some(source), Some(Ok(parsed)), true) = (read.source, parsed, faults.is_empty()) else {
        return Err(ApiError::invalid_debrief(&faults));
    };

    let mut course = state.course.write().await;
    // The quizzes of a page only grow, and an answer stays: the checks above
    // still hold, and this reads the file that the write starts from.
    let (mut file, at) = answered_quiz(&state.store, &course, &id, &n)?;
    let mut points = state.store.load_points(&course.slug)?;
    let choices = shown_choices(&state, &course.slug);
    let mut graded = Vec::new();
    for (point, result) in read.grades {
        let label = crate::quiz::point_of(&mut points, &point)?.label.clone();
        graded.push(GradedPoint {
            id: point,
            label,
            result,
        });
    }
    let now = OffsetDateTime::now_utc();
    let quiz = &mut file.quizzes[at - 1];
    // A debrief written again grades the same answer: its results keep the
    // date and the place that the first one gave them.
    let stamp = match &quiz.debrief {
        Some(earlier) => earlier.at.clone(),
        None => model::stamp(now),
    };
    let images = parsed.media;
    let debrief = StoredDebrief {
        source,
        blocks: parsed.blocks,
        points: graded,
        at: stamp,
    };
    debrief.record(&mut points)?;
    let gone: Vec<String> = quiz
        .debrief
        .iter()
        .flat_map(|earlier| &earlier.blocks)
        .map(|block| block.id().to_owned())
        .collect();
    quiz.debrief = Some(debrief);

    // The copy in memory changes only once every file is written, and a
    // write that fails puts the others back, so a debrief that gets an
    // error leaves the store as it was.
    let mut changed = course.clone();
    if file.debriefed() {
        changed.debrief_page(&id, now);
    }
    changed.score_page(&id, file.score(), now);
    // The page file goes first: it holds the debrief, and the other files
    // follow from it. A server killed before them writes them at its next
    // start, in `settle`.
    let media = state.store.save_media(&changed.slug, &images)?;
    let mut undo = match state.store.save_blocks(&changed.slug, &id, &file) {
        Ok(undo) => media.and(undo),
        Err(error) => return Err(media.after(error).into()),
    };
    match state.store.save_points(&changed.slug, &points) {
        Ok(later) => undo = undo.and(later),
        Err(error) => return Err(undo.after(error).into()),
    }
    if changed != *course
        && let Err(error) = state.store.save_undoable(&changed)
    {
        return Err(undo.after(error).into());
    }
    *course = changed;
    // A new version of the debrief: a block of it that is still broken is
    // news.
    state
        .broken
        .lock()
        .expect("the broken blocks are poisoned")
        .retain(|(page, block), _| !(*page == id && gone.contains(block)));
    // Under the lock, so the events come in the order of the writes.
    state.events.push(
        EventKind::Debrief,
        Author::Agent,
        Some(&id),
        Some(&at.to_string()),
    );
    drop(course);

    Ok(Json(file.page(&id, &choices)))
}

/// What an image may do once a browser opens it: draw, with its own
/// styles, and nothing more. An SVG opened in a tab of its own is a page of
/// this origin, so `sandbox` stops its scripts and gives it no cookie.
const MEDIA_POLICY: &str = "default-src 'none'; style-src 'unsafe-inline'; img-src data:; sandbox";

/// An image of the course, as its bytes, with the type that its name says.
///
/// The only answer that is not JSON. The name is the hash of the bytes, so
/// a browser may keep the answer for good.
async fn media(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
) -> Result<Response, ApiError> {
    use axum::http::header;

    let Path(name) = path.map_err(bad_path)?;
    let none = || ApiError::not_found(format!("{name} names no image of the course"));
    let Some(kind) = model::ImageType::of_media(&name) else {
        return Err(none());
    };
    let course = state.course.read().await;
    let bytes = state
        .store
        .load_media(&course.slug, &name)?
        .ok_or_else(none)?;
    drop(course);

    Ok((
        [
            (header::CONTENT_TYPE, kind.mime()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable",
            ),
            (header::CONTENT_SECURITY_POLICY, MEDIA_POLICY),
        ],
        bytes,
    )
        .into_response())
}

/// The points of the course, in the order of section 5.3 of the design:
/// the order in which the agent picks the point of a question on an
/// earlier lesson.
async fn points(State(state): State<AppState>) -> Result<Json<Vec<Point>>, ApiError> {
    let course = state.course.read().await;
    let points = state.store.load_points(&course.slug)?;
    drop(course);

    Ok(Json(crate::quiz::in_order(points)))
}

/// The whole course, for an agent that lost its context. See section 8.4
/// of the design.
async fn export(State(state): State<AppState>) -> Result<Json<Export>, ApiError> {
    // Held while the files are read, so the export never sees a write
    // halfway through.
    let course = state.course.read().await;
    let points = state.store.load_points(&course.slug)?;
    let export = crate::export::build(&state.store, &course, state.current(), points)?;
    drop(course);

    Ok(Json(export))
}

/// Add the result of each graded choice to its point, at the stamp `at`.
fn add_results(
    points: &mut [model::Point],
    grades: Vec<(String, model::Grade)>,
    at: &str,
) -> anyhow::Result<()> {
    for (point, grade) in grades {
        crate::quiz::point_of(points, &point)?
            .results
            .push(PointResult {
                at: at.to_owned(),
                result: grade,
                how: Graded::Choice,
            });
    }
    Ok(())
}

/// Write what the page files say and points.json or course.json does not.
/// Called before the server starts.
///
/// An answer and a debrief write the page file first, so a server killed in
/// the middle leaves graded choices with no results, the results of a
/// debrief that a new one replaced, or a page whose state and score are
/// behind its quizzes.
pub fn settle(store: &Store, course: &mut Course) -> anyhow::Result<()> {
    let before = store.load_points(&course.slug)?;
    let (changed, points) = settled(store, course, before.clone())?;

    // points.json first: a page that is answered has its results.
    if points != before {
        store.save_points(&course.slug, &points)?;
    }
    if changed != *course {
        store.save(&changed)?;
        *course = changed;
    }
    Ok(())
}

/// The course and the points as `settle` would write them, from `points`,
/// those of points.json. Writes nothing, so `qlearn export` with no server
/// reads the course as a server would show it.
pub fn settled(
    store: &Store,
    course: &Course,
    mut points: Vec<Point>,
) -> anyhow::Result<(Course, Vec<Point>)> {
    let mut changed = course.clone();
    let now = OffsetDateTime::now_utc();
    let mut debriefs: Vec<StoredDebrief> = Vec::new();
    let ids: Vec<String> = course
        .parts
        .iter()
        .flat_map(|part| &part.pages)
        .filter(|page| course.has_content(&page.id))
        .map(|page| page.id.clone())
        .collect();
    for id in ids {
        let Some(file) = store.load_page(&course.slug, &id)? else {
            continue;
        };
        for quiz in &file.quizzes {
            debriefs.extend(quiz.debrief.clone());
            let Some(at) = &quiz.sent.graded else {
                continue;
            };
            // One write of points.json holds every result of a grading, so
            // one of them is enough to know.
            if !holds(&points, Graded::Choice, at) {
                add_results(&mut points, quiz.grades(), at)?;
            }
        }
        if file.answered() {
            changed.answer_page(&id, now);
        }
        if file.debriefed() {
            changed.debrief_page(&id, now);
        }
        changed.score_page(&id, file.score(), now);
    }

    // A debrief grades its results in place, so this also completes one
    // that was written again. A result stays even when no debrief names it:
    // it is an answer of the learner.
    for debrief in &debriefs {
        debrief.record(&mut points)?;
    }
    Ok((changed, points))
}

/// Whether a point holds a result that `how` gave at the stamp `at`.
fn holds(points: &[Point], how: Graded, at: &str) -> bool {
    points.iter().any(|point| {
        point
            .results
            .iter()
            .any(|result| result.how == how && result.at == at)
    })
}

/// Change the title, the goal, the profile, the quiz mode, the status or
/// the follow-up date. Answers the course.
async fn change_course(
    State(state): State<AppState>,
    body: Result<Json<CourseChange>, JsonRejection>,
) -> Result<Json<Course>, ApiError> {
    let Json(change) = body.map_err(bad_json)?;

    let mut course = state.course.write().await;
    // The copy in memory changes only once `course.json` is written, so a
    // write that fails leaves the course as the store has it.
    let mut changed = course.clone();
    changed
        .change(change, OffsetDateTime::now_utc())
        .map_err(ApiError::bad_request)?;
    state.store.save(&changed)?;
    *course = changed;
    // Under the lock, so the events come in the order of the writes.
    state
        .events
        .push(EventKind::Course, Author::Agent, None, None);

    Ok(Json(course.clone()))
}

/// Write the syllabus as a whole. Answers the course.
///
/// The state and the score of each page come from the store. A refused
/// syllabus gets every fault at once, so the agent fixes them in one pass.
async fn write_syllabus(
    State(state): State<AppState>,
    body: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<Course>, ApiError> {
    let Json(body) = body.map_err(bad_json)?;

    let mut course = state.course.write().await;
    let parts = crate::syllabus::plan(&body, &course)
        .map_err(|faults| ApiError::invalid_syllabus(&faults))?;
    // The copy in memory changes only once `course.json` is written, so a
    // write that fails leaves the course as the store has it.
    let mut changed = course.clone();
    changed.parts = parts;
    changed.updated = model::stamp(OffsetDateTime::now_utc());
    state.store.save(&changed)?;
    *course = changed;
    // Under the lock, so the events come in the order of the writes.
    state
        .events
        .push(EventKind::Syllabus, Author::Agent, None, None);

    Ok(Json(course.clone()))
}

/// Say what the agent does, until its next wait.
async fn set_status(
    State(state): State<AppState>,
    body: Result<Json<StatusText>, JsonRejection>,
) -> Result<Json<AgentStatus>, ApiError> {
    let Json(body) = body.map_err(bad_json)?;

    Ok(Json(AgentStatus {
        status: state.events.set_status(&body.text),
    }))
}

/// The learner asks the agent for the next page. Answers the request.
async fn next(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<Event>, ApiError> {
    let Path(id) = path.map_err(bad_path)?;

    // Held while the request is pushed, so the events come in the order of
    // the writes.
    let course = state.course.read().await;
    // The page sends Next from a page that it shows, so a page with no
    // content is a request that went astray.
    let left = course
        .name_of(&id)
        .ok_or_else(|| ApiError::not_found(format!("page {id} has no content yet")))?;
    let mut event = state
        .events
        .request(EventKind::Next, Some(&id), None, Detail::Next { left });
    drop(course);

    event.detail = None;
    Ok(Json(event))
}

/// The page cannot draw a block of a page or of a debrief. Answers the
/// request, which hands the agent the source of the block and the error of
/// the page. The request names the quiz of a block of a debrief.
///
/// Only a diagram can fail: the server checks every other block before it
/// stores the page. A diagram of a steps block reports the steps block,
/// and the error names the step, such as `steps[2].diagram: ...`.
async fn broken(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<Broken>, JsonRejection>,
) -> Result<Json<Event>, ApiError> {
    let Path(id) = path.map_err(bad_path)?;
    let Json(body) = body.map_err(bad_json)?;

    // Held while the request is pushed, so the events come in the order of
    // the writes, and a push cannot change the block under the check.
    let course = state.course.read().await;
    let no_content = || ApiError::not_found(format!("page {id} has no content yet"));
    // The page reports a block that it shows, so a page with no content is
    // a request that went astray.
    if !course.has_content(&id) {
        return Err(no_content());
    }
    let file = state
        .store
        .load_page(&course.slug, &id)?
        .ok_or_else(no_content)?;
    // A block that a later push took away: the agent has nothing to fix. A
    // block of a debrief names its quiz, so the agent knows to write that
    // debrief again rather than the page.
    let (block, quiz) = file
        .block(&body.block)
        .ok_or_else(|| ApiError::not_found(format!("page {id} holds no block {:?}", body.block)))?;
    let source = match block {
        Block::Diagram { source, .. } => BlockSource {
            id: body.block.clone(),
            kind: block.kind().to_owned(),
            source: source.clone(),
        },
        // The agent fixes the fence, which holds every step.
        Block::Steps { steps, .. } if steps.steps.iter().any(|step| step.diagram.is_some()) => {
            block_source(&state.store, &course.slug, &id, &file, &body.block)?
        }
        _ => {
            return Err(ApiError::bad_request(format!(
                "block {} of page {id} is a {} block with no diagram, which the server \
                 checked: only a diagram can fail to draw",
                body.block,
                block.kind()
            )));
        }
    };
    let quiz = quiz.map(|n| n.to_string());
    // A reload, a visit or a second tab reports the block again. The agent
    // hears of it once for each push of the page.
    let mut heard = state.broken.lock().expect("the broken blocks are poisoned");
    let key = (id.clone(), body.block.clone());
    let mut event = match heard.get(&key) {
        Some(event) => event.clone(),
        None => {
            let detail = Detail::Broken {
                block: source,
                error: body.error,
            };
            let event = state
                .events
                .request(EventKind::Broken, Some(&id), quiz.as_deref(), detail);
            heard.insert(key, event.clone());
            event
        }
    };
    drop(heard);
    drop(course);

    event.detail = None;
    Ok(Json(event))
}

/// Open a thread on a block of a page, or about the whole page. Answers
/// the thread as the page sees it.
///
/// Both parties write here. A thread of the learner is a question, so the
/// agent gets an `ask` request. A thread of the agent is a `thread` event.
async fn open_thread(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<ThreadOpened>, JsonRejection>,
) -> Result<Json<Thread>, ApiError> {
    let Path(id) = path.map_err(bad_path)?;
    let Json(body) = body.map_err(bad_json)?;
    let text = thread::body(&body.body).map_err(ApiError::bad_request)?;
    let quote = thread::quote(body.quote.as_deref());
    if quote.is_some() && body.block.is_none() {
        return Err(ApiError::bad_request(
            "a quote is text of a block: name its block, or send no quote".to_owned(),
        ));
    }
    let author = body.author.unwrap_or(Author::Learner);

    // Held for the whole change, so two threads never take one id, and a
    // push cannot take the block away under the check.
    let course = state.course.write().await;
    let no_content = || ApiError::not_found(format!("page {id} has no content yet"));
    if PageKind::of_id(&id).is_none() || !course.has_content(&id) {
        return Err(no_content());
    }
    let mut file = state
        .store
        .load_page(&course.slug, &id)?
        .ok_or_else(no_content)?;
    let block = match &body.block {
        Some(block) => Some(block_source(&state.store, &course.slug, &id, &file, block)?),
        None => None,
    };
    let stored = StoredThread {
        id: thread::next_id(&id, &file.threads),
        block,
        quote,
        comments: vec![StoredComment {
            author,
            body: text,
            at: model::stamp(OffsetDateTime::now_utc()),
        }],
    };
    file.threads.push(stored.clone());
    state.store.save_blocks(&course.slug, &id, &file)?;
    // Under the lock, so the events come in the order of the writes.
    tell(&state, &file, &id, &stored, author, EventKind::Thread);
    drop(course);

    Ok(Json(stored.view(&id, lesson::markdown)))
}

/// Write in a thread. Answers the thread as the page sees it.
///
/// A reply of the learner asks the agent again, with an `ask` request. A
/// reply of the agent is a `reply` event. A thread whose block is gone
/// still takes a reply: it holds answers of the learner.
async fn reply(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<ReplySent>, JsonRejection>,
) -> Result<Json<Thread>, ApiError> {
    let Path(id) = path.map_err(bad_path)?;
    let Json(body) = body.map_err(bad_json)?;
    let text = thread::body(&body.body).map_err(ApiError::bad_request)?;
    let author = body.author.unwrap_or(Author::Learner);

    let unknown = || ApiError::not_found(format!("the course holds no thread {id:?}"));
    let (page, _) = thread::split_id(&id).ok_or_else(unknown)?;
    // Held for the whole change, so two replies never lose one another.
    let course = state.course.write().await;
    if !course.has_content(page) {
        return Err(unknown());
    }
    let mut file = state
        .store
        .load_page(&course.slug, page)?
        .ok_or_else(unknown)?;
    let at = file
        .threads
        .iter()
        .position(|stored| stored.id == id)
        .ok_or_else(unknown)?;
    file.threads[at].comments.push(StoredComment {
        author,
        body: text,
        at: model::stamp(OffsetDateTime::now_utc()),
    });
    state.store.save_blocks(&course.slug, page, &file)?;
    let stored = file.threads[at].clone();
    // Under the lock, so the events come in the order of the writes.
    tell(&state, &file, page, &stored, author, EventKind::Reply);
    drop(course);

    Ok(Json(stored.view(page, lesson::markdown)))
}

/// Tell the other party of a write in a thread. A write of the learner is
/// an `ask` request, with the thread and the source of its block, so the
/// agent needs no second command. A write of the agent is `kind`.
fn tell(
    state: &AppState,
    file: &PageFile,
    page: &str,
    stored: &StoredThread,
    author: Author,
    kind: EventKind,
) {
    match author {
        Author::Learner => {
            let detail = Detail::Ask {
                thread: stored.view(page, str::to_owned),
                block: stored.block.clone(),
                earlier: file.lost(stored),
            };
            state
                .events
                .request(EventKind::Ask, Some(page), Some(&stored.id), detail);
        }
        Author::Agent => state
            .events
            .push(kind, Author::Agent, Some(page), Some(&stored.id)),
    }
}

/// A block that the page holds, in its lesson or in a debrief, as the agent
/// wrote it.
fn block_source(
    store: &Store,
    slug: &str,
    id: &str,
    file: &PageFile,
    block: &str,
) -> Result<BlockSource, ApiError> {
    // A block that a later push took away is not one to ask about: the
    // page that shows it is out of date.
    let (found, quiz) = file
        .block(block)
        .ok_or_else(|| ApiError::not_found(format!("page {id} holds no block {block:?}")))?;
    let (source, within) = match quiz {
        None => (
            store.load_source(slug, id)?.unwrap_or_default(),
            lesson::Within::Lesson,
        ),
        Some(n) => (
            file.quizzes[n - 1]
                .debrief
                .as_ref()
                .map(|debrief| debrief.source.clone())
                .unwrap_or_default(),
            lesson::Within::Debrief(n),
        ),
    };
    // The page file and its source are written together, so the source
    // holds the block. The kind alone still tells the agent where to look.
    Ok(lesson::sources(&source, within)
        .into_iter()
        .find(|source| source.id == block)
        .unwrap_or_else(|| BlockSource {
            id: block.to_owned(),
            kind: found.kind().to_owned(),
            source: String::new(),
        }))
}

/// Ask the learner to choose, at the end of a page or at the top of the
/// main column. Answers the id of the choice.
///
/// A refused choice gets every fault at once, so the agent fixes them in
/// one pass.
async fn choose(
    State(state): State<AppState>,
    body: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<ChoiceId>, ApiError> {
    let Json(body) = body.map_err(bad_json)?;

    // Held for the whole change, so two choices never take one id, and the
    // page that a choice names keeps its content under the check.
    let course = state.course.write().await;
    let parsed =
        choice::parse(&body, &course).map_err(|faults| ApiError::invalid_choice(&faults))?;
    let mut choices = state.store.load_choices(&course.slug)?;
    let id = choice::next_id(&choices);
    choices.push(StoredChoosing {
        id: id.clone(),
        page: parsed.page.clone(),
        questions: parsed.questions,
        at: model::stamp(OffsetDateTime::now_utc()),
        answered: None,
    });
    state.store.save_choices(&course.slug, &choices)?;
    // Under the lock, so the events come in the order of the writes.
    state.events.push(
        EventKind::Choose,
        Author::Agent,
        parsed.page.as_deref(),
        Some(&id),
    );
    drop(course);

    Ok(Json(ChoiceId { id }))
}

/// The learner answers a choice. Answers the choice as the page sees it.
///
/// The agent gets a `chosen` request, with the labels of the picks, so it
/// needs no second command.
async fn answer_choice(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<ChoiceSent>, JsonRejection>,
) -> Result<Json<Choosing>, ApiError> {
    let Path(id) = path.map_err(bad_path)?;
    let Json(body) = body.map_err(bad_json)?;

    let unknown = || ApiError::not_found(format!("the course holds no choice {id:?}"));
    choice::rank_of(&id).ok_or_else(unknown)?;
    // Held for the whole change, so two tabs cannot answer one choice twice.
    let course = state.course.write().await;
    let mut choices = state.store.load_choices(&course.slug)?;
    let at = choices
        .iter()
        .position(|stored| stored.id == id)
        .ok_or_else(unknown)?;
    let stored = &mut choices[at];
    stored
        .answer(&body.answers, &model::stamp(OffsetDateTime::now_utc()))
        .map_err(refused)?;
    let stored = stored.clone();
    state.store.save_choices(&course.slug, &choices)?;
    // Under the lock, so the events come in the order of the writes.
    state.events.request(
        EventKind::Chosen,
        stored.page.as_deref(),
        Some(&stored.id),
        stored.detail(),
    );
    drop(course);

    Ok(Json(stored.view()))
}

/// Write a finding: a new one, or in place of the one with its id.
/// Answers the finding as the agent wrote it, in Markdown.
///
/// A refused finding gets every fault at once, so the agent fixes them in
/// one pass.
async fn write_finding(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<Finding>, ApiError> {
    let Path(id) = path.map_err(bad_path)?;
    let Json(body) = body.map_err(bad_json)?;
    let finding =
        crate::finding::check(&id, &body).map_err(|faults| ApiError::invalid_finding(&faults))?;

    let mut course = state.course.write().await;
    // The copy in memory changes only once `course.json` is written, so a
    // write that fails leaves the course as the store has it.
    let mut changed = course.clone();
    crate::finding::write(&mut changed.findings, finding.clone());
    changed.updated = model::stamp(OffsetDateTime::now_utc());
    state.store.save(&changed)?;
    *course = changed;
    // Under the lock, so the events come in the order of the writes.
    state
        .events
        .push(EventKind::Finding, Author::Agent, None, Some(&finding.id));
    drop(course);

    Ok(Json(finding))
}

/// The cheatsheet, with Map of the code and To revisit. Before the agent
/// writes it, `written` is false and every list is empty: the page asks at
/// each load, and an answer of 404 would be an error in its console.
async fn cheatsheet(State(state): State<AppState>) -> Result<Json<Cheatsheet>, ApiError> {
    // Held while the files are read, so a read never sees a write halfway
    // through.
    let course = state.course.read().await;
    let sheet = state.store.load_cheatsheet(&course.slug)?;
    let sheet = crate::cheatsheet::build(&state.store, &course, sheet)?;
    drop(course);

    Ok(Json(sheet))
}

/// The cheatsheet as Markdown, as a file to save: the export for a wiki
/// (section 9 of the design).
///
/// Before the write, a 404: the page asks only from its button, which shows
/// once the cheatsheet is written, and the agent learns which command
/// writes it.
async fn cheatsheet_markdown(State(state): State<AppState>) -> Result<Response, ApiError> {
    use axum::http::header;

    // Held while the files are read, so a read never sees a write halfway
    // through.
    let course = state.course.read().await;
    let text = crate::cheatsheet::markdown(&state.store, &course, || {
        state.store.load_points(&course.slug)
    })?
    .ok_or_else(|| ApiError::not_found(crate::cheatsheet::NOT_WRITTEN.to_owned()))?;
    // A slug holds lowercase letters, digits and hyphens only, so the name
    // needs no quoting rule.
    let name = format!("attachment; filename=\"{}-cheatsheet.md\"", course.slug);
    drop(course);

    Ok((
        [
            (
                header::CONTENT_TYPE,
                "text/markdown; charset=utf-8".to_owned(),
            ),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_owned()),
            (header::CONTENT_DISPOSITION, name),
        ],
        text,
    )
        .into_response())
}

/// Write the cheatsheet, in place of the one before. The body is the
/// source. Answers the cheatsheet, as the page gets it.
///
/// The whole source is checked before anything is stored, and a refused
/// source gets every error at once, so the agent fixes them in one pass.
async fn write_cheatsheet(
    State(state): State<AppState>,
    body: Result<String, StringRejection>,
) -> Result<Json<Cheatsheet>, ApiError> {
    let source = body.map_err(|rejection| ApiError {
        status: rejection.status(),
        code: "badRequest",
        message: rejection.body_text(),
    })?;
    let parsed = lesson::parse_cheatsheet(
        &source,
        state.root.git(),
        &state.notes,
        Highlighter::shared(),
    )
    .await
    .map_err(|faults| ApiError::invalid_cheatsheet(&faults))?;

    // The write lock keeps the store to one writer.
    let course = state.course.write().await;
    let file = crate::cheatsheet::CheatsheetFile {
        blocks: parsed.blocks,
    };
    // Built before the write: a store that cannot be read refuses the
    // cheatsheet, rather than store one that no read can show.
    let sheet = crate::cheatsheet::build(&state.store, &course, Some(file.clone()))?;
    // The images first: a cheatsheet never names an image that is not
    // there.
    let media = state.store.save_media(&course.slug, &parsed.media)?;
    if let Err(error) = state.store.save_cheatsheet(&course.slug, &source, &file) {
        return Err(media.after(error).into());
    }
    // Under the lock, so the events come in the order of the writes.
    state
        .events
        .push(EventKind::Cheatsheet, Author::Agent, None, None);
    drop(course);

    Ok(Json(sheet))
}

/// An empty id matches no `{id}`, and owes the refusal of a bad id, not
/// a route that names nothing.
async fn write_unnamed_finding(
    state: State<AppState>,
    body: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<Finding>, ApiError> {
    write_finding(state, Ok(Path(String::new())), body).await
}

/// An answer that the quiz does not take.
fn refused(refusal: Refusal) -> ApiError {
    match refusal {
        Refusal::NotFound(message) => ApiError::not_found(message),
        Refusal::Conflict(message) => ApiError::conflict(message),
        Refusal::BadRequest(message) => ApiError::bad_request(message),
    }
}

/// An extractor answers a body that it cannot read with plain text. Every
/// refusal of a body is a bad request, whichever check of the extractor
/// fails, so the agent reads one code.
fn bad_json(rejection: JsonRejection) -> ApiError {
    ApiError::bad_request(rejection.body_text())
}

/// An extractor answers a path that it cannot read with plain text.
fn bad_path(rejection: PathRejection) -> ApiError {
    ApiError {
        status: rejection.status(),
        code: "badRequest",
        message: rejection.body_text(),
    }
}

#[derive(Deserialize)]
struct EventsQuery {
    after: Option<u64>,
    who: Option<Who>,
}

/// Who waits. The server learns from it whether a page is open and whether
/// an agent is present.
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum Who {
    Page,
    Agent,
}

/// The writes after a number, as soon as there is one. See section 8.3 of
/// the design.
///
/// The course lock is not held: a wait of 30 seconds must not stop a write.
async fn wait_events(
    State(state): State<AppState>,
    query: Result<Query<EventsQuery>, QueryRejection>,
) -> Result<Json<Batch>, ApiError> {
    let Query(query) = query.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    let mut agent = (query.who == Some(Who::Agent)).then(|| state.events.agent_waits());
    let page = (query.who == Some(Who::Page)).then(|| state.events.page_waits());

    let mut batch = match &page {
        Some(page) => page.after(query.after, events::HOLD).await,
        None => state.events.after(query.after, events::HOLD).await,
    };
    // Reached only when the agent did not hang up, which drops this future.
    match &mut agent {
        Some(agent) => agent.answered(&batch),
        // Only the agent answers a request.
        None => batch
            .events
            .iter_mut()
            .for_each(|event| event.detail = None),
    }
    Ok(Json(batch))
}

/// One error shape for every route.
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn bad_request(message: String) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "badRequest",
            message,
        }
    }

    fn not_found(message: String) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "notFound",
            message,
        }
    }

    /// A write that the state of the store does not allow now, such as an
    /// answer sent twice.
    fn conflict(message: String) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: "conflict",
            message,
        }
    }

    fn method_not_allowed(message: String) -> Self {
        Self {
            status: StatusCode::METHOD_NOT_ALLOWED,
            code: "methodNotAllowed",
            message,
        }
    }

    /// A page that fails a check. The message holds one line per error,
    /// in the format of section 4.7 of the design. The file is the one of
    /// the store, `pages/<id>.md`.
    fn invalid_page(id: &str, faults: &[Fault]) -> Self {
        let file = format!("{id}.md");
        let lines: Vec<String> = faults.iter().map(|fault| fault.display(&file)).collect();

        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalidPage",
            message: lines.join("\n"),
        }
    }

    /// A cheatsheet that fails a check. The message holds one line per
    /// error, in the format of section 4.7 of the design, with the file of
    /// the store.
    fn invalid_cheatsheet(faults: &[Fault]) -> Self {
        let lines: Vec<String> = faults
            .iter()
            .map(|fault| fault.display(store::CHEATSHEET))
            .collect();

        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalidCheatsheet",
            message: lines.join("\n"),
        }
    }

    /// A quiz that fails a check. The message holds one line per fault,
    /// each with its place in the body.
    fn invalid_quiz(faults: &[String]) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalidQuiz",
            message: faults.join("\n"),
        }
    }

    /// A debrief that fails a check. The message holds one line per
    /// fault: those of the source in the format of section 4.7 of the
    /// design, then those of the points, each with its place in the body.
    fn invalid_debrief(faults: &[String]) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalidDebrief",
            message: faults.join("\n"),
        }
    }

    /// A choice that fails a check. The message holds one line per fault,
    /// each with its place in the body.
    fn invalid_choice(faults: &[String]) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalidChoice",
            message: faults.join("\n"),
        }
    }

    /// A syllabus that fails a check. The message holds one line per
    /// fault, each with its place in the body.
    fn invalid_syllabus(faults: &[String]) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalidSyllabus",
            message: faults.join("\n"),
        }
    }

    /// A finding that fails a check. The message holds one line per
    /// fault, each with its field.
    fn invalid_finding(faults: &[String]) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalidFinding",
            message: faults.join("\n"),
        }
    }

    fn forbidden(message: String) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "forbidden",
            message,
        }
    }

    fn unauthorized(message: String) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message,
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "failed",
            message: format!("{error:#}"),
        }
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: ErrorFields<'a>,
}

#[derive(Serialize)]
struct ErrorFields<'a> {
    code: &'a str,
    message: &'a str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            error: ErrorFields {
                code: self.code,
                message: &self.message,
            },
        };

        (self.status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, header};
    use time::OffsetDateTime;
    use tower::ServiceExt;

    use crate::git::exec::Git;
    use crate::model::{PageRef, PageState, Part};
    use crate::testutil::{Repo, build_repo, commit};

    const TOKEN: &str = "0123456789abcdef";
    const PORT: u16 = 7420;

    fn server() -> Router {
        app(state())
    }

    /// A state whose store is never reached: the routes of these tests
    /// read the course in memory only.
    fn state() -> AppState {
        state_in(std::path::Path::new("/nonexistent/qlearn-test"))
    }

    fn state_in(root: &std::path::Path) -> AppState {
        let at_new_year = OffsetDateTime::from_unix_timestamp(1_767_225_600).unwrap();
        let mut course = Course::new("raft", at_new_year);
        course.title = "Raft, the leader election".to_owned();
        course.parts = vec![Part {
            title: "Why".to_owned(),
            pages: vec![PageRef {
                id: "1".to_owned(),
                kind: PageKind::Lesson,
                title: "Two leaders, one term".to_owned(),
                state: PageState::Taught,
                score: None,
            }],
        }];

        AppState::new(
            Root::Repository(Git::at(root)),
            Store::at(root),
            course,
            None,
            TOKEN.to_owned(),
            PORT,
        )
    }

    fn get(uri: &str) -> Request<Body> {
        Request::builder().uri(uri).body(Body::empty()).unwrap()
    }

    fn get_with_cookie(uri: &str, token: &str) -> Request<Body> {
        Request::builder()
            .uri(uri)
            .header(
                header::COOKIE,
                format!("{}={token}", auth::cookie_name(PORT)),
            )
            .body(Body::empty())
            .unwrap()
    }

    fn put(uri: &str, body: impl Into<Body>) -> Request<Body> {
        Request::builder()
            .method("PUT")
            .uri(uri)
            .header(
                header::COOKIE,
                format!("{}={TOKEN}", auth::cookie_name(PORT)),
            )
            .body(body.into())
            .unwrap()
    }

    /// A request of the agent with a JSON body, as `qlearn` sends it.
    fn send_json(method: &str, uri: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(
                header::COOKIE,
                format!("{}={TOKEN}", auth::cookie_name(PORT)),
            )
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap()
    }

    fn post(uri: &str) -> Request<Body> {
        send_json("POST", uri, "")
    }

    /// The repository of the excerpts. Its only commit is fixed, so a
    /// snapshot names the same hash on every run.
    async fn repo() -> Repo {
        build_repo(&[commit("raft").file("src/raft.c", RAFT)]).await
    }

    const RAFT: &str = "/* The election timer. */\n\
                        int election_timeout(int base)\n\
                        {\n\
                        \x20   return base + rand() % base;\n\
                        }\n";

    /// Every file under `.learn/` of `dir`, relative to `dir`.
    fn files(dir: &std::path::Path) -> Vec<String> {
        let mut out = Vec::new();
        let mut todo = vec![dir.join(".learn")];
        while let Some(at) = todo.pop() {
            let Ok(entries) = std::fs::read_dir(&at) else {
                continue;
            };
            for entry in entries {
                let path = entry.unwrap().path();
                match path.is_dir() {
                    true => todo.push(path),
                    false => out.push(path.strip_prefix(dir).unwrap().display().to_string()),
                }
            }
        }
        out.sort();
        out
    }

    /// Every file of `dir` outside `.learn/`, `.git/` included, with the
    /// hash of its content: the server writes nothing there.
    fn outside(dir: &std::path::Path) -> std::collections::BTreeMap<String, String> {
        use sha2::{Digest, Sha256};

        let mut out = std::collections::BTreeMap::new();
        let mut todo = vec![dir.to_path_buf()];
        while let Some(at) = todo.pop() {
            for entry in std::fs::read_dir(&at).unwrap() {
                let path = entry.unwrap().path();
                let name = path.strip_prefix(dir).unwrap().display().to_string();
                match path.is_dir() {
                    true if name == ".learn" => {}
                    true => todo.push(path),
                    false => {
                        let hash = hex::encode(Sha256::digest(std::fs::read(&path).unwrap()));
                        out.insert(name, hash);
                    }
                }
            }
        }
        out
    }

    const LESSON: &str = "# Two leaders, one term\n\n\
                          A follower that hears nothing starts an election.\n\n\
                          ```c\nint term = 1;\n```\n\n\
                          ## The vote\n\nEach server votes once per term.\n";

    async fn json(app: Router, request: Request<Body>) -> (StatusCode, serde_json::Value) {
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);

        (status, value)
    }

    #[tokio::test]
    async fn a_request_without_the_token_gets_the_error_shape() {
        let app = server();
        let (status, body) = json(app, get("/api/session")).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "unauthorized");
        assert!(body["error"]["message"].is_string(), "{body}");
    }

    #[tokio::test]
    async fn a_wrong_token_is_refused() {
        for request in [
            get("/api/session?t=wrong"),
            get_with_cookie("/api/session", "wrong"),
            get("/"),
        ] {
            let (status, _) = json(server(), request).await;

            assert_eq!(status, StatusCode::UNAUTHORIZED);
        }
    }

    #[tokio::test]
    async fn the_token_in_the_query_hands_over_a_cookie() {
        let app = server();
        let response = app
            .oneshot(get(&format!("/api/session?t={TOKEN}")))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .expect("the first load must set the cookie")
            .to_str()
            .unwrap();
        assert_eq!(
            cookie,
            format!("qlearn_token_{PORT}={TOKEN}; Path=/; SameSite=Strict; HttpOnly")
        );
    }

    /// A browser sends the cookies of every server on 127.0.0.1, whatever
    /// its port. The one of this server must still be found among them.
    #[tokio::test]
    async fn the_cookie_of_another_server_does_not_hide_this_one() {
        for header in [
            format!("qlearn_token_7421=other; qlearn_token_{PORT}={TOKEN}"),
            format!("qlearn_token_{PORT}={TOKEN}; qlearn_token_7421=other"),
        ] {
            let request = Request::builder()
                .uri("/api/session")
                .header(header::COOKIE, header.as_str())
                .body(Body::empty())
                .unwrap();
            let response = server().oneshot(request).await.unwrap();

            assert_eq!(response.status(), StatusCode::OK, "{header}");
        }
    }

    /// An extractor answers a query that it cannot read with plain text.
    #[tokio::test]
    async fn a_query_with_a_repeated_token_gets_the_error_shape_or_passes() {
        let (status, body) = json(server(), get("/api/session?t=a&t=b")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "unauthorized");

        let (status, _) = json(server(), get(&format!("/api/session?t=a&t={TOKEN}"))).await;
        assert_eq!(status, StatusCode::OK);

        let (status, _) = json(server(), get_with_cookie("/api/session?t=a&t=b", TOKEN)).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn the_cookie_alone_is_enough_afterwards() {
        let app = server();
        let response = app
            .oneshot(get_with_cookie("/api/session", TOKEN))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get(header::SET_COOKIE).is_none());
    }

    #[tokio::test]
    async fn the_interface_is_served_after_the_token() {
        let app = server();
        let response = app.oneshot(get(&format!("/?t={TOKEN}"))).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "text/html");
        assert!(response.headers().get(header::SET_COOKIE).is_some());
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("<html"));
    }

    /// A lesson loads nothing from the network (section 4.1 of the
    /// design), and Mermaid fetches the image of a shape, or the `url()` of
    /// a style, before any sanitizer sees its output.
    #[tokio::test]
    async fn the_interface_loads_nothing_from_another_origin() {
        let response = server()
            .oneshot(get(&format!("/?t={TOKEN}")))
            .await
            .unwrap();

        let policy = response.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap();
        let directives: Vec<&str> = policy.split(';').map(str::trim).collect();
        for directive in [
            "default-src 'self'",
            "img-src 'self' data:",
            "style-src 'self' 'unsafe-inline'",
        ] {
            assert!(directives.contains(&directive), "{directive}: {policy}");
        }
    }

    #[tokio::test]
    async fn a_route_that_does_not_exist_gets_the_error_shape() {
        let app = server();
        let (status, body) = json(app, get_with_cookie("/api/nothing", TOKEN)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "notFound");
    }

    #[tokio::test]
    async fn a_route_called_with_the_wrong_method_gets_the_error_shape() {
        let request = Request::builder()
            .method("POST")
            .uri("/api/session")
            .header(
                header::COOKIE,
                format!("{}={TOKEN}", auth::cookie_name(PORT)),
            )
            .body(Body::empty())
            .unwrap();
        let (status, body) = json(server(), request).await;

        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(body["error"]["code"], "methodNotAllowed");
        assert!(body["error"]["message"].is_string(), "{body}");
    }

    #[tokio::test]
    async fn the_session_route_answers_with_the_course_and_the_version() {
        let state = state();
        let (status, mut body) =
            json(app(state.clone()), get_with_cookie("/api/session", TOKEN)).await;

        assert_eq!(status, StatusCode::OK);
        // The commit under the release changes with every commit of qlearn.
        assert_eq!(body["version"], crate::version::LONG);
        body["version"] = "[version]".into();
        insta::assert_json_snapshot!("session", server_redacted(&state, body));
    }

    #[tokio::test]
    async fn the_events_route_answers_the_writes_after_a_number() {
        let state = state();
        state
            .events
            .push(EventKind::Page, Author::Agent, Some("1"), None);
        state
            .events
            .push(EventKind::Next, Author::Learner, Some("1"), None);

        let (status, body) = json(
            app(state.clone()),
            get_with_cookie("/api/events?after=1&who=page", TOKEN),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        insta::assert_json_snapshot!("events", server_redacted(&state, body));
    }

    #[tokio::test]
    async fn a_wait_with_no_number_answers_the_one_to_start_from() {
        let state = state();
        state
            .events
            .push(EventKind::Page, Author::Agent, Some("1"), None);

        let (status, body) = json(app(state), get_with_cookie("/api/events", TOKEN)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["next"], 1);
        assert_eq!(body["events"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn a_request_that_the_wait_of_the_agent_hands_over_makes_it_work() {
        let state = state();
        let server = app(state.clone());
        let waiting = tokio::spawn(
            server
                .clone()
                .oneshot(get_with_cookie("/api/events?after=0&who=agent", TOKEN)),
        );
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !state.events.agent_listens() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the wait of the agent must count");

        state
            .events
            .push(EventKind::Next, Author::Learner, Some("1"), None);
        let response = waiting.await.unwrap().unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let batch: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(batch["events"][0]["kind"], "next");

        let (_, page) = json(server, get_with_cookie("/api/events?who=page", TOKEN)).await;
        assert_eq!(page["agent"], true);
        assert_eq!(
            page["working"], true,
            "the agent works until it waits again"
        );
        assert_eq!(page["preparing"], true, "a Next request asks for a page");
    }

    #[tokio::test]
    async fn a_wait_with_a_bad_query_gets_the_error_shape() {
        for uri in ["/api/events?who=reader", "/api/events?after=-1"] {
            let (status, body) = json(server(), get_with_cookie(uri, TOKEN)).await;

            assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
            assert_eq!(body["error"]["code"], "badRequest");
            assert!(body["error"]["message"].is_string(), "{body}");
        }
    }

    #[tokio::test]
    async fn the_push_route_stores_the_page_and_answers_it() {
        let dir = repo().await;
        let state = state_in(dir.path());
        let untouched = outside(dir.path());

        let (status, body) = json(app(state.clone()), put("/api/pages/1", LESSON)).await;

        assert_eq!(status, StatusCode::OK, "{body}");
        insta::assert_json_snapshot!("push_page", body);
        assert_eq!(
            files(dir.path()),
            [
                ".learn/raft/course.json",
                ".learn/raft/pages/1.json",
                ".learn/raft/pages/1.md"
            ]
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".learn/raft/pages/1.md")).unwrap(),
            LESSON,
            "the source is kept as pushed"
        );

        let batch = state.events.after(Some(0), events::HOLD).await;
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].kind, EventKind::Page);
        assert_eq!(batch.events[0].author, Author::Agent);
        assert_eq!(batch.events[0].page.as_deref(), Some("1"));

        let (_, session) = json(app(state), get_with_cookie("/api/session", TOKEN)).await;
        assert_eq!(session["current"], "1");
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
    }

    #[tokio::test]
    async fn the_page_route_answers_the_blocks_that_were_pushed() {
        let dir = repo().await;
        let state = state_in(dir.path());
        let (_, pushed) = json(app(state.clone()), put("/api/pages/1", LESSON)).await;

        let (status, body) = json(app(state), get_with_cookie("/api/pages/1", TOKEN)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, pushed);
        insta::assert_json_snapshot!("page", body);
    }

    const CHART: &str = "# Election time\n\n\
                         ```chart\n{ \"kind\": \"bar\", \"title\": \"Election <b>time</b>\",\n  \
                         \"x\": { \"label\": \"Timeout range (ms)\", \"values\": [\"150-300\", 150] },\n  \
                         \"y\": { \"label\": \"Median election time\", \"unit\": \"ms\" },\n  \
                         \"series\": [{ \"name\": \"5 servers\", \"values\": [283, 1520.5] }] }\n```\n";

    /// The chart crosses the wire as JSON, with its texts as the agent
    /// wrote them: the page escapes them when it draws them.
    #[tokio::test]
    async fn a_chart_crosses_the_wire_as_json() {
        let dir = repo().await;
        let state = state_in(dir.path());

        let (status, pushed) = json(app(state.clone()), put("/api/pages/1", CHART)).await;
        assert_eq!(status, StatusCode::OK, "{pushed}");
        let (status, body) = json(app(state), get_with_cookie("/api/pages/1", TOKEN)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, pushed);
        insta::assert_json_snapshot!("page_chart", body);
        assert_eq!(body["blocks"][1]["chart"]["title"], "Election <b>time</b>");
    }

    const STEPS: &str = "# Two candidates\n\n\
                         ```steps\n{ \"title\": \"Two <b>candidates</b>\",\n  \
                         \"columns\": [\"Server\", \"Term\"],\n  \"steps\": [\n    \
                         { \"label\": \"t = 0\", \"body\": \"S1 *times out*. <i>x</i>\", \
                         \"rows\": [[\"S1\", 6], [\"S2\", 5]] },\n    \
                         { \"label\": \"t = 2\", \"body\": \"S1 asks S2.\", \
                         \"diagram\": \"sequenceDiagram\\n  S1->>S2: vote?\\n\" }\n  ] }\n```\n";

    /// The body of a step crosses the wire as the HTML of the server, and
    /// the rest as the agent wrote it: the page escapes it.
    #[tokio::test]
    async fn steps_cross_the_wire_as_json_with_the_body_as_html() {
        let dir = repo().await;
        let state = state_in(dir.path());

        let (status, pushed) = json(app(state.clone()), put("/api/pages/1", STEPS)).await;
        assert_eq!(status, StatusCode::OK, "{pushed}");
        let (status, body) = json(app(state), get_with_cookie("/api/pages/1", TOKEN)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, pushed);
        insta::assert_json_snapshot!("page_steps", body);
        let steps = &body["blocks"][1]["steps"];
        assert_eq!(steps["title"], "Two <b>candidates</b>");
        assert_eq!(
            steps["steps"][0]["html"],
            "S1 <em>times out</em>. &lt;i&gt;x&lt;/i&gt;"
        );
    }

    /// The repository of the images: a PNG and an SVG, beside the code.
    async fn image_repo() -> Repo {
        build_repo(&[commit("raft")
            .file("src/raft.c", RAFT)
            .bytes("docs/flow.png", crate::testutil::PNG)
            .file("docs/states.svg", crate::testutil::SVG)])
        .await
    }

    const IMAGES: &str = "# The states\n\n\
                          ![The states of a server](docs/states.svg)\n\n\
                          A follower times out, then:\n\n\
                          ![The flow of a vote](docs/flow.png)\n";

    /// A raw request with the cookie, so a test can send a path that a
    /// client would have cleaned.
    fn get_raw(uri: &str) -> Request<Body> {
        get_with_cookie(uri, TOKEN)
    }

    async fn bytes(
        app: Router,
        request: Request<Body>,
    ) -> (StatusCode, header::HeaderMap, Vec<u8>) {
        let response = app.oneshot(request).await.unwrap();
        let (parts, body) = response.into_parts();
        let bytes = axum::body::to_bytes(body, 1 << 24).await.unwrap();
        (parts.status, parts.headers, bytes.to_vec())
    }

    /// An image crosses the wire as the name of its copy in `media/`, with
    /// where it comes from and its size. The bytes stay in the store.
    #[tokio::test]
    async fn an_image_crosses_the_wire_as_its_media_name() {
        let dir = image_repo().await;
        let state = state_in(dir.path());
        let untouched = outside(dir.path());

        let (status, pushed) = json(app(state.clone()), put("/api/pages/1", IMAGES)).await;
        assert_eq!(status, StatusCode::OK, "{pushed}");
        let (status, body) = json(app(state), get_with_cookie("/api/pages/1", TOKEN)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, pushed);
        insta::assert_json_snapshot!("page_image", body);
        let svg = &body["blocks"][1];
        let png = &body["blocks"][3];
        let svg_name = svg["media"].as_str().unwrap();
        let png_name = png["media"].as_str().unwrap();
        assert_eq!(
            files(dir.path()),
            [
                ".learn/raft/course.json".to_owned(),
                format!(".learn/raft/media/{png_name}"),
                format!(".learn/raft/media/{svg_name}"),
                ".learn/raft/pages/1.json".to_owned(),
                ".learn/raft/pages/1.md".to_owned(),
            ]
        );
        assert_eq!(
            std::fs::read(dir.path().join(".learn/raft/media").join(png_name)).unwrap(),
            crate::testutil::PNG
        );
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
    }

    const NOTES_IMAGE: &str = "# The ring\n\n\
                               ![The magic ring, drawn by the agent](.learn/raft/notes/ring.svg)\n";

    /// Write the figure of `NOTES_IMAGE`, as the agent does with its own
    /// tools.
    fn draw_ring(dir: &std::path::Path) {
        let notes = dir.join(".learn/raft/notes");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::write(notes.join("ring.svg"), crate::testutil::SVG).unwrap();
    }

    /// An image of the notes crosses the wire with no repository and no
    /// commit, inside a repository and outside one. Its copy in `media/`
    /// is what the page shows.
    #[tokio::test]
    async fn an_image_of_the_notes_crosses_the_wire_with_no_commit() {
        let repo = image_repo().await;
        let plain = tempfile::tempdir().unwrap();
        let mut outside = state_in(plain.path());
        outside.root = Root::Directory(plain.path().to_path_buf());

        for (dir, state) in [
            (repo.path(), state_in(repo.path())),
            (plain.path(), outside),
        ] {
            draw_ring(dir);

            let (status, pushed) = json(app(state.clone()), put("/api/pages/1", NOTES_IMAGE)).await;
            assert_eq!(status, StatusCode::OK, "{pushed}");
            let (status, body) = json(app(state), get_with_cookie("/api/pages/1", TOKEN)).await;

            assert_eq!(status, StatusCode::OK);
            assert_eq!(body, pushed);
            insta::assert_json_snapshot!("page_image_notes", body);
            let name = body["blocks"][1]["media"].as_str().unwrap();
            assert_eq!(
                std::fs::read(dir.join(".learn/raft/media").join(name)).unwrap(),
                crate::testutil::SVG.as_bytes()
            );
        }
    }

    /// A page with one bad image stores nothing, not even its good image.
    #[tokio::test]
    async fn a_refused_page_stores_no_image() {
        let dir = image_repo().await;
        let state = state_in(dir.path());
        let source = format!("{IMAGES}\n![A logo](https://example.com/logo.png)\n");

        let (status, body) = json(app(state), put("/api/pages/1", source)).await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"]["code"], "invalidPage");
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap()
                .contains("1.md:9: image: \"https://example.com/logo.png\" is a URL"),
            "{body}"
        );
        assert_eq!(files(dir.path()), Vec::<String>::new());
    }

    /// Outside a repository, the page names each excerpt and each image,
    /// and stores nothing. A page with neither is stored.
    #[tokio::test]
    async fn a_course_outside_a_repository_refuses_an_excerpt_and_an_image() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = state_in(dir.path());
        state.root = Root::Directory(dir.path().to_path_buf());
        let source = "# Raft\n\n```excerpt\npath: a.c\nlines: 1\n```\n\n\
                      ![The states](docs/states.svg)\n";

        let (status, body) = json(app(state.clone()), put("/api/pages/1", source)).await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"]["code"], "invalidPage");
        let message = body["error"]["message"].as_str().unwrap();
        let lines: Vec<_> = message
            .lines()
            .map(|line| &line[..line.find(": ").unwrap()])
            .collect();
        assert_eq!(lines, ["1.md:3", "1.md:8"], "{message}");
        assert_eq!(files(dir.path()), Vec::<String>::new());

        let sketch = "# Raft\n\n```c\nint term;\n```\n";
        let (status, body) = json(app(state), put("/api/pages/1", sketch)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            files(dir.path()),
            [
                ".learn/raft/course.json",
                ".learn/raft/pages/1.json",
                ".learn/raft/pages/1.md"
            ]
        );
    }

    /// The media route sends the bytes with the type of the name, and
    /// tells the browser never to guess another type or run a script.
    #[tokio::test]
    async fn the_media_route_sends_the_bytes_with_their_type() {
        let dir = image_repo().await;
        let state = state_in(dir.path());
        let (_, pushed) = json(app(state.clone()), put("/api/pages/1", IMAGES)).await;

        for (block, bytes, mime) in [
            (
                &pushed["blocks"][1],
                crate::testutil::SVG.as_bytes(),
                "image/svg+xml",
            ),
            (&pushed["blocks"][3], crate::testutil::PNG, "image/png"),
        ] {
            let uri = format!("/api/media/{}", block["media"].as_str().unwrap());
            let (status, headers, body) = bytes_of(&state, &uri).await;

            assert_eq!(status, StatusCode::OK, "{uri}");
            assert_eq!(body, bytes);
            assert_eq!(headers[header::CONTENT_TYPE], mime);
            assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
            assert_eq!(
                headers[header::CACHE_CONTROL],
                "private, max-age=31536000, immutable"
            );
            assert_eq!(
                headers[header::CONTENT_SECURITY_POLICY],
                "default-src 'none'; style-src 'unsafe-inline'; img-src data:; sandbox"
            );
        }
    }

    async fn bytes_of(state: &AppState, uri: &str) -> (StatusCode, header::HeaderMap, Vec<u8>) {
        bytes(app(state.clone()), get_raw(uri)).await
    }

    /// A name that is not the hash of an image of the store names nothing,
    /// and no name reaches a file outside `media/`.
    #[tokio::test]
    async fn the_media_route_refuses_a_name_that_is_not_an_image_of_the_store() {
        let dir = image_repo().await;
        let state = state_in(dir.path());
        let (_, pushed) = json(app(state.clone()), put("/api/pages/1", IMAGES)).await;
        let name = pushed["blocks"][3]["media"].as_str().unwrap();
        let hash = name.strip_suffix(".png").unwrap();
        // A file beside `media/` that a traversal would reach.
        std::fs::write(
            dir.path().join(format!(".learn/raft/{hash}.png")),
            "outside",
        )
        .unwrap();

        for uri in [
            format!("/api/media/{}.png", "0".repeat(64)),
            format!("/api/media/{hash}.svg"),
            format!("/api/media/{hash}.PNG"),
            format!("/api/media/{}", hash),
            "/api/media/course.json".to_owned(),
            "/api/media/..%2Fcourse.json".to_owned(),
            "/api/media/%2E%2E%2Fpages%2F1.json".to_owned(),
            format!("/api/media/..%2F{hash}.png"),
            format!("/api/media/%2Fetc%2F{hash}.png"),
            "/api/media/../course.json".to_owned(),
        ] {
            let (status, _, body) = bytes_of(&state, &uri).await;
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

            assert_eq!(status, StatusCode::NOT_FOUND, "{uri}: {body}");
            assert_eq!(body["error"]["code"], "notFound", "{uri}");
        }
    }

    #[tokio::test]
    async fn the_media_route_needs_the_token() {
        let dir = image_repo().await;
        let state = state_in(dir.path());
        let (_, pushed) = json(app(state.clone()), put("/api/pages/1", IMAGES)).await;
        let uri = format!(
            "/api/media/{}",
            pushed["blocks"][3]["media"].as_str().unwrap()
        );

        for request in [get(&uri), get_with_cookie(&uri, "wrong")] {
            let (status, body) = json(app(state.clone()), request).await;

            assert_eq!(status, StatusCode::UNAUTHORIZED);
            assert_eq!(body["error"]["code"], "unauthorized");
        }
    }

    /// A debrief is a page source: its images go to `media/` too.
    #[tokio::test]
    async fn a_debrief_stores_its_images() {
        let (dir, state) = with_quiz(LESSON, QUIZ_TWO_POINTS).await;
        answer_quiz(&state, 1).await;
        dir.commit(&commit("images").bytes("docs/flow.png", crate::testutil::PNG))
            .await;
        let body = debrief_body(
            "# Almost\n\n![The flow of a vote](docs/flow.png)\n",
            serde_json::json!({ "random-timeout": "ok", "majority": "partial" }),
        );

        let (status, page) = write_debrief(&state, 1, &body).await;

        assert_eq!(status, StatusCode::OK, "{page}");
        let block = &page["quizzes"][0]["debrief"]["blocks"][1];
        assert_eq!(block["kind"], "image");
        let uri = format!("/api/media/{}", block["media"].as_str().unwrap());
        let (status, _, bytes) = bytes_of(&state, &uri).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(bytes, crate::testutil::PNG);
    }

    /// A step diagram that Mermaid refuses reports its steps block. The
    /// agent gets the fence, and the error names the step.
    #[tokio::test]
    async fn a_broken_step_diagram_hands_the_agent_the_fence() {
        let dir = repo().await;
        let state = state_in(dir.path());
        let (status, pushed) = json(app(state.clone()), put("/api/pages/1", STEPS)).await;
        assert_eq!(status, StatusCode::OK, "{pushed}");
        let block = pushed["blocks"][1]["id"].as_str().unwrap().to_owned();
        let after = state.events.after(None, events::HOLD).await.next;

        let body = serde_json::json!({ "block": block, "error": "steps[1].diagram: bad" });
        let (status, answer) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/broken", &body.to_string()),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{answer}");
        let batch = state.events.after(Some(after), events::HOLD).await;
        let Some(Detail::Broken {
            block: source,
            error,
        }) = &batch.events[0].detail
        else {
            panic!("no broken detail: {:?}", batch.events);
        };
        assert_eq!(source.id, block);
        assert_eq!(source.kind, "steps");
        assert!(
            source.source.starts_with("```steps\n") && source.source.ends_with("```"),
            "{}",
            source.source
        );
        assert_eq!(error, "steps[1].diagram: bad");

        // Steps with no diagram cannot fail to draw.
        let rows_only = STEPS.replace(
            "\"diagram\": \"sequenceDiagram\\n  S1->>S2: vote?\\n\"",
            "\"rows\": [[\"S1\", 6]]",
        );
        let (status, pushed) = json(app(state.clone()), put("/api/pages/1", rows_only)).await;
        assert_eq!(status, StatusCode::OK, "{pushed}");
        let block = pushed["blocks"][1]["id"].as_str().unwrap();
        let body = serde_json::json!({ "block": block, "error": "bad" });
        let (status, answer) = json(
            app(state),
            send_json("POST", "/api/pages/1/broken", &body.to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    }

    const EXCERPT: &str = "# The timer\n\n\
                           ```excerpt\npath: src/raft.c\nlines: 1-4\nmark: 2\nhide: 4\n```\n";

    /// The hidden line crosses the wire as null, and its text stays in the
    /// store for the day the page may show it.
    #[tokio::test]
    async fn an_excerpt_crosses_the_wire_with_its_hidden_line_as_null() {
        let dir = repo().await;
        let state = state_in(dir.path());
        let untouched = outside(dir.path());

        let (status, pushed) = json(app(state.clone()), put("/api/pages/1", EXCERPT)).await;
        assert_eq!(status, StatusCode::OK, "{pushed}");
        let (status, body) = json(app(state), get_with_cookie("/api/pages/1", TOKEN)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, pushed);
        insta::assert_json_snapshot!("page_excerpt", body);
        assert_eq!(
            body["blocks"][1]["excerpt"]["lines"][3],
            serde_json::Value::Null
        );
        assert!(!body.to_string().contains("return base"), "{body}");
        let stored = std::fs::read_to_string(dir.path().join(".learn/raft/pages/1.json")).unwrap();
        assert!(stored.contains("return base + rand() % base;"), "{stored}");
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
    }

    /// The page shows the code as the lesson taught it, even after the
    /// code changes.
    #[tokio::test]
    async fn an_excerpt_keeps_its_commit_after_a_new_commit() {
        let dir = repo().await;
        let state = state_in(dir.path());
        let (_, pushed) = json(app(state.clone()), put("/api/pages/1", EXCERPT)).await;
        let taught = dir.sha("HEAD").await;

        dir.commit(&commit("faster").file("src/raft.c", "int election_timeout;\n"))
            .await;
        let (status, body) = json(app(state), get_with_cookie("/api/pages/1", TOKEN)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, pushed);
        assert_eq!(body["blocks"][1]["excerpt"]["commit"], taught.as_str());
        assert_ne!(dir.sha("HEAD").await, taught);
    }

    #[tokio::test]
    async fn a_page_with_no_content_is_not_found() {
        let dir = repo().await;
        let state = state_in(dir.path());
        state.course.write().await.parts[0].pages[0].state = PageState::Planned;

        for uri in [
            "/api/pages/1",
            "/api/pages/9",
            "/api/pages/x",
            "/api/pages/..%2Fcourse",
        ] {
            let (status, body) = json(app(state.clone()), get_with_cookie(uri, TOKEN)).await;

            assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
            assert_eq!(body["error"]["code"], "notFound", "{uri}");
        }
    }

    #[tokio::test]
    async fn a_refused_page_gets_every_error_and_stores_nothing() {
        let dir = repo().await;
        let state = state_in(dir.path());
        let untouched = outside(dir.path());
        let before = state.course.read().await.clone();
        let source = "# Refused\n\nText.\n\n```excerpt\npath: a.c\n```\n\n\
                      ![A figure](figure.png)\n";

        let (status, body) = json(app(state.clone()), put("/api/pages/2", source)).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        insta::assert_json_snapshot!("push_page_refused", body);
        assert_eq!(files(dir.path()), Vec::<String>::new());
        assert_eq!(*state.course.read().await, before);
        assert_eq!(state.current(), None);
        assert_eq!(state.events.after(None, events::HOLD).await.next, 0);
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
    }

    #[tokio::test]
    async fn a_page_outside_the_syllabus_goes_at_the_end_of_the_last_part() {
        let dir = repo().await;
        let state = state_in(dir.path());

        let source = "Intro.\n\n# Checkpoint one\n\nWhat holds?\n";
        let (status, _) = json(app(state.clone()), put("/api/pages/c1", source)).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = json(app(state.clone()), put("/api/pages/7a", "No heading.\n")).await;
        assert_eq!(status, StatusCode::OK);

        let (_, session) = json(app(state.clone()), get_with_cookie("/api/session", TOKEN)).await;
        assert_eq!(
            session["course"]["parts"][0]["pages"],
            serde_json::json!([
                { "id": "1", "kind": "lesson", "title": "Two leaders, one term",
                  "state": "taught", "score": null },
                { "id": "c1", "kind": "checkpoint", "title": "Checkpoint one",
                  "state": "taught", "score": null },
                { "id": "7a", "kind": "lesson", "title": "7a", "state": "taught",
                  "score": null },
            ])
        );
        assert_eq!(session["current"], "7a", "the page pushed last");

        let stored = Store::at(dir.path()).load("raft").unwrap().unwrap();
        assert_eq!(
            stored,
            *state.course.read().await,
            "course.json holds it too"
        );
    }

    /// A push that answers an error must leave the page as it was, so the
    /// agent can trust the error and push again.
    #[tokio::test]
    async fn a_push_that_cannot_write_the_course_leaves_the_pages_as_they_were() {
        let dir = repo().await;
        let state = state_in(dir.path());
        let untouched = outside(dir.path());
        let (_, first) = json(app(state.clone()), put("/api/pages/1", LESSON)).await;
        let course_file = dir.path().join(".learn/raft/course.json");
        std::fs::remove_file(&course_file).unwrap();
        std::fs::create_dir(&course_file).unwrap();
        let before = state.course.read().await.clone();

        for id in ["1", "2"] {
            let uri = format!("/api/pages/{id}");
            let (status, body) = json(app(state.clone()), put(&uri, "# Changed\n\nText.\n")).await;
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
        }

        let (status, body) = json(app(state.clone()), get_with_cookie("/api/pages/1", TOKEN)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, first);
        let (status, _) = json(app(state.clone()), get_with_cookie("/api/pages/2", TOKEN)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".learn/raft/pages/1.md")).unwrap(),
            LESSON
        );
        assert_eq!(
            files(dir.path()),
            [".learn/raft/pages/1.json", ".learn/raft/pages/1.md"]
        );
        assert_eq!(*state.course.read().await, before);
        assert_eq!(state.current().as_deref(), Some("1"));
        assert_eq!(state.events.after(None, events::HOLD).await.next, 1);
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
    }

    /// A page file that `course.json` does not back is the trace of a push
    /// that stopped halfway. It is not served.
    #[tokio::test]
    async fn a_page_file_that_the_course_does_not_hold_is_not_found() {
        let dir = repo().await;
        let state = state_in(dir.path());
        let parsed = lesson::parse(
            LESSON,
            state.root.git(),
            &state.notes,
            Highlighter::shared(),
        )
        .await
        .unwrap();
        let page = PageFile {
            blocks: parsed.blocks,
            hidden: parsed.hidden,
            ..PageFile::default()
        };
        state.store.save_page("raft", "2", LESSON, &page).unwrap();

        let (status, body) = json(app(state), get_with_cookie("/api/pages/2", TOKEN)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "notFound");
    }

    #[tokio::test]
    async fn a_push_with_a_bad_id_or_a_bad_body_gets_the_error_shape() {
        let dir = repo().await;
        let state = state_in(dir.path());
        let untouched = outside(dir.path());

        for request in [
            put("/api/pages/x1", "Text.\n"),
            put("/api/pages/..%2Fcourse", "Text.\n"),
            put("/api/pages/%FF", "Text.\n"),
            put("/api/pages/1", vec![0xff, 0xfe]),
            get_with_cookie("/api/pages/%FF", TOKEN),
        ] {
            let (status, body) = json(app(state.clone()), request).await;

            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(body["error"]["code"], "badRequest");
            assert!(body["error"]["message"].is_string(), "{body}");
        }
        assert_eq!(files(dir.path()), Vec::<String>::new());
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
    }

    #[tokio::test]
    async fn the_course_route_changes_the_fields_named_and_answers_the_course() {
        let dir = repo().await;
        let state = state_in(dir.path());
        let body = r#"{"title":"Raft, the election","goal":"Know **why** a term ends.",
                       "profile":"C, no Raft yet.","quizMode":"current",
                       "status":"running","followUp":"2026-11-04"}"#;

        let (status, mut body) =
            json(app(state.clone()), send_json("PATCH", "/api/course", body)).await;

        assert_eq!(status, StatusCode::OK, "{body}");
        // `updated` takes the clock of the write, which a snapshot cannot pin.
        assert_ne!(body["updated"], body["created"]);
        body["updated"] = "[updated]".into();
        insta::assert_json_snapshot!("change_course", body);

        let stored = Store::at(dir.path()).load("raft").unwrap().unwrap();
        assert_eq!(stored, *state.course.read().await, "course.json holds it");
        let batch = state.events.after(Some(0), events::HOLD).await;
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].kind, EventKind::Course);
        assert_eq!(batch.events[0].author, Author::Agent);
    }

    #[tokio::test]
    async fn a_field_left_out_stays_and_a_null_follow_up_drops_the_date() {
        let dir = repo().await;
        let state = state_in(dir.path());
        let patch = |body: &'static str| send_json("PATCH", "/api/course", body);

        json(app(state.clone()), patch(r#"{"followUp":"2026-11-04"}"#)).await;
        let (_, body) = json(app(state.clone()), patch(r#"{"goal":"Read it"}"#)).await;
        assert_eq!(body["followUp"], "2026-11-04");
        assert_eq!(body["title"], "Raft, the leader election");

        let (status, body) = json(app(state.clone()), patch(r#"{"followUp":null}"#)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["followUp"], serde_json::Value::Null);
        assert_eq!(body["goal"], "Read it");
    }

    #[tokio::test]
    async fn a_bad_change_of_the_course_is_refused_and_stores_nothing() {
        let dir = repo().await;
        let state = state_in(dir.path());
        let untouched = outside(dir.path());
        let before = state.course.read().await.clone();

        for body in [
            "",
            "{}",
            "[]",
            "not json",
            r#"{"title":"Raft","author":"learner"}"#,
            r#"{"author":"agent"}"#,
            r#"{"slug":"other"}"#,
            r#"{"parts":[]}"#,
            r#"{"title":"  "}"#,
            r#"{"title":null}"#,
            r#"{"goal":"A goal","title":null}"#,
            r#"{"goal":"A goal","status":null}"#,
            r#"{"goal":"A goal","quizMode":null}"#,
            r#"{"goal":"A goal","profile":null}"#,
            r#"{"profile":"C","goal":null}"#,
            r#"{"quizMode":"all"}"#,
            r#"{"status":"new"}"#,
            r#"{"status":"paused"}"#,
            r#"{"followUp":"next week"}"#,
            r#"{"followUp":"2026-02-30"}"#,
            r#"{"goal":"A goal","followUp":"2026-13-01"}"#,
        ] {
            let request = send_json("PATCH", "/api/course", body);
            let (status, answer) = json(app(state.clone()), request).await;

            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {answer}");
            assert_eq!(answer["error"]["code"], "badRequest", "{body}");
            assert!(answer["error"]["message"].is_string(), "{answer}");
        }
        // With no JSON content type, the body is refused too.
        let request = Request::builder()
            .method("PATCH")
            .uri("/api/course")
            .header(
                header::COOKIE,
                format!("{}={TOKEN}", auth::cookie_name(PORT)),
            )
            .body(Body::from(r#"{"title":"Raft"}"#))
            .unwrap();
        let (status, answer) = json(app(state.clone()), request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(answer["error"]["code"], "badRequest");

        assert_eq!(files(dir.path()), Vec::<String>::new());
        assert_eq!(*state.course.read().await, before);
        assert_eq!(state.events.after(None, events::HOLD).await.next, 0);
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
    }

    const SYLLABUS: &str = r#"{"parts":[
        {"title":"Why","pages":[
            {"id":"2","kind":"lesson","title":"The vote"},
            {"id":"1","kind":"lesson","title":"Two leaders"}]},
        {"title":"Check","pages":[{"id":"c1","kind":"checkpoint","title":"Checkpoint"}]}]}"#;

    #[tokio::test]
    async fn the_syllabus_route_writes_the_parts_and_answers_the_course() {
        let dir = repo().await;
        let state = state_in(dir.path());
        let untouched = outside(dir.path());

        let (status, mut body) = json(
            app(state.clone()),
            send_json("PUT", "/api/syllabus", SYLLABUS),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{body}");
        // `updated` takes the clock of the write, which a snapshot cannot pin.
        assert_ne!(body["updated"], body["created"]);
        body["updated"] = "[updated]".into();
        insta::assert_json_snapshot!("syllabus", body);

        let stored = Store::at(dir.path()).load("raft").unwrap().unwrap();
        assert_eq!(stored, *state.course.read().await, "course.json holds it");
        let batch = state.events.after(Some(0), events::HOLD).await;
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].kind, EventKind::Syllabus);
        assert_eq!(batch.events[0].author, Author::Agent);
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
    }

    #[tokio::test]
    async fn a_refused_syllabus_gets_every_fault_and_stores_nothing() {
        let dir = repo().await;
        let state = state_in(dir.path());
        let untouched = outside(dir.path());
        let before = state.course.read().await.clone();
        let body = r#"{"parts":[{"title":"Why","pages":[
            {"id":"x1","kind":"lesson","title":"A bad id"},
            {"id":"c2","kind":"lesson","title":"A wrong kind"},
            {"id":"2","kind":"lesson","title":"", "state":"debriefed"},
            {"id":"2","kind":"lesson","title":"Twice"}]}]}"#;

        let (status, answer) =
            json(app(state.clone()), send_json("PUT", "/api/syllabus", body)).await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
        insta::assert_json_snapshot!("syllabus_refused", answer);

        for body in ["", "not json", "[]", "{}", r#"{"parts":[]}"#] {
            let (status, answer) =
                json(app(state.clone()), send_json("PUT", "/api/syllabus", body)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {answer}");
            assert!(answer["error"]["message"].is_string(), "{answer}");
        }
        assert_eq!(files(dir.path()), Vec::<String>::new());
        assert_eq!(*state.course.read().await, before);
        assert_eq!(state.events.after(None, events::HOLD).await.next, 0);
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
    }

    /// The answers of the learner live with the pages, so a syllabus moves
    /// a page and its state and score go with it. The agent cannot set them.
    #[tokio::test]
    async fn a_syllabus_keeps_renames_and_reorders_the_pages_with_content() {
        let dir = repo().await;
        let state = state_in(dir.path());
        for id in ["1", "2"] {
            let (status, _) =
                json(app(state.clone()), put(&format!("/api/pages/{id}"), LESSON)).await;
            assert_eq!(status, StatusCode::OK);
        }
        let (_, before) = json(app(state.clone()), get_with_cookie("/api/pages/1", TOKEN)).await;
        {
            let mut course = state.course.write().await;
            let page = &mut course.parts[0].pages[0];
            page.state = PageState::Answered;
            page.score = Some(crate::model::Score {
                right: 2.0,
                total: 3,
            });
        }

        let (status, body) = json(
            app(state.clone()),
            send_json("PUT", "/api/syllabus", SYLLABUS),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body["parts"],
            serde_json::json!([
                { "title": "Why", "pages": [
                    { "id": "2", "kind": "lesson", "title": "The vote", "state": "taught",
                      "score": null },
                    { "id": "1", "kind": "lesson", "title": "Two leaders", "state": "answered",
                      "score": { "right": 2, "total": 3 } },
                ] },
                { "title": "Check", "pages": [
                    { "id": "c1", "kind": "checkpoint", "title": "Checkpoint",
                      "state": "planned", "score": null },
                ] },
            ])
        );
        let (status, page) = json(app(state.clone()), get_with_cookie("/api/pages/1", TOKEN)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(page, before, "the blocks stay as pushed");
        let (_, session) = json(app(state), get_with_cookie("/api/session", TOKEN)).await;
        assert_eq!(session["current"], "2", "the page pushed last");
    }

    #[tokio::test]
    async fn a_syllabus_that_drops_a_page_with_content_names_each_one() {
        let dir = repo().await;
        let state = state_in(dir.path());
        let (status, _) = json(app(state.clone()), put("/api/pages/c1", LESSON)).await;
        assert_eq!(status, StatusCode::OK);
        let before = state.course.read().await.clone();
        let body =
            r#"{"parts":[{"title":"","pages":[{"id":"2","kind":"lesson","title":"The vote"}]}]}"#;

        let (status, answer) =
            json(app(state.clone()), send_json("PUT", "/api/syllabus", body)).await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
        assert_eq!(answer["error"]["code"], "invalidSyllabus");
        let message = answer["error"]["message"].as_str().unwrap();
        let lines: Vec<_> = message.lines().collect();
        assert_eq!(lines.len(), 2, "{message}");
        assert!(
            lines[0].starts_with("page 1 (\"Two leaders, one term\") has content"),
            "{message}"
        );
        assert!(
            lines[1].starts_with("page c1 (\"Two leaders, one term\") has content"),
            "{message}"
        );
        assert_eq!(*state.course.read().await, before);
    }

    /// A page pushed before the syllabus goes at the end of the last part.
    /// The syllabus that comes later puts it in its place.
    #[tokio::test]
    async fn a_page_outside_the_syllabus_is_moved_by_a_later_syllabus() {
        let dir = repo().await;
        let state = state_in(dir.path());
        let (status, _) = json(app(state.clone()), put("/api/pages/c1", LESSON)).await;
        assert_eq!(status, StatusCode::OK);
        let (_, session) = json(app(state.clone()), get_with_cookie("/api/session", TOKEN)).await;
        assert_eq!(session["course"]["parts"][0]["pages"][1]["id"], "c1");

        let body = r#"{"parts":[
            {"title":"Why","pages":[{"id":"1","kind":"lesson","title":"Two leaders"}]},
            {"title":"Check","pages":[{"id":"c1","kind":"checkpoint","title":"Checkpoint one"}]}]}"#;
        let (status, course) =
            json(app(state.clone()), send_json("PUT", "/api/syllabus", body)).await;

        assert_eq!(status, StatusCode::OK, "{course}");
        assert_eq!(
            course["parts"][1],
            serde_json::json!({ "title": "Check", "pages": [
                { "id": "c1", "kind": "checkpoint", "title": "Checkpoint one", "state": "taught",
                  "score": null },
            ] })
        );
        assert_eq!(course["parts"][0]["pages"].as_array().unwrap().len(), 1);
        let (status, _) = json(app(state), get_with_cookie("/api/pages/c1", TOKEN)).await;
        assert_eq!(status, StatusCode::OK);
    }

    const FINDING: &str = r#"{"title":"  Two timers can fire in one term  ",
        "body":"`election_timeout` draws from <b>one</b> seed.\n","author":"agent"}"#;

    async fn write_finding(
        state: &AppState,
        id: &str,
        body: &str,
    ) -> (StatusCode, serde_json::Value) {
        let uri = format!("/api/findings/{id}");
        json(app(state.clone()), send_json("PUT", &uri, body)).await
    }

    /// The findings as `course.json` holds them.
    fn stored_findings(dir: &std::path::Path) -> serde_json::Value {
        let text = std::fs::read_to_string(dir.join(".learn/raft/course.json")).unwrap();
        serde_json::from_str::<serde_json::Value>(&text).unwrap()["findings"].clone()
    }

    #[tokio::test]
    async fn the_finding_route_writes_a_finding_and_answers_it_in_markdown() {
        let dir = repo().await;
        let state = state_in(dir.path());
        let untouched = outside(dir.path());

        let (status, body) = write_finding(&state, "seed-race", FINDING).await;

        assert_eq!(status, StatusCode::OK, "{body}");
        insta::assert_json_snapshot!("finding", body);
        assert_eq!(stored_findings(dir.path()), serde_json::json!([body]));
        let stored = Store::at(dir.path()).load("raft").unwrap().unwrap();
        assert_eq!(stored, *state.course.read().await, "course.json holds it");
        let batch = state.events.after(Some(0), events::HOLD).await;
        assert_eq!(batch.events.len(), 1);
        let event = &batch.events[0];
        assert_eq!(event.kind, EventKind::Finding);
        assert_eq!(event.author, Author::Agent);
        assert_eq!(event.page, None);
        assert_eq!(event.id.as_deref(), Some("seed-race"));
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
    }

    /// The agent marks a finding checked by writing it again whole. It keeps
    /// its place, so the list in the page does not move.
    #[tokio::test]
    async fn a_finding_written_again_replaces_the_one_with_its_id() {
        let dir = repo().await;
        let state = state_in(dir.path());
        for (id, title) in [("seed-race", "A race"), ("stale-comment", "A comment")] {
            let body = serde_json::json!({ "title": title, "body": "" }).to_string();
            let (status, answer) = write_finding(&state, id, &body).await;
            assert_eq!(status, StatusCode::OK, "{answer}");
        }

        let body = r#"{"title":"The race is real","body":"The team confirmed it.","checked":true}"#;
        let (status, answer) = write_finding(&state, "seed-race", body).await;

        assert_eq!(status, StatusCode::OK, "{answer}");
        assert_eq!(
            stored_findings(dir.path()),
            serde_json::json!([
                { "id": "seed-race", "title": "The race is real",
                  "body": "The team confirmed it.", "checked": true },
                { "id": "stale-comment", "title": "A comment", "body": "", "checked": false },
            ])
        );
        let batch = state.events.after(Some(0), events::HOLD).await;
        let ids: Vec<_> = batch.events.iter().map(|e| e.id.as_deref()).collect();
        assert_eq!(
            ids,
            [Some("seed-race"), Some("stale-comment"), Some("seed-race")]
        );
    }

    #[tokio::test]
    async fn a_refused_finding_gets_every_fault_and_stores_nothing() {
        let dir = repo().await;
        let state = state_in(dir.path());
        let untouched = outside(dir.path());
        let before = state.course.read().await.clone();

        let (status, answer) =
            write_finding(&state, "Seed_Race", r#"{"title":" ","body":"x"}"#).await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
        insta::assert_json_snapshot!("finding_refused", answer);

        // A field of the wrong shape is one fault more, not the only one.
        let (status, answer) =
            write_finding(&state, "Seed_Race", r#"{"title":" ","checked":"yes"}"#).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
        insta::assert_json_snapshot!("finding_refused_shape", answer);

        // An empty id matches no `{id}` of the route, and owes the same
        // refusal as any other bad id.
        let (status, answer) = write_finding(&state, "", r#"{"title":"A race","body":""}"#).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
        assert_eq!(answer["error"]["code"], "invalidFinding", "{answer}");
        let message = answer["error"]["message"].as_str().unwrap();
        assert!(
            message.starts_with("id: \"\" is not a finding id"),
            "{message}"
        );

        let long = "a".repeat(crate::finding::ID_MOST + 1);
        for (id, body) in [
            ("race", ""),
            ("race", "not json"),
            ("race", "{}"),
            ("race", r#"{"title":"A race"}"#),
            ("race", r#"{"title":null,"body":""}"#),
            ("race", r#"{"title":"A race","body":"","checked":"yes"}"#),
            ("race", r#"{"title":"A race","body":"","state":"open"}"#),
            ("race", r#"{"title":"A race","body":"","author":"learner"}"#),
            ("race-", r#"{"title":"A race","body":""}"#),
            (long.as_str(), r#"{"title":"A race","body":""}"#),
        ] {
            let (status, answer) = write_finding(&state, id, body).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{id} {body}: {answer}");
            assert!(answer["error"]["message"].is_string(), "{answer}");
        }
        assert_eq!(files(dir.path()), Vec::<String>::new());
        assert_eq!(*state.course.read().await, before);
        assert_eq!(state.events.after(None, events::HOLD).await.next, 0);
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
    }

    /// The page draws the body with no Markdown parser of its own, and the
    /// HTML of the source never reaches it as HTML.
    #[tokio::test]
    async fn the_session_sends_the_body_of_a_finding_as_html() {
        let dir = repo().await;
        let state = state_in(dir.path());
        let (status, answer) = write_finding(&state, "seed-race", FINDING).await;
        assert_eq!(status, StatusCode::OK, "{answer}");

        let (status, session) =
            json(app(state.clone()), get_with_cookie("/api/session", TOKEN)).await;

        assert_eq!(status, StatusCode::OK, "{session}");
        insta::assert_json_snapshot!("session_findings", session["course"]["findings"]);
        // The store and the agent keep the Markdown.
        assert_eq!(
            state.course.read().await.findings[0].body,
            "`election_timeout` draws from <b>one</b> seed."
        );
    }

    /// A null reads as a field left out unless the server refuses it, and
    /// the rest of the change would then pass without a word.
    #[tokio::test]
    async fn a_null_field_is_refused_by_its_name() {
        let request = send_json("PATCH", "/api/course", r#"{"title":null}"#);
        let (status, answer) = json(server(), request).await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
        let message = answer["error"]["message"].as_str().unwrap();
        assert!(message.contains("title"), "{message}");
        assert!(!message.contains("names no field"), "{message}");
    }

    /// The commands of the agent may send `"author": "agent"`. It names
    /// the writer and changes nothing else.
    #[tokio::test]
    async fn a_write_of_the_agent_may_name_its_author() {
        let dir = repo().await;
        let state = state_in(dir.path());

        let body = r#"{"title":"Raft","author":"agent"}"#;
        let (status, answer) =
            json(app(state.clone()), send_json("PATCH", "/api/course", body)).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        assert_eq!(answer["title"], "Raft");

        let body = r#"{"text":"Reading","author":"agent"}"#;
        let (status, answer) =
            json(app(state.clone()), send_json("PUT", "/api/status", body)).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        assert_eq!(answer["status"], "Reading");

        let batch = state.events.after(Some(0), events::HOLD).await;
        assert!(batch.events.iter().all(|e| e.author == Author::Agent));
    }

    /// A browser sends the cookie of 127.0.0.1 to every port, and a POST in
    /// text/plain needs no preflight. Another local site could then push a
    /// request for the agent.
    #[tokio::test]
    async fn a_write_sent_from_another_site_is_forbidden() {
        let state = state();
        let from = |origin: Option<&str>, fetch_site: Option<&str>| {
            let mut request = Request::builder()
                .method("POST")
                .uri("/api/pages/1/next")
                .header(
                    header::COOKIE,
                    format!("{}={TOKEN}", auth::cookie_name(PORT)),
                )
                .header(header::CONTENT_TYPE, "text/plain");
            if let Some(origin) = origin {
                request = request.header(header::ORIGIN, origin);
            }
            if let Some(site) = fetch_site {
                request = request.header("sec-fetch-site", site);
            }
            request.body(Body::from("x")).unwrap()
        };

        for request in [
            from(Some("http://127.0.0.1:3000"), Some("same-site")),
            from(Some("http://127.0.0.1:3000"), None),
            from(Some("https://example.com"), Some("cross-site")),
            from(Some("null"), None),
            from(None, Some("same-site")),
            from(None, Some("cross-site")),
        ] {
            let (status, body) = json(app(state.clone()), request).await;

            assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
            assert_eq!(body["error"]["code"], "forbidden");
        }
        assert_eq!(state.events.after(None, events::HOLD).await.next, 0);

        for request in [
            from(
                Some(&format!("http://127.0.0.1:{PORT}")),
                Some("same-origin"),
            ),
            from(Some(&format!("http://localhost:{PORT}")), None),
            from(None, Some("none")),
            // The commands of the agent send neither.
            from(None, None),
        ] {
            let (status, body) = json(app(state.clone()), request).await;

            assert_eq!(status, StatusCode::OK, "{body}");
        }

        // A link from another site still opens the page.
        let request = Request::builder()
            .uri("/api/session")
            .header(
                header::COOKIE,
                format!("{}={TOKEN}", auth::cookie_name(PORT)),
            )
            .header("sec-fetch-site", "cross-site")
            .body(Body::empty())
            .unwrap();
        let (status, _) = json(app(state), request).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn the_status_route_sets_the_text_until_the_next_wait_of_the_agent() {
        let state = state();
        let body = r#"{"text":"Writing lesson 2: the vote"}"#;

        let (status, answer) =
            json(app(state.clone()), send_json("PUT", "/api/status", body)).await;

        assert_eq!(status, StatusCode::OK, "{answer}");
        insta::assert_json_snapshot!("set_status", answer);
        let (_, page) = json(
            app(state.clone()),
            get_with_cookie("/api/events?after=0&who=page", TOKEN),
        )
        .await;
        assert_eq!(page["status"], "Writing lesson 2: the vote");
        assert_eq!(page["events"][0]["kind"], "status");
        assert_eq!(page["events"][0]["author"], "agent");

        json(
            app(state.clone()),
            get_with_cookie("/api/events?who=agent", TOKEN),
        )
        .await;
        let (_, page) = json(app(state), get_with_cookie("/api/events?who=page", TOKEN)).await;
        assert_eq!(page["status"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn an_empty_status_clears_it_and_a_bad_body_is_refused() {
        let state = state();
        json(
            app(state.clone()),
            send_json("PUT", "/api/status", r#"{"text":"Reading"}"#),
        )
        .await;

        let (status, answer) = json(
            app(state.clone()),
            send_json("PUT", "/api/status", r#"{"text":""}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(answer, serde_json::json!({ "status": null }));
        assert_eq!(state.events.after(None, events::HOLD).await.status, None);

        for body in [
            "",
            "{}",
            r#"{"text":null}"#,
            r#"{"text":"a","page":"1"}"#,
            r#"{"text":3}"#,
        ] {
            let (status, answer) =
                json(app(state.clone()), send_json("PUT", "/api/status", body)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {answer}");
            assert_eq!(answer["error"]["code"], "badRequest", "{body}");
        }
    }

    #[tokio::test]
    async fn the_next_route_pushes_a_request_with_the_page_that_the_learner_left() {
        let state = state();

        let (status, answer) = json(app(state.clone()), post("/api/pages/1/next")).await;

        assert_eq!(status, StatusCode::OK, "{answer}");
        insta::assert_json_snapshot!("next", answer);

        let (_, agent) = json(
            app(state.clone()),
            get_with_cookie("/api/events?after=0&who=agent", TOKEN),
        )
        .await;
        insta::assert_json_snapshot!("events_of_the_agent", server_redacted(&state, agent));
        assert!(
            state.events.agent_works(),
            "the wait handed the request over"
        );

        let (_, page) = json(
            app(state),
            get_with_cookie("/api/events?after=0&who=page", TOKEN),
        )
        .await;
        assert_eq!(
            page["events"],
            serde_json::json!([
                { "seq": 1, "kind": "next", "author": "learner", "page": "1", "id": null }
            ]),
            "the page needs no detail"
        );
    }

    #[tokio::test]
    async fn next_on_a_page_with_no_content_is_not_found() {
        let state = state();
        let mut course = state.course.write().await;
        course.parts[0].pages.push(PageRef {
            id: "2".to_owned(),
            kind: PageKind::Lesson,
            title: "The vote".to_owned(),
            state: PageState::Planned,
            score: None,
        });
        drop(course);

        for uri in [
            "/api/pages/2/next",
            "/api/pages/9/next",
            "/api/pages/x/next",
            "/api/pages/..%2Fcourse/next",
        ] {
            let (status, body) = json(app(state.clone()), post(uri)).await;

            assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
            assert_eq!(body["error"]["code"], "notFound", "{uri}");
        }
        assert_eq!(state.events.after(None, events::HOLD).await.next, 0);
    }

    const DIAGRAM: &str = "# The states\n\nA server changes state.\n\n\
                           ```mermaid\nstateDiagram-v2\n  Follower --> Candidat e\n```\n";

    /// The id of the diagram of `DIAGRAM`, and a page that holds it.
    async fn with_diagram() -> (Repo, AppState, String) {
        let dir = repo().await;
        let state = state_in(dir.path());
        let (status, pushed) = json(app(state.clone()), put("/api/pages/1", DIAGRAM)).await;
        assert_eq!(status, StatusCode::OK, "{pushed}");
        assert_eq!(pushed["blocks"][1]["kind"], "diagram");
        let block = pushed["blocks"][1]["id"].as_str().unwrap().to_owned();

        (dir, state, block)
    }

    #[tokio::test]
    async fn the_broken_route_hands_the_agent_the_source_and_the_error() {
        let (_dir, state, block) = with_diagram().await;
        let body = serde_json::json!({
            "block": block,
            "error": "Parse error on line 2:\n...ollower --> Candidat e\nExpecting 'EOF', got 'ID'",
        });
        // The agent waited since its push, so its idle event is behind.
        let (_, waited) = json(
            app(state.clone()),
            get_with_cookie("/api/events?who=agent", TOKEN),
        )
        .await;
        let after = waited["next"].as_u64().unwrap();

        let (status, answer) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/broken", &body.to_string()),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{answer}");
        insta::assert_json_snapshot!("broken", answer);

        let (_, agent) = json(
            app(state.clone()),
            get_with_cookie(&format!("/api/events?after={after}&who=agent"), TOKEN),
        )
        .await;
        insta::assert_json_snapshot!("events_broken", server_redacted(&state, agent));
        assert!(
            state.events.agent_works(),
            "the wait handed the request over"
        );

        let (_, page) = json(
            app(state),
            get_with_cookie(&format!("/api/events?after={after}&who=page"), TOKEN),
        )
        .await;
        assert_eq!(page["events"][0]["kind"], "broken");
        assert!(
            page["events"][0].get("detail").is_none(),
            "the page needs no detail: {page}"
        );
        // The agent fixes the page on the screen, and prepares no other.
        assert_eq!(page["working"], true);
        assert_eq!(page["preparing"], false);
    }

    /// A reload or a second tab reports the block again, and the agent must
    /// not get the same request twice.
    #[tokio::test]
    async fn a_broken_block_is_passed_on_once_for_each_push_of_its_page() {
        let (_dir, state, block) = with_diagram().await;
        let after = state.events.after(None, events::HOLD).await.next;
        let body = serde_json::json!({ "block": block, "error": "bad" }).to_string();
        let report = || async {
            let (status, answer) = json(
                app(state.clone()),
                send_json("POST", "/api/pages/1/broken", &body),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{answer}");
            answer
        };

        let first = report().await;
        let again = report().await;

        assert_eq!(again, first, "the repeat answers the first request");
        let batch = state.events.after(Some(after), events::HOLD).await;
        assert_eq!(batch.events.len(), 1, "{:?}", batch.events);

        // A push of the same page, with the same broken diagram, is a new
        // version that the agent hears of.
        let (status, _) = json(app(state.clone()), put("/api/pages/1", DIAGRAM)).await;
        assert_eq!(status, StatusCode::OK);
        let third = report().await;
        assert!(third["seq"].as_u64() > first["seq"].as_u64(), "{third}");
        let batch = state.events.after(Some(after), events::HOLD).await;
        let kinds: Vec<_> = batch.events.iter().map(|event| event.kind).collect();
        assert_eq!(
            kinds,
            [EventKind::Broken, EventKind::Page, EventKind::Broken]
        );
    }

    #[tokio::test]
    async fn a_broken_block_that_the_page_does_not_hold_is_refused() {
        let (_dir, state, block) = with_diagram().await;
        let text = state.store.load_page("raft", "1").unwrap().unwrap().blocks[0]
            .id()
            .to_owned();
        state.course.write().await.parts[0].pages.push(PageRef {
            id: "2".to_owned(),
            kind: PageKind::Lesson,
            title: "The vote".to_owned(),
            state: PageState::Planned,
            score: None,
        });
        let after = state.events.after(None, events::HOLD).await.next;
        let report =
            |block: &str| serde_json::json!({ "block": block, "error": "bad" }).to_string();

        for (uri, body, code) in [
            ("/api/pages/2/broken", report(&block), "notFound"),
            ("/api/pages/9/broken", report(&block), "notFound"),
            ("/api/pages/x/broken", report(&block), "notFound"),
            ("/api/pages/..%2Fcourse/broken", report(&block), "notFound"),
            ("/api/pages/1/broken", report("000000000000"), "notFound"),
            ("/api/pages/1/broken", report(&text), "badRequest"),
            ("/api/pages/1/broken", "{}".to_owned(), "badRequest"),
            (
                "/api/pages/1/broken",
                serde_json::json!({ "block": block }).to_string(),
                "badRequest",
            ),
            (
                "/api/pages/1/broken",
                serde_json::json!({ "block": block, "error": "bad", "page": "1" }).to_string(),
                "badRequest",
            ),
            ("/api/pages/1/broken", "not json".to_owned(), "badRequest"),
        ] {
            let (status, answer) = json(app(state.clone()), send_json("POST", uri, &body)).await;

            let expected = match code {
                "notFound" => StatusCode::NOT_FOUND,
                _ => StatusCode::BAD_REQUEST,
            };
            assert_eq!(status, expected, "{uri} {body}: {answer}");
            assert_eq!(answer["error"]["code"], code, "{uri} {body}");
            assert!(answer["error"]["message"].is_string(), "{answer}");
        }
        assert_eq!(state.events.after(None, events::HOLD).await.next, after);
    }

    /// Another local site could otherwise send the agent a request, with the
    /// error text of its choice.
    #[tokio::test]
    async fn a_broken_report_from_another_site_is_forbidden() {
        let (_dir, state, block) = with_diagram().await;
        let after = state.events.after(None, events::HOLD).await.next;
        let body = serde_json::json!({ "block": block, "error": "bad" }).to_string();
        let request = Request::builder()
            .method("POST")
            .uri("/api/pages/1/broken")
            .header(
                header::COOKIE,
                format!("{}={TOKEN}", auth::cookie_name(PORT)),
            )
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, "http://127.0.0.1:3000")
            .body(Body::from(body))
            .unwrap();

        let (status, answer) = json(app(state.clone()), request).await;

        assert_eq!(status, StatusCode::FORBIDDEN, "{answer}");
        assert_eq!(answer["error"]["code"], "forbidden");
        assert_eq!(state.events.after(None, events::HOLD).await.next, after);
    }

    /// The quiz of section 5.1 of the design, with inline Markdown in its
    /// options and the author that the commands of the agent send.
    const QUIZ: &str = r#"{
        "open": {
            "prompt": "In 2 sentences: why is the timeout random?",
            "points": ["random-timeout"]
        },
        "choices": [
            {
                "header": "Split vote",
                "prompt": "Two candidates get 2 votes each out of 5. What happens next?",
                "options": [
                    "The candidate with the higher id wins",
                    "Both time out and start a *new* election in a new `term`",
                    "The leader of the previous term breaks the tie <b>now</b>"
                ],
                "answer": 1,
                "point": "split-vote"
            }
        ],
        "points": {
            "random-timeout": { "label": "Why the election timeout is random" },
            "split-vote": { "label": "How a split vote ends", "lesson": "1" }
        },
        "author": "agent"
    }"#;

    /// Every key of a JSON value, at any depth.
    fn keys(value: &serde_json::Value) -> Vec<String> {
        match value {
            serde_json::Value::Object(fields) => fields
                .iter()
                .flat_map(|(key, value)| std::iter::once(key.clone()).chain(keys(value)))
                .collect(),
            serde_json::Value::Array(items) => items.iter().flat_map(keys).collect(),
            _ => Vec::new(),
        }
    }

    /// A course whose page 1 holds `LESSON`.
    async fn with_lesson() -> (Repo, AppState) {
        let dir = repo().await;
        let state = state_in(dir.path());
        let (status, pushed) = json(app(state.clone()), put("/api/pages/1", LESSON)).await;
        assert_eq!(status, StatusCode::OK, "{pushed}");
        (dir, state)
    }

    #[tokio::test]
    async fn the_quiz_route_stores_the_quiz_and_answers_its_number() {
        let (dir, state) = with_lesson().await;
        let untouched = outside(dir.path());
        let after = state.events.after(None, events::HOLD).await.next;

        let (status, body) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes", QUIZ),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{body}");
        insta::assert_json_snapshot!("push_quiz", body);
        let batch = state.events.after(Some(after), events::HOLD).await;
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].kind, EventKind::Quiz);
        assert_eq!(batch.events[0].author, Author::Agent);
        assert_eq!(batch.events[0].page.as_deref(), Some("1"));
        assert_eq!(batch.events[0].id.as_deref(), Some("1"));

        let stored = std::fs::read_to_string(dir.path().join(".learn/raft/pages/1.json")).unwrap();
        assert!(stored.contains("\"answer\": 1"), "{stored}");
        let points = Store::at(dir.path()).load_points("raft").unwrap();
        let labels: Vec<_> = points
            .iter()
            .map(|point| (point.id.as_str(), point.lesson.as_str()))
            .collect();
        assert_eq!(labels, [("random-timeout", "1"), ("split-vote", "1")]);
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".learn/raft/pages/1.md")).unwrap(),
            LESSON,
            "a quiz leaves the source alone"
        );

        // A point that the course knows needs no label.
        let second = r#"{"choices":[{"header":"Timer","prompt":"Who starts?",
            "options":["The first to time out","The leader"],"answer":0,
            "point":"random-timeout"}]}"#;
        let (status, body) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes", second),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body, serde_json::json!({ "page": "1", "n": 2 }));
        assert_eq!(Store::at(dir.path()).load_points("raft").unwrap(), points);
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
    }

    /// The right option stays on the server until the learner sends the
    /// choices, and the hidden line until the learner answers the quiz.
    #[tokio::test]
    async fn the_page_route_sends_the_quizzes_without_their_answers() {
        let dir = repo().await;
        let state = state_in(dir.path());
        let (status, _) = json(app(state.clone()), put("/api/pages/1", EXCERPT)).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes", QUIZ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, body) = json(app(state), get_with_cookie("/api/pages/1", TOKEN)).await;

        assert_eq!(status, StatusCode::OK);
        insta::assert_json_snapshot!("page_quiz", body);
        let keys = keys(&body);
        assert!(keys.contains(&"options".to_owned()), "{keys:?}");
        assert!(!keys.contains(&"answer".to_owned()), "{body}");
        assert!(!keys.contains(&"point".to_owned()), "{body}");
        assert_eq!(
            body["blocks"][1]["excerpt"]["lines"][3],
            serde_json::Value::Null
        );
    }

    #[tokio::test]
    async fn a_quiz_on_an_answered_page_takes_it_back_to_taught() {
        let (dir, state) = with_lesson().await;
        {
            let mut course = state.course.write().await;
            course.parts[0].pages[0].state = PageState::Answered;
            state.store.save(&course).unwrap();
        }

        let (status, body) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes", QUIZ),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{body}");
        let (_, session) = json(app(state.clone()), get_with_cookie("/api/session", TOKEN)).await;
        assert_eq!(session["course"]["parts"][0]["pages"][0]["state"], "taught");
        let stored = Store::at(dir.path()).load("raft").unwrap().unwrap();
        assert_eq!(stored.parts[0].pages[0].state, PageState::Taught);
        assert_eq!(stored, *state.course.read().await, "course.json holds it");
    }

    /// The quizzes may hold answers of the learner, which a new version of
    /// the blocks must not lose.
    #[tokio::test]
    async fn a_page_pushed_again_keeps_its_quizzes() {
        let (_dir, state) = with_lesson().await;
        json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes", QUIZ),
        )
        .await;
        let (_, before) = json(app(state.clone()), get_with_cookie("/api/pages/1", TOKEN)).await;

        let (status, pushed) = json(app(state.clone()), put("/api/pages/1", "# Rewritten\n")).await;

        assert_eq!(status, StatusCode::OK, "{pushed}");
        assert_eq!(pushed["quizzes"], before["quizzes"]);
        assert_eq!(pushed["quizzes"].as_array().unwrap().len(), 1);
    }

    /// The page file holds the quizzes and, later, the answers of the
    /// learner. A push must not write over it when it cannot read it.
    #[tokio::test]
    async fn a_push_over_a_corrupt_page_file_is_refused_and_leaves_it() {
        let (dir, state) = with_lesson().await;
        let path = dir.path().join(".learn/raft/pages/1.json");
        std::fs::write(&path, "{ not json").unwrap();
        let before = state.course.read().await.clone();

        let (status, pushed) = json(app(state.clone()), put("/api/pages/1", LESSON)).await;
        let (_, quiz) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes", QUIZ),
        )
        .await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{pushed}");
        for answer in [&pushed, &quiz] {
            let message = answer["error"]["message"].as_str().unwrap();
            assert!(message.contains("repair it by hand"), "{message}");
            // Pushing again gives the same error, so the advice must not
            // send the agent round in a loop.
            assert!(!message.contains("push the page again"), "{message}");
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
        assert_eq!(*state.course.read().await, before);
    }

    /// The page file is written last, so a quiz is stored once it holds
    /// it. A write of the page file that fails puts the points and the
    /// course back.
    #[tokio::test]
    async fn a_quiz_whose_page_file_cannot_be_written_stores_nothing() {
        use std::os::unix::fs::PermissionsExt;

        let (dir, state) = with_lesson().await;
        {
            let mut course = state.course.write().await;
            course.parts[0].pages[0].state = PageState::Answered;
            state.store.save(&course).unwrap();
        }
        let stored = files(dir.path());
        let course_file = dir.path().join(".learn/raft/course.json");
        let course_bytes = std::fs::read(&course_file).unwrap();
        let page = std::fs::read(dir.path().join(".learn/raft/pages/1.json")).unwrap();
        let before = state.course.read().await.clone();
        let pages = dir.path().join(".learn/raft/pages");
        let mode = |mode| std::fs::set_permissions(&pages, std::fs::Permissions::from_mode(mode));
        mode(0o555).unwrap();
        // Root writes in a directory with no write bit, so the case cannot
        // happen there.
        if std::fs::write(pages.join("probe"), "").is_ok() {
            std::fs::remove_file(pages.join("probe")).unwrap();
            mode(0o755).unwrap();
            return;
        }

        let (status, answer) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes", QUIZ),
        )
        .await;
        mode(0o755).unwrap();

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{answer}");
        assert_eq!(files(dir.path()), stored, "no points.json");
        assert_eq!(std::fs::read(&course_file).unwrap(), course_bytes);
        assert_eq!(
            std::fs::read(dir.path().join(".learn/raft/pages/1.json")).unwrap(),
            page
        );
        assert_eq!(*state.course.read().await, before);
    }

    #[tokio::test]
    async fn a_refused_quiz_gets_every_fault_and_stores_nothing() {
        let (dir, state) = with_lesson().await;
        let stored = files(dir.path());
        let page = std::fs::read_to_string(dir.path().join(".learn/raft/pages/1.json")).unwrap();
        let before = state.course.read().await.clone();
        let after = state.events.after(None, events::HOLD).await.next;
        let body = r#"{
            "open": { "prompt": "  ", "points": [] },
            "choices": [
                { "header": "A", "prompt": "Who?", "options": ["S1", "S2", "S3"],
                  "answer": 3, "point": "who" },
                { "header": "B", "prompt": "When?", "options": ["now"], "answer": 0,
                  "point": "when" },
                { "header": "C", "prompt": "How?", "options": ["- a list", "b"],
                  "answer": 0, "point": "how" }
            ],
            "points": { "who": {}, "how": { "label": "How" } }
        }"#;

        let (status, answer) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes", body),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
        insta::assert_json_snapshot!("push_quiz_refused", answer);
        for body in ["", "not json", "[]", "{}", r#"{"open":null,"choices":[]}"#] {
            let (status, answer) = json(
                app(state.clone()),
                send_json("POST", "/api/pages/1/quizzes", body),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {answer}");
            assert!(answer["error"]["message"].is_string(), "{answer}");
        }
        assert_eq!(files(dir.path()), stored);
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".learn/raft/pages/1.json")).unwrap(),
            page
        );
        assert_eq!(*state.course.read().await, before);
        assert_eq!(state.events.after(None, events::HOLD).await.next, after);
    }

    /// A quiz sits under its lesson, so the lesson comes first.
    #[tokio::test]
    async fn a_quiz_on_a_page_with_no_content_is_refused() {
        let dir = repo().await;
        let state = state_in(dir.path());
        state.course.write().await.parts[0].pages[0].state = PageState::Planned;
        let untouched = outside(dir.path());

        for (uri, code) in [
            ("/api/pages/1/quizzes", "notFound"),
            ("/api/pages/9/quizzes", "notFound"),
            ("/api/pages/x1/quizzes", "badRequest"),
            ("/api/pages/..%2Fcourse/quizzes", "badRequest"),
        ] {
            let (_, answer) = json(app(state.clone()), send_json("POST", uri, QUIZ)).await;

            assert_eq!(answer["error"]["code"], code, "{uri}: {answer}");
        }
        let (_, answer) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes", QUIZ),
        )
        .await;
        let message = answer["error"]["message"].as_str().unwrap();
        assert!(message.contains("`qlearn page 1`"), "{message}");
        assert_eq!(files(dir.path()), Vec::<String>::new());
        assert_eq!(state.events.after(None, events::HOLD).await.next, 0);
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
    }

    /// A course whose page 1 holds `source` and the quiz `quiz`.
    async fn with_quiz(source: &str, quiz: &str) -> (Repo, AppState) {
        let dir = repo().await;
        let state = state_in(dir.path());
        let (status, pushed) =
            json(app(state.clone()), put("/api/pages/1", source.to_owned())).await;
        assert_eq!(status, StatusCode::OK, "{pushed}");
        let (status, body) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes", quiz),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        (dir, state)
    }

    const OPEN: &str = r#"{"text":"  So that two followers rarely wake up at once.\n"}"#;

    /// The learner picks the wrong option, and says why in the Other box.
    const CHOICES: &str = r#"{"choices":[{"option":0,"other":"Unless the ids tie too"}]}"#;

    #[tokio::test]
    async fn the_open_route_takes_the_open_answer_and_keeps_the_right_option() {
        let (dir, state) = with_quiz(LESSON, QUIZ).await;
        let untouched = outside(dir.path());
        let after = state.events.after(None, events::HOLD).await.next;

        let (status, body) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes/1/open", OPEN),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{body}");
        insta::assert_json_snapshot!("open", body);
        assert_eq!(
            body["quizzes"][0]["open"]["sent"],
            "So that two followers rarely wake up at once."
        );
        let (_, page) = json(app(state.clone()), get_with_cookie("/api/pages/1", TOKEN)).await;
        assert_eq!(page, body);
        assert!(!keys(&page).contains(&"answer".to_owned()), "{page}");
        assert!(!keys(&page).contains(&"picked".to_owned()), "{page}");

        // The choices are left, so the agent has nothing to answer yet. The
        // other tabs of the page show the open answer.
        let batch = state.events.after(Some(after), events::HOLD).await;
        let told: Vec<_> = batch
            .events
            .iter()
            .map(|event| (event.kind, event.page.as_deref(), event.id.as_deref()))
            .collect();
        assert_eq!(told, [(EventKind::Answered, Some("1"), Some("1"))]);
        let (_, session) = json(app(state), get_with_cookie("/api/session", TOKEN)).await;
        assert_eq!(session["course"]["parts"][0]["pages"][0]["state"], "taught");
        let points = Store::at(dir.path()).load_points("raft").unwrap();
        assert!(
            points.iter().all(|point| point.results.is_empty()),
            "the agent grades the open answer: {points:?}"
        );
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
    }

    #[tokio::test]
    async fn the_choices_route_grades_the_choices_and_answers_the_right_option() {
        let (dir, state) = with_quiz(LESSON, QUIZ).await;
        let untouched = outside(dir.path());
        json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes/1/open", OPEN),
        )
        .await;
        // The agent waits since its quiz.
        let (_, waited) = json(
            app(state.clone()),
            get_with_cookie("/api/events?who=agent", TOKEN),
        )
        .await;
        let after = waited["next"].as_u64().unwrap();

        let (status, body) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes/1/choices", CHOICES),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{body}");
        insta::assert_json_snapshot!("choices", body);
        let (status, page) = json(app(state.clone()), get_with_cookie("/api/pages/1", TOKEN)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(page, body);
        insta::assert_json_snapshot!("page_answered", page);
        assert_eq!(
            page["quizzes"][0]["choices"][0]["graded"],
            serde_json::json!({ "picked": 0, "answer": 1, "ok": false,
                                "other": "Unless the ids tie too" })
        );

        let (_, agent) = json(
            app(state.clone()),
            get_with_cookie(&format!("/api/events?after={after}&who=agent"), TOKEN),
        )
        .await;
        insta::assert_json_snapshot!("events_answer", server_redacted(&state, agent));
        assert!(
            state.events.agent_works(),
            "the wait handed the request over"
        );
        let (_, events) = json(
            app(state.clone()),
            get_with_cookie(&format!("/api/events?after={after}&who=page"), TOKEN),
        )
        .await;
        assert_eq!(
            events["events"],
            serde_json::json!([
                { "seq": after + 1, "kind": "answered", "author": "learner", "page": "1", "id": "1" },
                { "seq": after + 2, "kind": "answer", "author": "learner", "page": "1", "id": "1" }
            ]),
            "the page needs no detail"
        );
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
    }

    /// A choice adds its result at once. The open answer gets its grade
    /// from the agent, in the debrief.
    #[tokio::test]
    async fn the_choices_add_their_results_to_points_json() {
        let (dir, state) = with_quiz(LESSON, QUIZ).await;
        json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes/1/open", OPEN),
        )
        .await;
        let second = r#"{"choices":[
            {"header":"Timer","prompt":"Who starts?","options":["The first to time out","The leader"],
             "answer":0,"point":"random-timeout"},
            {"header":"Again","prompt":"And then?","options":["A new term","The same term"],
             "answer":0,"point":"split-vote"}]}"#;
        json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes", second),
        )
        .await;

        json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes/1/choices", CHOICES),
        )
        .await;
        let (status, body) = json(
            app(state.clone()),
            send_json(
                "POST",
                "/api/pages/1/quizzes/2/choices",
                r#"{"choices":[{"option":0},{"option":0,"other":"  "}]}"#,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body["quizzes"][1]["choices"][1]["graded"]["other"],
            serde_json::Value::Null,
            "an Other box of spaces is empty"
        );

        let text = std::fs::read_to_string(dir.path().join(".learn/raft/points.json")).unwrap();
        let mut points: serde_json::Value = serde_json::from_str(&text).unwrap();
        for point in points["points"].as_array_mut().unwrap() {
            for result in point["results"].as_array_mut().unwrap() {
                let at = result["at"].as_str().unwrap();
                assert!(
                    OffsetDateTime::parse(at, &time::format_description::well_known::Rfc3339)
                        .is_ok(),
                    "{at}"
                );
                result["at"] = "[at]".into();
            }
        }
        assert_eq!(
            points,
            serde_json::json!({ "points": [
                { "id": "random-timeout", "label": "Why the election timeout is random",
                  "lesson": "1", "results": [
                    { "at": "[at]", "result": "ok", "how": "choice" },
                  ] },
                { "id": "split-vote", "label": "How a split vote ends", "lesson": "1",
                  "results": [
                    { "at": "[at]", "result": "missed", "how": "choice" },
                    { "at": "[at]", "result": "ok", "how": "choice" },
                  ] },
            ] })
        );
    }

    /// A server killed in the middle of an answer completes it at its next
    /// start: each result lands once, and the learner sends nothing again.
    #[tokio::test]
    async fn a_server_killed_in_an_answer_completes_it_at_its_next_start() {
        // The kill comes after the page file, or after points.json too.
        for put_back in [&["points.json", "course.json"][..], &["course.json"][..]] {
            let (dir, state) = with_quiz(LESSON, QUIZ).await;
            let course_dir = dir.path().join(".learn/raft");
            json(
                app(state.clone()),
                send_json("POST", "/api/pages/1/quizzes/1/open", OPEN),
            )
            .await;
            let before: Vec<_> = put_back
                .iter()
                .map(|name| (name, std::fs::read(course_dir.join(name)).unwrap()))
                .collect();
            let (status, body) = json(
                app(state.clone()),
                send_json("POST", "/api/pages/1/quizzes/1/choices", CHOICES),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{body}");
            for (name, bytes) in before {
                std::fs::write(course_dir.join(name), bytes).unwrap();
            }

            let store = Store::at(dir.path());
            let mut course = store.load("raft").unwrap().unwrap();
            assert_eq!(course.parts[0].pages[0].state, PageState::Taught);
            // `qlearn export` with no server sees the course settled, and
            // writes nothing.
            let stored = files(dir.path())
                .iter()
                .map(|name| std::fs::read(dir.path().join(name)).unwrap())
                .collect::<Vec<_>>();
            let points = store.load_points("raft").unwrap();
            let (seen, _) = settled(&store, &course, points).unwrap();
            assert_eq!(seen.parts[0].pages[0].state, PageState::Answered);
            let after = files(dir.path())
                .iter()
                .map(|name| std::fs::read(dir.path().join(name)).unwrap())
                .collect::<Vec<_>>();
            assert_eq!(after, stored, "{put_back:?}");
            settle(&store, &mut course).unwrap();
            settle(&store, &mut course).unwrap();

            assert_eq!(course.parts[0].pages[0].state, PageState::Answered);
            assert_eq!(store.load("raft").unwrap().unwrap(), course);
            let points = store.load_points("raft").unwrap();
            let split = points
                .iter()
                .find(|point| point.id == "split-vote")
                .unwrap();
            assert_eq!(
                split
                    .results
                    .iter()
                    .map(|result| (result.result, result.how))
                    .collect::<Vec<_>>(),
                [(model::Grade::Missed, Graded::Choice)],
                "{put_back:?}"
            );

            // The page holds the choices, so a resend changes nothing.
            let restarted = AppState::new(
                Root::Repository(Git::at(dir.path())),
                Store::at(dir.path()),
                course,
                None,
                TOKEN.to_owned(),
                PORT,
            );
            let (status, _) = json(
                app(restarted),
                send_json("POST", "/api/pages/1/quizzes/1/choices", CHOICES),
            )
            .await;
            assert_eq!(status, StatusCode::CONFLICT);
            assert_eq!(store.load_points("raft").unwrap(), points);
        }
    }

    /// A page is answered when the learner answered every quiz of it, not
    /// one of them.
    #[tokio::test]
    async fn a_page_goes_from_taught_to_answered_with_its_last_quiz() {
        let only_choices = r#"{"choices":[{"header":"Timer","prompt":"Who starts?",
            "options":["The first to time out","The leader"],"answer":0,
            "point":"random-timeout"}]}"#;
        let (dir, state) = with_quiz(LESSON, QUIZ).await;
        json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes", only_choices),
        )
        .await;
        let state_of = || async {
            let (_, session) =
                json(app(state.clone()), get_with_cookie("/api/session", TOKEN)).await;
            session["course"]["parts"][0]["pages"][0]["state"].clone()
        };
        let send = |uri: &'static str, body: &'static str| {
            let state = state.clone();
            async move {
                let (status, answer) = json(app(state), send_json("POST", uri, body)).await;
                assert_eq!(status, StatusCode::OK, "{uri}: {answer}");
            }
        };

        // A quiz with no open question starts at its choices.
        send(
            "/api/pages/1/quizzes/2/choices",
            r#"{"choices":[{"option":0}]}"#,
        )
        .await;
        assert_eq!(state_of().await, "taught");
        send("/api/pages/1/quizzes/1/open", OPEN).await;
        assert_eq!(state_of().await, "taught");
        send("/api/pages/1/quizzes/1/choices", CHOICES).await;
        assert_eq!(state_of().await, "answered");

        let stored = Store::at(dir.path()).load("raft").unwrap().unwrap();
        assert_eq!(stored.parts[0].pages[0].state, PageState::Answered);
        assert_eq!(stored, *state.course.read().await, "course.json holds it");
        let kinds: Vec<_> = state
            .events
            .after(Some(0), events::HOLD)
            .await
            .events
            .iter()
            .filter(|event| event.kind.is_request())
            .map(|event| event.id.clone())
            .collect();
        assert_eq!(
            kinds,
            [Some("2".to_owned()), Some("1".to_owned())],
            "one request per quiz, once its last part is sent"
        );
    }

    /// An exercise is a quiz with no choices: its open answer is the last
    /// part, and the agent gets it at once.
    #[tokio::test]
    async fn a_quiz_with_no_choices_is_answered_by_its_open_answer() {
        let exercise = r#"{"open":{"prompt":"Fix the timer.","points":["random-timeout"]},
            "points":{"random-timeout":{"label":"Why the election timeout is random"}}}"#;
        let (_dir, state) = with_quiz(LESSON, exercise).await;
        let after = state.events.after(None, events::HOLD).await.next;

        let (status, body) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes/1/open", OPEN),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{body}");
        let batch = state.events.after(Some(after), events::HOLD).await;
        let kinds: Vec<_> = batch.events.iter().map(|event| event.kind).collect();
        assert_eq!(kinds, [EventKind::Answered, EventKind::Answer]);
        assert_eq!(
            batch.events[1].detail,
            Some(Detail::Answer {
                quiz: 1,
                open: Some(model::OpenAnswer {
                    prompt: "Fix the timer.".to_owned(),
                    points: vec!["random-timeout".to_owned()],
                    text: "So that two followers rarely wake up at once.".to_owned(),
                }),
                choices: Vec::new(),
            })
        );
        assert_eq!(
            state.course.read().await.parts[0].pages[0].state,
            PageState::Answered
        );
    }

    /// The text of a hidden line is the answer to "Complete the code", so
    /// it waits for every quiz of the page.
    #[tokio::test]
    async fn the_hidden_line_shows_once_every_quiz_is_answered() {
        let (_dir, state) = with_quiz(EXCERPT, QUIZ).await;
        let line = || async {
            let (_, page) = json(app(state.clone()), get_with_cookie("/api/pages/1", TOKEN)).await;
            page["blocks"][1]["excerpt"]["lines"][3].clone()
        };

        assert_eq!(line().await, serde_json::Value::Null);
        json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes/1/open", OPEN),
        )
        .await;
        assert_eq!(line().await, serde_json::Value::Null);
        let (_, body) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes/1/choices", CHOICES),
        )
        .await;

        assert_eq!(line().await["text"], "    return base + rand() % base;");
        assert_eq!(body["blocks"][1]["excerpt"]["lines"][3], line().await);
    }

    #[tokio::test]
    async fn an_answer_that_the_quiz_does_not_take_is_refused_and_stores_nothing() {
        let only_choices = r#"{"choices":[{"header":"Timer","prompt":"Who starts?",
            "options":["The first to time out","The leader"],"answer":0,
            "point":"random-timeout"}]}"#;
        let exercise = r#"{"open":{"prompt":"Fix the timer.","points":["random-timeout"]}}"#;
        let (dir, state) = with_quiz(LESSON, QUIZ).await;
        for quiz in [only_choices, exercise] {
            let (status, body) = json(
                app(state.clone()),
                send_json("POST", "/api/pages/1/quizzes", quiz),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{body}");
        }
        state.course.write().await.parts[0].pages.push(PageRef {
            id: "2".to_owned(),
            kind: PageKind::Lesson,
            title: "The vote".to_owned(),
            state: PageState::Planned,
            score: None,
        });
        let untouched = outside(dir.path());
        // Every file of the store, with its bytes.
        let stored = |root: &std::path::Path| {
            files(root)
                .into_iter()
                .map(|file| {
                    let bytes = std::fs::read(root.join(&file)).unwrap();
                    (file, bytes)
                })
                .collect::<Vec<_>>()
        };
        let check = |uri: &str, body: &str, code: &str| {
            let state = state.clone();
            let (uri, body, code) = (uri.to_owned(), body.to_owned(), code.to_owned());
            async move {
                let before = stored(state.root.dir());
                let course = state.course.read().await.clone();
                let after = state.events.after(None, events::HOLD).await.next;

                let (status, answer) =
                    json(app(state.clone()), send_json("POST", &uri, &body)).await;

                let expected = match code.as_str() {
                    "notFound" => StatusCode::NOT_FOUND,
                    "conflict" => StatusCode::CONFLICT,
                    _ => StatusCode::BAD_REQUEST,
                };
                assert_eq!(status, expected, "{uri} {body}: {answer}");
                assert_eq!(answer["error"]["code"], code.as_str(), "{uri} {body}");
                assert!(answer["error"]["message"].is_string(), "{answer}");
                assert_eq!(stored(state.root.dir()), before, "{uri} {body}");
                assert_eq!(*state.course.read().await, course, "{uri} {body}");
                assert_eq!(state.events.after(None, events::HOLD).await.next, after);
                answer
            }
        };

        // An unknown page, or an unknown quiz of a known page.
        for uri in [
            "/api/pages/2/quizzes/1/open",
            "/api/pages/9/quizzes/1/open",
            "/api/pages/x/quizzes/1/open",
            "/api/pages/..%2Fcourse/quizzes/1/open",
            "/api/pages/1/quizzes/0/open",
            "/api/pages/1/quizzes/4/open",
            "/api/pages/1/quizzes/01/open",
            "/api/pages/1/quizzes/+1/open",
            "/api/pages/1/quizzes/x/open",
        ] {
            check(uri, OPEN, "notFound").await;
        }
        check("/api/pages/1/quizzes/4/choices", CHOICES, "notFound").await;
        // A part that the quiz does not have.
        check("/api/pages/1/quizzes/2/open", OPEN, "notFound").await;
        check("/api/pages/1/quizzes/3/choices", CHOICES, "notFound").await;
        // The choices come after the open answer.
        let answer = check("/api/pages/1/quizzes/1/choices", CHOICES, "conflict").await;
        let message = answer["error"]["message"].as_str().unwrap();
        assert!(
            message.starts_with("send the open answer first"),
            "{message}"
        );
        // Bodies that do not fit.
        for body in [
            "",
            "not json",
            "{}",
            r#"{"text":null}"#,
            r#"{"text":"  "}"#,
            r#"{"text":"A","page":"1"}"#,
        ] {
            check("/api/pages/1/quizzes/1/open", body, "badRequest").await;
        }
        let (status, _) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes/1/open", OPEN),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        check("/api/pages/1/quizzes/1/open", OPEN, "conflict").await;
        for body in [
            r#"{"choices":[]}"#,
            r#"{"choices":[{"option":0},{"option":1}]}"#,
            r#"{"choices":[{"option":3}]}"#,
            r#"{"choices":[{"option":-1}]}"#,
            r#"{"choices":[{"other":"A new term"}]}"#,
            r#"{"choices":[{"option":0,"answer":0}]}"#,
            r#"{"choices":[null]}"#,
            "{}",
        ] {
            check("/api/pages/1/quizzes/1/choices", body, "badRequest").await;
        }
        let answer = check(
            "/api/pages/1/quizzes/1/choices",
            r#"{"choices":[{"option":3}]}"#,
            "badRequest",
        )
        .await;
        assert_eq!(
            answer["error"]["message"],
            "choices[0].option: 3 is out of range. The choice has 3 options, so option takes \
             0 to 2"
        );
        let answer = check(
            "/api/pages/1/quizzes/1/choices",
            r#"{"choices":[]}"#,
            "badRequest",
        )
        .await;
        assert_eq!(
            answer["error"]["message"],
            "choices: holds 0 picks, and the quiz has 1 choice. Pick one option in each"
        );
        let (status, _) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes/1/choices", CHOICES),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        check("/api/pages/1/quizzes/1/choices", CHOICES, "conflict").await;
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
    }

    /// Another local site could otherwise answer a quiz in the name of the
    /// learner.
    #[tokio::test]
    async fn an_answer_from_another_site_is_forbidden() {
        let (_dir, state) = with_quiz(LESSON, QUIZ).await;
        let after = state.events.after(None, events::HOLD).await.next;
        let request = Request::builder()
            .method("POST")
            .uri("/api/pages/1/quizzes/1/open")
            .header(
                header::COOKIE,
                format!("{}={TOKEN}", auth::cookie_name(PORT)),
            )
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, "http://127.0.0.1:3000")
            .body(Body::from(OPEN))
            .unwrap();

        let (status, answer) = json(app(state.clone()), request).await;

        assert_eq!(status, StatusCode::FORBIDDEN, "{answer}");
        assert_eq!(answer["error"]["code"], "forbidden");
        let (_, page) = json(app(state.clone()), get_with_cookie("/api/pages/1", TOKEN)).await;
        assert_eq!(page["quizzes"][0]["open"]["sent"], serde_json::Value::Null);
        assert_eq!(state.events.after(None, events::HOLD).await.next, after);
    }

    /// A quiz whose open question names two points, and whose choice
    /// checks a third.
    const QUIZ_TWO_POINTS: &str = r#"{
        "open": {
            "prompt": "Why is the timeout random, and how many votes win?",
            "points": ["random-timeout", "majority"]
        },
        "choices": [
            {
                "header": "Split vote",
                "prompt": "Two candidates get 2 votes each out of 5. What happens next?",
                "options": ["The higher id wins", "Both time out and start a new term"],
                "answer": 1,
                "point": "split-vote"
            }
        ],
        "points": {
            "random-timeout": { "label": "Why the election timeout is random" },
            "majority": { "label": "How many votes make a leader" },
            "split-vote": { "label": "How a split vote ends" }
        }
    }"#;

    /// A debrief with an excerpt and a diagram, as section 5.4 of the
    /// design allows.
    const DEBRIEF: &str = "# Almost\n\n\
                           The random part is right. A majority is 3 of 5, not 2.\n\n\
                           ```excerpt\npath: src/raft.c\nlines: 2-4\nmark: 4\n```\n\n\
                           ```mermaid\nflowchart LR\n  F[Follower] --> C[Candidate]\n```\n";

    fn debrief_body(source: &str, points: serde_json::Value) -> String {
        serde_json::json!({ "source": source, "points": points, "author": "agent" }).to_string()
    }

    /// The learner answers quiz n of page 1: the open answer, then option 0
    /// of each choice.
    async fn answer_quiz(state: &AppState, n: usize) {
        let quiz = state.store.load_page("raft", "1").unwrap().unwrap().quizzes[n - 1].clone();
        if quiz.open.is_some() {
            let uri = format!("/api/pages/1/quizzes/{n}/open");
            let (status, body) = json(app(state.clone()), send_json("POST", &uri, OPEN)).await;
            assert_eq!(status, StatusCode::OK, "{body}");
        }
        if !quiz.choices.is_empty() {
            let picks: Vec<_> = quiz
                .choices
                .iter()
                .map(|_| serde_json::json!({ "option": 0 }))
                .collect();
            let uri = format!("/api/pages/1/quizzes/{n}/choices");
            let body = serde_json::json!({ "choices": picks }).to_string();
            let (status, body) = json(app(state.clone()), send_json("POST", &uri, &body)).await;
            assert_eq!(status, StatusCode::OK, "{body}");
        }
    }

    async fn write_debrief(
        state: &AppState,
        n: usize,
        body: &str,
    ) -> (StatusCode, serde_json::Value) {
        let uri = format!("/api/pages/1/quizzes/{n}/debrief");
        json(app(state.clone()), send_json("PUT", &uri, body)).await
    }

    /// points.json, with each stamp checked and then put as `[at]`.
    fn points_file(dir: &std::path::Path) -> serde_json::Value {
        let text = std::fs::read_to_string(dir.join(".learn/raft/points.json")).unwrap();
        let mut points: serde_json::Value = serde_json::from_str(&text).unwrap();
        for point in points["points"].as_array_mut().unwrap() {
            for result in point["results"].as_array_mut().unwrap() {
                let at = result["at"].as_str().unwrap();
                assert!(
                    OffsetDateTime::parse(at, &time::format_description::well_known::Rfc3339)
                        .is_ok(),
                    "{at}"
                );
                result["at"] = "[at]".into();
            }
        }
        points
    }

    fn page_ref(state: &serde_json::Value) -> serde_json::Value {
        state["course"]["parts"][0]["pages"][0].clone()
    }

    #[tokio::test]
    async fn the_debrief_route_stores_the_debrief_and_answers_the_page() {
        let (dir, state) = with_quiz(LESSON, QUIZ_TWO_POINTS).await;
        answer_quiz(&state, 1).await;
        let untouched = outside(dir.path());
        let after = state.events.after(None, events::HOLD).await.next;
        let body = debrief_body(
            DEBRIEF,
            serde_json::json!({ "random-timeout": "ok", "majority": "partial" }),
        );

        let (status, answer) = write_debrief(&state, 1, &body).await;

        assert_eq!(status, StatusCode::OK, "{answer}");
        insta::assert_json_snapshot!("debrief", answer);
        let (_, page) = json(app(state.clone()), get_with_cookie("/api/pages/1", TOKEN)).await;
        assert_eq!(page, answer);
        let kinds: Vec<_> = answer["quizzes"][0]["debrief"]["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|block| block["kind"].clone())
            .collect();
        assert_eq!(kinds, ["text", "excerpt", "diagram"]);

        // The open results go to points.json, beside the result of the
        // choice, and the page counts a partial point half.
        assert_eq!(
            points_file(dir.path()),
            serde_json::json!({ "points": [
                { "id": "majority", "label": "How many votes make a leader", "lesson": "1",
                  "results": [{ "at": "[at]", "result": "partial", "how": "open" }] },
                { "id": "random-timeout", "label": "Why the election timeout is random",
                  "lesson": "1", "results": [{ "at": "[at]", "result": "ok", "how": "open" }] },
                { "id": "split-vote", "label": "How a split vote ends", "lesson": "1",
                  "results": [{ "at": "[at]", "result": "missed", "how": "choice" }] },
            ] })
        );
        let (_, session) = json(app(state.clone()), get_with_cookie("/api/session", TOKEN)).await;
        assert_eq!(
            page_ref(&session),
            serde_json::json!({ "id": "1", "kind": "lesson", "title": "Two leaders, one term",
                                "state": "debriefed", "score": { "right": 1.5, "total": 3 } })
        );
        let stored = Store::at(dir.path()).load("raft").unwrap().unwrap();
        assert_eq!(stored, *state.course.read().await, "course.json holds it");
        let file = std::fs::read_to_string(dir.path().join(".learn/raft/pages/1.json")).unwrap();
        assert!(
            file.contains("A majority is 3 of 5"),
            "the source is kept: {file}"
        );

        let batch = state.events.after(Some(after), events::HOLD).await;
        let events: Vec<_> = batch
            .events
            .iter()
            .map(|event| {
                (
                    event.kind,
                    event.author,
                    event.page.as_deref(),
                    event.id.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            events,
            [(EventKind::Debrief, Author::Agent, Some("1"), Some("1"))]
        );
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
    }

    /// The order of section 5.3: the missed and partial points first, then
    /// the others, each by the date of its last result.
    #[tokio::test]
    async fn the_points_route_answers_the_missed_points_first() {
        let (_dir, state) = with_quiz(LESSON, QUIZ_TWO_POINTS).await;
        let (status, empty) = json(app(state.clone()), get_with_cookie("/api/points", TOKEN)).await;
        assert_eq!(status, StatusCode::OK);
        let ids = |points: &serde_json::Value| -> Vec<String> {
            points
                .as_array()
                .unwrap()
                .iter()
                .map(|point| point["id"].as_str().unwrap().to_owned())
                .collect()
        };
        assert_eq!(
            ids(&empty),
            ["majority", "random-timeout", "split-vote"],
            "a point with no result keeps its place"
        );
        answer_quiz(&state, 1).await;
        let body = debrief_body(
            "# Almost\n",
            serde_json::json!({ "random-timeout": "ok", "majority": "partial" }),
        );
        let (status, _) = write_debrief(&state, 1, &body).await;
        assert_eq!(status, StatusCode::OK);

        let (status, mut points) =
            json(app(state.clone()), get_with_cookie("/api/points", TOKEN)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(ids(&points), ["split-vote", "majority", "random-timeout"]);
        for point in points.as_array_mut().unwrap() {
            for result in point["results"].as_array_mut().unwrap() {
                result["at"] = "[at]".into();
            }
        }
        insta::assert_json_snapshot!("points", points);
    }

    /// The agent writes a debrief again to fix it. The results that the
    /// first one recorded go, so the answer counts once.
    #[tokio::test]
    async fn a_debrief_written_again_replaces_its_source_and_its_results() {
        let (dir, state) = with_quiz(LESSON, QUIZ_TWO_POINTS).await;
        answer_quiz(&state, 1).await;
        let first = debrief_body(
            DEBRIEF,
            serde_json::json!({ "random-timeout": "missed", "majority": "missed" }),
        );
        let (status, _) = write_debrief(&state, 1, &first).await;
        assert_eq!(status, StatusCode::OK);
        let after = state.events.after(None, events::HOLD).await.next;

        let second = debrief_body(
            "# Almost right\n\nA majority is 3 of 5.\n",
            serde_json::json!({ "random-timeout": "ok", "majority": "partial" }),
        );
        let (status, answer) = write_debrief(&state, 1, &second).await;

        assert_eq!(status, StatusCode::OK, "{answer}");
        let debrief = &answer["quizzes"][0]["debrief"];
        assert_eq!(debrief["blocks"].as_array().unwrap().len(), 1, "{debrief}");
        assert_eq!(
            debrief["points"],
            serde_json::json!([
                { "id": "random-timeout", "label": "Why the election timeout is random",
                  "result": "ok" },
                { "id": "majority", "label": "How many votes make a leader",
                  "result": "partial" },
            ])
        );
        let results: Vec<_> = points_file(dir.path())["points"]
            .as_array()
            .unwrap()
            .iter()
            .map(|point| (point["id"].clone(), point["results"].clone()))
            .collect();
        assert_eq!(
            serde_json::json!(results),
            serde_json::json!([
                ["majority", [{ "at": "[at]", "result": "partial", "how": "open" }]],
                ["random-timeout", [{ "at": "[at]", "result": "ok", "how": "open" }]],
                ["split-vote", [{ "at": "[at]", "result": "missed", "how": "choice" }]],
            ])
        );
        let stored = state.store.load_page("raft", "1").unwrap().unwrap();
        let at = &stored.quizzes[0].debrief.as_ref().unwrap().at;
        let points = state.store.load_points("raft").unwrap();
        assert_eq!(points[1].results[0].at, *at, "the results carry its stamp");
        assert_eq!(
            state.course.read().await.parts[0].pages[0].score,
            Some(model::Score {
                right: 1.5,
                total: 3
            })
        );
        let batch = state.events.after(Some(after), events::HOLD).await;
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].kind, EventKind::Debrief);
    }

    /// A debrief written again after a later quiz graded the same point
    /// keeps the place and the stamp of its results: the last result of the
    /// point is still the latest answer of the learner.
    #[tokio::test]
    async fn a_debrief_written_again_keeps_the_place_of_its_results() {
        let only_choices = r#"{"choices":[{"header":"Timer","prompt":"Who starts?",
            "options":["The first to time out","The leader"],"answer":0,
            "point":"random-timeout"}]}"#;
        let (_dir, state) = with_quiz(LESSON, QUIZ).await;
        answer_quiz(&state, 1).await;
        let body = debrief_body(
            "# Almost\n",
            serde_json::json!({ "random-timeout": "partial" }),
        );
        let (status, answer) = write_debrief(&state, 1, &body).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        let (status, _) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes", only_choices),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        answer_quiz(&state, 2).await;
        let before = state.store.load_points("raft").unwrap();

        let body = debrief_body(
            "# Almost, with a diagram that draws\n",
            serde_json::json!({ "random-timeout": "partial" }),
        );
        let (status, answer) = write_debrief(&state, 1, &body).await;

        assert_eq!(status, StatusCode::OK, "{answer}");
        assert_eq!(state.store.load_points("raft").unwrap(), before);
        let (_, points) = json(app(state.clone()), get_with_cookie("/api/points", TOKEN)).await;
        let ids: Vec<_> = points
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].clone())
            .collect();
        assert_eq!(
            ids,
            ["split-vote", "random-timeout"],
            "random-timeout ends on its right choice"
        );
    }

    /// A page is debriefed once every quiz of it has its debrief, and a
    /// quiz pushed after that asks for an answer again.
    #[tokio::test]
    async fn a_page_goes_from_answered_to_debriefed_with_its_last_debrief() {
        let only_choices = r#"{"choices":[{"header":"Timer","prompt":"Who starts?",
            "options":["The first to time out","The leader"],"answer":0,
            "point":"random-timeout"}]}"#;
        let (dir, state) = with_quiz(LESSON, QUIZ).await;
        let (status, _) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes", only_choices),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let page = || async {
            let (_, session) =
                json(app(state.clone()), get_with_cookie("/api/session", TOKEN)).await;
            let page = page_ref(&session);
            (page["state"].clone(), page["score"].clone())
        };
        answer_quiz(&state, 1).await;
        answer_quiz(&state, 2).await;
        // The first quiz has its wrong choice, the second its right one.
        assert_eq!(
            page().await,
            (
                "answered".into(),
                serde_json::json!({ "right": 1, "total": 2 })
            )
        );

        let body = debrief_body(
            "# Almost\n",
            serde_json::json!({ "random-timeout": "partial" }),
        );
        let (status, answer) = write_debrief(&state, 1, &body).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        assert_eq!(
            page().await,
            (
                "answered".into(),
                serde_json::json!({ "right": 1.5, "total": 3 })
            ),
            "the second quiz has no debrief yet"
        );
        let body = debrief_body("# Right\n", serde_json::json!({}));
        let (status, answer) = write_debrief(&state, 2, &body).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        assert_eq!(
            page().await,
            (
                "debriefed".into(),
                serde_json::json!({ "right": 1.5, "total": 3 })
            )
        );
        let stored = Store::at(dir.path()).load("raft").unwrap().unwrap();
        assert_eq!(stored.parts[0].pages[0].state, PageState::Debriefed);

        let (status, _) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes", only_choices),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            page().await,
            (
                "taught".into(),
                serde_json::json!({ "right": 1.5, "total": 3 })
            )
        );
    }

    /// A debrief shows once its own quiz is answered, while another quiz of
    /// the page still waits. Its excerpt keeps the hidden line of the lesson
    /// as null until the learner answers every quiz of the page.
    #[tokio::test]
    async fn a_debrief_shows_no_hidden_line_before_the_page_is_answered() {
        let only_choices = r#"{"choices":[{"header":"Timer","prompt":"Who starts?",
            "options":["The first to time out","The leader"],"answer":0,
            "point":"random-timeout"}]}"#;
        let (_dir, state) = with_quiz(EXCERPT, QUIZ).await;
        let (status, _) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes", only_choices),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let hidden = state.store.load_page("raft", "1").unwrap().unwrap().hidden;
        let hidden = hidden.values().next().unwrap().text.clone();
        let page = || async {
            let (status, body) =
                json(app(state.clone()), get_with_cookie("/api/pages/1", TOKEN)).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            body
        };

        answer_quiz(&state, 1).await;
        let source = "# Almost\n\n```excerpt\npath: src/raft.c\nlines: 1-4\n```\n";
        let body = debrief_body(source, serde_json::json!({ "random-timeout": "partial" }));
        let (status, answer) = write_debrief(&state, 1, &body).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        assert!(!answer.to_string().contains(&hidden), "{answer}");
        let body = page().await;
        assert!(!body.to_string().contains(&hidden), "{body}");
        let lines = &body["quizzes"][0]["debrief"]["blocks"][1]["excerpt"]["lines"];
        assert_eq!(lines[3], serde_json::Value::Null);
        assert_ne!(lines[2], serde_json::Value::Null);

        answer_quiz(&state, 2).await;
        let body = page().await;
        let lines = &body["quizzes"][0]["debrief"]["blocks"][1]["excerpt"]["lines"];
        assert_eq!(lines[3]["text"], hidden.as_str());

        // A quiz pushed after the debrief hides the line there again.
        let (status, _) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes", only_choices),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(!page().await.to_string().contains(&hidden));
    }

    #[tokio::test]
    async fn a_debrief_that_the_route_does_not_take_is_refused_and_stores_nothing() {
        let (dir, state) = with_quiz(EXCERPT, QUIZ_TWO_POINTS).await;
        let (status, _) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes", QUIZ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        answer_quiz(&state, 1).await;
        // The second quiz has its open answer, and its choices are left.
        let (status, _) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes/2/open", OPEN),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        state.course.write().await.parts[0].pages.push(PageRef {
            id: "2".to_owned(),
            kind: PageKind::Lesson,
            title: "The vote".to_owned(),
            state: PageState::Planned,
            score: None,
        });
        let untouched = outside(dir.path());
        let stored = |root: &std::path::Path| {
            files(root)
                .into_iter()
                .map(|file| {
                    let bytes = std::fs::read(root.join(&file)).unwrap();
                    (file, bytes)
                })
                .collect::<Vec<_>>()
        };
        let check = |uri: &str, body: &str, code: &str| {
            let state = state.clone();
            let (uri, body, code) = (uri.to_owned(), body.to_owned(), code.to_owned());
            async move {
                let before = stored(state.root.dir());
                let course = state.course.read().await.clone();
                let after = state.events.after(None, events::HOLD).await.next;

                let (status, answer) =
                    json(app(state.clone()), send_json("PUT", &uri, &body)).await;

                let expected = match code.as_str() {
                    "notFound" => StatusCode::NOT_FOUND,
                    "conflict" => StatusCode::CONFLICT,
                    _ => StatusCode::BAD_REQUEST,
                };
                assert_eq!(status, expected, "{uri} {body}: {answer}");
                assert_eq!(answer["error"]["code"], code.as_str(), "{uri} {body}");
                assert!(answer["error"]["message"].is_string(), "{answer}");
                assert_eq!(stored(state.root.dir()), before, "{uri} {body}");
                assert_eq!(*state.course.read().await, course, "{uri} {body}");
                assert_eq!(state.events.after(None, events::HOLD).await.next, after);
                answer
            }
        };
        let good = debrief_body(
            "# Almost\n",
            serde_json::json!({ "random-timeout": "ok", "majority": "partial" }),
        );

        // An unknown page, or an unknown quiz of a known page.
        for uri in [
            "/api/pages/2/quizzes/1/debrief",
            "/api/pages/9/quizzes/1/debrief",
            "/api/pages/x/quizzes/1/debrief",
            "/api/pages/..%2Fcourse/quizzes/1/debrief",
            "/api/pages/1/quizzes/0/debrief",
            "/api/pages/1/quizzes/3/debrief",
            "/api/pages/1/quizzes/01/debrief",
            "/api/pages/1/quizzes/x/debrief",
        ] {
            check(uri, &good, "notFound").await;
        }
        // A quiz that the learner has not answered in full.
        let answer = check("/api/pages/1/quizzes/2/debrief", &good, "conflict").await;
        let message = answer["error"]["message"].as_str().unwrap();
        assert!(message.contains("has not answered quiz 2"), "{message}");

        // Every fault of the source, with its line, and of the points.
        let wrong = debrief_body(
            "# Almost\n\n```excerpt\npath: src/raft.c\nlines: 2-4\nhide: 4\n```\n\n\
             ```excerpt\npath: src/raft.c\nlines: 4-90\n```\n\n> [!tip]\n> Look again.\n",
            serde_json::json!({ "random-timeout": "great", "leader": "ok" }),
        );
        let answer = check("/api/pages/1/quizzes/1/debrief", &wrong, "invalidDebrief").await;
        insta::assert_json_snapshot!("debrief_refused", answer);
        for body in ["", "not json"] {
            check("/api/pages/1/quizzes/1/debrief", body, "badRequest").await;
        }
        for body in [
            "[]",
            "{}",
            r##"{"source":"# Almost\n"}"##,
            r#"{"source":"","points":{"random-timeout":"ok","majority":"ok"}}"#,
            r##"{"source":"# A\n","points":{"random-timeout":"ok","majority":"half"}}"##,
            r##"{"source":"# A\n","points":{"random-timeout":"ok","majority":null}}"##,
            r##"{"source":"# A\n","points":{"random-timeout":"ok","majority":"ok","x":"ok"}}"##,
            r##"{"source":"# A\n","points":{"random-timeout":"ok","majority":"ok"},"n":1}"##,
            r##"{"source":"# A\n","points":{"random-timeout":"ok","majority":"ok"},"author":"learner"}"##,
        ] {
            check("/api/pages/1/quizzes/1/debrief", body, "invalidDebrief").await;
        }
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
    }

    /// A diagram of a debrief that Mermaid refuses comes back to the agent
    /// as one of a page does. The request names the quiz, so the agent
    /// writes that debrief again.
    #[tokio::test]
    async fn a_broken_block_of_a_debrief_names_its_quiz() {
        let (_dir, state) = with_quiz(LESSON, QUIZ).await;
        answer_quiz(&state, 1).await;
        let source = "# Almost\n\n```mermaid\ngraph LR\n  A -->\n```\n";
        let body = debrief_body(source, serde_json::json!({ "random-timeout": "ok" }));
        let (status, answer) = write_debrief(&state, 1, &body).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        let block = answer["quizzes"][0]["debrief"]["blocks"][1]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let after = state.events.after(None, events::HOLD).await.next;
        let report = serde_json::json!({ "block": block, "error": "Parse error" }).to_string();

        let (status, event) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/broken", &report),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{event}");
        assert_eq!(event["kind"], "broken");
        assert_eq!(event["id"], "1", "the quiz of the debrief");
        let batch = state.events.after(Some(after), events::HOLD).await;
        assert_eq!(
            batch.events[0].detail,
            Some(Detail::Broken {
                block: BlockSource {
                    id: block.clone(),
                    kind: "diagram".to_owned(),
                    source: "graph LR\n  A -->\n".to_owned(),
                },
                error: "Parse error".to_owned(),
            })
        );

        // The same debrief again is a new version: the agent hears of the
        // block again. A report of the same version is passed on once.
        let (_, again) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/broken", &report),
        )
        .await;
        assert_eq!(again["seq"], event["seq"]);
        let (status, _) = write_debrief(&state, 1, &body).await;
        assert_eq!(status, StatusCode::OK);
        let (_, third) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/broken", &report),
        )
        .await;
        assert!(third["seq"].as_u64() > event["seq"].as_u64(), "{third}");
    }

    /// A server killed in the middle of a debrief completes it at its next
    /// start: each open result lands once, and a debrief written again
    /// leaves no result of the one before.
    #[tokio::test]
    async fn a_server_killed_in_a_debrief_completes_it_at_its_next_start() {
        let first = debrief_body(
            "# Wrong\n",
            serde_json::json!({ "random-timeout": "missed", "majority": "missed" }),
        );
        let second = debrief_body(
            "# Almost\n",
            serde_json::json!({ "random-timeout": "ok", "majority": "partial" }),
        );
        // The kill comes after the page file, or after points.json too.
        for put_back in [&["points.json", "course.json"][..], &["course.json"][..]] {
            for again in [false, true] {
                let (dir, state) = with_quiz(LESSON, QUIZ_TWO_POINTS).await;
                let course_dir = dir.path().join(".learn/raft");
                answer_quiz(&state, 1).await;
                if again {
                    let (status, _) = write_debrief(&state, 1, &first).await;
                    assert_eq!(status, StatusCode::OK);
                }
                let before: Vec<_> = put_back
                    .iter()
                    .map(|name| (name, std::fs::read(course_dir.join(name)).unwrap()))
                    .collect();
                let (status, _) = write_debrief(&state, 1, &second).await;
                assert_eq!(status, StatusCode::OK);
                let done = (
                    state.course.read().await.clone(),
                    state.store.load_points("raft").unwrap(),
                );
                for (name, bytes) in before {
                    std::fs::write(course_dir.join(name), bytes).unwrap();
                }

                let store = Store::at(dir.path());
                let mut course = store.load("raft").unwrap().unwrap();
                settle(&store, &mut course).unwrap();
                settle(&store, &mut course).unwrap();

                let case = format!("{put_back:?}, again: {again}");
                assert_eq!(course.parts[0].pages[0], done.0.parts[0].pages[0], "{case}");
                assert_eq!(store.load("raft").unwrap().unwrap(), course, "{case}");
                assert_eq!(store.load_points("raft").unwrap(), done.1, "{case}");
            }
        }
    }

    /// An earlier release writes the page file with no debrief. The open
    /// results stay in points.json: they are answers of the learner.
    #[tokio::test]
    async fn an_open_result_stays_when_the_page_file_loses_its_debrief() {
        let (dir, state) = with_quiz(LESSON, QUIZ).await;
        answer_quiz(&state, 1).await;
        let body = debrief_body(
            "# Almost\n",
            serde_json::json!({ "random-timeout": "partial" }),
        );
        let (status, answer) = write_debrief(&state, 1, &body).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        let points = state.store.load_points("raft").unwrap();
        let store = Store::at(dir.path());
        let mut file = store.load_page("raft", "1").unwrap().unwrap();
        file.quizzes[0].debrief = None;
        store.save_blocks("raft", "1", &file).unwrap();

        let mut course = store.load("raft").unwrap().unwrap();
        settle(&store, &mut course).unwrap();

        assert_eq!(store.load_points("raft").unwrap(), points);
    }

    /// The score of a page that an earlier release answered fills in at
    /// the next start: that release kept none.
    #[tokio::test]
    async fn the_score_of_an_answered_page_fills_in_at_the_next_start() {
        let (dir, state) = with_quiz(LESSON, QUIZ).await;
        answer_quiz(&state, 1).await;
        let store = Store::at(dir.path());
        let mut course = store.load("raft").unwrap().unwrap();
        course.parts[0].pages[0].score = None;
        store.save(&course).unwrap();

        settle(&store, &mut course).unwrap();

        assert_eq!(
            course.parts[0].pages[0].score,
            Some(model::Score {
                right: 0.0,
                total: 1
            })
        );
        assert_eq!(store.load("raft").unwrap().unwrap(), course);
    }

    /// Each `at` of a value, checked and then put as `[at]`, so a snapshot
    /// does not move with the clock.
    fn at_redacted(mut value: serde_json::Value) -> serde_json::Value {
        fn walk(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Object(fields) => {
                    for (key, field) in fields.iter_mut() {
                        if key == "at"
                            && let Some(at) = field.as_str()
                        {
                            let format = time::format_description::well_known::Rfc3339;
                            assert!(OffsetDateTime::parse(at, &format).is_ok(), "{at}");
                            *field = "[at]".into();
                        } else {
                            walk(field);
                        }
                    }
                }
                serde_json::Value::Array(items) => items.iter_mut().for_each(walk),
                _ => {}
            }
        }
        walk(&mut value);
        value
    }

    /// The id of block `at` of page 1.
    fn block_of(state: &AppState, at: usize) -> String {
        state.store.load_page("raft", "1").unwrap().unwrap().blocks[at]
            .id()
            .to_owned()
    }

    /// Write in a thread, as the page sends it, or as the agent with
    /// `"author": "agent"` in the body.
    async fn thread_write(
        state: &AppState,
        uri: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        json(
            app(state.clone()),
            send_json("POST", uri, &body.to_string()),
        )
        .await
    }

    /// A batch or a session with the id of its server in a fixed place: each
    /// start draws another one.
    fn server_redacted(state: &AppState, mut value: serde_json::Value) -> serde_json::Value {
        assert_eq!(value["server"], state.events.server(), "{value}");
        value["server"] = "[server]".into();
        value
    }

    /// What the wait of the agent gets after `after`.
    async fn agent_waits(state: &AppState, after: u64) -> serde_json::Value {
        let uri = format!("/api/events?after={after}&who=agent");
        json(app(state.clone()), get_with_cookie(&uri, TOKEN))
            .await
            .1
    }

    /// The learner asks about a block, the agent gets the question with
    /// the source of the block, answers it, and the learner asks again.
    #[tokio::test]
    async fn the_thread_routes_open_a_thread_and_answer_it() {
        let (dir, state) = with_lesson().await;
        let untouched = outside(dir.path());
        let block = block_of(&state, 1);
        // The agent waited since its push, so its idle event is behind.
        let after = agent_waits(&state, 0).await["next"].as_u64().unwrap();

        let (status, opened) = thread_write(
            &state,
            "/api/pages/1/threads",
            serde_json::json!({
                "block": block,
                "quote": "  votes once  ",
                "body": "Once per term, or once ever?\n\n<script>alert(1)</script>",
            }),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{opened}");
        insta::assert_json_snapshot!("open_thread", at_redacted(opened.clone()));
        assert_eq!(opened["id"], "1-1");
        let agent = agent_waits(&state, after).await;
        insta::assert_json_snapshot!(
            "events_ask",
            server_redacted(&state, at_redacted(agent.clone()))
        );
        assert!(state.events.agent_works(), "the wait handed the ask over");

        // The answer of the agent is no request.
        let next = agent["next"].as_u64().unwrap();
        let (status, answered) = thread_write(
            &state,
            "/api/threads/1-1",
            serde_json::json!({ "body": "Once per **term**.", "author": "agent" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{answered}");
        insta::assert_json_snapshot!("reply", at_redacted(answered.clone()));
        let batch = state.events.after(Some(next), events::HOLD).await;
        let seen: Vec<_> = batch
            .events
            .iter()
            .map(|event| (event.kind, event.author, event.id.as_deref()))
            .collect();
        assert_eq!(seen, [(EventKind::Reply, Author::Agent, Some("1-1"))]);

        // The page gets the thread with the page, in HTML.
        let (_, page) = json(app(state.clone()), get_with_cookie("/api/pages/1", TOKEN)).await;
        insta::assert_json_snapshot!("page_threads", at_redacted(page.clone()));
        assert_eq!(page["threads"], serde_json::json!([answered]));

        // A reply of the learner asks again, with the whole thread.
        let next = state.events.after(None, events::HOLD).await.next;
        let (status, _) = thread_write(
            &state,
            "/api/threads/1-1",
            serde_json::json!({ "body": "And after a restart?" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let again = agent_waits(&state, next).await;
        assert_eq!(again["events"][0]["kind"], "ask");
        assert_eq!(again["events"][0]["author"], "learner");
        let bodies: Vec<_> = again["events"][0]["detail"]["thread"]["comments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|comment| comment["body"].clone())
            .collect();
        assert_eq!(
            bodies,
            [
                "Once per term, or once ever?\n\n<script>alert(1)</script>",
                "Once per **term**.",
                "And after a restart?"
            ]
        );
        assert_eq!(
            files(dir.path()),
            [
                ".learn/raft/course.json",
                ".learn/raft/pages/1.json",
                ".learn/raft/pages/1.md"
            ]
        );
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
    }

    /// The agent opens a thread about the whole page: a `thread` event,
    /// which no wait returns.
    #[tokio::test]
    async fn the_agent_opens_a_thread_about_the_whole_page() {
        let (_dir, state) = with_lesson().await;
        let after = state.events.after(None, events::HOLD).await.next;

        let (status, opened) = thread_write(
            &state,
            "/api/pages/1/threads",
            serde_json::json!({ "block": null, "quote": null,
                                "body": "We stopped at the vote.", "author": "agent" }),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{opened}");
        assert_eq!(opened["block"], serde_json::Value::Null);
        assert_eq!(opened["comments"][0]["author"], "agent");
        assert_eq!(
            opened["comments"][0]["body"],
            "<p>We stopped at the vote.</p>"
        );
        let batch = state.events.after(Some(after), events::HOLD).await;
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].kind, EventKind::Thread);
        assert!(!batch.events[0].kind.is_request());
        assert_eq!(batch.events[0].id.as_deref(), Some("1-1"));
    }

    /// The learner asks about the whole page: an `ask` request whose
    /// detail has no block, as a thread of the agent about the page has.
    #[tokio::test]
    async fn the_learner_asks_about_the_whole_page() {
        let (_dir, state) = with_lesson().await;
        let after = agent_waits(&state, 0).await["next"].as_u64().unwrap();

        let (status, opened) = thread_write(
            &state,
            "/api/pages/1/threads",
            serde_json::json!({ "block": null, "body": "Where does the term *start*?" }),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{opened}");
        assert_eq!(opened["block"], serde_json::Value::Null);
        assert_eq!(opened["quote"], serde_json::Value::Null);
        let agent = agent_waits(&state, after).await;
        insta::assert_json_snapshot!(
            "events_ask_page",
            server_redacted(&state, at_redacted(agent.clone()))
        );
        assert!(state.events.agent_works(), "the wait handed the ask over");

        // The page shows it at its end, with no block.
        let (_, page) = json(app(state.clone()), get_with_cookie("/api/pages/1", TOKEN)).await;
        assert_eq!(page["threads"], serde_json::json!([opened]));
    }

    #[tokio::test]
    async fn a_write_in_a_thread_that_the_route_does_not_take_is_refused() {
        let (_dir, state) = with_lesson().await;
        let block = block_of(&state, 0);
        let (status, _) = thread_write(
            &state,
            "/api/pages/1/threads",
            serde_json::json!({ "block": block, "body": "Why?" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let before = std::fs::read(state.store.dir().join("raft/pages/1.json")).unwrap();
        let after = state.events.after(None, events::HOLD).await.next;

        let open = |body: serde_json::Value| ("/api/pages/1/threads", body);
        for ((uri, body), status, code) in [
            (
                (
                    "/api/pages/2/threads",
                    serde_json::json!({ "block": block, "body": "x" }),
                ),
                StatusCode::NOT_FOUND,
                "notFound",
            ),
            (
                (
                    "/api/pages/..%2F1/threads",
                    serde_json::json!({ "block": block, "body": "x" }),
                ),
                StatusCode::NOT_FOUND,
                "notFound",
            ),
            (
                open(serde_json::json!({ "block": "0123456789ab", "body": "x" })),
                StatusCode::NOT_FOUND,
                "notFound",
            ),
            (
                open(serde_json::json!({ "block": block, "body": " \n " })),
                StatusCode::BAD_REQUEST,
                "badRequest",
            ),
            (
                open(serde_json::json!({ "block": null, "quote": "votes", "body": "x" })),
                StatusCode::BAD_REQUEST,
                "badRequest",
            ),
            (
                open(serde_json::json!({ "block": block, "body": "x", "page": "1" })),
                StatusCode::BAD_REQUEST,
                "badRequest",
            ),
            (
                open(serde_json::json!({ "block": block })),
                StatusCode::BAD_REQUEST,
                "badRequest",
            ),
            (
                ("/api/threads/1-2", serde_json::json!({ "body": "x" })),
                StatusCode::NOT_FOUND,
                "notFound",
            ),
            (
                ("/api/threads/2-1", serde_json::json!({ "body": "x" })),
                StatusCode::NOT_FOUND,
                "notFound",
            ),
            (
                ("/api/threads/thread", serde_json::json!({ "body": "x" })),
                StatusCode::NOT_FOUND,
                "notFound",
            ),
            (
                ("/api/threads/1-1", serde_json::json!({ "body": "" })),
                StatusCode::BAD_REQUEST,
                "badRequest",
            ),
            (
                (
                    "/api/threads/1-1",
                    serde_json::json!({ "body": "x", "author": "reader" }),
                ),
                StatusCode::BAD_REQUEST,
                "badRequest",
            ),
        ] {
            let (got, answer) = thread_write(&state, uri, body.clone()).await;

            assert_eq!(got, status, "{uri} {body}: {answer}");
            assert_eq!(answer["error"]["code"], code, "{uri} {body}");
            assert!(answer["error"]["message"].is_string(), "{answer}");
        }
        assert_eq!(
            std::fs::read(state.store.dir().join("raft/pages/1.json")).unwrap(),
            before
        );
        assert_eq!(state.events.after(None, events::HOLD).await.next, after);
    }

    /// A push that takes a block away keeps its threads at the end of the
    /// page, with their quote. A push that keeps a block keeps its threads
    /// on it, and a block that comes back takes its threads back.
    #[tokio::test]
    async fn a_thread_whose_block_is_gone_moves_to_the_end_of_the_page() {
        let (_dir, state) = with_lesson().await;
        let (first, vote) = (block_of(&state, 0), block_of(&state, 1));
        for body in [
            serde_json::json!({ "block": vote, "quote": "once per term", "body": "Why once?" }),
            serde_json::json!({ "block": first, "body": "Who starts it?" }),
            serde_json::json!({ "body": "We stopped here.", "author": "agent" }),
        ] {
            let (status, answer) = thread_write(&state, "/api/pages/1/threads", body).await;
            assert_eq!(status, StatusCode::OK, "{answer}");
        }
        let order = |page: &serde_json::Value| -> Vec<String> {
            page["threads"]
                .as_array()
                .unwrap()
                .iter()
                .map(|thread| thread["id"].as_str().unwrap().to_owned())
                .collect()
        };

        // The vote changes, and the first block stays as it was.
        let changed = LESSON.replace("once per term", "once in each term");
        let (status, pushed) = json(app(state.clone()), put("/api/pages/1", changed)).await;
        assert_eq!(status, StatusCode::OK, "{pushed}");

        assert_eq!(pushed["blocks"][0]["id"], first.as_str());
        assert_ne!(pushed["blocks"][1]["id"], vote.as_str());
        assert_eq!(order(&pushed), ["1-2", "1-3", "1-1"]);
        let gone = &pushed["threads"][2];
        assert_eq!(gone["block"], vote.as_str());
        assert_eq!(gone["quote"], "once per term");
        assert_eq!(gone["comments"][0]["body"], "<p>Why once?</p>");
        let (_, page) = json(app(state.clone()), get_with_cookie("/api/pages/1", TOKEN)).await;
        assert_eq!(page, pushed);

        // The learner answers in it, and the agent reads the block as it
        // was when the learner asked.
        let after = agent_waits(&state, 0).await["next"].as_u64().unwrap();
        let (status, _) = thread_write(
            &state,
            "/api/threads/1-1",
            serde_json::json!({ "body": "Still?" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let asked = agent_waits(&state, after).await;
        let detail = &asked["events"][0]["detail"];
        assert_eq!(detail["earlier"], true);
        assert_eq!(
            detail["block"],
            serde_json::json!({ "id": vote, "kind": "text",
                                "source": "## The vote\n\nEach server votes once per term." })
        );

        // The vote comes back, and its thread with it.
        let (_, pushed) = json(app(state.clone()), put("/api/pages/1", LESSON)).await;
        assert_eq!(order(&pushed), ["1-1", "1-2", "1-3"]);
    }

    /// A debrief is a page source, and its blocks take questions too.
    #[tokio::test]
    async fn a_thread_opens_on_a_block_of_a_debrief() {
        let (_dir, state) = with_quiz(LESSON, QUIZ).await;
        answer_quiz(&state, 1).await;
        let body = debrief_body(DEBRIEF, serde_json::json!({ "random-timeout": "ok" }));
        let (status, page) = write_debrief(&state, 1, &body).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        let diagram = page["quizzes"][0]["debrief"]["blocks"][2]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let after = agent_waits(&state, 0).await["next"].as_u64().unwrap();

        let (status, opened) = thread_write(
            &state,
            "/api/pages/1/threads",
            serde_json::json!({ "block": diagram, "body": "Where is the leader?" }),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{opened}");
        let asked = agent_waits(&state, after).await;
        assert_eq!(
            asked["events"][0]["detail"]["block"],
            serde_json::json!({ "id": diagram, "kind": "diagram",
                                "source": "```mermaid\nflowchart LR\n  F[Follower] --> C[Candidate]\n```" })
        );
        assert_eq!(asked["events"][0]["detail"]["earlier"], false);
    }

    /// The page places a thread by the id of its block: a debrief that
    /// repeats a block of the lesson would show its threads twice.
    #[tokio::test]
    async fn a_block_of_a_debrief_never_takes_the_id_of_a_block_of_the_lesson() {
        let (_dir, state) = with_quiz(LESSON, QUIZ).await;
        answer_quiz(&state, 1).await;
        let repeat =
            "# Almost\n\nRead it again.\n\n## The vote\n\nEach server votes once per term.\n";
        let body = debrief_body(repeat, serde_json::json!({ "random-timeout": "ok" }));

        let (status, page) = write_debrief(&state, 1, &body).await;

        assert_eq!(status, StatusCode::OK, "{page}");
        let ids = |blocks: &serde_json::Value| -> Vec<String> {
            let blocks = blocks.as_array().unwrap();
            blocks
                .iter()
                .map(|block| block["id"].as_str().unwrap().to_owned())
                .collect()
        };
        let lesson = ids(&page["blocks"]);
        let debrief = ids(&page["quizzes"][0]["debrief"]["blocks"]);
        assert_eq!(debrief.len(), 2, "{page}");
        assert!(
            debrief.iter().all(|id| !lesson.contains(id)),
            "{lesson:?} {debrief:?}"
        );
    }

    /// The next step at the end of page 1, as `qlearn choose` sends it.
    const CHOICE: &str = r#"{
        "page": "1",
        "questions": [
            {
                "header": "Next step",
                "prompt": "Part 1 is done. What next?",
                "options": [
                    { "label": "Part 2", "description": "The code tour of the election" },
                    { "label": "A break", "description": "The course waits here" }
                ],
                "multi": false
            },
            {
                "header": "Topics",
                "prompt": "What should the next part show?",
                "options": [
                    { "label": "The timer" },
                    { "label": "The vote" },
                    { "label": "The log" }
                ],
                "multi": true
            }
        ],
        "author": "agent"
    }"#;

    /// The level at the start, with no page: the top of the main column.
    const LEVEL: &str = r#"{
        "questions": [
            {
                "header": "Level",
                "prompt": "How well do you know Raft?",
                "options": [ { "label": "New to it" }, { "label": "Read the paper" } ]
            }
        ],
        "author": "agent"
    }"#;

    async fn choose(state: &AppState, body: &str) -> (StatusCode, serde_json::Value) {
        json(app(state.clone()), send_json("POST", "/api/choices", body)).await
    }

    async fn answer_choice(
        state: &AppState,
        id: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let uri = format!("/api/choices/{id}");
        json(
            app(state.clone()),
            send_json("POST", &uri, &body.to_string()),
        )
        .await
    }

    /// The agent asks a choice at the end of a page and one with no page,
    /// the page finds both, the learner answers, and the agent gets the
    /// labels of the picks.
    #[tokio::test]
    async fn the_choice_routes_ask_a_choice_and_answer_it() {
        let (dir, state) = with_lesson().await;
        let untouched = outside(dir.path());
        let after = agent_waits(&state, 0).await["next"].as_u64().unwrap();

        let (status, asked) = choose(&state, CHOICE).await;
        assert_eq!(status, StatusCode::OK, "{asked}");
        insta::assert_json_snapshot!("choose", asked);
        let (status, level) = choose(&state, LEVEL).await;
        assert_eq!(status, StatusCode::OK, "{level}");
        assert_eq!(level, serde_json::json!({ "id": "choice-2" }));
        // A question of the agent is no request.
        let batch = state.events.after(Some(after), events::HOLD).await;
        let seen: Vec<_> = batch
            .events
            .iter()
            .map(|event| {
                (
                    event.kind,
                    event.author,
                    event.page.as_deref(),
                    event.id.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            seen,
            [
                (
                    EventKind::Choose,
                    Author::Agent,
                    Some("1"),
                    Some("choice-1")
                ),
                (EventKind::Choose, Author::Agent, None, Some("choice-2")),
            ]
        );
        assert!(seen.iter().all(|(kind, ..)| !kind.is_request()));

        // The page finds a choice on a page with the page, and one with no
        // page with the session, so a reload shows both.
        let (_, page) = json(app(state.clone()), get_with_cookie("/api/pages/1", TOKEN)).await;
        insta::assert_json_snapshot!("page_choices", page["choices"]);
        let (_, mut session) =
            json(app(state.clone()), get_with_cookie("/api/session", TOKEN)).await;
        session["version"] = "[version]".into();
        insta::assert_json_snapshot!("session_choices", session["choices"]);
        assert_eq!(session["choices"][0]["id"], "choice-2");
        assert_eq!(session["choices"].as_array().unwrap().len(), 1);

        // The learner picks one option of the first question, and answers
        // the second one with two options and words of their own.
        let (status, chosen) = answer_choice(
            &state,
            "choice-1",
            serde_json::json!({ "answers": [
                { "picked": [1] },
                { "picked": [2, 0], "other": "  and the restart  " },
            ] }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{chosen}");
        insta::assert_json_snapshot!("chosen", chosen);
        let agent = agent_waits(&state, batch.next).await;
        insta::assert_json_snapshot!("events_chosen", server_redacted(&state, agent.clone()));
        assert!(
            state.events.agent_works(),
            "the wait handed the choice over"
        );

        // The answer stays on the page, and a push of the page keeps it.
        let (_, pushed) = json(app(state.clone()), put("/api/pages/1", LESSON)).await;
        assert_eq!(pushed["choices"], serde_json::json!([chosen]));

        // A choice of the column is answered by free text alone.
        let (status, typed) = answer_choice(
            &state,
            "choice-2",
            serde_json::json!({ "answers": [ { "picked": [], "other": "I read half of it" } ] }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{typed}");
        let agent = agent_waits(&state, agent["next"].as_u64().unwrap()).await;
        // The push of the page comes first in the batch.
        let event = agent["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["kind"] == "chosen")
            .unwrap();
        assert_eq!(
            (&event["kind"], &event["page"], &event["id"]),
            (
                &"chosen".into(),
                &serde_json::Value::Null,
                &"choice-2".into()
            )
        );
        assert_eq!(
            event["detail"]["answers"],
            serde_json::json!([{ "header": "Level", "picked": [], "labels": [],
                                 "other": "I read half of it" }])
        );

        let stored: serde_json::Value = serde_json::from_slice(
            &std::fs::read(dir.path().join(".learn/raft/choices.json")).unwrap(),
        )
        .unwrap();
        insta::assert_json_snapshot!("choices_json", at_redacted(stored));
        assert_eq!(
            files(dir.path()),
            [
                ".learn/raft/choices.json",
                ".learn/raft/course.json",
                ".learn/raft/pages/1.json",
                ".learn/raft/pages/1.md"
            ]
        );
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
    }

    #[tokio::test]
    async fn a_refused_choice_gets_every_fault_and_stores_nothing() {
        let (dir, state) = with_lesson().await;
        // Page 2 is in the syllabus, with no content yet.
        state.course.write().await.parts[0].pages.push(PageRef {
            id: "2".to_owned(),
            kind: PageKind::Lesson,
            title: "The vote".to_owned(),
            state: PageState::Planned,
            score: None,
        });
        let after = state.events.after(None, events::HOLD).await.next;

        let (status, body) = choose(
            &state,
            r#"{ "page": "2", "questions": [
                   { "header": "", "prompt": "What next?", "options": [ { "label": "Part 2" } ] }
               ], "multi": true }"#,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        insta::assert_json_snapshot!("choose_refused", body);

        let one = r#"[ { "header": "Next", "prompt": "What next?",
                         "options": [ { "label": "Part 2" }, { "label": "A break" } ] } ]"#;
        for body in [
            format!(r#"{{ "page": "7", "questions": {one} }}"#),
            format!(r#"{{ "page": "2", "questions": {one} }}"#),
            format!(r#"{{ "page": "../1", "questions": {one} }}"#),
            format!(r#"{{ "questions": {one}, "author": "learner" }}"#),
            r#"{ "questions": [] }"#.to_owned(),
            "[]".to_owned(),
        ] {
            let (status, answer) = choose(&state, &body).await;

            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {answer}");
            assert_eq!(answer["error"]["code"], "invalidChoice", "{body}");
        }
        let (status, answer) = choose(&state, "{ not json").await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
        assert_eq!(answer["error"]["code"], "badRequest");

        assert!(!dir.path().join(".learn/raft/choices.json").exists());
        assert_eq!(state.events.after(None, events::HOLD).await.next, after);
    }

    #[tokio::test]
    async fn an_answer_that_the_choice_does_not_take_is_refused_and_stores_nothing() {
        let (dir, state) = with_lesson().await;
        let (status, _) = choose(&state, CHOICE).await;
        assert_eq!(status, StatusCode::OK);
        let path = dir.path().join(".learn/raft/choices.json");
        let before = std::fs::read(&path).unwrap();
        let after = state.events.after(None, events::HOLD).await.next;

        let two = |first: serde_json::Value, second: serde_json::Value| serde_json::json!({ "answers": [first, second] });
        let pick = |picked: &[usize]| serde_json::json!({ "picked": picked });
        for (id, body, status, code) in [
            (
                "choice-2",
                two(pick(&[0]), pick(&[0])),
                StatusCode::NOT_FOUND,
                "notFound",
            ),
            (
                "choice",
                two(pick(&[0]), pick(&[0])),
                StatusCode::NOT_FOUND,
                "notFound",
            ),
            (
                "choice-01",
                two(pick(&[0]), pick(&[0])),
                StatusCode::NOT_FOUND,
                "notFound",
            ),
            // A pick out of range.
            (
                "choice-1",
                two(pick(&[2]), pick(&[0])),
                StatusCode::BAD_REQUEST,
                "badRequest",
            ),
            // Two picks on a question that takes one.
            (
                "choice-1",
                two(pick(&[0, 1]), pick(&[0])),
                StatusCode::BAD_REQUEST,
                "badRequest",
            ),
            // One option picked twice.
            (
                "choice-1",
                two(pick(&[0]), pick(&[1, 1])),
                StatusCode::BAD_REQUEST,
                "badRequest",
            ),
            // No pick and no words of the learner.
            (
                "choice-1",
                two(
                    pick(&[0]),
                    serde_json::json!({ "picked": [], "other": "  " }),
                ),
                StatusCode::BAD_REQUEST,
                "badRequest",
            ),
            // A missing question.
            (
                "choice-1",
                serde_json::json!({ "answers": [ { "picked": [0] } ] }),
                StatusCode::BAD_REQUEST,
                "badRequest",
            ),
            // An empty answer.
            (
                "choice-1",
                serde_json::json!({ "answers": [] }),
                StatusCode::BAD_REQUEST,
                "badRequest",
            ),
            (
                "choice-1",
                serde_json::json!({}),
                StatusCode::BAD_REQUEST,
                "badRequest",
            ),
            (
                "choice-1",
                serde_json::json!({ "answers": [ { "picked": [0], "ok": true }, pick(&[0]) ] }),
                StatusCode::BAD_REQUEST,
                "badRequest",
            ),
            (
                "choice-1",
                serde_json::json!({ "answers": [ { "picked": [-1] }, pick(&[0]) ] }),
                StatusCode::BAD_REQUEST,
                "badRequest",
            ),
        ] {
            let (got, answer) = answer_choice(&state, id, body.clone()).await;

            assert_eq!(got, status, "{id} {body}: {answer}");
            assert_eq!(answer["error"]["code"], code, "{id} {body}");
            assert!(answer["error"]["message"].is_string(), "{answer}");
        }
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(state.events.after(None, events::HOLD).await.next, after);

        // An answer sent twice: the second one changes nothing.
        let (status, _) = answer_choice(&state, "choice-1", two(pick(&[0]), pick(&[1]))).await;
        assert_eq!(status, StatusCode::OK);
        let answered = std::fs::read(&path).unwrap();
        let after = state.events.after(None, events::HOLD).await.next;
        let (status, answer) = answer_choice(&state, "choice-1", two(pick(&[1]), pick(&[2]))).await;
        assert_eq!(status, StatusCode::CONFLICT, "{answer}");
        assert_eq!(answer["error"]["code"], "conflict");
        assert_eq!(std::fs::read(&path).unwrap(), answered);
        assert_eq!(state.events.after(None, events::HOLD).await.next, after);
    }

    #[tokio::test]
    async fn an_answer_to_a_choice_from_another_site_is_forbidden() {
        let (dir, state) = with_lesson().await;
        let (status, _) = choose(&state, LEVEL).await;
        assert_eq!(status, StatusCode::OK);
        let before = std::fs::read(dir.path().join(".learn/raft/choices.json")).unwrap();

        let request = Request::builder()
            .method("POST")
            .uri("/api/choices/choice-1")
            .header(
                header::COOKIE,
                format!("{}={TOKEN}", auth::cookie_name(PORT)),
            )
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, "http://127.0.0.1:3000")
            .header("sec-fetch-site", "same-site")
            .body(Body::from(r#"{"answers":[{"picked":[0]}]}"#))
            .unwrap();
        let (status, body) = json(app(state.clone()), request).await;

        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(body["error"]["code"], "forbidden");
        assert_eq!(
            std::fs::read(dir.path().join(".learn/raft/choices.json")).unwrap(),
            before
        );
    }

    /// A corrupt choices.json holds answers of the learner: a choice is
    /// refused rather than written over it.
    #[tokio::test]
    async fn a_choice_over_a_corrupt_choices_file_is_refused_and_leaves_it() {
        let (dir, state) = with_lesson().await;
        let path = dir.path().join(".learn/raft/choices.json");
        std::fs::write(&path, "{ not json").unwrap();

        let (status, body) = choose(&state, LEVEL).await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap()
                .contains("repair it by hand"),
            "{body}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
    }

    /// A corrupt choices.json refuses only the choices. The session
    /// answers, as every command of the agent probes it, and says why the
    /// choices are gone. The pages, a push and a quiz answer go on.
    #[tokio::test]
    async fn a_corrupt_choices_file_blocks_only_the_choices() {
        let (dir, state) = with_quiz(LESSON, QUIZ).await;
        let path = dir.path().join(".learn/raft/choices.json");
        std::fs::write(&path, "{ not json").unwrap();

        let (status, session) =
            json(app(state.clone()), get_with_cookie("/api/session", TOKEN)).await;
        assert_eq!(status, StatusCode::OK, "{session}");
        assert_eq!(session["choices"], serde_json::json!([]));
        let unreadable = session["choicesError"].as_str().unwrap();
        assert!(unreadable.contains("repair it by hand"), "{session}");

        let (status, page) = json(app(state.clone()), get_with_cookie("/api/pages/1", TOKEN)).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert_eq!(page["choices"], serde_json::json!([]));
        let (status, pushed) = json(app(state.clone()), put("/api/pages/1", LESSON)).await;
        assert_eq!(status, StatusCode::OK, "{pushed}");
        let (status, sent) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes/1/open", OPEN),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{sent}");

        let (status, refused) = answer_choice(
            &state,
            "choice-1",
            serde_json::json!({ "answers": [{ "picked": [0] }] }),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{refused}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
    }

    /// The export holds what an agent needs to resume the course: the
    /// sources, the answers with their grades and debriefs, the threads,
    /// the choices, the points in their order, and the names of the notes.
    #[tokio::test]
    async fn the_export_route_answers_the_whole_course() {
        let (dir, state) = with_quiz(LESSON, QUIZ_TWO_POINTS).await;
        answer_quiz(&state, 1).await;
        let body = debrief_body(
            DEBRIEF,
            serde_json::json!({ "random-timeout": "ok", "majority": "partial" }),
        );
        let (status, written) = write_debrief(&state, 1, &body).await;
        assert_eq!(status, StatusCode::OK, "{written}");
        let block = block_of(&state, 0);
        let opened = serde_json::json!({ "block": block, "quote": "starts an election",
                                         "body": "Why does it *start* one?" });
        let (status, thread) = thread_write(&state, "/api/pages/1/threads", opened).await;
        assert_eq!(status, StatusCode::OK, "{thread}");
        let reply = serde_json::json!({ "body": "Nobody leads: it must.", "author": "agent" });
        let (status, thread) = thread_write(&state, "/api/threads/1-1", reply).await;
        assert_eq!(status, StatusCode::OK, "{thread}");
        let (status, asked) = choose(&state, CHOICE).await;
        assert_eq!(status, StatusCode::OK, "{asked}");
        let answers = serde_json::json!({ "answers": [
            { "picked": [1] }, { "picked": [0, 2], "other": "And the snapshots" } ] });
        let (status, chosen) = answer_choice(&state, "choice-1", answers).await;
        assert_eq!(status, StatusCode::OK, "{chosen}");
        let (status, open) = choose(&state, LEVEL).await;
        assert_eq!(status, StatusCode::OK, "{open}");
        // A second quiz that the learner has not answered yet.
        let (status, quiz) = json(
            app(state.clone()),
            send_json("POST", "/api/pages/1/quizzes", QUIZ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{quiz}");
        // A finding goes out in Markdown, as the agent wrote it.
        let (status, finding) = write_finding(&state, "seed-race", FINDING).await;
        assert_eq!(status, StatusCode::OK, "{finding}");
        let notes = dir.path().join(".learn/raft/notes");
        std::fs::create_dir_all(notes.join("sources")).unwrap();
        std::fs::write(notes.join("plan.md"), "The plan.\n").unwrap();
        std::fs::write(notes.join("sources/paper.txt"), "In Search of...\n").unwrap();
        let stored = files(dir.path());
        let untouched = outside(dir.path());

        let (status, mut body) =
            json(app(state.clone()), get_with_cookie("/api/export", TOKEN)).await;

        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(files(dir.path()), stored, "an export writes nothing");
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
        // The right option of a quiz with no answer stays on the server, as
        // in the page: the page could read this route too.
        let waiting = &body["pages"][0]["quizzes"][1];
        assert_eq!(waiting["choices"][0]["graded"], serde_json::Value::Null);
        assert!(!keys(waiting).contains(&"answer".to_owned()), "{waiting}");
        assert_eq!(body["version"], crate::version::LONG);
        body["version"] = "[version]".into();
        // The writes of the test stamp the course with the clock.
        body["course"]["updated"] = "[updated]".into();
        insta::assert_json_snapshot!("export", at_redacted(body));
    }

    /// The cheatsheet of the agent: a hook, a table and an excerpt.
    const SHEET: &str = "# Raft in one page\n\n\
                         > [!hook]\n> The first follower to wake up wins.\n\n\
                         | Name | What |\n|---|---|\n\
                         | `election_timeout` | the wait of a follower |\n\n\
                         ```excerpt\npath: src/raft.c\nlines: 2-4\nmark: 4\n```\n";

    async fn write_sheet(state: &AppState, source: &str) -> (StatusCode, serde_json::Value) {
        json(
            app(state.clone()),
            put("/api/cheatsheet", source.to_owned()),
        )
        .await
    }

    #[tokio::test]
    async fn the_cheatsheet_route_answers_an_empty_cheatsheet_before_the_write() {
        let dir = repo().await;
        let state = state_in(dir.path());

        let (status, body) = json(app(state), get_with_cookie("/api/cheatsheet", TOKEN)).await;

        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body,
            serde_json::json!({ "written": false, "blocks": [], "map": [], "revisit": [] })
        );
        assert!(files(dir.path()).is_empty(), "a read writes nothing");
    }

    /// The agent writes the source. The server adds the excerpts of the
    /// lessons and of their debriefs, merged, and the points whose last
    /// result is missed or partial. A later page shows in the next read.
    #[tokio::test]
    async fn the_cheatsheet_route_stores_the_source_and_adds_the_map_and_the_points() {
        let lesson = "# The timer\n\n```excerpt\npath: src/raft.c\nlines: 1-2\n```\n";
        let (dir, state) = with_quiz(lesson, QUIZ_TWO_POINTS).await;
        answer_quiz(&state, 1).await;
        let body = debrief_body(
            DEBRIEF,
            serde_json::json!({ "random-timeout": "ok", "majority": "partial" }),
        );
        assert_eq!(write_debrief(&state, 1, &body).await.0, StatusCode::OK);
        let end = "# The end\n\n```excerpt\npath: src/raft.c\nlines: 5\n```\n";
        assert_eq!(
            json(app(state.clone()), put("/api/pages/2", end)).await.0,
            StatusCode::OK
        );
        let untouched = outside(dir.path());
        let after = state.events.after(None, events::HOLD).await.next;

        let (status, written) = write_sheet(&state, SHEET).await;

        assert_eq!(status, StatusCode::OK, "{written}");
        insta::assert_json_snapshot!("cheatsheet_write", written);
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".learn/raft/cheatsheet.md")).unwrap(),
            SHEET,
            "the source is kept as written"
        );
        assert!(dir.path().join(".learn/raft/cheatsheet.json").is_file());
        let batch = state.events.after(Some(after), events::HOLD).await;
        let events: Vec<_> = batch
            .events
            .iter()
            .map(|event| {
                (
                    event.kind,
                    event.author,
                    event.page.clone(),
                    event.id.clone(),
                )
            })
            .collect();
        assert_eq!(events, [(EventKind::Cheatsheet, Author::Agent, None, None)]);
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");

        // The code changes, and a new page shows it: the map gives the file
        // at each commit, and the cheatsheet keeps the commit of its write.
        // The store stays out of the commit, so its hash is the same on
        // every run.
        std::fs::write(dir.path().join(".git/info/exclude"), ".learn/\n").unwrap();
        dir.commit(&commit("faster").file("src/raft.c", "int election_timeout;\n"))
            .await;
        let faster = "# Faster\n\n```excerpt\npath: src/raft.c\nlines: 1\n```\n";
        assert_eq!(
            json(app(state.clone()), put("/api/pages/3", faster))
                .await
                .0,
            StatusCode::OK
        );
        let (status, read) = json(app(state), get_with_cookie("/api/cheatsheet", TOKEN)).await;

        assert_eq!(status, StatusCode::OK, "{read}");
        assert_eq!(read["blocks"], written["blocks"]);
        insta::assert_json_snapshot!("cheatsheet", read);
    }

    #[tokio::test]
    async fn a_refused_cheatsheet_gets_every_error_and_stores_nothing() {
        let dir = repo().await;
        let state = state_in(dir.path());
        let after = state.events.after(None, events::HOLD).await.next;
        let source = "# Raft\n\n```excerpt\npath: src/raft.c\nlines: 2-4\nhide: 4\n```\n\n\
                      ```excerpt\npath: src/raft.c\nlines: 90\n```\n";

        let (status, body) = write_sheet(&state, source).await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        insta::assert_json_snapshot!("cheatsheet_refused", body);
        assert!(files(dir.path()).is_empty(), "{:?}", files(dir.path()));
        assert_eq!(state.events.after(None, events::HOLD).await.next, after);
    }

    /// The cheatsheet may show a line that a page hides: it stays hidden
    /// there until the learner answers every quiz of that page.
    #[tokio::test]
    async fn the_cheatsheet_hides_a_line_that_a_page_still_hides() {
        let (_dir, state) = with_quiz(EXCERPT, QUIZ_TWO_POINTS).await;
        let source = "# Raft\n\n```excerpt\npath: src/raft.c\nlines: 3-4\n```\n";
        let (status, written) = write_sheet(&state, source).await;
        assert_eq!(status, StatusCode::OK, "{written}");
        let lines = |sheet: &serde_json::Value| sheet["blocks"][1]["excerpt"]["lines"].clone();

        assert_eq!(lines(&written)[1], serde_json::Value::Null);
        assert_eq!(lines(&written)[0]["text"], "{");

        answer_quiz(&state, 1).await;
        let (_, read) = json(app(state), get_with_cookie("/api/cheatsheet", TOKEN)).await;
        assert_eq!(lines(&read)[1]["text"], "    return base + rand() % base;");
    }

    /// The Markdown of the cheatsheet comes as a file to save, under the
    /// token. Before the write, a 404 names the command that writes it.
    #[tokio::test]
    async fn the_markdown_route_answers_the_cheatsheet_as_a_file() {
        let uri = "/api/cheatsheet.md";
        let (dir, state) = with_quiz(EXCERPT, QUIZ_TWO_POINTS).await;
        let stored = files(dir.path());
        let (before, missing) = json(app(state.clone()), get_with_cookie(uri, TOKEN)).await;
        assert_eq!(files(dir.path()), stored, "a read writes nothing");
        // Line 4 shows here, and page 1 hides it until its quiz is answered.
        let source = "# Raft\n\n```excerpt\npath: src/raft.c\nlines: 3-4\n```\n";
        let (status, written) = write_sheet(&state, source).await;
        assert_eq!(status, StatusCode::OK, "{written}");
        let untouched = outside(dir.path());
        let stored = files(dir.path());

        let (status, headers, body) = bytes(app(state.clone()), get_with_cookie(uri, TOKEN)).await;

        let text = String::from_utf8(body).unwrap();
        assert!(text.starts_with("# Raft\n"), "{text}");
        assert!(text.contains(crate::cheatsheet::markdown::HIDDEN), "{text}");
        assert!(!text.contains("return base"), "{text}");
        assert!(text.contains("\n## Map of the code\n"), "{text}");
        assert!(
            text.ends_with("## To revisit\n\nNothing to revisit.\n"),
            "{text}"
        );
        assert_eq!(files(dir.path()), stored, "a read writes nothing");
        assert_eq!(outside(dir.path()), untouched, "a write outside .learn/");
        let header = |name: header::HeaderName| headers[name].to_str().unwrap().to_owned();
        let (unauthorized, refused) = json(app(state), get(uri)).await;
        let shape = serde_json::json!({
            "before": { "status": before.as_u16(), "body": missing },
            "written": {
                "status": status.as_u16(),
                "contentType": header(header::CONTENT_TYPE),
                "contentDisposition": header(header::CONTENT_DISPOSITION),
                "contentTypeOptions": header(header::X_CONTENT_TYPE_OPTIONS),
            },
            "withoutToken": { "status": unauthorized.as_u16(), "body": refused },
        });
        insta::assert_json_snapshot!("cheatsheet_markdown", shape);
    }

    /// A trace goes into a log or a bug report. The token must not.
    #[test]
    fn a_traced_address_carries_no_token() {
        let uri = |text: &str| text.parse::<axum::http::Uri>().unwrap();

        assert_eq!(without_token(&uri("/?t=secret")), "/");
        assert_eq!(
            without_token(&uri("/api/events?after=3&t=secret")),
            "/api/events?after=3"
        );
        assert_eq!(without_token(&uri("/api/session")), "/api/session");
    }
}
