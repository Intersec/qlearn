//! The HTTP routes.
//!
//! Every answer is JSON, and every error carries the same shape, so the
//! page and the commands of the agent never have to guess what came back.

pub mod auth;

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

use crate::events::{self, Events};
use crate::highlight::Highlighter;
use crate::lesson::{self, Fault};
use crate::model::{
    AgentStatus, Author, Batch, Course, CourseChange, Detail, Event, EventKind, Page, PageKind,
    Session, StatusText,
};
use crate::store::{self, PageFile, Store};

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
    pub events: Arc<Events>,
}

impl AppState {
    /// `current` is the page that the agent pushed last, as the store says.
    pub fn new(
        store: Store,
        course: Course,
        current: Option<String>,
        token: String,
        port: u16,
    ) -> Self {
        Self {
            token: Arc::new(token),
            port,
            course: Arc::new(RwLock::new(course)),
            current: Arc::new(Mutex::new(current)),
            store: Arc::new(store),
            events: Arc::new(Events::default()),
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
        .route("/api/pages/{id}", get(page).put(push_page))
        .route("/api/pages/{id}/next", post(next))
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

async fn interface(uri: axum::http::Uri) -> Response {
    match crate::assets::get(uri.path()) {
        Some((body, mime)) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, mime)],
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

    Json(Session {
        course: course.clone(),
        current: state.current(),
        version: crate::version::LONG.to_owned(),
    })
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

    Ok(Json(Page {
        id,
        blocks: file.blocks,
    }))
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
    let parsed = lesson::parse(&source, Highlighter::shared())
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
    let file = PageFile {
        blocks: parsed.blocks,
    };
    let undo = state.store.save_page(&changed.slug, &id, &source, &file)?;
    if let Err(error) = state.store.save(&changed) {
        return Err(undo.after(error).into());
    }
    *course = changed;
    *state.current.lock().expect("the current page is poisoned") = Some(id.clone());
    // Under the lock, so the events come in the order of the writes.
    state
        .events
        .push(EventKind::Page, Author::Agent, Some(&id), None);
    drop(course);

    Ok(Json(Page {
        id,
        blocks: file.blocks,
    }))
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
    let _page = (query.who == Some(Who::Page)).then(|| state.events.page_waits());

    let mut batch = state.events.after(query.after, events::HOLD).await;
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

    use crate::model::{PageRef, PageState, Part};

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

        AppState::new(Store::at(root), course, None, TOKEN.to_owned(), PORT)
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

    /// Every file under `dir`, relative to it.
    fn files(dir: &std::path::Path) -> Vec<String> {
        let mut out = Vec::new();
        let mut todo = vec![dir.to_path_buf()];
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
        let app = server();
        let (status, mut body) = json(app, get_with_cookie("/api/session", TOKEN)).await;

        assert_eq!(status, StatusCode::OK);
        // The commit under the release changes with every commit of qlearn.
        assert_eq!(body["version"], crate::version::LONG);
        body["version"] = "[version]".into();
        insta::assert_json_snapshot!("session", body);
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
            app(state),
            get_with_cookie("/api/events?after=1&who=page", TOKEN),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        insta::assert_json_snapshot!("events", body);
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
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path());

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
    }

    #[tokio::test]
    async fn the_page_route_answers_the_blocks_that_were_pushed() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path());
        let (_, pushed) = json(app(state.clone()), put("/api/pages/1", LESSON)).await;

        let (status, body) = json(app(state), get_with_cookie("/api/pages/1", TOKEN)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, pushed);
        insta::assert_json_snapshot!("page", body);
    }

    #[tokio::test]
    async fn a_page_with_no_content_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
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
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path());
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
    }

    #[tokio::test]
    async fn a_page_outside_the_syllabus_goes_at_the_end_of_the_last_part() {
        let dir = tempfile::tempdir().unwrap();
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
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path());
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
    }

    /// A page file that `course.json` does not back is the trace of a push
    /// that stopped halfway. It is not served.
    #[tokio::test]
    async fn a_page_file_that_the_course_does_not_hold_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path());
        let page = PageFile {
            blocks: lesson::parse(LESSON, Highlighter::shared()).unwrap().blocks,
        };
        state.store.save_page("raft", "2", LESSON, &page).unwrap();

        let (status, body) = json(app(state), get_with_cookie("/api/pages/2", TOKEN)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "notFound");
    }

    #[tokio::test]
    async fn a_push_with_a_bad_id_or_a_bad_body_gets_the_error_shape() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path());

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
    }

    #[tokio::test]
    async fn the_course_route_changes_the_fields_named_and_answers_the_course() {
        let dir = tempfile::tempdir().unwrap();
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
        let dir = tempfile::tempdir().unwrap();
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
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path());
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
        let dir = tempfile::tempdir().unwrap();
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
        insta::assert_json_snapshot!("events_of_the_agent", agent);
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
