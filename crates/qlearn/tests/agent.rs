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

/// The agent adds a quiz under a page that it pushed. A refused quiz
/// prints every fault, and the page never gets the right option.
#[tokio::test]
async fn an_agent_adds_a_quiz_and_reads_every_fault_of_a_refused_one() {
    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    let quiz = r#"{ "choices": [{ "header": "Vote", "prompt": "Who wins?",
        "options": ["The first to time out", "The highest id"], "answer": 0,
        "point": "random-timeout" }],
        "points": { "random-timeout": { "label": "Why the timeout is random" } } }"#;

    let early = run(root, &["quiz", "1", "--file", "-"], Some(quiz)).await;
    assert_eq!(early.status.code(), Some(1), "{early:?}");
    assert!(
        stderr(&early).contains("`qlearn page 1`"),
        "{}",
        stderr(&early)
    );

    json_of(&run(root, &["page", "1", "--file", "-"], Some("# Two leaders\n")).await);
    std::fs::write(root.join("quiz.json"), quiz).unwrap();
    let number = json_of(&run(root, &["quiz", "1", "--file", "quiz.json"], None).await);
    assert_eq!(number, json!({ "page": "1", "n": 1 }));

    let wrong = r#"{ "choices": [{ "header": "Vote", "prompt": "Who wins?",
        "options": ["S1"], "answer": 1, "point": "new-point" }] }"#;
    let refused = run(root, &["quiz", "1", "--file", "-"], Some(wrong)).await;
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(refused.stdout.is_empty(), "{refused:?}");
    let errors = stderr(&refused);
    let lines: Vec<_> = errors.lines().collect();
    assert_eq!(lines.len(), 3, "{errors}");
    assert!(lines[0].starts_with("choices[0].options: "), "{errors}");
    assert!(lines[1].starts_with("choices[0].answer: "), "{errors}");
    assert!(lines[2].starts_with("choices[0].point: "), "{errors}");

    let broken = run(root, &["quiz", "1", "--file", "-"], Some("{ choices")).await;
    assert_eq!(broken.status.code(), Some(1), "{broken:?}");
    assert!(
        stderr(&broken).contains("- is not JSON"),
        "{}",
        stderr(&broken)
    );

    let page = page_of(root).get("/api/pages/1").await.unwrap();
    assert_eq!(page["quizzes"].as_array().unwrap().len(), 1, "{page}");
    assert_eq!(
        page["quizzes"][0]["choices"][0],
        json!({ "header": "Vote", "prompt": "Who wins?",
                "options": ["The first to time out", "The highest id"], "graded": null })
    );
    assert!(interrupt(server).await.success());
}

/// The learner answers a quiz, and the wait of the agent gets the answer
/// with the questions, so the agent needs no second command to debrief.
#[tokio::test]
async fn an_answer_reaches_the_wait_with_its_detail() {
    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    let quiz = r#"{ "open": { "prompt": "Why random?", "points": ["random-timeout"] },
        "choices": [{ "header": "Vote", "prompt": "Who wins?",
        "options": ["The first to time out", "The *highest* id"], "answer": 0,
        "point": "random-timeout" }],
        "points": { "random-timeout": { "label": "Why the timeout is random" } } }"#;
    json_of(&run(root, &["page", "1", "--file", "-"], Some("# Two leaders\n")).await);
    json_of(&run(root, &["quiz", "1", "--file", "-"], Some(quiz)).await);

    let waiting = tokio::spawn({
        let root = root.to_owned();
        async move { run(&root, &["wait", "--after", "0", "--timeout", "20"], None).await }
    });
    let client = page_of(root);
    until_the_agent_waits(&client).await;
    client
        .post(
            "/api/pages/1/quizzes/1/open",
            &json!({ "text": "Two followers rarely wake up at once." }),
        )
        .await
        .unwrap();
    client
        .post(
            "/api/pages/1/quizzes/1/choices",
            &json!({ "choices": [{ "option": 1, "other": "Or the oldest?" }] }),
        )
        .await
        .unwrap();

    let batch = json_of(&waiting.await.unwrap());
    let events = batch["events"].as_array().unwrap();
    assert_eq!(events.len(), 1, "{batch}");
    assert_eq!(events[0]["kind"], "answer");
    assert_eq!(events[0]["page"], "1");
    assert_eq!(events[0]["id"], "1");
    assert_eq!(
        events[0]["detail"],
        json!({
            "quiz": 1,
            "open": { "prompt": "Why random?", "points": ["random-timeout"],
                      "text": "Two followers rarely wake up at once." },
            "choices": [{ "header": "Vote", "prompt": "Who wins?",
                          "options": ["The first to time out", "The *highest* id"],
                          "picked": 1, "answer": 0, "ok": false, "other": "Or the oldest?",
                          "point": "random-timeout" }],
        })
    );

    assert!(interrupt(server).await.success());
}

