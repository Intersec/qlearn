//! The directory of the course: the worktree of a repository, or a plain
//! directory outside one.
//!
//! A subject with no code needs no repository (section 1 of the design).
//! Every command finds the directory the same way, so the agent commands
//! reach the server that `qlearn` started, from any directory below it.

use std::ffi::OsStr;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::git::exec::{Found, Git};
use crate::store::DIR;

/// Where the course lives. The code that reads the repository takes the
/// `Git` of `Repository`, so it cannot run outside one.
#[derive(Clone, Debug)]
pub enum Root {
    /// Inside the worktree of a repository: its top level directory.
    Repository(Git),
    /// Outside a repository, or with no git on the PATH. A course here
    /// shows no excerpt and no image.
    Directory(PathBuf),
}

impl Root {
    /// The root of the course that `cwd` belongs to.
    ///
    /// Inside a worktree, its top level, even when a `.learn/` sits
    /// nearer. Outside one, the nearest directory from `cwd` up that holds
    /// `.learn/`, else `cwd`.
    pub async fn discover(cwd: &Path) -> Result<Self> {
        let ceilings = std::env::var_os("GIT_CEILING_DIRECTORIES");
        let told = std::env::var_os("GIT_DIR").is_some();
        let uid = std::fs::metadata("/proc/self")
            .context("cannot read who runs this process")?
            .uid();

        Self::discover_with(cwd, ceilings.as_deref(), told, uid).await
    }

    /// `ceilings` is `GIT_CEILING_DIRECTORIES`, given to git and to the
    /// search for `.git`, so a test sets it without touching the process.
    /// `told` says that `GIT_DIR` names a repository. `uid` runs qlearn.
    async fn discover_with(
        cwd: &Path,
        ceilings: Option<&OsStr>,
        told: bool,
        uid: u32,
    ) -> Result<Self> {
        let env: Vec<(&str, &OsStr)> = ceilings
            .map(|value| ("GIT_CEILING_DIRECTORIES", value))
            .into_iter()
            .collect();

        match Git::find(cwd, &env).await? {
            Found::Repository(git) => return Ok(Self::Repository(git)),
            Found::NoGit => {}
            // git says "not a git repository" of a broken one too: it skips
            // a `.git` that it cannot read. That must not turn a repository
            // into a plain directory in silence.
            Found::NoRepository(message) => {
                if told {
                    bail!("fix GIT_DIR, or unset it: {message}");
                }
                if let Some(dot_git) = git_entry(cwd, ceilings)? {
                    bail!(
                        "fix the repository at {}: git cannot read {} ({message})",
                        dot_git.parent().unwrap_or(&dot_git).display(),
                        dot_git.display()
                    );
                }
            }
        }

        let cwd = cwd
            .canonicalize()
            .with_context(|| format!("cannot read the real path of {}", cwd.display()))?;
        let dir = cwd
            .ancestors()
            .find(|dir| dir.join(DIR).is_dir())
            .unwrap_or(&cwd)
            .to_path_buf();
        // git refuses a repository of another user. A `.learn/` of another
        // user, such as one in /tmp, would give them the course and the token.
        let store = dir.join(DIR);
        if let Ok(meta) = std::fs::metadata(&store)
            && meta.uid() != uid
        {
            bail!(
                "remove {}, or work in another directory: it belongs to another user",
                store.display()
            );
        }
        Ok(Self::Directory(dir))
    }

    /// The top level of the worktree, or the directory of the course.
    pub fn dir(&self) -> &Path {
        match self {
            Self::Repository(git) => git.root(),
            Self::Directory(dir) => dir,
        }
    }

    /// The repository, when the course has one.
    pub fn git(&self) -> Option<&Git> {
        match self {
            Self::Repository(git) => Some(git),
            Self::Directory(_) => None,
        }
    }
}

