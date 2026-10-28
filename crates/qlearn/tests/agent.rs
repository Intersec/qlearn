//! The commands of the agent, as an agent runs them: the built binary,
//! against the server of a real repository.

mod common;

use std::path::Path;
use std::process::Output;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use qlearn::agent::client::Client;

use common::{address, build_repo, interrupt, qlearn, read_url, run};

/// What a command printed on standard output, as JSON.
fn json_of(out: &Output) -> Value {
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{out:?}"))
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// A client of the server, as the page is one.
fn page_of(repo: &Path) -> Client {
    Client::new(address(repo))
}

/// Wait until a `qlearn wait` is open on the server, so a request made
/// after this reaches it.
async fn until_the_agent_waits(page: &Client) {
    let waits = async {
        while page.get("/api/events").await.unwrap()["agent"] != true {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(20), waits)
        .await
        .expect("the wait of the agent never reached the server");
}

#[tokio::test]
async fn an_agent_changes_the_course_pushes_a_page_and_waits_for_next() {
    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;

    let course = json_of(&run(root, &["course", "--title", "  Raft  "], None).await);
    assert_eq!(course["slug"], "raft");
    assert_eq!(course["title"], "Raft", "the server trims the title");

    std::fs::write(root.join("page.md"), "# Two leaders\n\nOne term.\n").unwrap();
    let page = json_of(&run(root, &["page", "1", "--file", "page.md"], None).await);
    assert_eq!(page["id"], "1");
    assert_eq!(page["blocks"][0]["kind"], "text");

    // The errors name the file that the agent gave, not the one of the
    // store, so the agent knows which lines to fix.
    let wrong = "# Two leaders\n\n```excerpt\npath: a.txt\n```\n";
    std::fs::write(root.join("wrong.md"), wrong).unwrap();
    let refused = run(root, &["page", "1", "--file", "wrong.md"], None).await;
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(refused.stdout.is_empty(), "{refused:?}");
    assert!(
        stderr(&refused).starts_with("wrong.md:3: excerpt: "),
        "{}",
        stderr(&refused)
    );
    let refused = run(root, &["page", "1", "--file", "-"], Some(wrong)).await;
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(
        stderr(&refused).starts_with("-:3: excerpt: "),
        "{}",
        stderr(&refused)
    );
    let kept = page_of(root).get("/api/pages/1").await.unwrap();
    assert_eq!(
        kept["blocks"], page["blocks"],
        "a refused page stores nothing"
    );

    let status = json_of(&run(root, &["status", "Writing page 2"], None).await);
    assert_eq!(status, json!({ "status": "Writing page 2" }));

    // With no --after, the wait starts from now: a request made before it
    // is not handed over.
    let page = page_of(root);
    let earlier = page.post("/api/pages/1/next", &json!({})).await.unwrap();
    let waiting = tokio::spawn({
        let root = root.to_owned();
        async move { run(&root, &["wait", "--timeout", "20"], None).await }
    });
    until_the_agent_waits(&page).await;
    page.post("/api/pages/1/next", &json!({})).await.unwrap();

    let batch = json_of(&waiting.await.unwrap());
    let events = batch["events"].as_array().unwrap();
    assert_eq!(events.len(), 1, "{batch}");
    assert_eq!(events[0]["kind"], "next");
    assert!(
        events[0]["seq"].as_u64() > earlier["seq"].as_u64(),
        "{batch} {earlier}"
    );
    assert_eq!(
        events[0]["detail"],
        json!({ "left": { "id": "1", "title": "Two leaders" } })
    );
    let next = batch["next"].as_u64().unwrap();
    assert_eq!(batch.get("reset"), None, "{batch}");

    let after = next.to_string();
    let started = Instant::now();
    let out = run(root, &["wait", "--after", &after, "--timeout", "1"], None).await;
    assert_eq!(out.status.code(), Some(1), "the time ran out: {out:?}");
    let empty: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(empty["events"], json!([]));
    assert!(empty["next"].as_u64().unwrap() >= next, "{empty}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "a short timeout is a short wait: {:?}",
        started.elapsed()
    );

    assert!(interrupt(server).await.success());
}

/// The page reports a diagram that Mermaid refuses. The wait hands the
/// agent the source and the error, so it fixes the page with no second
/// command.
#[tokio::test]
async fn a_diagram_that_the_page_cannot_draw_reaches_the_wait() {
    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;

    let source = "# States\n\n```mermaid\ngraph LR\n  A --> \n```\n";
    let page = json_of(&run(root, &["page", "1", "--file", "-"], Some(source)).await);
    let block = page["blocks"][1]["id"].as_str().unwrap().to_owned();
    assert_eq!(page["blocks"][1]["kind"], "diagram");

    let waiting = tokio::spawn({
        let root = root.to_owned();
        async move { run(&root, &["wait", "--after", "0", "--timeout", "20"], None).await }
    });
    let client = page_of(root);
    until_the_agent_waits(&client).await;
    let error = "Parse error on line 2:\ngraph LR  A -->\nExpecting 'NODE_STRING', got 'EOF'";
    client
        .post(
            "/api/pages/1/broken",
            &json!({ "block": block, "error": error }),
        )
        .await
        .unwrap();

    let batch = json_of(&waiting.await.unwrap());
    let events = batch["events"].as_array().unwrap();
    assert_eq!(events.len(), 1, "{batch}");
    assert_eq!(events[0]["kind"], "broken");
    assert_eq!(events[0]["page"], "1");
    assert_eq!(
        events[0]["detail"],
        json!({
            "block": { "id": block, "kind": "diagram", "source": "graph LR\n  A --> \n" },
            "error": error,
        })
    );

    assert!(interrupt(server).await.success());
}

/// The agent writes the syllabus from a file or from standard input, and
/// a refused one prints every fault, so the agent fixes them in one pass.
#[tokio::test]
async fn an_agent_writes_the_syllabus_and_reads_every_fault_of_a_refused_one() {
    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;

    let syllabus = r#"{ "parts": [{ "title": "Why", "pages": [
        { "id": "1", "kind": "lesson", "title": "Two leaders" },
        { "id": "c1", "kind": "checkpoint", "title": "Checkpoint" } ] }] }"#;
    std::fs::write(root.join("syllabus.json"), syllabus).unwrap();
    let course = json_of(&run(root, &["syllabus", "--file", "syllabus.json"], None).await);
    assert_eq!(course["parts"][0]["title"], "Why");
    assert_eq!(course["parts"][0]["pages"][1]["state"], "planned");

    json_of(&run(root, &["page", "1", "--file", "-"], Some("# Two leaders\n")).await);
    let dropped = r#"{ "parts": [{ "title": "Why", "pages": [
        { "id": "c1", "kind": "lesson", "title": "" } ] }] }"#;
    let refused = run(root, &["syllabus", "--file", "-"], Some(dropped)).await;
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(refused.stdout.is_empty(), "{refused:?}");
    let errors = stderr(&refused);
    let lines: Vec<_> = errors.lines().collect();
    assert_eq!(lines.len(), 3, "{errors}");
    assert!(lines[0].starts_with("parts[0].pages[0].kind: "), "{errors}");
    assert!(
        lines[1].starts_with("parts[0].pages[0].title: "),
        "{errors}"
    );
    assert!(
        lines[2].starts_with("page 1 (\"Two leaders\") has content"),
        "{errors}"
    );

    let broken = run(root, &["syllabus", "--file", "-"], Some("{ parts")).await;
    assert_eq!(broken.status.code(), Some(1), "{broken:?}");
    assert!(
        stderr(&broken).contains("- is not JSON"),
        "{}",
        stderr(&broken)
    );

    // The refused syllabus left the one before it, with page 1 taught.
    let session = page_of(root).get("/api/session").await.unwrap();
    let pages = &session["course"]["parts"][0]["pages"];
    assert_eq!(pages[0]["state"], "taught", "{session}");
    assert_eq!(pages[1]["kind"], "checkpoint", "{session}");
    assert!(interrupt(server).await.success());
}