/// The learner asks about a block, the wait of the agent gets the question
/// with the source of the block, and the agent answers with `qlearn reply`.
/// The agent opens a thread of its own with `qlearn comment`.
#[tokio::test]
async fn an_agent_answers_a_question_and_opens_a_thread() {
    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    let source = "# Two leaders\n\nOne vote per term.\n";
    let page = json_of(&run(root, &["page", "1", "--file", "-"], Some(source)).await);
    let block = page["blocks"][0]["id"].as_str().unwrap().to_owned();

    let waiting = tokio::spawn({
        let root = root.to_owned();
        async move { run(&root, &["wait", "--after", "0", "--timeout", "20"], None).await }
    });
    let client = page_of(root);
    until_the_agent_waits(&client).await;
    client
        .post(
            "/api/pages/1/threads",
            &json!({ "block": block, "quote": "One vote", "body": "Per server?" }),
        )
        .await
        .unwrap();

    let batch = json_of(&waiting.await.unwrap());
    let events = batch["events"].as_array().unwrap();
    assert_eq!(events.len(), 1, "{batch}");
    assert_eq!(events[0]["kind"], "ask");
    assert_eq!(events[0]["id"], "1-1");
    let detail = &events[0]["detail"];
    assert_eq!(
        detail["block"],
        json!({ "id": block, "kind": "text", "source": "# Two leaders\n\nOne vote per term." })
    );
    assert_eq!(detail["thread"]["quote"], "One vote");
    assert_eq!(detail["thread"]["comments"][0]["body"], "Per server?");
    assert_eq!(detail["earlier"], false);

    let thread = json_of(
        &run(
            root,
            &["reply", "1-1", "--body", "-"],
            Some("Per server, *per term*.\n"),
        )
        .await,
    );
    assert_eq!(thread["comments"][1]["author"], "agent");
    assert_eq!(
        thread["comments"][1]["body"],
        "<p>Per server, <em>per term</em>.</p>"
    );

    let target = format!("1#{block}");
    let opened = json_of(&run(root, &["comment", &target, "--body", "- a recap"], None).await);
    assert_eq!(opened["id"], "1-2");
    assert_eq!(opened["block"], block.as_str());
    let whole = json_of(&run(root, &["comment", "1", "--body", "We stopped here."], None).await);
    assert_eq!(whole["id"], "1-3");
    assert_eq!(whole["block"], Value::Null);

    // A refusal exits 1 and says why on standard error.
    let refused = run(root, &["reply", "1-9", "--body", "x"], None).await;
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(
        stderr(&refused).contains("no thread"),
        "{}",
        stderr(&refused)
    );
    let refused = run(root, &["comment", "1#", "--body", "x"], None).await;
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");

    let page = client.get("/api/pages/1").await.unwrap();
    let ids: Vec<_> = page["threads"]
        .as_array()
        .unwrap()
        .iter()
        .map(|thread| thread["id"].clone())
        .collect();
    assert_eq!(ids, ["1-1", "1-2", "1-3"]);
    assert!(interrupt(server).await.success());
}

