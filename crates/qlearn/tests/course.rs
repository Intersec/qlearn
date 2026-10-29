//! The commands of the learner on a whole course: the export, the resume
//! after a restart, and the delete. The built binary, in a real
//! repository.

mod common;

use std::path::Path;
use std::process::Output;

use serde_json::{Value, json};

use qlearn::agent::address::Address;
use qlearn::agent::client::Client;

use common::{address, build_repo, interrupt, qlearn, read_url, run, send};

fn json_of(out: &Output) -> Value {
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{out:?}"))
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

const QUIZ: &str = r#"{ "open": { "prompt": "Why random?", "points": ["random-timeout"] },
    "choices": [{ "header": "Vote", "prompt": "Who wins?",
    "options": ["The first to time out", "The highest id"], "answer": 0,
    "point": "split-vote" }],
    "points": { "random-timeout": { "label": "Why the timeout is random" },
                "split-vote": { "label": "How a split vote ends" } } }"#;

const CHOICE: &str = r#"{ "page": "1", "questions": [{ "header": "Next step",
    "prompt": "What next?", "options": [{ "label": "Part 2" }, { "label": "A break" }] }] }"#;

/// A course whose page 1 the learner answered, with a debrief, a thread
/// and an answered choice, as an agent and a page make it.
async fn teach(root: &Path) {
    let syllabus = r#"{ "parts": [{ "title": "Why", "pages": [
        { "id": "1", "kind": "lesson", "title": "Two leaders" },
        { "id": "c1", "kind": "checkpoint", "title": "Checkpoint" } ] }] }"#;
    json_of(
        &run(
            root,
            &["course", "--title", "Raft elections", "--status", "running"],
            None,
        )
        .await,
    );
    json_of(&run(root, &["syllabus", "--file", "-"], Some(syllabus)).await);
    let page = "# Two leaders\n\nA follower that hears nothing starts an election.\n";
    let pushed = json_of(&run(root, &["page", "1", "--file", "-"], Some(page)).await);
    json_of(&run(root, &["quiz", "1", "--file", "-"], Some(QUIZ)).await);

    let client = Client::new(address(root));
    client
        .post("/api/pages/1/quizzes/1/open", &json!({ "text": "Luck." }))
        .await
        .unwrap();
    client
        .post(
            "/api/pages/1/quizzes/1/choices",
            &json!({ "choices": [{ "option": 1 }] }),
        )
        .await
        .unwrap();
    let debrief = "# Almost\n\nThe first to time out wins.\n";
    let args = [
        "debrief",
        "1",
        "1",
        "--file",
        "-",
        "--point",
        "random-timeout=partial",
    ];
    json_of(&run(root, &args, Some(debrief)).await);

    let block = pushed["blocks"][0]["id"].as_str().unwrap();
    client
        .post(
            "/api/pages/1/threads",
            &json!({ "block": block, "body": "Why an election?" }),
        )
        .await
        .unwrap();
    json_of(&run(root, &["reply", "1-1", "--body", "Nobody leads."], None).await);
    json_of(&run(root, &["choose", "--file", "-"], Some(CHOICE)).await);
    client
        .post(
            "/api/choices/choice-1",
            &json!({ "answers": [{ "picked": [1] }] }),
        )
        .await
        .unwrap();
}

