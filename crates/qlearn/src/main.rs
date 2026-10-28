//! qlearn — a course on a code base, in the browser.
//!
//! The binary is a thin shell: it reads the arguments, opens the course,
//! and serves the interface. Everything else lives in the library.

mod cli;

use std::future::IntoFuture;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Parser;
use serde_json::Value;
use time::OffsetDateTime;
use tokio::net::{TcpListener, UnixListener};
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::watch;

use qlearn::agent::address::Address;
use qlearn::agent::client::Refusal;
use qlearn::agent::{self, Claim, Home, socket};
use qlearn::api::{self, AppState, auth};
use qlearn::events::Events;
use qlearn::git::exec::Git;
use qlearn::model::{CourseChange, CourseStatus, QuizMode};
use qlearn::status::{self, Agent, Snapshot, Terminal};
use qlearn::store::{self, Store};
use qlearn::trace;

/// What a request in flight gets after the stop. The learner asked for it.
const GRACE: Duration = Duration::from_secs(3);

#[tokio::main]
async fn main() -> Result<()> {
    let cli = cli::Cli::parse();

    // Before the first git call, so the whole start is timed.
    trace::enable_from_env();
    if cli.trace {
        trace::enable();
    }

    // The procedure is the same in any directory, a repository or not.
    if let Some(cli::Command::Skill) = cli.command {
        print!("{}", agent::SKILL);
        return Ok(());
    }

    let cwd = std::env::current_dir().context("cannot read the working directory")?;
    let started = trace::start();
    let git = Git::discover(&cwd).await?;
    trace::since(started, || "find the worktree".to_owned());
    if let Some(code) = agent_command(&cli, &Home::of(git.root())).await? {
        std::process::exit(code);
    }
    let store = Store::at(git.root());

    if let Some(cli::Command::List) = cli.command {
        print!("{}", store.listing()?);
        return Ok(());
    }

    // One server per worktree. An agent that starts qlearn twice gets the
    // course that is open, not a second server and a second tab. This comes
    // before the course is opened, so a new slug creates nothing.
    let home = Home::of(git.root());
    let Some(claim) = agent::claim(&home)? else {
        return report_running(&home, cli.slug.as_deref()).await;
    };

    let started = trace::start();
    let course = store.open(cli.slug.as_deref(), OffsetDateTime::now_utc())?;
    store::exclude(&git).await?;
    trace::since(started, || format!("open the course {}", course.slug));

    if cli.verbose {
        print!("{}", qlearn::report::outline(&course));
        println!();
    }

    let current = store.current(&course)?;
    let state = |token, port| AppState::new(git, store, course, current, token, port);

    serve(cli.port, state, cli.no_open, &home, claim).await
}

/// Run a command of the agent. None when the command is another one.
///
/// Each prints what it wrote as JSON, so the agent reads the ids back.
async fn agent_command(cli: &cli::Cli, home: &Home) -> Result<Option<i32>> {
    let print = |value: &Value| {
        println!(
            "{}",
            serde_json::to_string_pretty(value).unwrap_or_default()
        );
    };

    match &cli.command {
        Some(cli::Command::Course {
            title,
            goal,
            profile,
            quiz_mode,
            status,
            follow_up,
        }) => {
            let change = CourseChange {
                title: title.clone(),
                goal: goal.clone(),
                profile: profile.clone(),
                quiz_mode: quiz_mode.map(|mode| match mode {
                    cli::QuizMode::Mixed => QuizMode::Mixed,
                    cli::QuizMode::Current => QuizMode::Current,
                }),
                status: status.map(|status| match status {
                    cli::Status::Running => CourseStatus::Running,
                    cli::Status::Finished => CourseStatus::Finished,
                }),
                follow_up: follow_up
                    .as_deref()
                    .map(|date| (date != "none").then(|| date.to_owned())),
                author: None,
            };
            let client = agent::connect(home).await?;
            print(&agent::course(&client, change).await?);
        }
        Some(cli::Command::Page { id, file }) => {
            let source = agent::read_file(file)?;
            let client = agent::connect(home).await?;
            match agent::page(&client, id, &source, file).await {
                Ok(page) => print(&page),
                // One line per error, as the server wrote it, so the agent
                // reads `<file>:<line>:` at the start of each.
                Err(error) => match error.downcast_ref::<Refusal>() {
                    Some(refusal) if refusal.code == "invalidPage" => {
                        eprintln!("{}", refusal.message);
                        return Ok(Some(1));
                    }
                    _ => return Err(error),
                },
            }
        }
        Some(cli::Command::Status { text }) => {
            let client = agent::connect(home).await?;
            print(&agent::status(&client, text).await?);
        }
        Some(cli::Command::Wait { after, timeout }) => {
            let timeout = Duration::from_secs(*timeout);
            let (events, any) = agent::wait(home, *after, timeout).await?;
            print(&events);
            return Ok(Some(if any { 0 } else { 1 }));
        }
        _ => return Ok(None),
    }
    Ok(Some(0))
}

