//! The server as a learner starts it: the built binary, in a real
//! repository, on a free port.

use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use qlearn::agent::address::Address;
use qlearn::agent::client::Client;

const QLEARN: &str = env!("CARGO_BIN_EXE_qlearn");

/// A real repository with one commit, as `testutil::build_repo` makes it.
/// That fixture belongs to the unit tests, so this one is its own.
fn build_repo() -> tempfile::TempDir {
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

fn qlearn(repo: &Path, args: &[&str]) -> Command {
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
async fn read_url(server: &mut Child) -> (u16, String) {
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

fn send(server: &Child, signal: &str) {
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
async fn stop(mut server: Child, signal: &str) -> ExitStatus {
    send(&server, signal);

    tokio::time::timeout(Duration::from_secs(10), server.wait())
        .await
        .unwrap_or_else(|_| panic!("qlearn stops on SIG{signal}"))
        .unwrap()
}

/// What Ctrl-C does in the terminal of the learner.
async fn interrupt(server: Child) -> ExitStatus {
    stop(server, "INT").await
}

/// The address that the server wrote.
fn address(repo: &Path) -> Address {
    let text = std::fs::read_to_string(repo.join(".learn/server.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[tokio::test]
async fn a_server_answers_on_its_port_and_its_socket_and_cleans_up_at_stop() {
    let repo = build_repo();
    let mut server = qlearn(repo.path(), &["raft", "--no-open", "--port", "0"])
        .spawn()
        .unwrap();
    let (port, token) = read_url(&mut server).await;

    let file = repo.path().join(".learn/server.json");
    let written: Address = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(written.port, port);
    assert_eq!(written.token, token);
    assert_eq!(written.pid, server.id().unwrap());
    let socket = written.socket.clone().expect("the server made its socket");
    assert!(socket.starts_with("/tmp/qlearn-"), "{socket}");

    let check = |session: Value| {
        assert_eq!(session["course"]["slug"], "raft");
        assert_eq!(session["version"], qlearn::version::LONG);
    };
    // Each way in alone. The client takes the socket first, so port 1,
    // where nothing listens, shows that the socket answered.
    let tcp = Client::new(Address {
        socket: None,
        ..written.clone()
    });
    check(tcp.get("/api/session").await.unwrap());
    let unix = Client::new(Address {
        port: 1,
        ..written.clone()
    });
    check(unix.get("/api/session").await.unwrap());
    for stranger in [&tcp, &unix] {
        let stranger = Client::new(Address {
            token: "wrong".to_owned(),
            ..stranger.address().clone()
        });
        let refused = stranger.get("/api/session").await.unwrap_err();
        assert!(refused.to_string().contains("token"), "{refused}");
    }

    // A second qlearn on the worktree finds the first and leaves it alone.
    let second = qlearn(repo.path(), &["--no-open"]).output().await.unwrap();
    let said = String::from_utf8_lossy(&second.stdout);
    assert!(second.status.success(), "{second:?}");
    assert!(said.contains("already runs"), "{said}");
    assert!(said.contains("raft"), "{said}");
    assert!(said.contains(&written.url()), "{said}");
    assert!(
        file.exists(),
        "the second qlearn must not touch the address"
    );

    let status = interrupt(server).await;

    assert!(status.success(), "{status:?}");
    assert!(!file.exists(), "server.json is removed at stop");
    assert!(
        !Path::new(&socket).exists(),
        "the socket is removed at stop"
    );
}

#[tokio::test]
async fn an_address_left_by_a_crash_counts_as_no_server() {
    let repo = build_repo();
    std::fs::create_dir(repo.path().join(".learn")).unwrap();
    let stale = Address {
        pid: 1,
        port: 1,
        token: "gone".to_owned(),
        socket: None,
    };
    std::fs::write(
        repo.path().join(".learn/server.json"),
        serde_json::to_string(&stale).unwrap(),
    )
    .unwrap();

    let mut server = qlearn(repo.path(), &["raft", "--no-open"]).spawn().unwrap();
    let (port, _) = read_url(&mut server).await;

    assert_ne!(port, 1);
    let text = std::fs::read_to_string(repo.path().join(".learn/server.json")).unwrap();
    let written: Address = serde_json::from_str(&text).unwrap();
    assert_eq!(written.port, port, "the new server owns the file");
    assert!(interrupt(server).await.success());
}

/// `kill` and a terminal that closes stop the server as Ctrl-C does, and
/// leave nothing behind. `make dev` stops its server with SIGTERM.
#[tokio::test]
async fn sigterm_and_sighup_clean_up_as_ctrl_c_does() {
    for signal in ["TERM", "HUP"] {
        let repo = build_repo();
        let mut server = qlearn(repo.path(), &["raft", "--no-open"]).spawn().unwrap();
        read_url(&mut server).await;
        let socket = address(repo.path())
            .socket
            .expect("the server made its socket");

        let status = stop(server, signal).await;

        assert!(status.success(), "SIG{signal}: {status:?}");
        assert!(
            !repo.path().join(".learn/server.json").exists(),
            "SIG{signal}"
        );
        assert!(!Path::new(&socket).exists(), "SIG{signal}");
    }
}

/// What a qlearn says first: its own URL, or the server that runs.
async fn first_word(mut child: Child) -> (String, Child) {
    let stdout = child.stdout.take().unwrap();
    let mut lines = BufReader::new(stdout).lines();
    let read = async {
        while let Some(line) = lines.next_line().await.unwrap() {
            if line.contains("qlearn is at") || line.contains("already runs") {
                return line;
            }
        }
        String::new()
    };
    let line = tokio::time::timeout(Duration::from_secs(30), read)
        .await
        .expect("qlearn said nothing in time");
    (line, child)
}

/// Two qlearn started at once: one serves, the other finds it. Neither
/// takes the socket or the address file of the other.
#[tokio::test]
async fn two_qlearn_started_at_once_make_one_server() {
    let repo = build_repo();
    std::fs::create_dir(repo.path().join(".learn")).unwrap();
    let one = qlearn(repo.path(), &["raft", "--no-open"]).spawn().unwrap();
    let two = qlearn(repo.path(), &["raft", "--no-open"]).spawn().unwrap();

    let ((one_said, one), (two_said, two)) = tokio::join!(first_word(one), first_word(two));

    let (server, other, other_said) = match one_said.contains("qlearn is at") {
        true => (one, two, two_said),
        false => (two, one, one_said),
    };
    assert!(other_said.contains("already runs"), "{other_said}");
    let other = other.wait_with_output().await.unwrap();
    assert!(other.status.success(), "{other:?}");

    let written = address(repo.path());
    assert_eq!(written.pid, server.id().unwrap());
    let socket = written.socket.clone().expect("the server kept its socket");
    let unix = Client::new(Address {
        port: 1,
        ..written.clone()
    });
    unix.get("/api/session").await.expect("the socket answers");

    assert!(interrupt(server).await.success());
    assert!(!Path::new(&socket).exists());
}

/// A server suspended with Ctrl-Z holds its worktree. A second qlearn says
/// so soon, and leaves its socket and its address alone.
#[tokio::test]
async fn a_suspended_server_keeps_its_worktree() {
    let repo = build_repo();
    let mut server = qlearn(repo.path(), &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    let written = address(repo.path());
    let socket = written.socket.clone().expect("the server made its socket");
    send(&server, "STOP");

    let second = qlearn(repo.path(), &["raft", "--no-open"]).output();
    let second = tokio::time::timeout(Duration::from_secs(20), second)
        .await
        .expect("a second qlearn does not wait for a suspended server")
        .unwrap();

    send(&server, "CONT");
    let said = String::from_utf8_lossy(&second.stderr);
    assert!(!second.status.success(), "{second:?}");
    assert!(said.contains("does not answer"), "{said}");
    assert!(
        Path::new(&socket).exists(),
        "the socket of the first is left"
    );
    assert_eq!(address(repo.path()), written);
    assert!(interrupt(server).await.success());
}