/// The agent writes a finding with `qlearn finding`, then writes it again
/// with `--checked` once the team confirms it. The page gets the body as
/// HTML, and the export gives it back in Markdown.
#[tokio::test]
async fn an_agent_writes_a_finding_and_marks_it_checked() {
    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;

    let body = "- `vote()` takes no lock.\n- Two *timers* may fire.\n";
    let args = ["finding", "vote-race", "--title", "A race in the vote"];
    let written = json_of(&run(root, &[&args[..], &["--body", "-"]].concat(), Some(body)).await);
    assert_eq!(
        written,
        json!({ "id": "vote-race", "title": "A race in the vote",
                "body": "- `vote()` takes no lock.\n- Two *timers* may fire.", "checked": false })
    );
    let other = [
        "finding",
        "stale-doc",
        "--title",
        "The doc names a gone flag",
    ];
    json_of(
        &run(
            root,
            &[&other[..], &["--body", "`--fast` is gone."]].concat(),
            None,
        )
        .await,
    );
    let confirmed = json_of(
        &run(
            root,
            &[
                &args[..],
                &["--body", "The team confirmed it.", "--checked"],
            ]
            .concat(),
            None,
        )
        .await,
    );
    assert_eq!(confirmed["checked"], true);

    // A refusal prints every fault on standard error, and exits 1.
    let refused = run(
        root,
        &["finding", "Vote Race", "--title", " ", "--body", "x"],
        None,
    )
    .await;
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(refused.stdout.is_empty(), "{refused:?}");
    let errors = stderr(&refused);
    let lines: Vec<_> = errors.lines().collect();
    assert_eq!(lines.len(), 2, "{errors}");
    assert!(
        lines[0].starts_with("id: \"Vote Race\" is not a finding id"),
        "{errors}"
    );
    assert_eq!(lines[1], "title: empty", "{errors}");

    // An empty id gets the fault of its id, as any other bad id.
    let refused = run(root, &["finding", "", "--title", "t", "--body", "b"], None).await;
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let errors = stderr(&refused);
    assert!(
        errors.starts_with("id: \"\" is not a finding id"),
        "{errors}"
    );

    let stored: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join(".learn/raft/course.json")).unwrap(),
    )
    .unwrap();
    let ids: Vec<_> = stored["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| (finding["id"].clone(), finding["checked"].clone()))
        .collect();
    assert_eq!(
        ids,
        [
            (json!("vote-race"), json!(true)),
            (json!("stale-doc"), json!(false))
        ]
    );

    let session = page_of(root).get("/api/session").await.unwrap();
    assert_eq!(
        session["course"]["findings"][1]["body"],
        "<p><code>--fast</code> is gone.</p>"
    );
    let export = json_of(&run(root, &["export", "--json"], None).await);
    assert_eq!(export["course"]["findings"], stored["findings"]);
    assert!(interrupt(server).await.success());
}

/// The agent asks a choice with `qlearn choose`, the learner answers it in
/// the page, and the wait of the agent gets the labels of the picks. A
/// refused choice prints every fault.
#[tokio::test]
async fn an_agent_asks_a_choice_and_the_wait_gets_the_labels() {
    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    let choice = json!({
        "questions": [{
            "header": "Level",
            "prompt": "How well do you know Raft?",
            "options": [
                { "label": "New to it", "description": "I start from the paper" },
                { "label": "Read the paper" }
            ]
        }]
    });
    std::fs::write(root.join("choice.json"), choice.to_string()).unwrap();

    let asked = json_of(&run(root, &["choose", "--file", "choice.json"], None).await);
    assert_eq!(asked, json!({ "id": "choice-1" }));
    let client = page_of(root);
    let session = client.get("/api/session").await.unwrap();
    assert_eq!(session["choices"][0]["id"], "choice-1");
    assert_eq!(session["choices"][0]["answers"], Value::Null);

    let waiting = tokio::spawn({
        let root = root.to_owned();
        async move { run(&root, &["wait", "--after", "0", "--timeout", "20"], None).await }
    });
    until_the_agent_waits(&client).await;
    client
        .post(
            "/api/choices/choice-1",
            &json!({ "answers": [{ "picked": [1], "other": "Twice" }] }),
        )
        .await
        .unwrap();

    let batch = json_of(&waiting.await.unwrap());
    let events = batch["events"].as_array().unwrap();
    assert_eq!(events.len(), 1, "{batch}");
    assert_eq!(events[0]["kind"], "chosen");
    assert_eq!(events[0]["id"], "choice-1");
    assert_eq!(events[0]["page"], Value::Null);
    assert_eq!(
        events[0]["detail"]["answers"],
        json!([{ "header": "Level", "picked": [1], "labels": ["Read the paper"],
                 "other": "Twice" }])
    );

    // Every fault at once, each with its place in the JSON.
    let wrong = r#"{ "page": "3", "questions": [{ "header": "", "prompt": "Why?",
                     "options": [{ "label": "One" }] }] }"#;
    let refused = run(root, &["choose", "--file", "-"], Some(wrong)).await;
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(refused.stdout.is_empty(), "{refused:?}");
    let lines: Vec<String> = stderr(&refused).lines().map(str::to_owned).collect();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(
        lines[0].starts_with("page: the syllabus holds no page 3"),
        "{lines:?}"
    );
    assert_eq!(lines[1], "questions[0].header: empty");
    let refused = run(root, &["choose", "--file", "-"], Some("{ not json")).await;
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(
        stderr(&refused).contains("- is not JSON"),
        "{}",
        stderr(&refused)
    );
    assert!(interrupt(server).await.success());
}