/// Print the server that holds the worktree, as a second qlearn does.
async fn report_running(home: &Home, asked: Option<&str>) -> Result<()> {
    let Some((client, session)) = agent::running_soon(home).await else {
        bail!(
            "stop the other qlearn of this worktree, or resume it if it is \
             suspended: it holds the worktree but does not answer"
        );
    };
    let slug = &session.course.slug;

    println!(
        "qlearn already runs on this worktree, with the course {slug}, at {}",
        client.address().url()
    );
    if asked.is_some_and(|asked| asked != slug) {
        println!("To open another course, stop that server first, with Ctrl-C.");
    }
    Ok(())
}

/// The claim is a parameter, so it is dropped after `Leftovers`: no other
/// server may start before the files of this one are gone.
///
/// `state` makes the state of the server once the token and the port are
/// known.
async fn serve(
    port: u16,
    state: impl FnOnce(String, u16) -> AppState,
    no_open: bool,
    home: &Home,
    _claim: Claim,
) -> Result<()> {
    // First, before any file is made: a signal that came before the
    // handlers would kill the process and leave its files behind.
    let stopped = stop_signals()?;
    let mut leftovers = Leftovers {
        home,
        address: None,
        socket: None,
    };

    // The loopback address only. Never another interface.
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .context("cannot listen on the loopback address")?;
    let addr = listener.local_addr()?;
    let token = auth::new_token();
    let state = state(token.clone(), addr.port());
    let events = state.events.clone();
    let app = api::app(state);

    // The way in for an agent in a sandbox. The course works without it,
    // so a socket that cannot be made only gives a warning.
    let unix = match unix_listener(home) {
        Ok((listener, path)) => {
            leftovers.socket = Some(path);
            Some(listener)
        }
        Err(error) => {
            eprintln!("qlearn: an agent in a sandbox will not reach this server: {error:#}");
            None
        }
    };

    // The commands of an agent find the server here. The course still
    // works without them, so a file that cannot be written only gives a
    // warning.
    match agent::announce(home, addr.port(), &token, leftovers.socket.as_deref()) {
        Ok(address) => leftovers.address = Some(address),
        Err(error) => {
            eprintln!("qlearn: the commands of an agent will not find this server: {error:#}");
        }
    }

    // Printed after the address file and the socket, so whoever reads the
    // URL can reach the server at once.
    let url = format!("http://{addr}/?t={token}");
    println!("qlearn is at {url}");
    println!("Press Ctrl-C to stop.");
    if !no_open {
        open_browser(&url);
    }

    let on_stop = |mut stopped: watch::Receiver<bool>| async move {
        let _ = stopped.wait_for(|stop| *stop).await;
    };
    let watcher = tokio::spawn(watch_parties(events.clone(), url));
    // On a stop, the waits on the events answer at once, so the requests in
    // flight end.
    tokio::spawn({
        let stopped = on_stop(stopped.clone());
        async move {
            stopped.await;
            events.close();
        }
    });

    let on_port = axum::serve(listener, app.clone())
        .with_graceful_shutdown(on_stop(stopped.clone()))
        .into_future();
    let on_unix = async {
        match unix {
            Some(listener) => {
                axum::serve(listener, app)
                    .with_graceful_shutdown(on_stop(stopped.clone()))
                    .await
            }
            None => Ok(()),
        }
    };
    // A request that still hangs after the stop gets a few seconds, no more.
    let deadline = async {
        on_stop(stopped.clone()).await;
        tokio::time::sleep(GRACE).await;
    };
    let served = tokio::select! {
        (port, unix) = async { tokio::join!(on_port, on_unix) } => {
            port.and(unix).context("the server stopped with an error")
        }
        () = deadline => Ok(()),
    };

    // Awaited, so no draw comes after the line is cleared.
    watcher.abort();
    let _ = watcher.await;
    status::clear();
    served
}

