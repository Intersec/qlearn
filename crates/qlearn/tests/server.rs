//! The server as a learner starts it: the built binary, in a real
//! repository, on a free port.

mod common;

use std::path::Path;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;

use qlearn::agent::address::Address;
use qlearn::agent::client::Client;

use common::{address, build_repo, interrupt, qlearn, read_url, send, stop};

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
