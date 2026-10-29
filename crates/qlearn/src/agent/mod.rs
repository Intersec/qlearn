//! The commands of the agent, and how they find the server of their
//! worktree.
//!
//! The commands run in another process, and maybe in a sandbox. They write
//! as the agent, through the server, so the server stays the only writer
//! of the store and the page hears of every write. None of them starts a
//! server. The server says where it listens in `.learn/server.json`, and
//! listens on a Unix socket too, for an agent whose network is its own.

pub mod address;
pub mod client;
pub mod socket;

use std::fs::{self, File, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use address::Address;
use client::{Client, Refusal};

use crate::events::HOLD;
use crate::model::{AgentAuthor, Batch, CourseChange, Session, StatusText};

/// The procedure that an agent follows, printed by `qlearn skill`.
///
/// It ships in the binary, so it names the commands of this release, and
/// the skill that an agent installs only points at it.
pub const SKILL: &str = include_str!("skill.md");

/// What a command says when no server answers. The agent must not start
/// one itself: in a sandbox, the browser could not reach it.
const NO_SERVER: &str = "no qlearn server runs on this worktree. Run `qlearn` first";

/// Where the server of a worktree says how to reach it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Home {
    /// `.learn/`, which belongs to one worktree.
    pub dir: PathBuf,
    /// Names the socket, which sits outside the worktree.
    pub worktree: String,
}

impl Home {
    /// The home of the worktree whose top level is `root`.
    pub fn of(root: &Path) -> Self {
        Self {
            dir: crate::store::Store::at(root).dir().to_path_buf(),
            worktree: crate::git::worktree_id(root),
        }
    }
}

/// How long a server has to say that it lives. It answers from memory, so
/// a server that takes longer is stopped or hung, not busy.
const PROBE: Duration = Duration::from_secs(2);

/// How long a second qlearn waits for the server of the worktree to write
/// its address, when that server is still starting.
const STARTING: Duration = Duration::from_secs(5);

/// The claim of a server on its worktree: a lock on `.learn/`, held for as
/// long as the server runs.
///
/// The system drops the lock when the process ends, a crash included. So a
/// claim never outlives its server, and two qlearn started at once cannot
/// both serve, bind the same socket and write the same `server.json`.
pub struct Claim {
    _lock: File,
}

/// Claim the worktree. None when another qlearn holds it.
pub fn claim(home: &Home) -> Result<Option<Claim>> {
    let dir = &home.dir;
    fs::create_dir_all(dir).with_context(|| format!("cannot make {}", dir.display()))?;
    let lock = File::open(dir).with_context(|| format!("cannot open {}", dir.display()))?;

    match lock.try_lock() {
        Ok(()) => Ok(Some(Claim { _lock: lock })),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => {
            Err(error).with_context(|| format!("cannot lock {}", dir.display()))
        }
    }
}

/// The server that runs on the worktree, and its session, when one
/// answers.
///
/// A file whose server does not answer was left by a crash, and is no
/// server at all.
pub async fn running(home: &Home) -> Option<(Client, Session)> {
    let client = Client::with_timeout(address::read(home)?, PROBE);
    let session = client.get("/api/session").await.ok()?;
    let session = serde_json::from_value(session).ok()?;

    Some((client, session))
}

