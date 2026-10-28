//! A client for the server of the worktree.
//!
//! The commands of an agent send a few small JSON requests to a server that
//! this same binary runs. HTTP/1.1 with `Connection: close` is all that
//! takes, and it costs no dependency.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpStream, UnixStream};

use super::address::Address;
use crate::api::auth;

/// A server that hangs must not hang the command with it.
const TIMEOUT: Duration = Duration::from_secs(45);

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

    async fn send(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
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
            bail!("{message}");
        }
        Ok(value)
    }

    async fn exchange(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Vec<u8>> {
        let payload = body.map(Value::to_string).unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\n\
             Host: 127.0.0.1:{port}\r\n\
             Cookie: {cookie}={token}\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {length}\r\n\
             Connection: close\r\n\r\n{payload}",
            port = self.address.port,
            cookie = auth::cookie_name(self.address.port),
            token = self.address.token,
            length = payload.len(),
        );

        // The socket first: it is the way in from a sandbox, where the port
        // is in another network.
        if let Some(socket) = &self.address.socket
            && let Ok(mut stream) = UnixStream::connect(socket).await
        {
            return talk(&mut stream, &request).await;
        }
        let mut stream = TcpStream::connect(("127.0.0.1", self.address.port))
            .await
            .context("no qlearn server answers on this worktree. Run `qlearn` first")?;
        talk(&mut stream, &request).await
    }
}

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
}
