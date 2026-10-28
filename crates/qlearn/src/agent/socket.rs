//! The Unix socket of a server, for the commands of an agent.
//!
//! An agent in a sandbox runs each command in a network of its own, where
//! `127.0.0.1` is not the machine of the user, so the loopback port is out
//! of its reach. A socket file under `/tmp` is not: a sandbox shares `/tmp`
//! with the machine. The directory belongs to the user and nobody else can
//! enter it, so the socket opens to the user alone. The token is still
//! asked for, as on the port.

use std::fs;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::Home;

/// `/tmp` itself, and not `$TMPDIR`: a sandbox points `$TMPDIR` somewhere
/// of its own, and the server and the agent must name the same place.
pub const ROOT: &str = "/tmp";

/// The socket of the server of a worktree, its directory made safe first.
pub fn path_for(home: &Home) -> Result<PathBuf> {
    path_in(Path::new(ROOT), home)
}

/// As `path_for`, under another root, so a test never touches `/tmp`.
pub fn path_in(root: &Path, home: &Home) -> Result<PathBuf> {
    let uid = fs::metadata("/proc/self")
        .context("cannot read who runs this process")?
        .uid();
    let dir = root.join(format!("qlearn-{uid}"));

    private_dir(&dir, uid)?;
    Ok(dir.join(format!("{}.sock", home.worktree)))
}

/// Make the directory, or check the one that is there.
///
/// Anybody can create a name under `/tmp`. A directory that another user
/// made, or that others can enter, could hand the token to them, so it is
/// refused rather than used.
fn private_dir(dir: &Path, uid: u32) -> Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(error).with_context(|| format!("cannot make {}", dir.display()));
        }
    }

    let meta =
        fs::symlink_metadata(dir).with_context(|| format!("cannot read {}", dir.display()))?;
    if !meta.is_dir() {
        bail!("remove {}: it is not a directory", dir.display());
    }
    if meta.uid() != uid {
        bail!("remove {}: it belongs to another user", dir.display());
    }
    if meta.permissions().mode() & 0o077 != 0 {
        bail!("run `chmod 700 {}`: others can enter it", dir.display());
    }
    Ok(())
}

/// Bind the socket, readable and writable by its owner alone. The caller
/// holds the claim on the worktree.
pub fn bind(path: &Path) -> Result<tokio::net::UnixListener> {
    // A server that crashed left its socket behind, and a bind on it fails.
    // No server of this worktree runs: the caller holds the claim on it.
    clear(path);
    let listener = tokio::net::UnixListener::bind(path)
        .with_context(|| format!("cannot listen on {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("cannot restrict {}", path.display()))?;

    Ok(listener)
}

/// Remove a socket that a server left behind, so a new one can bind. Only a
/// socket: never another kind of file that happens to have the name.
pub fn clear(path: &Path) {
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_socket()) {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid() -> u32 {
        fs::metadata("/proc/self").unwrap().uid()
    }

    fn home() -> Home {
        Home {
            dir: PathBuf::from("/home/u/myproject/.learn"),
            worktree: "c710c85489f3b4c3".to_owned(),
        }
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_new_directory_is_made_for_the_user_alone() {
        let root = tempfile::tempdir().unwrap();

        let path = path_in(root.path(), &home()).unwrap();

        let dir = root.path().join(format!("qlearn-{}", uid()));
        assert_eq!(path, dir.join("c710c85489f3b4c3.sock"));
        assert_eq!(mode(&dir), 0o700);
    }

    #[test]
    fn a_directory_that_others_can_enter_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(format!("qlearn-{}", uid()));
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();

        let error = path_in(root.path(), &home()).unwrap_err();

        assert!(error.to_string().contains("chmod 700"), "{error}");
        assert_eq!(mode(&dir), 0o755, "the directory is left as it is");
    }

    #[test]
    fn a_directory_of_another_user_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("qlearn-x");
        private_dir(&dir, uid()).unwrap();

        // A test cannot make a directory that another user owns. It can
        // check the directory of this user as if it ran as another one.
        let error = private_dir(&dir, uid() + 1).unwrap_err();

        assert!(error.to_string().contains("another user"), "{error}");
    }

    #[test]
    fn a_file_in_the_way_is_refused_and_never_removed() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join(format!("qlearn-{}", uid()));
        fs::write(&file, "mine").unwrap();

        let error = path_in(root.path(), &home()).unwrap_err();
        assert!(error.to_string().contains("not a directory"), "{error}");
        clear(&file);
        assert_eq!(fs::read_to_string(&file).unwrap(), "mine", "not a socket");
    }

    #[tokio::test]
    async fn the_socket_opens_to_its_owner_alone_and_replaces_a_stale_one() {
        let root = tempfile::tempdir().unwrap();
        let path = path_in(root.path(), &home()).unwrap();

        // A crash leaves the socket file behind.
        drop(bind(&path).unwrap());
        assert!(path.exists());

        let _listener = bind(&path).unwrap();
        assert_eq!(mode(&path), 0o600);

        clear(&path);
        assert!(!path.exists());
    }
}
