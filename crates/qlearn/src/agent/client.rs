//! A client for the server of the worktree.
//!
//! The commands of an agent send a few small JSON requests to a server that
//! this same binary runs. HTTP/1.1 with `Connection: close` is all that
//! takes, and it costs no dependency.

use std::io::ErrorKind;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpStream, UnixStream};

use super::address::Address;
use crate::api::auth;

/// A server that hangs must not hang the command with it.
const TIMEOUT: Duration = Duration::from_secs(45);

const NO_SERVER: &str = "no qlearn server answers on this worktree. Run `qlearn` first";

pub struct Client {
    address: Address,
    timeout: Duration,
}

impl Client {
    pub fn new(address: Address) -> Self {
        Self::with_timeout(address, TIMEOUT)
    }

    /// A client that gives up sooner, for a call that only asks whether
    /// the server lives.
    pub fn with_timeout(address: Address, timeout: Duration) -> Self {
        Self { address, timeout }
    }

    pub fn address(&self) -> &Address {
        &self.address
    }

    pub async fn get(&self, path: &str) -> Result<Value> {
        self.send("GET", path, None).await
    }

    pub async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        self.send("POST", path, Some(Body::json(body))).await
    }

    pub async fn put(&self, path: &str, body: &Value) -> Result<Value> {
        self.send("PUT", path, Some(Body::json(body))).await
    }

    pub async fn patch(&self, path: &str, body: &Value) -> Result<Value> {
        self.send("PATCH", path, Some(Body::json(body))).await
    }

    /// Send a text as it is, for a route whose body is a source and not
    /// JSON.
    pub async fn put_text(&self, path: &str, text: &str) -> Result<Value> {
        let body = Body {
            kind: "text/markdown; charset=utf-8",
            payload: text.to_owned(),
        };
        self.send("PUT", path, Some(body)).await
    }

    async fn send(&self, method: &str, path: &str, body: Option<Body>) -> Result<Value> {
        let call = self.exchange(method, path, body);
        let raw = tokio::time::timeout(self.timeout, call)
            .await
            .with_context(|| format!("the server did not answer {method} {path}"))??;
        let (status, body) = parse(&raw)?;
        let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);

        if !(200..300).contains(&status) {
            let message = value["error"]["message"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| String::from_utf8_lossy(&body).into_owned());
            let code = value["error"]["code"].as_str().unwrap_or_default();
            return Err(Refusal {
                code: code.to_owned(),
                message,
            }
            .into());
        }
        Ok(value)
    }

    async fn exchange(&self, method: &str, path: &str, body: Option<Body>) -> Result<Vec<u8>> {
        let Body { kind, payload } = body.unwrap_or(Body {
            kind: "application/json",
            payload: String::new(),
        });
        let request = format!(
            "{method} {path} HTTP/1.1\r\n\
             Host: 127.0.0.1:{port}\r\n\
             Cookie: {cookie}={token}\r\n\
             Content-Type: {kind}\r\n\
             Content-Length: {length}\r\n\
             Connection: close\r\n\r\n{payload}",
            port = self.address.port,
            cookie = auth::cookie_name(self.address.port),
            token = self.address.token,
            length = payload.len(),
        );

        // The socket first: it is the way in from a sandbox, where the port
        // is in another network.
        if let Some(socket) = &self.address.socket {
            match UnixStream::connect(socket).await {
                Ok(mut stream) => return talk(&mut stream, &request).await,
                // The socket is there and nothing listens: its server is
                // gone, and the port may be another user's by now. The
                // token must not reach them.
                Err(error) if error.kind() == ErrorKind::ConnectionRefused => {
                    return Err(error).context(NO_SERVER);
                }
                // A sandbox may hide `/tmp` and still share the network.
                Err(_) => {}
            }
        }
        let mut stream = TcpStream::connect(("127.0.0.1", self.address.port))
            .await
            .context(NO_SERVER)?;
        talk(&mut stream, &request).await
    }
}

