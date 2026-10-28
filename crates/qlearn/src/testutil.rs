//! Test fixtures.
//!
//! Git code is tested against a real repository, never against a mock of the
//! `git` binary. A mock agrees with the code that wrote it, not with git.

use std::cell::Cell;
use std::path::Path;
use std::process::Stdio;

use tempfile::TempDir;
use tokio::process::Command;

const AUTHOR: &str = "Test Author";
const EMAIL: &str = "author@example.com";

/// A commit to create.
#[derive(Clone, Debug)]
pub struct Commit {
    subject: String,
    files: Vec<(String, String)>,
    deletes: Vec<String>,
}

pub fn commit(subject: &str) -> Commit {
    Commit {
        subject: subject.to_owned(),
        files: Vec::new(),
        deletes: Vec::new(),
    }
}

impl Commit {
    /// Write a file. The content replaces whatever was there.
    pub fn file(mut self, path: &str, content: &str) -> Self {
        self.files.push((path.to_owned(), content.to_owned()));
        self
    }

    pub fn delete(mut self, path: &str) -> Self {
        self.deletes.push(path.to_owned());
        self
    }
}

/// A real repository in a temporary directory. It is removed on drop.
pub struct Repo {
    dir: TempDir,
    clock: Cell<u32>,
}

impl Repo {
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Run a git command and return its trimmed output. It panics on failure,
    /// because a fixture that cannot be built is a broken test, not a result.
    pub async fn git(&self, args: &[&str]) -> String {
        self.git_with(args, &[]).await
    }

    /// The full hash of a revision.
    pub async fn sha(&self, rev: &str) -> String {
        self.git(&["rev-parse", rev]).await
    }

    async fn git_with(&self, args: &[&str], env: &[(&str, &str)]) -> String {
        let out = Command::new("git")
            .current_dir(self.path())
            .args(args)
            .envs(env.iter().copied())
            .stdin(Stdio::null())
            .env("LC_ALL", "C")
            // The fixture must not depend on the machine it runs on: a
            // global hook or object format would change what it builds.
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .await
            .unwrap_or_else(|e| panic!("cannot run git {args:?}: {e}"));

        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// Add one commit to the repository as it stands.
    async fn add(&self, spec: &Commit) {
        for (path, content) in &spec.files {
            let full = self.path().join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&full, content).unwrap();
        }
        for path in &spec.deletes {
            std::fs::remove_file(self.path().join(path)).unwrap();
        }

        self.git(&["add", "-A"]).await;

        // A fixed, increasing clock, one minute per commit. A test that
        // depends on the wall clock is a test that fails on a Monday. Both
        // dates are fixed, so a hash is the same on every run.
        let n = self.clock.get();
        self.clock.set(n + 1);
        let date = format!("2026-01-01T{:02}:{:02}:00+00:00", n / 60, n % 60);

        self.git_with(
            &["commit", "--quiet", "--allow-empty", "-m", &spec.subject],
            &[
                ("GIT_AUTHOR_NAME", AUTHOR),
                ("GIT_AUTHOR_EMAIL", EMAIL),
                ("GIT_AUTHOR_DATE", &date),
                ("GIT_COMMITTER_NAME", AUTHOR),
                ("GIT_COMMITTER_EMAIL", EMAIL),
                ("GIT_COMMITTER_DATE", &date),
            ],
        )
        .await;
    }
}

/// Build a repository from a list of commits, in order.
pub async fn build_repo(commits: &[Commit]) -> Repo {
    let repo = Repo {
        dir: tempfile::tempdir().unwrap(),
        clock: Cell::new(0),
    };

    repo.git(&["init", "--quiet", "--initial-branch=main", "."])
        .await;
    repo.git(&["config", "user.name", AUTHOR]).await;
    repo.git(&["config", "user.email", EMAIL]).await;
    repo.git(&["config", "commit.gpgsign", "false"]).await;
    // The fixture must not depend on the machine it runs on. A developer with
    // core.autocrlf=input would otherwise commit different bytes than one
    // without it.
    repo.git(&["config", "core.autocrlf", "false"]).await;
    repo.git(&["config", "core.safecrlf", "false"]).await;
    // The code under test reads the machine's config. Its default ignore
    // and attributes files live outside the global config, so they are
    // turned off here.
    repo.git(&["config", "core.excludesFile", "/dev/null"])
        .await;
    repo.git(&["config", "core.attributesFile", "/dev/null"])
        .await;

    for spec in commits {
        repo.add(spec).await;
    }
    repo
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_repository_has_the_commits_in_order() {
        let repo = build_repo(&[
            commit("first").file("a.txt", "a\n"),
            commit("second").file("a.txt", "b\n"),
        ])
        .await;

        let log = repo.git(&["log", "--format=%s", "--reverse"]).await;
        assert_eq!(log, "first\nsecond");
        assert_eq!(
            repo.sha("HEAD~1").await,
            repo.git(&["log", "-1", "--format=%P"]).await
        );
    }

    #[tokio::test]
    async fn the_author_and_the_dates_are_fixed() {
        let repo = build_repo(&[
            commit("first").file("a.txt", "a\n"),
            commit("second").file("a.txt", "b\n"),
        ])
        .await;

        // Seconds since the epoch, not a written date: one git prints
        // `+00:00` for UTC and another prints `Z`.
        let dates = repo
            .git(&["log", "--format=%an <%ae> %at %ct", "--reverse"])
            .await;
        assert_eq!(
            dates,
            "Test Author <author@example.com> 1767225600 1767225600\n\
             Test Author <author@example.com> 1767225660 1767225660"
        );
    }

    #[tokio::test]
    async fn a_delete_removes_the_file_from_the_tree() {
        let repo = build_repo(&[
            commit("first")
                .file("a.txt", "a\n")
                .file("src/b.txt", "b\n"),
            commit("second").delete("a.txt"),
        ])
        .await;

        let files = repo.git(&["ls-tree", "-r", "--name-only", "HEAD"]).await;
        assert_eq!(files, "src/b.txt");
    }
}
