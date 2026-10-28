//! What the tests of the binary share: a real repository, the built
//! binary, and the server that it starts.

// Each test file is a crate of its own, and none uses every helper.
#![allow(dead_code)]

use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use qlearn::agent::address::Address;

pub const QLEARN: &str = env!("CARGO_BIN_EXE_qlearn");

/// A real repository with one commit, as `testutil::build_repo` makes it.
/// That fixture belongs to the unit tests, so this one is its own.
pub fn build_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
    for args in [
        &["init", "--quiet", "--initial-branch=main", "."][..],
        &["add", "a.txt"],
        &["commit", "--quiet", "-m", "first"],
    ] {
        let out = std::process::Command::new("git")
            .current_dir(dir.path())
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Test Author")
            .env("GIT_AUTHOR_EMAIL", "author@example.com")
            .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00+00:00")
            .env("GIT_COMMITTER_NAME", "Test Author")
            .env("GIT_COMMITTER_EMAIL", "author@example.com")
            .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00+00:00")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
    }
    dir
}

pub fn qlearn(repo: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(QLEARN);
    command
        .current_dir(repo)
        .args(args)
        .env_remove("QLEARN_TRACE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

/// The port and the token, from the line that carries the URL.
pub async fn read_url(server: &mut Child) -> (u16, String) {
    let stdout = server.stdout.take().unwrap();
    let mut lines = BufReader::new(stdout).lines();
    let read = async {
        while let Some(line) = lines.next_line().await.unwrap() {
            if let Some(at) = line.find("http://127.0.0.1:") {
                return line[at..].to_owned();
            }
        }
        panic!("qlearn printed no URL");
    };
    let url = tokio::time::timeout(Duration::from_secs(30), read)
        .await
        .expect("qlearn printed no URL in time");

    let rest = url.strip_prefix("http://127.0.0.1:").unwrap();
    let (port, token) = rest.split_once("/?t=").expect("the URL carries the token");
    (port.parse().unwrap(), token.to_owned())
}

pub fn send(server: &Child, signal: &str) {
    let pid = server.id().unwrap().to_string();
    let sent = std::process::Command::new("kill")
        .args([&format!("-{signal}"), &pid])
        .status()
        .unwrap();
    assert!(sent.success());
}

/// Stop the server with a signal, and wait for its end.
///
/// A server that a test leaves to `kill_on_drop` dies on SIGKILL and
/// leaves its socket in `/tmp`, so every test stops its server this way.
pub async fn stop(mut server: Child, signal: &str) -> ExitStatus {
    send(&server, signal);

    tokio::time::timeout(Duration::from_secs(10), server.wait())
        .await
        .unwrap_or_else(|_| panic!("qlearn stops on SIG{signal}"))
        .unwrap()
}

/// What Ctrl-C does in the terminal of the learner.
pub async fn interrupt(server: Child) -> ExitStatus {
    stop(server, "INT").await
}

/// The address that the server wrote.
pub fn address(repo: &Path) -> Address {
    let text = std::fs::read_to_string(repo.join(".learn/server.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// Run a command of the binary to its end, with `input` on its standard
/// input.
pub async fn run(repo: &Path, args: &[&str], input: Option<&str>) -> std::process::Output {
    let mut command = qlearn(repo, args);
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().unwrap();
    if let Some(input) = input {
        use tokio::io::AsyncWriteExt;
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(input.as_bytes()).await.unwrap();
        // Closed, so the command reads the end of its input.
        drop(stdin);
    }

    tokio::time::timeout(Duration::from_secs(60), child.wait_with_output())
        .await
        .unwrap_or_else(|_| panic!("qlearn {args:?} did not end in time"))
        .unwrap()
}
