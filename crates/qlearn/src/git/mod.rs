//! Everything the tool reads from git.

pub mod exec;

use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use sha2::{Digest, Sha256};

/// What tells two worktrees of one clone apart.
///
/// Each worktree runs a server of its own, and the commands of the agent
/// must reach that one. The id names its socket.
pub fn worktree_id(root: &Path) -> String {
    let real = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    // The bytes of the path: two names that differ outside UTF-8 differ.
    let digest = Sha256::digest(real.as_os_str().as_bytes());

    hex::encode(&digest[..8])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::exec::Git;
    use crate::testutil::{build_repo, commit};

    #[test]
    fn the_id_is_sixteen_hex_characters() {
        let dir = tempfile::tempdir().unwrap();
        let id = worktree_id(dir.path());

        assert_eq!(id.len(), 16, "{id}");
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "{id}");
    }

    #[test]
    fn two_spellings_of_one_path_have_one_id() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();

        assert_eq!(
            worktree_id(dir.path()),
            worktree_id(&dir.path().join("sub").join(".."))
        );
    }

    #[tokio::test]
    async fn two_worktrees_of_one_clone_have_two_ids() {
        let repo = build_repo(&[commit("first").file("a", "1\n")]).await;
        let elsewhere = tempfile::tempdir().unwrap();
        let tree = elsewhere.path().join("second-worktree");
        repo.git(&["worktree", "add", tree.to_str().unwrap(), "-b", "other"])
            .await;

        let one = Git::discover(repo.path()).await.unwrap();
        let two = Git::discover(&tree).await.unwrap();

        assert_ne!(
            worktree_id(one.root()),
            worktree_id(two.root()),
            "each worktree runs a server of its own"
        );
    }
}
