//! `.learn/server.json`: where the server of a worktree listens.
//!
//! The commands of an agent run in another process, and need the port and
//! the token. The file holds both, and only its owner can read it, so
//! another local user still cannot reach the server.

use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::Home;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Address {
    pub pid: u32,
    pub port: u16,
    pub token: String,
    /// The Unix socket, which an agent in a sandbox can reach when the port
    /// is out of its network. Absent when the server could not make one.
    #[serde(default)]
    pub socket: Option<String>,
}

impl Address {
    /// What the browser opens. The token goes once, and the cookie after.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/?t={}", self.port, self.token)
    }
}

fn path_in(home: &Home) -> PathBuf {
    home.dir.join("server.json")
}

/// Write the address, readable by its owner alone.
///
/// The write is atomic: a command that reads the file while the server
/// starts gets the whole address, or none.
pub fn write(home: &Home, address: &Address) -> Result<()> {
    let dir = &home.dir;
    fs::create_dir_all(dir).with_context(|| format!("cannot make {}", dir.display()))?;
    let path = path_in(home);
    let temp = path.with_extension("json.tmp");

    let mut handle = open_private(&temp)?;
    handle.write_all(serde_json::to_string(address)?.as_bytes())?;
    handle.sync_all()?;
    drop(handle);

    fs::rename(&temp, &path).with_context(|| format!("cannot put {} in place", path.display()))
}

fn open_private(path: &Path) -> Result<fs::File> {
    // A stale temporary file keeps the mode that it was made with.
    let _ = fs::remove_file(path);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("cannot write {}", path.display()))
}

/// The address written last. Nothing says that its server still runs: a
/// crash leaves the file behind, so a caller asks the server before it
/// trusts it.
pub fn read(home: &Home) -> Option<Address> {
    let text = fs::read_to_string(path_in(home)).ok()?;

    serde_json::from_str(&text).ok()
}

/// Remove the address, when it is still the one that this process wrote.
/// A second server that started since owns the file now.
pub fn remove(home: &Home, mine: &Address) {
    if read(home).as_ref() == Some(mine) {
        let _ = fs::remove_file(path_in(home));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;

    fn home(dir: &tempfile::TempDir) -> Home {
        Home {
            dir: dir.path().join(".learn"),
            worktree: "c710c85489f3b4c3".to_owned(),
        }
    }

    fn address(port: u16) -> Address {
        Address {
            pid: 7,
            port,
            token: "abc".to_owned(),
            socket: Some("/tmp/qlearn-1000/c710c85489f3b4c3.sock".to_owned()),
        }
    }

    #[test]
    fn an_address_is_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        write(&home, &address(4100)).unwrap();

        assert_eq!(read(&home), Some(address(4100)));
    }

    #[test]
    fn the_file_has_the_shape_of_the_contract() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        write(&home, &address(38412)).unwrap();

        let text = fs::read_to_string(dir.path().join(".learn/server.json")).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&text).unwrap(),
            json!({
                "pid": 7,
                "port": 38412,
                "token": "abc",
                "socket": "/tmp/qlearn-1000/c710c85489f3b4c3.sock",
            })
        );
    }

    #[test]
    fn only_its_owner_reads_the_token() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        write(&home, &address(4100)).unwrap();
        let mode = fs::metadata(path_in(&home)).unwrap().permissions().mode();

        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_server_removes_its_own_address_and_no_other() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        write(&home, &address(4100)).unwrap();
        write(&home, &address(4200)).unwrap();

        remove(&home, &address(4100));
        assert_eq!(read(&home), Some(address(4200)), "a later server owns it");

        remove(&home, &address(4200));
        assert_eq!(read(&home), None);
        assert!(!path_in(&home).exists());
    }

    #[test]
    fn a_missing_or_broken_file_is_no_address() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        assert_eq!(read(&home), None);

        fs::create_dir_all(&home.dir).unwrap();
        fs::write(path_in(&home), "{ not json").unwrap();
        assert_eq!(read(&home), None);
    }
}