/// Say in the terminal when the page of the course opens, closes, and
/// comes back, and when the agent connects and leaves. The server goes
/// on without either one. On a terminal, a status line under the messages
/// says how things stand now.
async fn watch_parties(events: Arc<Events>, url: String) {
    let mut terminal = Terminal::new();
    let mut page = None;
    let mut agent = false;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tick.tick().await;
        let state = Snapshot::of(&events);
        match (page, state.page) {
            (Some(true), Some(false)) => {
                terminal.say(&format!(
                    "The course page closed. Go to {url} to open it again."
                ));
                terminal.say("Press Ctrl-C to stop.");
            }
            (None, Some(true)) => terminal.say("The course page is open."),
            (Some(false), Some(true)) => terminal.say("The course page is open again."),
            _ => {}
        }
        page = state.page;

        // From the snapshot, so the message and the line agree.
        let listens = matches!(state.agent, Agent::Connected | Agent::Working);
        if listens && !agent {
            terminal.say("The agent is connected.");
        } else if agent && !listens {
            terminal.say("The agent left. Ask it to run `qlearn wait` to connect it again.");
        }
        agent = listens;

        terminal.draw(&state);
    }
}

/// Ctrl-C, a `kill`, or a terminal that closes: each stops the server, and
/// each leaves the cleanup to run.
fn stop_signals() -> Result<watch::Receiver<bool>> {
    let catch = |kind| signal(kind).context("cannot catch the signals that stop the server");
    let mut interrupt = catch(SignalKind::interrupt())?;
    let mut terminate = catch(SignalKind::terminate())?;
    let mut hangup = catch(SignalKind::hangup())?;

    let (stopping, stopped) = watch::channel(false);
    tokio::spawn(async move {
        tokio::select! {
            _ = interrupt.recv() => {}
            _ = terminate.recv() => {}
            _ = hangup.recv() => {}
        }
        let _ = stopping.send(true);
    });
    Ok(stopped)
}

/// What a server leaves on disk. It is removed when `serve` returns, by
/// any path, an early error included.
struct Leftovers<'a> {
    home: &'a Home,
    address: Option<Address>,
    socket: Option<PathBuf>,
}

impl Drop for Leftovers<'_> {
    fn drop(&mut self) {
        if let Some(address) = &self.address {
            agent::address::remove(self.home, address);
        }
        if let Some(path) = &self.socket {
            socket::clear(path);
        }
    }
}

/// A Unix socket for the server, in the private directory of the user.
fn unix_listener(home: &Home) -> Result<(UnixListener, PathBuf)> {
    let path = socket::path_for(home)?;

    Ok((socket::bind(&path)?, path))
}

/// Show the course in the browser of the user.
///
/// Linux is the only target, so `xdg-open` is what a desktop provides. A
/// failure is not worth stopping for: the URL is printed above it.
fn open_browser(url: &str) {
    let started = std::process::Command::new("xdg-open")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    if started.is_err() {
        eprintln!("qlearn: no browser opened. Open the address above by hand.");
    }
}