#[tokio::test]
async fn a_wait_stays_attached_across_a_restart_of_the_server() {
    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    let waiting = tokio::spawn({
        let root = root.to_owned();
        async move { run(&root, &["wait", "--timeout", "30"], None).await }
    });
    until_the_agent_waits(&page_of(root)).await;

    assert!(interrupt(server).await.success());
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;

    let batch = json_of(&waiting.await.unwrap());
    assert_eq!(batch["reset"], true, "{batch}");
    assert_eq!(batch["events"], json!([]));
    assert_eq!(batch["next"], 0, "no wait took a request of the new server");
    assert!(interrupt(server).await.success());
}

/// The learner may press Next on a new server before the agent waits on
/// it. The reset must not skip that request.
#[tokio::test]
async fn a_reset_hands_over_the_requests_of_the_new_server() {
    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    std::fs::write(root.join("page.md"), "# Two leaders\n\nOne term.\n").unwrap();
    for _ in 0..3 {
        json_of(&run(root, &["page", "1", "--file", "page.md"], None).await);
    }
    let out = run(root, &["wait", "--timeout", "1"], None).await;
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let old: Value = serde_json::from_slice(&out.stdout).unwrap();
    let old = old["next"].as_u64().unwrap();
    assert!(old >= 3, "{old}");
    assert!(interrupt(server).await.success());

    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    let asked = page_of(root)
        .post("/api/pages/1/next", &json!({}))
        .await
        .unwrap();

    let reset = run(root, &["wait", "--after", &old.to_string()], None).await;
    let reset = json_of(&reset);
    assert_eq!(reset["reset"], true, "{reset}");
    let next = reset["next"].as_u64().unwrap().to_string();
    let batch = json_of(&run(root, &["wait", "--after", &next, "--timeout", "10"], None).await);
    assert_eq!(batch["events"][0]["seq"], asked["seq"], "{batch}");
    assert!(interrupt(server).await.success());
}

/// A wait that runs out of time is a wait that the server answered, not
/// an agent that hung up: the agent starts the next one on its next turn,
/// which may take more than a few seconds.
#[tokio::test]
async fn an_agent_whose_wait_runs_out_of_time_stays_present() {
    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;

    let out = run(root, &["wait", "--timeout", "2"], None).await;
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    // Past the 5 seconds of an agent that hung up.
    tokio::time::sleep(Duration::from_secs(7)).await;

    let batch = page_of(root).get("/api/events").await.unwrap();
    assert_eq!(batch["agent"], true, "{batch}");
    assert_eq!(batch["left"], false, "{batch}");
    assert!(interrupt(server).await.success());
}

#[tokio::test]
async fn a_command_with_no_server_says_to_run_qlearn() {
    let repo = build_repo();
    let root = repo.path();

    for args in [
        &["status", "Reading"][..],
        &["course", "--title", "Raft"],
        &["wait", "--timeout", "1"],
    ] {
        let out = run(root, args, None).await;

        assert_eq!(out.status.code(), Some(1), "{args:?}: {out:?}");
        assert!(
            stderr(&out).contains("Run `qlearn` first"),
            "{args:?}: {out:?}"
        );
    }
    assert!(
        !root.join(".learn/server.json").exists(),
        "no command starts a server"
    );
}

#[tokio::test]
async fn the_procedure_is_printed_outside_a_repository() {
    let dir = tempfile::tempdir().unwrap();

    let out = run(dir.path(), &["skill"], None).await;

    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), qlearn::agent::SKILL);
}