/// A corrupt choices.json leaves the server reachable: the agent reads the
/// error of the file, not that no server runs.
#[tokio::test]
async fn a_corrupt_choices_file_reaches_the_agent_as_its_error() {
    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    std::fs::write(root.join(".learn/raft/choices.json"), "{ not json").unwrap();

    let status = run(root, &["status", "Reading"], None).await;
    assert!(status.status.success(), "{status:?}");
    let choice = r#"{ "questions": [{ "header": "Level", "prompt": "Your level?",
                      "options": [{ "label": "New" }, { "label": "Old" }] }] }"#;
    let refused = run(root, &["choose", "--file", "-"], Some(choice)).await;
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(
        stderr(&refused).contains("repair it by hand"),
        "{}",
        stderr(&refused)
    );
    assert!(interrupt(server).await.success());
}

/// The agent writes the debrief of an answered quiz with a grade for each
/// point of the open question, and reads the points in the order that it
/// picks them for a later question.
#[tokio::test]
async fn an_agent_debriefs_a_quiz_and_reads_the_points() {
    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    let quiz = r#"{ "open": { "prompt": "Why random?", "points": ["random-timeout"] },
        "choices": [{ "header": "Vote", "prompt": "Who wins?",
        "options": ["The first to time out", "The highest id"], "answer": 0,
        "point": "split-vote" }],
        "points": { "random-timeout": { "label": "Why the timeout is random" },
                    "split-vote": { "label": "How a split vote ends" } } }"#;
    json_of(&run(root, &["page", "1", "--file", "-"], Some("# Two leaders\n")).await);
    json_of(&run(root, &["quiz", "1", "--file", "-"], Some(quiz)).await);
    let debrief = |extra: &[&'static str]| {
        let mut args = vec!["debrief", "1", "1", "--file", "debrief.md"];
        args.extend_from_slice(extra);
        args
    };
    std::fs::write(
        root.join("debrief.md"),
        "# Almost\n\nOne case is missing.\n",
    )
    .unwrap();

    // The learner has not answered yet.
    let early = run(root, &debrief(&["--point", "random-timeout=ok"]), None).await;
    assert_eq!(early.status.code(), Some(1), "{early:?}");
    assert!(
        stderr(&early).contains("has not answered quiz 1"),
        "{}",
        stderr(&early)
    );

    let client = page_of(root);
    client
        .post(
            "/api/pages/1/quizzes/1/open",
            &json!({ "text": "So that two followers rarely start at once." }),
        )
        .await
        .unwrap();
    client
        .post(
            "/api/pages/1/quizzes/1/choices",
            &json!({ "choices": [{ "option": 1 }] }),
        )
        .await
        .unwrap();

    // Every fault at once: the source with its line, each point by its
    // option.
    let wrong = "# Almost\n\n> [!tip]\n> One case.\n";
    let refused = run(
        root,
        &[
            "debrief",
            "1",
            "1",
            "--file",
            "-",
            "--point",
            "leader=ok",
            "--point",
            "random-timeout=half",
        ],
        Some(wrong),
    )
    .await;
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(refused.stdout.is_empty(), "{refused:?}");
    let errors = stderr(&refused);
    let lines: Vec<_> = errors.lines().collect();
    assert_eq!(lines.len(), 3, "{errors}");
    assert!(lines[0].starts_with("-:3: callout: "), "{errors}");
    assert!(lines[1].starts_with("--point leader: "), "{errors}");
    assert!(lines[2].starts_with("--point random-timeout: "), "{errors}");
    let twice = run(
        root,
        &debrief(&[
            "--point",
            "random-timeout=ok",
            "--point",
            "random-timeout=missed",
        ]),
        None,
    )
    .await;
    assert_eq!(twice.status.code(), Some(1), "{twice:?}");
    assert!(stderr(&twice).contains("given twice"), "{}", stderr(&twice));

    let page = json_of(&run(root, &debrief(&["--point", "random-timeout=partial"]), None).await);
    assert_eq!(
        page["quizzes"][0]["debrief"]["points"],
        json!([{ "id": "random-timeout", "label": "Why the timeout is random",
                 "result": "partial" }])
    );
    let session = client.get("/api/session").await.unwrap();
    let entry = &session["course"]["parts"][0]["pages"][0];
    assert_eq!(entry["state"], "debriefed", "{session}");
    assert_eq!(
        entry["score"],
        json!({ "right": 0.5, "total": 2 }),
        "{session}"
    );

    // The missed choice first, then the partial point: both are to learn
    // again, and the choice has the older result.
    let points = json_of(&run(root, &["points"], None).await);
    let order: Vec<_> = points
        .as_array()
        .unwrap()
        .iter()
        .map(|point| (point["id"].clone(), point["results"][0]["result"].clone()))
        .collect();
    assert_eq!(
        json!(order),
        json!([["split-vote", "missed"], ["random-timeout", "partial"]])
    );

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

