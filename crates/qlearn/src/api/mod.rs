//! The HTTP routes.
//!
//! Every answer is JSON, and every error carries the same shape, so the
//! page and the commands of the agent never have to guess what came back.

pub mod auth;

use std::sync::Arc;

use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router, middleware};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::events::{self, Events};
use crate::model::{Batch, Course, Session};

#[derive(Clone)]
pub struct AppState {
    pub token: Arc<String>,
    /// The port of this server, which names its cookie.
    pub port: u16,
    /// The course of this server. The server is the only writer of the
    /// store, so the copy in memory is the course.
    pub course: Arc<RwLock<Course>>,
    pub events: Arc<Events>,
}

impl AppState {
    pub fn new(course: Course, token: String, port: u16) -> Self {
        Self {
            token: Arc::new(token),
            port,
            course: Arc::new(RwLock::new(course)),
            events: Arc::new(Events::default()),
        }
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
    let course = state.course.read().await.clone();

    Json(Session {
        course,
        version: crate::version::LONG.to_owned(),
    })
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

    let batch = state.events.after(query.after, events::HOLD).await;
    // Reached only when the agent did not hang up, which drops this future.
    if let Some(agent) = &mut agent {
        agent.answered(&batch);
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

    use crate::model::{Author, EventKind, PageKind, PageRef, PageState, Part};

    const TOKEN: &str = "0123456789abcdef";
    const PORT: u16 = 7420;

    fn server() -> Router {
        app(state())
    }

    fn state() -> AppState {
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

        AppState::new(course, TOKEN.to_owned(), PORT)
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
