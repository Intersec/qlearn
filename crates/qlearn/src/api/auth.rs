//! Who may talk to the server.
//!
//! The server binds the loopback address, which stops another machine. It
//! does not stop another user on this one, or a page in another tab. A token
//! does.

use axum::extract::{Request, State};
use axum::http::header;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::{ApiError, AppState};

/// The cookie that the first page load leaves behind, for the server on
/// `port`.
///
/// A browser keeps one jar for 127.0.0.1 and ignores the port. With one name
/// for every server, the course of a second worktree would overwrite the
/// cookie of the first, and the first tab would get 401.
pub fn cookie_name(port: u16) -> String {
    format!("qlearn_token_{port}")
}

/// A random token, made once per run.
pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the system has no randomness");

    hex::encode(bytes)
}

/// Let the request through when it carries the token, as a query parameter
/// or as the cookie that the first load set.
pub async fn guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let token = state.token.as_bytes();
    let from_query = query_tokens(&request).any(|t| same(t.as_bytes(), token));
    let from_cookie = cookies(&request).any(|value| same(value.as_bytes(), token));

    // JSON, as every other error: the page and the commands of the agent
    // read one shape.
    if !from_query && !from_cookie {
        return ApiError::unauthorized(
            "open the address that qlearn printed: it carries the token".to_owned(),
        )
        .into_response();
    }

    let mut response = next.run(request).await;

    // Hand the cookie over once, so the address bar stays clean afterwards.
    if from_query && !from_cookie {
        let value = format!(
            "{}={}; Path=/; SameSite=Strict; HttpOnly",
            cookie_name(state.port),
            state.token
        );
        if let Ok(value) = value.parse() {
            response.headers_mut().insert(header::SET_COOKIE, value);
        }
    }
    response
}

/// Every `t` of the query. Read by hand, because an extractor answers a
/// query that it cannot read, a repeated `t` for one, with plain text.
fn query_tokens(request: &Request) -> impl Iterator<Item = String> + '_ {
    let query = request.uri().query().unwrap_or_default();

    form_urlencoded::parse(query.as_bytes())
        .filter(|(name, _)| name == "t")
        .map(|(_, value)| value.into_owned())
}

/// The value of every cookie. The browser sends the cookies of every qlearn
/// server on the loopback, so the one of this server is found by its value.
fn cookies(request: &Request) -> impl Iterator<Item = &str> {
    request
        .headers()
        .get_all(header::COOKIE)
        .into_iter()
        .filter_map(|header| header.to_str().ok())
        .flat_map(|header| header.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .map(|(_, value)| value)
}

/// Equal or not, in a time that does not tell where the two differ, so
/// another local user cannot guess the token byte by byte.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |diff, (x, y)| diff | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_32_random_bytes_in_hex() {
        let one = new_token();

        assert_eq!(one.len(), 64, "{one}");
        assert!(one.chars().all(|c| c.is_ascii_hexdigit()), "{one}");
        assert_ne!(one, new_token());
    }

    #[test]
    fn two_tokens_are_the_same_only_byte_for_byte() {
        assert!(same(b"abcd", b"abcd"));
        assert!(!same(b"abcd", b"abce"));
        assert!(!same(b"abcd", b"abc"));
        assert!(!same(b"", b"a"));
    }
}