/// A new server numbers its events from 1 again. Once it has counted past
/// the `next` of the old one, that number names events of the new server,
/// and a wait after it would skip a request made before them. The id of
/// the server tells the wait.
#[tokio::test]
async fn a_wait_after_a_number_of_the_old_server_hands_over_the_new_requests() {
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
    let (after, id) = (old["next"].as_u64().unwrap(), old["server"].clone());
    assert!(id.as_str().is_some_and(|id| !id.is_empty()), "{old}");
    assert!(interrupt(server).await.success());

    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    let asked = page_of(root)
        .post("/api/pages/1/next", &json!({}))
        .await
        .unwrap();
    while page_of(root).get("/api/events").await.unwrap()["next"].as_u64() < Some(after) {
        json_of(&run(root, &["page", "1", "--file", "page.md"], None).await);
    }

    let id = id.as_str().unwrap();
    let after = after.to_string();
    let args = ["wait", "--after", &after, "--server", id, "--timeout", "10"];
    let reset = json_of(&run(root, &args, None).await);
    assert_eq!(reset["reset"], true, "{reset}");
    assert_eq!(reset["next"], 0, "{reset}");
    let new = reset["server"].as_str().unwrap();
    assert_ne!(new, id);

    let args = ["wait", "--after", "0", "--server", new, "--timeout", "10"];
    let batch = json_of(&run(root, &args, None).await);
    assert_eq!(batch["events"][0]["seq"], asked["seq"], "{batch}");
    assert_eq!(batch["server"], new);
    assert!(interrupt(server).await.success());
}

/// A part of a quiz tells the other tabs of the page, and is no request:
/// the wait of the agent returns on the last part only.
#[tokio::test]
async fn a_part_of_a_quiz_does_not_end_the_wait() {
    let repo = build_repo();
    let root = repo.path();
    let mut server = qlearn(root, &["raft", "--no-open"]).spawn().unwrap();
    read_url(&mut server).await;
    std::fs::write(root.join("page.md"), "# Two leaders\n\nOne term.\n").unwrap();
    json_of(&run(root, &["page", "1", "--file", "page.md"], None).await);
    let quiz = r#"{ "open": { "prompt": "Why?", "points": ["why"] },
        "choices": [{ "header": "Who", "prompt": "Who wins?",
        "options": ["The first to time out", "The highest id"], "answer": 0,
        "point": "who" }],
        "points": { "why": { "label": "Why" }, "who": { "label": "Who" } } }"#;
    json_of(&run(root, &["quiz", "1", "--file", "-"], Some(quiz)).await);
    let page = page_of(root);
    let after = page.get("/api/events").await.unwrap()["next"].to_string();

    page.post("/api/pages/1/quizzes/1/open", &json!({ "text": "Luck." }))
        .await
        .unwrap();
    let told = page
        .get(&format!("/api/events?after={after}"))
        .await
        .unwrap();
    assert_eq!(told["events"][0]["kind"], "answered", "{told}");
    let out = run(root, &["wait", "--after", &after, "--timeout", "2"], None).await;
    assert_eq!(out.status.code(), Some(1), "no request yet: {out:?}");

    page.post(
        "/api/pages/1/quizzes/1/choices",
        &json!({ "choices": [{ "option": 0 }] }),
    )
    .await
    .unwrap();
    let batch = json_of(&run(root, &["wait", "--after", &after, "--timeout", "10"], None).await);
    let kinds: Vec<_> = batch["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["kind"].clone())
        .collect();
    assert_eq!(kinds, [json!("answer")], "{batch}");
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
        &["points"],
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