/// An agent whose context was reset reads the course with `qlearn export
/// --json`. The course comes back whole after a restart of the server, and
/// with no server at all.
#[tokio::test]
async fn a_course_exported_and_reopened_after_a_restart_shows_everything() {
    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    teach(root).await;
    std::fs::create_dir_all(root.join(".learn/raft/notes")).unwrap();
    std::fs::write(root.join(".learn/raft/notes/plan.md"), "The plan.\n").unwrap();
    let page = Client::new(address(root))
        .get("/api/pages/1")
        .await
        .unwrap();

    let export = json_of(&run(root, &["export", "--json"], None).await);
    assert_eq!(export["course"]["title"], "Raft elections");
    assert_eq!(export["current"], "1");
    let pushed = &export["pages"][0];
    assert_eq!(pushed["id"], "1");
    assert!(
        pushed["source"]
            .as_str()
            .unwrap()
            .starts_with("# Two leaders")
    );
    let quiz = &pushed["quizzes"][0];
    assert_eq!(quiz["open"]["sent"], "Luck.");
    assert_eq!(quiz["choices"][0]["graded"]["ok"], false);
    assert_eq!(quiz["debrief"]["points"][0]["result"], "partial");
    assert_eq!(pushed["threads"][0]["comments"][1]["body"], "Nobody leads.");
    assert_eq!(export["choices"][0]["answers"][0]["picked"], json!([1]));
    let ids: Vec<_> = export["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|point| point["id"].clone())
        .collect();
    assert_eq!(ids, [json!("split-vote"), json!("random-timeout")]);
    assert_eq!(export["notes"], json!(["plan.md"]));

    let summary = run(root, &["export"], None).await;
    assert!(summary.status.success(), "{summary:?}");
    assert_eq!(
        stdout(&summary),
        "raft  running  Raft elections\n\
         \x20 Why\n\
         \x20   1    lesson     debriefed Two leaders  0.5/2\n\
         \x20   c1   checkpoint planned   Checkpoint\n\
         \n\
         To revisit\n\
         \x20 How a split vote ends  (page 1, missed)\n\
         \x20 Why the timeout is random  (page 1, partial)\n"
    );
    assert!(interrupt(server).await.success());

    // With no server, the store is read as it is, and nothing is written.
    let stored = std::fs::read_to_string(root.join(".learn/raft/course.json")).unwrap();
    let offline = json_of(&run(root, &["export", "--json"], None).await);
    assert_eq!(offline, export);
    let offline = run(root, &["export"], None).await;
    assert_eq!(stdout(&offline), stdout(&summary));
    assert_eq!(
        std::fs::read_to_string(root.join(".learn/raft/course.json")).unwrap(),
        stored
    );
    assert!(!root.join(".learn/server.json").exists());

    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    let again = json_of(&run(root, &["export", "--json"], None).await);
    assert_eq!(again, export);
    let client = Client::new(address(root));
    assert_eq!(client.get("/api/pages/1").await.unwrap(), page);
    let session = client.get("/api/session").await.unwrap();
    assert_eq!(session["current"], "1");
    assert_eq!(session["course"], export["course"]);
    assert!(interrupt(server).await.success());
}

#[tokio::test]
async fn an_export_with_no_server_and_several_courses_says_what_to_run() {
    let repo = build_repo();
    let root = repo.path();
    let out = run(root, &["export"], None).await;
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(stderr(&out).contains("holds no course yet"), "{out:?}");

    for slug in ["raft", "paxos"] {
        let mut server = qlearn(root, &[slug, "--no-open"]).spawn().unwrap();
        read_url(&mut server).await;
        assert!(interrupt(server).await.success());
    }
    let out = run(root, &["export", "--json"], None).await;
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(stderr(&out).contains("`qlearn <slug>`"), "{out:?}");
    assert!(
        stderr(&out).contains("several courses: paxos, raft"),
        "{out:?}"
    );
    assert!(out.stdout.is_empty(), "{out:?}");
}

/// A server that holds the worktree and does not answer may still be
/// starting, and write the store. The export refuses rather than read
/// the store under it.
#[tokio::test]
async fn an_export_while_a_server_does_not_answer_reads_nothing() {
    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["paxos", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    assert!(interrupt(server).await.success());
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    send(&server, "STOP");

    let out = run(root, &["export"], None).await;

    send(&server, "CONT");
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(
        stderr(&out).contains("holds the worktree but does not answer"),
        "{out:?}"
    );
    assert!(out.stdout.is_empty(), "{out:?}");
    assert!(interrupt(server).await.success());
}

#[tokio::test]
async fn a_course_deleted_with_yes_is_gone_and_the_others_stay() {
    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    teach(root).await;
    assert!(interrupt(server).await.success());
    let mut server = qlearn(root, &["paxos", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    assert!(interrupt(server).await.success());
    // As the server writes it: the source, then the blocks.
    std::fs::write(root.join(".learn/raft/cheatsheet.md"), "# Raft\n").unwrap();
    std::fs::write(root.join(".learn/raft/cheatsheet.json"), "{}\n").unwrap();

    let out = run(root, &["delete", "raft", "--yes"], None).await;

    assert!(out.status.success(), "{out:?}");
    let dir = root.join(".learn/raft");
    let dir = dir.display();
    assert_eq!(
        stdout(&out),
        format!(
            "This removes {dir}/, the course \"Raft elections\":\n\
             \x20 1 page, with the answers of the learner\n\
             \x20 the cheatsheet\n\
             Deleted {dir}/.\n"
        )
    );
    assert!(!root.join(".learn/raft").exists());
    assert!(root.join(".learn/paxos/course.json").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\n"
    );
    let list = run(root, &["list"], None).await;
    assert_eq!(stdout(&list), "paxos  new  paxos\n");
}

/// The server is the only writer of the store: a course goes only when no
/// server serves it. A server of another course never writes it.
#[tokio::test]
async fn a_course_that_a_server_serves_is_not_deleted() {
    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["paxos", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    assert!(interrupt(server).await.success());
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;

    let out = run(root, &["delete", "raft", "--yes"], None).await;

    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(
        stderr(&out).contains("stop the server of the course raft first"),
        "{out:?}"
    );
    assert!(stderr(&out).contains(&address(root).url()), "{out:?}");
    assert!(out.stdout.is_empty(), "{out:?}");
    assert!(root.join(".learn/raft/course.json").exists());

    let out = run(root, &["delete", "paxos", "--yes"], None).await;
    assert!(out.status.success(), "{out:?}");
    assert!(!root.join(".learn/paxos").exists());
    assert!(interrupt(server).await.success());
}

/// A terminal for the standard input of a command: what is written to the
/// file reaches the command as the learner types it.
fn terminal() -> (std::fs::File, std::os::fd::OwnedFd) {
    use std::os::fd::FromRawFd;

    let (mut ours, mut theirs) = (0, 0);
    // SAFETY: openpty writes two descriptors, which are owned from here on.
    unsafe {
        let made = libc::openpty(
            &mut ours,
            &mut theirs,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        );
        assert_eq!(made, 0, "{}", std::io::Error::last_os_error());
        (
            std::fs::File::from_raw_fd(ours),
            std::os::fd::OwnedFd::from_raw_fd(theirs),
        )
    }
}

/// The learner takes their time to answer. Meanwhile the server of
/// another course stops, and a server of this course starts: the delete
/// checks again before it removes anything.
#[tokio::test]
async fn a_course_whose_server_starts_during_the_question_is_not_deleted() {
    use std::io::Write;
    use tokio::io::AsyncReadExt;

    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    assert!(interrupt(server).await.success());
    let mut other = qlearn(root, &["paxos", "--no-open"]).spawn().unwrap();
    read_url(&mut other).await;

    let (mut keys, input) = terminal();
    let mut delete = qlearn(root, &["delete", "raft"])
        .stdin(std::process::Stdio::from(input))
        .spawn()
        .unwrap();
    let mut stdout = delete.stdout.take().unwrap();
    let mut shown = Vec::new();
    while !String::from_utf8_lossy(&shown).ends_with("[y/N] ") {
        let mut chunk = [0; 256];
        let n = tokio::time::timeout(std::time::Duration::from_secs(30), stdout.read(&mut chunk))
            .await
            .expect("qlearn delete asks in time")
            .unwrap();
        assert!(n > 0, "{}", String::from_utf8_lossy(&shown));
        shown.extend_from_slice(&chunk[..n]);
    }
    assert!(interrupt(other).await.success());
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;

    keys.write_all(b"y\n").unwrap();
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        delete.wait_with_output(),
    )
    .await
    .expect("qlearn delete ends in time")
    .unwrap();

    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(
        stderr(&out).contains("stop the server of the course raft first"),
        "{out:?}"
    );
    assert!(root.join(".learn/raft/course.json").exists());
    assert!(interrupt(server).await.success());
}

/// The address that a crash left names no server, and blocks nothing.
#[tokio::test]
async fn the_address_of_a_dead_server_does_not_block_a_delete() {
    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    assert!(interrupt(server).await.success());
    let stale = Address {
        pid: 1,
        port: 1,
        token: "gone".to_owned(),
        socket: None,
    };
    std::fs::write(
        root.join(".learn/server.json"),
        serde_json::to_string(&stale).unwrap(),
    )
    .unwrap();

    let out = run(root, &["delete", "raft", "--yes"], None).await;

    assert!(out.status.success(), "{out:?}");
    assert!(!root.join(".learn/raft").exists());
}

/// The question needs a terminal. With none, `--yes` is the only way to
/// agree, and nothing is deleted without it.
#[tokio::test]
async fn a_delete_with_no_terminal_and_no_yes_deletes_nothing() {
    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    assert!(interrupt(server).await.success());

    for input in [None, Some("y\n")] {
        let out = run(root, &["delete", "raft"], input).await;

        assert_eq!(out.status.code(), Some(1), "{out:?}");
        assert!(stdout(&out).starts_with("This removes "), "{out:?}");
        assert!(stderr(&out).contains("or add --yes"), "{out:?}");
        assert!(root.join(".learn/raft/course.json").exists());
    }
}

#[tokio::test]
async fn a_delete_of_what_is_no_course_is_refused() {
    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    assert!(interrupt(server).await.success());
    std::fs::create_dir_all(root.join(".learn/bully")).unwrap();
    std::fs::write(root.join(".learn/bully/notes.txt"), "bully\n").unwrap();

    let out = run(root, &["delete", "paxos", "--yes"], None).await;
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(
        stderr(&out).contains(".learn/ holds no course paxos. Its courses: raft"),
        "{out:?}"
    );

    let out = run(root, &["delete", "bully", "--yes"], None).await;
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(stderr(&out).contains("not a course of qlearn"), "{out:?}");
    assert_eq!(
        std::fs::read_to_string(root.join(".learn/bully/notes.txt")).unwrap(),
        "bully\n"
    );
    assert!(root.join(".learn/raft/course.json").exists());
}