/// What a request carries, with its media type.
struct Body {
    kind: &'static str,
    payload: String,
}

impl Body {
    fn json(value: &Value) -> Self {
        Self {
            kind: "application/json",
            payload: value.to_string(),
        }
    }
}

/// An answer of the server that is not a success. A command reads the code
/// to tell a refused page from any other error.
#[derive(Debug)]
pub struct Refusal {
    /// The `code` of the error shape, empty when the answer had none.
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Refusal {}

async fn talk<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, request: &str) -> Result<Vec<u8>> {
    stream.write_all(request.as_bytes()).await?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await?;
    Ok(raw)
}

/// The status and the body of an answer.
fn parse(raw: &[u8]) -> Result<(u16, Vec<u8>)> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .context("the server sent an answer with no end of headers")?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let body = &raw[split + 4..];

    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .context("the server sent an answer with no status")?;
    let chunked = head.lines().any(|line| {
        let line = line.to_ascii_lowercase();
        line.starts_with("transfer-encoding:") && line.contains("chunked")
    });

    match chunked {
        true => Ok((status, unchunk(body)?)),
        false => Ok((status, body.to_vec())),
    }
}

fn unchunk(mut body: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let end = body
            .windows(2)
            .position(|w| w == b"\r\n")
            .context("a chunk with no size")?;
        let size = std::str::from_utf8(&body[..end])?;
        let size = usize::from_str_radix(size.split(';').next().unwrap_or("").trim(), 16)?;
        body = &body[end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if body.len() < size {
            bail!("a chunk shorter than it says");
        }
        out.extend_from_slice(&body[..size]);
        body = body.get(size + 2..).unwrap_or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_with_a_length_gives_its_status_and_body() {
        let raw = b"HTTP/1.1 201 Created\r\ncontent-type: application/json\r\ncontent-length: 8\r\n\r\n{\"a\":1}\n";

        let (status, body) = parse(raw).unwrap();

        assert_eq!(status, 201);
        assert_eq!(body, b"{\"a\":1}\n");
    }

    #[test]
    fn a_chunked_answer_is_put_back_together() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"a\"\r\n3\r\n:1}\r\n0\r\n\r\n";

        let (status, body) = parse(raw).unwrap();

        assert_eq!(status, 200);
        assert_eq!(body, b"{\"a\":1}");
    }

    #[test]
    fn an_answer_cut_before_its_body_is_an_error() {
        assert!(parse(b"HTTP/1.1 200 OK\r\ncontent-length: 3").is_err());
    }

    /// Anyone answers on the port, as another user can bind it once the
    /// server crashed.
    async fn anyone_on_a_port() -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut request = [0; 4096];
                let _ = stream.read(&mut request).await;
                let answer = b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\n{}";
                let _ = stream.write_all(answer).await;
            }
        });
        port
    }

    fn client(port: u16, socket: &std::path::Path) -> Client {
        Client::new(Address {
            pid: 1,
            port,
            token: "abc".to_owned(),
            socket: Some(socket.display().to_string()),
        })
    }

    #[tokio::test]
    async fn a_socket_that_nothing_listens_on_is_a_server_gone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crashed.sock");
        // A crash leaves the socket file behind.
        drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
        let port = anyone_on_a_port().await;

        let error = client(port, &path).get("/api/session").await.unwrap_err();

        assert!(
            error.to_string().contains("Run `qlearn` first"),
            "{error:#}"
        );
    }

    #[tokio::test]
    async fn a_socket_that_a_sandbox_hides_leaves_the_port() {
        let dir = tempfile::tempdir().unwrap();
        let port = anyone_on_a_port().await;

        let answer = client(port, &dir.path().join("hidden.sock"))
            .get("/api/session")
            .await;

        assert_eq!(answer.unwrap(), serde_json::json!({}));
    }
}