/// The nearest `.git` from `cwd` up, in the directories that git searches:
/// it stops below a ceiling directory, and at the edge of the file system.
fn git_entry(cwd: &Path, ceilings: Option<&OsStr>) -> Result<Option<PathBuf>> {
    let cwd = cwd
        .canonicalize()
        .with_context(|| format!("cannot read the real path of {}", cwd.display()))?;
    let ceilings: Vec<PathBuf> = ceilings
        .map(std::env::split_paths)
        .into_iter()
        .flatten()
        .filter(|path| path.is_absolute())
        .map(|path| path.canonicalize().unwrap_or(path))
        .collect();
    let across = std::env::var("GIT_DISCOVERY_ACROSS_FILESYSTEM")
        .is_ok_and(|value| matches!(value.to_lowercase().as_str(), "1" | "true" | "yes" | "on"));
    let device = |dir: &Path| dir.metadata().map(|meta| meta.dev()).ok();

    let mut dir = cwd.as_path();
    loop {
        let entry = dir.join(".git");
        if entry.symlink_metadata().is_ok() {
            return Ok(Some(entry));
        }
        let Some(parent) = dir.parent() else {
            return Ok(None);
        };
        if ceilings.iter().any(|ceiling| ceiling == parent) {
            return Ok(None);
        }
        if !across && device(parent) != device(dir) {
            return Ok(None);
        }
        dir = parent;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{build_repo, commit};

    /// A directory that git sees outside any repository: the ceiling stops
    /// its search at the temporary directory, whatever lies above.
    struct Outside {
        _top: tempfile::TempDir,
        ceiling: PathBuf,
        dir: PathBuf,
    }

    fn outside() -> Outside {
        let top = tempfile::tempdir().unwrap();
        let ceiling = top.path().canonicalize().unwrap();
        let dir = ceiling.join("paper");
        std::fs::create_dir(&dir).unwrap();
        Outside {
            _top: top,
            ceiling,
            dir,
        }
    }

    impl Outside {
        async fn discover(&self, cwd: &Path) -> Result<Root> {
            Root::discover_with(cwd, Some(self.ceiling.as_os_str()), false, uid()).await
        }
    }

    #[tokio::test]
    async fn outside_a_repository_the_current_directory_is_the_root() {
        let place = outside();

        let root = place.discover(&place.dir).await.unwrap();

        assert!(matches!(root, Root::Directory(_)), "{root:?}");
        assert_eq!(root.dir(), place.dir);
        assert!(root.git().is_none());
        assert!(
            !place.dir.join(".learn").exists(),
            "a discovery writes nothing"
        );
    }

    #[tokio::test]
    async fn a_directory_below_finds_the_store_above_it() {
        let place = outside();
        std::fs::create_dir(place.dir.join(".learn")).unwrap();
        let below = place.dir.join("notes/raft");
        std::fs::create_dir_all(&below).unwrap();

        let root = place.discover(&below).await.unwrap();

        assert!(matches!(root, Root::Directory(_)), "{root:?}");
        assert_eq!(root.dir(), place.dir);
    }

    fn uid() -> u32 {
        std::fs::metadata("/proc/self").unwrap().uid()
    }

    #[tokio::test]
    async fn a_store_of_another_user_is_refused() {
        // In /tmp, another user may make the `.learn/` above the course.
        let place = outside();
        std::fs::create_dir(place.dir.join(".learn")).unwrap();
        let below = place.dir.join("raft");
        std::fs::create_dir(&below).unwrap();

        for cwd in [&place.dir, &below] {
            let err = Root::discover_with(cwd, Some(place.ceiling.as_os_str()), false, uid() + 1)
                .await
                .unwrap_err()
                .to_string();

            assert_eq!(
                err,
                format!(
                    "remove {}/.learn, or work in another directory: it belongs to another user",
                    place.dir.display()
                )
            );
        }
    }

    #[tokio::test]
    async fn the_root_is_the_real_path() {
        let place = outside();
        std::fs::create_dir(place.dir.join("sub")).unwrap();

        let root = place.discover(&place.dir.join("sub/..")).await.unwrap();

        assert_eq!(root.dir(), place.dir);
    }

    #[tokio::test]
    async fn a_repository_takes_its_top_level_over_a_nearer_store() {
        let repo = build_repo(&[commit("first").file("src/a.c", "int a;\n")]).await;
        let src = repo.path().join("src");
        std::fs::create_dir(src.join(".learn")).unwrap();

        let root = Root::discover_with(&src, None, false, uid()).await.unwrap();

        let Root::Repository(git) = &root else {
            panic!("{root:?}");
        };
        assert_eq!(
            git.root().canonicalize().unwrap(),
            repo.path().canonicalize().unwrap()
        );
    }

    #[tokio::test]
    async fn a_broken_repository_stays_an_error() {
        // A `.git` file that names no directory, and an empty `.git`
        // directory: git calls both "not a git repository".
        let place = outside();
        std::fs::write(place.dir.join(".git"), "gitdir: /nonexistent/qlearn\n").unwrap();
        let other = outside();
        std::fs::create_dir(other.dir.join(".git")).unwrap();
        let below = other.dir.join("src");
        std::fs::create_dir(&below).unwrap();

        for (place, cwd) in [(&place, &place.dir), (&other, &below)] {
            let err = place.discover(cwd).await.unwrap_err().to_string();

            assert!(
                err.starts_with(&format!(
                    "fix the repository at {}: git cannot read {}/.git (fatal: not a git \
                     repository",
                    place.dir.display(),
                    place.dir.display()
                )),
                "{err}"
            );
            assert!(!place.dir.join(".learn").exists());
        }
    }

    #[tokio::test]
    async fn a_git_dir_that_names_nothing_stays_an_error() {
        let place = outside();

        let err = Root::discover_with(&place.dir, Some(place.ceiling.as_os_str()), true, uid())
            .await
            .unwrap_err()
            .to_string();

        assert!(err.starts_with("fix GIT_DIR, or unset it: "), "{err}");
    }

    #[tokio::test]
    async fn a_git_directory_is_not_a_plain_directory() {
        // Inside `.git`, git answers with another error than "not a git
        // repository", and the course must not land in the object database.
        let repo = build_repo(&[commit("first").file("a.c", "int a;\n")]).await;

        let err = Root::discover_with(&repo.path().join(".git"), None, false, uid())
            .await
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("is not in the work tree of a git repository"),
            "{err}"
        );
    }

    #[test]
    fn the_search_for_git_stops_below_a_ceiling() {
        let place = outside();
        // Above the ceiling: git never looks there.
        std::fs::create_dir(place.ceiling.join(".git")).unwrap();

        let found = git_entry(&place.dir, Some(place.ceiling.as_os_str())).unwrap();
        assert_eq!(found, None);

        let found = git_entry(&place.dir, None).unwrap();
        assert_eq!(found, Some(place.ceiling.join(".git")));
    }
}
