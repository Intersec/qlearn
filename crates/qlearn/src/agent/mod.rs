//! How a command of the agent finds the server of its worktree.
//!
//! The commands of the agent run in another process, and maybe in a
//! sandbox. The server says where it listens in `.learn/server.json`, and
//! listens on a Unix socket too, for an agent whose network is its own.

pub mod address;
pub mod client;
pub mod socket;

use std::fs::{self, File, TryLockError};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

use address::Address;
use client::Client;

use crate::model::Session;

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
        assert!(claim(&home).unwrap().is_some(), "free again");
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
