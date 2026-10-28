//! Every call to git goes through here.
//!
//! git runs as a child process, never as a library. The tool must agree with
//! what the developer sees in the terminal.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use tokio::process::Command;

/// How long a single git call may take. Local git reads the object database
/// and answers in milliseconds. A call that runs this long is stuck.
const TIMEOUT: Duration = Duration::from_secs(30);

/// A repository, addressed by its top level directory.
#[derive(Clone, Debug)]
pub struct Git {
    root: PathBuf,
}

impl Git {
    /// Find the repository that contains `cwd`.
    pub async fn discover(cwd: &Path) -> Result<Self> {
        let out = raw(cwd, &["rev-parse", "--show-toplevel"]).await?;
        // Only the newline is git's: a directory name may end with a space.
        let mut root = out.stdout;
        if root.last() == Some(&b'\n') {
            root.pop();
        }

        // Outside a work tree, one git fails and an older one prints nothing.
        if !out.status.success() || root.is_empty() {
            let err = String::from_utf8_lossy(&out.stderr);
            bail!(
                "{} is not in the work tree of a git repository: {}",
                cwd.display(),
                err.trim()
            );
        }
        Ok(Self {
            root: PathBuf::from(OsString::from_vec(root)),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The repository at `root`, with no git call to find it. A test of a
    /// route that reads no code needs no repository.
    #[cfg(test)]
    pub fn at(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// Run git and return its standard output as text.
    pub async fn text<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<String> {
        let out = run_in(&self.root, args).await?;
        String::from_utf8(out.stdout).context("git printed output that is not UTF-8")
    }

    /// Run git and return its standard output as bytes, for a blob that is
    /// not text, such as an image.
    pub async fn bytes<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<Vec<u8>> {
        Ok(run_in(&self.root, args).await?.stdout)
    }

    /// Run git and return whether it said yes, with its output.
    ///
    /// Some commands answer with their exit code. `check-ignore` exits with
    /// 1 when git does not ignore the path, which is an answer, not a failure.
    /// Any other code is a failure, as in `text()`.
    pub async fn output<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<(bool, String)> {
        let out = raw(&self.root, args).await?;
        if !matches!(out.status.code(), Some(0 | 1)) {
            return Err(failure(args, &out));
        }
        let text = String::from_utf8_lossy(&out.stdout).into_owned();

        Ok((out.status.success(), text))
    }

    /// Run git and return its output whatever its exit code. An error means
    /// that git did not run or did not finish.
    pub async fn run<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<std::process::Output> {
        raw(&self.root, args).await
    }
}

async fn run_in<S: AsRef<OsStr>>(dir: &Path, args: &[S]) -> Result<std::process::Output> {
    let out = raw(dir, args).await?;

    if !out.status.success() {
        return Err(failure(args, &out));
    }
    Ok(out)
}

pub(crate) fn failure<S: AsRef<OsStr>>(args: &[S], out: &std::process::Output) -> anyhow::Error {
    let err = String::from_utf8_lossy(&out.stderr);
    anyhow!("`git {}` failed: {}", describe(args), err.trim())
}

async fn raw<S: AsRef<OsStr>>(dir: &Path, args: &[S]) -> Result<std::process::Output> {
    raw_within(dir, args, TIMEOUT).await
}

async fn raw_within<S: AsRef<OsStr>>(
    dir: &Path,
    args: &[S],
    timeout: Duration,
) -> Result<std::process::Output> {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // A pager, a prompt, or a translated message would all break parsing.
        .env("GIT_PAGER", "cat")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        // In a partial clone, git fetches a missing object from the promisor
        // remote. qlearn calls no network service and changes no repository.
        .env("GIT_NO_LAZY_FETCH", "1")
        // The timeout drops the child. A stuck git must not live on after
        // its call has failed.
        .kill_on_drop(true);

    match tokio::time::timeout(timeout, cmd.output()).await {
        Ok(Ok(out)) => Ok(out),
        // A missing directory is NotFound too.
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound && !dir.is_dir() => {
            Err(anyhow!("{} does not exist", dir.display()))
        }
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(anyhow!("git is not on the PATH"))
        }
        Ok(Err(e)) => Err(e).with_context(|| format!("cannot run `git {}`", describe(args))),
        Err(_) => Err(anyhow!(
            "`git {}` did not answer in {}s",
            describe(args),
            timeout.as_secs()
        )),
    }
}

fn describe<S: AsRef<OsStr>>(args: &[S]) -> String {
    args.iter()
        .map(|a| a.as_ref().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{build_repo, commit};

    #[tokio::test]
    async fn discover_finds_the_top_level() {
        let repo = build_repo(&[commit("first").file("a.txt", "a\n")]).await;
        let git = Git::discover(repo.path()).await.unwrap();

        assert_eq!(
            git.root().canonicalize().unwrap(),
            repo.path().canonicalize().unwrap()
        );
    }

    #[tokio::test]
    async fn discover_works_from_a_subdirectory() {
        let repo = build_repo(&[commit("first").file("src/a.txt", "a\n")]).await;
        let git = Git::discover(&repo.path().join("src")).await.unwrap();

        assert_eq!(
            git.root().canonicalize().unwrap(),
            repo.path().canonicalize().unwrap()
        );
    }

    #[tokio::test]
    async fn a_directory_outside_a_work_tree_is_an_error() {
        // A temporary directory may sit inside a repository. The git
        // directory of the fixture never sits inside a work tree.
        let repo = build_repo(&[commit("first").file("a.txt", "a\n")]).await;
        let err = Git::discover(&repo.path().join(".git"))
            .await
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("is not in the work tree of a git repository"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn a_directory_that_does_not_exist_is_named() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("gone");
        let err = Git::discover(&gone).await.unwrap_err().to_string();

        assert_eq!(err, format!("{} does not exist", gone.display()));
    }

    #[tokio::test]
    async fn a_top_level_that_ends_with_a_space_is_kept_whole() {
        let repo = build_repo(&[commit("first").file("a.txt", "a\n")]).await;
        let elsewhere = tempfile::tempdir().unwrap();
        let tree = elsewhere.path().join("tree ");
        repo.git(&["worktree", "add", tree.to_str().unwrap(), "-b", "other"])
            .await;

        let git = Git::discover(&tree).await.unwrap();

        assert_eq!(
            git.root().canonicalize().unwrap(),
            tree.canonicalize().unwrap()
        );
    }

    #[tokio::test]
    async fn a_call_that_runs_out_of_time_stops_git() {
        let repo = build_repo(&[commit("first").file("a.txt", "a\n")]).await;
        let pid_file = repo.path().join("pid");
        // The shell of the alias writes the pid of git, its parent.
        let alias = format!("alias.hang=!echo $PPID > '{}'; sleep 3", pid_file.display());

        let err = raw_within(
            repo.path(),
            &["-c", &alias, "hang"],
            Duration::from_millis(500),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("did not answer"), "{err}");

        let pid = std::fs::read_to_string(&pid_file).unwrap();
        let stat = Path::new("/proc").join(pid.trim()).join("stat");
        let mut alive = true;
        for _ in 0..50 {
            // Gone, or dead and not reaped yet.
            alive = std::fs::read_to_string(&stat)
                .is_ok_and(|text| !text.rsplit(')').next().unwrap_or("").starts_with(" Z"));
            if !alive {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(!alive, "git {} still runs", pid.trim());
    }

    #[tokio::test]
    async fn a_failing_command_carries_the_message_of_git() {
        let repo = build_repo(&[commit("first").file("a.txt", "a\n")]).await;
        let git = Git::discover(repo.path()).await.unwrap();
        let err = git
            .text(&["rev-parse", "no-such-ref"])
            .await
            .unwrap_err()
            .to_string();

        assert!(err.contains("`git rev-parse no-such-ref` failed"), "{err}");
        assert!(err.contains("unknown revision"), "{err}");
    }

    #[tokio::test]
    async fn bytes_reads_a_blob_as_it_is() {
        let repo = build_repo(&[commit("first").file("a.txt", "a\r\nb")]).await;
        let git = Git::discover(repo.path()).await.unwrap();

        assert_eq!(git.bytes(&["show", "HEAD:a.txt"]).await.unwrap(), b"a\r\nb");
    }

    #[tokio::test]
    async fn output_reports_a_non_zero_exit_as_an_answer() {
        let repo = build_repo(&[commit("first").file(".gitignore", "ignored/\n")]).await;
        let git = Git::discover(repo.path()).await.unwrap();

        let (ignored, text) = git
            .output(&["check-ignore", "-q", "ignored/"])
            .await
            .unwrap();
        assert!(ignored, "{text}");

        let (ignored, _) = git.output(&["check-ignore", "-q", "kept/"]).await.unwrap();
        assert!(!ignored);
    }

    #[tokio::test]
    async fn output_reports_a_fatal_exit_as_a_failure() {
        let repo = build_repo(&[commit("first").file("a.txt", "a\n")]).await;
        let git = Git::discover(repo.path()).await.unwrap();

        let err = git
            .output(&["check-ignore", "-q", "/outside/the/repository"])
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("` failed: fatal: "), "{err}");
    }
}