/// The server that holds the claim on the worktree, given a few seconds to
/// write its address if it is still starting.
pub async fn running_soon(home: &Home) -> Option<(Client, Session)> {
    let deadline = tokio::time::Instant::now() + STARTING;
    loop {
        if let Some(found) = running(home).await {
            return Some(found);
        }
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The server of the worktree, or an error that says what to run.
pub async fn connect(home: &Home) -> Result<Client> {
    let (probe, _) = running(home).await.context(NO_SERVER)?;

    // The probe gives up in seconds, and a push may take longer than that.
    Ok(Client::new(probe.address().clone()))
}

/// The text of a `--file`, or standard input when it is `-`, so a page
/// needs no file of its own.
pub fn read_file(path: &str) -> Result<String> {
    if path != "-" {
        return fs::read_to_string(path).with_context(|| format!("cannot read {path}"));
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut out)
        .context("cannot read standard input")?;
    Ok(out)
}

/// The text of a `--body`, or standard input when it is `-`.
pub fn read_body(text: &str) -> Result<String> {
    match text {
        "-" => read_file("-"),
        _ => Ok(text.to_owned()),
    }
}

/// Open a thread on a block of a page, or about the whole page. `target`
/// is the page, then `#` and the block. Answers the thread.
pub async fn comment(client: &Client, target: &str, body: &str) -> Result<Value> {
    let (page, block) = match target.split_once('#') {
        Some((page, block)) => (page, Some(block)),
        None => (target, None),
    };
    if block == Some("") {
        bail!("{target}: name the block after `#`, or give the page alone");
    }
    let body = json!({ "block": block, "body": body, "author": "agent" });

    let page = segment(page);
    client
        .post(&format!("/api/pages/{page}/threads"), &body)
        .await
}

/// Write in a thread. Answers the thread.
pub async fn reply(client: &Client, thread: &str, body: &str) -> Result<Value> {
    let body = json!({ "body": body, "author": "agent" });

    let thread = segment(thread);
    client.post(&format!("/api/threads/{thread}"), &body).await
}

/// Change the course. Answers the course.
pub async fn course(client: &Client, change: CourseChange) -> Result<Value> {
    let change = CourseChange {
        author: Some(AgentAuthor::Agent),
        ..change
    };

    client
        .patch("/api/course", &serde_json::to_value(change)?)
        .await
}

/// Write the syllabus. Answers the course.
///
/// The text is read as JSON here, so a file that is not JSON names the
/// file that the agent gave: `shown`.
pub async fn syllabus(client: &Client, text: &str, shown: &str) -> Result<Value> {
    let mut body: Value = serde_json::from_str(text)
        .with_context(|| format!("{shown} is not JSON. Fix it, then run the command again"))?;
    // Any other shape gets its fault from the server, with the others.
    if let Some(fields) = body.as_object_mut() {
        fields.insert("author".to_owned(), json!("agent"));
    }

    client.put("/api/syllabus", &body).await
}

/// Push the source of a page. Answers the page, with its blocks.
///
/// The server names the file of the store in its errors. The agent edits
/// the file that it gave, so a refusal names that one: `shown` is the path
/// as the agent gave it, or `-` for standard input.
pub async fn page(client: &Client, id: &str, source: &str, shown: &str) -> Result<Value> {
    let id = segment(id);
    let pushed = client.put_text(&format!("/api/pages/{id}"), source).await;

    pushed.map_err(|error| match error.downcast::<Refusal>() {
        Ok(refusal) if refusal.code == "invalidPage" => Refusal {
            message: in_file(&refusal.message, &format!("{id}.md"), shown),
            ..refusal
        }
        .into(),
        Ok(refusal) => refusal.into(),
        Err(error) => error,
    })
}

/// Add a quiz to a page. Answers its number in the page.
///
/// The text is read as JSON here, so a file that is not JSON names the
/// file that the agent gave: `shown`.
pub async fn quiz(client: &Client, id: &str, text: &str, shown: &str) -> Result<Value> {
    let mut body: Value = serde_json::from_str(text)
        .with_context(|| format!("{shown} is not JSON. Fix it, then run the command again"))?;
    // Any other shape gets its fault from the server, with the others.
    if let Some(fields) = body.as_object_mut() {
        fields.insert("author".to_owned(), json!("agent"));
    }

    let id = segment(id);
    client
        .post(&format!("/api/pages/{id}/quizzes"), &body)
        .await
}

/// Ask the learner to choose, at the end of a page or at the top of the
/// syllabus column. Answers the id of the choice.
///
/// The text is read as JSON here, so a file that is not JSON names the
/// file that the agent gave: `shown`.
pub async fn choose(client: &Client, text: &str, shown: &str) -> Result<Value> {
    let mut body: Value = serde_json::from_str(text)
        .with_context(|| format!("{shown} is not JSON. Fix it, then run the command again"))?;
    // Any other shape gets its fault from the server, with the others.
    if let Some(fields) = body.as_object_mut() {
        fields.insert("author".to_owned(), json!("agent"));
    }

    client.post("/api/choices", &body).await
}

/// Write the debrief of the quiz `n` of a page, with the grade of each
/// point of its open question. Answers the page.
///
/// `points` holds each `--point` as the agent gave it, `<point>=<grade>`.
/// A refusal names the file that the agent gave, `shown`, and each point
/// by its `--point`.
pub async fn debrief(
    client: &Client,
    id: &str,
    n: usize,
    source: &str,
    points: &[String],
    shown: &str,
) -> Result<Value> {
    let mut grades = serde_json::Map::new();
    for given in points {
        let Some((point, grade)) = given.split_once('=') else {
            bail!("--point {given}: takes <point>=ok, <point>=partial or <point>=missed");
        };
        if grades
            .insert(point.to_owned(), Value::from(grade))
            .is_some()
        {
            bail!("--point {point} is given twice. Grade each point once");
        }
    }
    let body = json!({ "source": source, "points": grades, "author": "agent" });

    let id = segment(id);
    let written = client
        .put(&format!("/api/pages/{id}/quizzes/{n}/debrief"), &body)
        .await;

    written.map_err(|error| match error.downcast::<Refusal>() {
        Ok(refusal) if refusal.code == "invalidDebrief" => Refusal {
            message: debrief_errors(&refusal.message, shown),
            ..refusal
        }
        .into(),
        Ok(refusal) => refusal.into(),
        Err(error) => error,
    })
}

/// The points of the course, in the order in which the agent picks the
/// point of a question on an earlier lesson.
pub async fn points(client: &Client) -> Result<Value> {
    client.get("/api/points").await
}

/// The errors of a debrief in the terms of the command. The options come
/// first: the file of the agent may start with `points.` too.
fn debrief_errors(errors: &str, file: &str) -> String {
    in_file(&as_options(errors), crate::api::DEBRIEF_FILE, file)
}

/// Each line of the errors, with the `--point` of the command in place of
/// the field of the body.
fn as_options(errors: &str) -> String {
    errors
        .lines()
        .map(|line| match line.strip_prefix("points.") {
            Some(rest) => format!("--point {rest}"),
            None => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Each line of the errors, with `file` in place of the file of the store.
fn in_file(errors: &str, store: &str, file: &str) -> String {
    let prefix = format!("{store}:");
    errors
        .lines()
        .map(|line| match line.strip_prefix(&prefix) {
            Some(rest) => format!("{file}:{rest}"),
            None => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Say what the agent does, until its next wait. Answers `{ status }`.
pub async fn status(client: &Client, text: &str) -> Result<Value> {
    let body = StatusText {
        text: text.to_owned(),
        author: Some(AgentAuthor::Agent),
    };

    client
        .put("/api/status", &serde_json::to_value(body)?)
        .await
}

/// A page id as one segment of a path. The server refuses an id that is
/// not one, and the request must reach it rather than another route.
fn segment(id: &str) -> String {
    form_urlencoded::byte_serialize(id.as_bytes()).collect()
}

/// How often a wait looks for a server that stopped.
const RETRY: Duration = Duration::from_secs(1);

/// How long before its deadline a wait stops holding a request on the
/// server: a hold lasts up to 30 seconds, and its answer takes a moment.
const HELD_UNTIL: Duration = HOLD.saturating_add(Duration::from_secs(2));

/// How often a wait asks for news once it no longer holds a request.
const POLL: Duration = Duration::from_millis(500);

/// Wait for the next request of the learner.
///
/// Answers with the requests, each with its detail, the number to pass as
/// `after` next time, and the id of the server to pass as `server`. The
/// flag is false when the time ran out with nothing to report.
///
/// A server that stops does not end the wait: the learner may start it
/// again, and the agent then stays attached with no word from it. A new
/// server numbers its events again, so the wait answers `reset` then. Its
/// id differs, so an `after` of the old server never skips events of the
/// new one, even when the new one has counted past it.
pub async fn wait(
    home: &Home,
    after: Option<u64>,
    server: Option<&str>,
    timeout: Duration,
) -> Result<(Value, bool)> {
    let deadline = Instant::now() + timeout;
    let (mut client, id) = reach(home, deadline).await?;
    if server.is_some_and(|server| server != id) {
        return Ok(reset(&id));
    }
    // With no `--after`, the wait starts now. The procedure passes
    // `--after 0` to the first wait of a course, so a request made
    // before it is not lost.
    let mut after = match after {
        Some(after) => after,
        None => start(&client).await?,
    };

    loop {
        let answer = match ask(&client, after, deadline).await {
            Ok(Some(answer)) => answer,
            Ok(None) => return Ok((json!({ "events": [], "next": after, "server": id }), false)),
            Err(_) => {
                let (back, back_id) = reach(home, deadline).await?;
                if back_id != id {
                    return Ok(reset(&back_id));
                }
                client = back;
                continue;
            }
        };
        let batch: Batch = serde_json::from_value(answer).context("the server sent no events")?;
        // The server stopped and a new one took its address since `reach`.
        if batch.server != id {
            return Ok(reset(&batch.server));
        }
        if batch.reset {
            // Ahead of the server: a new one, whose requests no wait has
            // handed over yet. Behind it: those requests are gone.
            let next = if after > batch.next { 0 } else { batch.next };
            let answer = json!({ "events": [], "next": next, "reset": true, "server": id });
            return Ok((answer, true));
        }

        let asked: Vec<_> = batch
            .events
            .into_iter()
            .filter(|event| event.kind.is_request())
            .collect();
        if !asked.is_empty() {
            return Ok((
                json!({ "events": asked, "next": batch.next, "server": id }),
                true,
            ));
        }

        after = batch.next;
        if Instant::now() >= deadline {
            return Ok((json!({ "events": [], "next": after, "server": id }), false));
        }
    }
}

/// What a wait answers on a new server. No wait handed over its requests
/// yet, so the next one starts from its first event.
fn reset(server: &str) -> (Value, bool) {
    let answer = json!({ "events": [], "next": 0, "reset": true, "server": server });
    (answer, true)
}

/// The events after `after`, as the agent. None when the deadline passed
/// with none.
///
/// The server holds the request for up to 30 seconds. It takes a request
/// that the client cuts for an agent that hung up, and shows it gone 5
/// seconds later. So near the deadline, the wait polls: each poll is a
/// wait that the server answers at once.
async fn ask(client: &Client, after: u64, deadline: Instant) -> Result<Option<Value>> {
    let held = format!("/api/events?after={after}&who=agent");
    if deadline.saturating_duration_since(Instant::now()) > HELD_UNTIL {
        return client.get(&held).await.map(Some);
    }
    loop {
        let now = client.get("/api/events?who=agent").await?;
        if now["next"].as_u64() != Some(after) {
            // News, or a new server: the server answers at once.
            return client.get(&held).await.map(Some);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(None);
        }
        tokio::time::sleep(POLL.min(left)).await;
    }
}

/// The number of the last event of the server, to wait after.
async fn start(client: &Client) -> Result<u64> {
    let batch = client.get("/api/events").await?;

    batch["next"]
        .as_u64()
        .context("the server sent no event number")
}

/// The server of the worktree and its id, as soon as one answers before
/// the deadline.
async fn reach(home: &Home, deadline: Instant) -> Result<(Client, String)> {
    loop {
        if let Some((probe, session)) = running(home).await {
            return Ok((Client::new(probe.address().clone()), session.server));
        }
        if Instant::now() >= deadline {
            bail!("{NO_SERVER}");
        }
        tokio::time::sleep(RETRY).await;
    }
}

/// Write the address of this server, for the commands of the agent.
pub fn announce(home: &Home, port: u16, token: &str, socket: Option<&Path>) -> Result<Address> {
    let address = Address {
        pid: std::process::id(),
        port,
        token: token.to_owned(),
        socket: socket.map(|path| path.display().to_string()),
    };
    address::write(home, &address)?;

    Ok(address)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(dir: &tempfile::TempDir) -> Home {
        Home {
            dir: dir.path().join(".learn"),
            worktree: "c710c85489f3b4c3".to_owned(),
        }
    }

    #[test]
    fn one_claim_at_a_time_and_none_after_its_end() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);

        let first = claim(&home).unwrap().expect("the worktree is free");
        assert!(claim(&home).unwrap().is_none(), "the first holds it");

        drop(first);
        // A git child that another test forks holds a copy of the lock until
        // its exec, so the lock may outlive the drop for a moment.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while claim(&home).unwrap().is_none() {
            assert!(std::time::Instant::now() < deadline, "free again");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn a_refusal_names_the_file_that_the_agent_gave() {
        let errors = "7a.md:3: excerpt: not yet\n7a.md:9: chart: not yet";

        assert_eq!(
            in_file(errors, "7a.md", "notes/lesson.md"),
            "notes/lesson.md:3: excerpt: not yet\nnotes/lesson.md:9: chart: not yet"
        );
        assert_eq!(
            in_file(errors, "7a.md", "-").lines().next(),
            Some("-:3: excerpt: not yet")
        );
    }

    #[test]
    fn a_refusal_of_a_debrief_names_each_point_by_its_option() {
        let errors = "debrief.md:3: excerpt: not yet\n\
                      points.leader: the open question does not name this point\n\
                      points.split-vote: missing";

        assert_eq!(
            debrief_errors(errors, "notes/debrief.md"),
            "notes/debrief.md:3: excerpt: not yet\n\
             --point leader: the open question does not name this point\n\
             --point split-vote: missing"
        );
        assert_eq!(
            debrief_errors(errors, "points.md"),
            "points.md:3: excerpt: not yet\n\
             --point leader: the open question does not name this point\n\
             --point split-vote: missing"
        );
    }

    #[test]
    fn an_id_never_leaves_its_segment() {
        assert_eq!(segment("7a"), "7a");
        assert_eq!(segment("../x?y"), "..%2Fx%3Fy");
    }

    /// A server stopped with Ctrl-Z still accepts the connection, and never
    /// answers. qlearn must not wait for it as long as a command does.
    #[tokio::test]
    async fn a_server_that_never_answers_is_given_up_on_soon() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        let path = dir.path().join("hung.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let _held = tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                held.push(stream);
            }
        });
        let hung = address::Address {
            pid: 1,
            port: 1,
            token: "abc".to_owned(),
            socket: Some(path.display().to_string()),
        };
        address::write(&home, &hung).unwrap();

        let started = std::time::Instant::now();
        assert!(running(&home).await.is_none());

        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{:?}",
            started.elapsed()
        );
    }
}
