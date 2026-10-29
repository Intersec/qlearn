//! A file read in the object database, at a commit, submodules included.
//!
//! An excerpt shows code that the repository holds, not the state of a file
//! on disk. The path is walked one directory at a time. When the tree holds
//! a submodule (mode `160000`), the walk goes on in the repository of the
//! submodule, at the commit that the tree names. The commit that the
//! submodule has checked out does not matter.
//!
//! git keeps the repository of a submodule in `modules/<name>` under the git
//! directory of the worktree that checked it out: `.git/modules/<name>` in
//! the main worktree, `.git/worktrees/<id>/modules/<name>` in a linked one.
//! `git rev-parse --git-path modules/<name>` finds both. A submodule cloned
//! in place keeps its `.git` in its own worktree, which is the fallback.
//!
//! That repository names its worktree in `core.worktree`, and git dies when
//! the directory is gone, as after `git rm` of the submodule. No read here
//! needs a worktree, so each call names the top level directory instead.

use std::ffi::OsString;
use std::fmt;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use super::exec::Git;

/// A file of the object database.
#[derive(Debug, PartialEq, Eq)]
pub struct File {
    pub bytes: Vec<u8>,
    /// "" for the main repository, or the path of the submodule that holds
    /// the file, from the top level directory.
    pub repo: String,
    /// The full hash of the commit, in the repository that holds the file.
    pub commit: String,
}

/// Why a file cannot be read. Each one becomes an error of a page check.
#[derive(Debug)]
pub enum ReadError {
    /// The rev names no commit.
    BadRev { rev: String },
    /// No file has this path at the commit.
    NotInTree { path: String, commit: String },
    /// The path names a directory or a submodule.
    Directory { path: String, commit: String },
    /// The blob of a symbolic link holds its target, not code.
    Symlink { path: String, commit: String },
    /// The blob is larger than the caller takes. It is never read.
    TooLarge {
        path: String,
        commit: String,
        size: u64,
        limit: u64,
    },
    /// The tree names a commit that was never fetched in the submodule.
    MissingCommit {
        submodule: String,
        commit: String,
        /// The command that fetches it, from the top level directory.
        fetch: String,
    },
    /// The submodule has no repository in this worktree.
    NotCheckedOut {
        submodule: String,
        /// The command that clones it.
        update: String,
    },
    /// git failed for another reason.
    Git(anyhow::Error),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadRev { rev } => write!(f, "{rev} is not a commit"),
            Self::NotInTree { path, commit } => {
                write!(f, "{path} is not a file at commit {}", short(commit))
            }
            Self::Directory { path, commit } => {
                write!(f, "{path} is a directory at commit {}", short(commit))
            }
            Self::Symlink { path, commit } => {
                write!(f, "{path} is a symbolic link at commit {}", short(commit))
            }
            Self::TooLarge {
                path,
                commit,
                size,
                limit,
            } => write!(
                f,
                "{path} holds {size} bytes at commit {}, more than {limit}",
                short(commit)
            ),
            Self::MissingCommit {
                submodule,
                commit,
                fetch,
            } => write!(
                f,
                "the submodule {submodule} does not hold commit {}. Run {fetch}",
                short(commit)
            ),
            Self::NotCheckedOut { submodule, update } => {
                write!(
                    f,
                    "the submodule {submodule} is not checked out. Run {update}"
                )
            }
            Self::Git(e) => write!(f, "{e:#}"),
        }
    }
}

impl std::error::Error for ReadError {}

impl From<anyhow::Error> for ReadError {
    fn from(e: anyhow::Error) -> Self {
        Self::Git(e)
    }
}

/// Resolve a rev to the full hash of a commit.
///
/// The caller resolves the rev once and keeps the hash, so the code stays
/// the one that the lesson taught after the branch moves.
pub async fn resolve(git: &Git, rev: &str) -> Result<String, ReadError> {
    let bad = || ReadError::BadRev {
        rev: rev.to_owned(),
    };
    // git would read a leading dash as an option.
    if rev.is_empty() || rev.starts_with('-') {
        return Err(bad());
    }
    // Two steps: `:/<text>^{commit}` would search for the suffix too.
    let Some(object) = rev_parse(git, rev).await? else {
        return Err(bad());
    };
    let peeled = format!("{object}^{{commit}}");
    rev_parse(git, &peeled).await?.ok_or_else(bad)
}

/// The hash that `rev` names, or None if it names no object.
async fn rev_parse(git: &Git, rev: &str) -> Result<Option<String>, ReadError> {
    let args = ["rev-parse", "--verify", "--quiet", rev];
    let out = git.run(&args).await?;
    match out.status.code() {
        Some(0) => Ok(Some(
            String::from_utf8_lossy(&out.stdout).trim_end().to_owned(),
        )),
        // `--quiet` silences a bad rev. HEAD@{99} exits with 128, as a
        // broken repository does, but the latter prints why.
        Some(1) => Ok(None),
        Some(128) if out.stderr.is_empty() => Ok(None),
        _ => Err(super::exec::failure(&args, &out).into()),
    }
}

/// Read the file at `path` in the tree of `commit`, a full hash.
pub async fn read_file(git: &Git, commit: &str, path: &str) -> Result<File, ReadError> {
    read_file_within(git, commit, path, None).await
}

/// Read the file at `path` in the tree of `commit`, and refuse a file of
/// more than `limit` bytes before its bytes reach memory.
pub async fn read_file_within(
    git: &Git,
    commit: &str,
    path: &str,
    limit: Option<u64>,
) -> Result<File, ReadError> {
    let parts: Vec<&str> = path.split('/').collect();
    let (name, dirs) = parts.split_last().expect("split yields one part at least");
    let not_in_tree = || ReadError::NotInTree {
        path: path.to_owned(),
        commit: commit.to_owned(),
    };

    let mut repo = Repo {
        git_dir: None,
        worktree: git.root().to_path_buf(),
        path: String::new(),
        outermost: None,
        commit: commit.to_owned(),
    };
    let mut tree = commit.to_owned();
    // The index in `parts` of the first name inside `repo`.
    let mut start = 0;
    let mut gitmodules = None;

    for (i, dir) in dirs.iter().enumerate() {
        let entries = repo.list(git, &tree).await?;
        if i == start {
            gitmodules = find(&entries, ".gitmodules")
                .filter(|e| e.kind == "blob")
                .map(|e| e.hash.clone());
        }
        let entry = find(&entries, dir).ok_or_else(not_in_tree)?;

        match entry.mode.as_str() {
            "040000" => tree = entry.hash.clone(),
            "160000" => {
                let inner = parts[start..=i].join("/");
                repo = repo
                    .enter(git, &inner, &entry.hash, gitmodules.as_deref())
                    .await?;
                tree = entry.hash.clone();
                start = i + 1;
            }
            _ => return Err(not_in_tree()),
        }
    }

    let entries = repo.list(git, &tree).await?;
    let entry = find(&entries, name).ok_or_else(not_in_tree)?;
    let (path, commit) = (path.to_owned(), commit.to_owned());
    match entry.mode.as_str() {
        "040000" | "160000" => Err(ReadError::Directory { path, commit }),
        "120000" => Err(ReadError::Symlink { path, commit }),
        _ => {
            if let Some(limit) = limit {
                let out = repo.git(git, &["cat-file", "-s", &entry.hash]).await?;
                let size = String::from_utf8_lossy(&out).trim().parse::<u64>();
                let size = size.map_err(|e| anyhow::anyhow!("git cat-file -s: {e}"))?;
                if size > limit {
                    return Err(ReadError::TooLarge {
                        path,
                        commit,
                        size,
                        limit,
                    });
                }
            }
            let bytes = repo.git(git, &["cat-file", "blob", &entry.hash]).await?;
            Ok(File {
                bytes,
                repo: repo.path,
                commit: repo.commit,
            })
        }
    }
}

/// The repository that the walk is in.
struct Repo {
    /// None for the main repository, which git finds from its worktree.
    git_dir: Option<PathBuf>,
    /// Where the worktree of the repository would be. It may not exist.
    worktree: PathBuf,
    /// "" for the main repository, or the path from the top level directory.
    path: String,
    /// The path of the submodule of the main repository that holds this
    /// one. `git submodule update` reaches a nested one through it.
    outermost: Option<String>,
    commit: String,
}

impl Repo {
    async fn git(&self, git: &Git, args: &[&str]) -> anyhow::Result<Vec<u8>> {
        git.bytes(&self.args(git, args)).await
    }

    fn args(&self, git: &Git, args: &[&str]) -> Vec<OsString> {
        args_in(git, self.git_dir.as_deref(), args)
    }

    /// The whole tree, matched later by exact name: a pathspec would read a
    /// `*` in a name as a glob.
    async fn list(&self, git: &Git, tree: &str) -> anyhow::Result<Vec<Entry>> {
        let out = self.git(git, &["ls-tree", "-z", tree]).await?;
        Ok(out.split(|&b| b == 0).filter_map(Entry::parse).collect())
    }

    /// Go on in the submodule at `inner`, a path inside this repository, at
    /// `commit`.
    async fn enter(
        self,
        git: &Git,
        inner: &str,
        commit: &str,
        gitmodules: Option<&str>,
    ) -> Result<Repo, ReadError> {
        let path = if self.path.is_empty() {
            inner.to_owned()
        } else {
            format!("{}/{inner}", self.path)
        };
        let Some(git_dir) = self.find_git_dir(git, inner, gitmodules).await? else {
            let update = match &self.outermost {
                Some(outer) => format!("git submodule update --init --recursive {outer}"),
                None => format!("git submodule update --init {path}"),
            };
            return Err(ReadError::NotCheckedOut {
                update,
                submodule: path,
            });
        };
        let found = git
            .output(&args_in(git, Some(&git_dir), &["cat-file", "-e", commit]))
            .await?
            .0;
        if !found {
            // A fetch in the repository of the submodule. `git submodule
            // update` would fetch the commit of the index only, skip a
            // submodule that is not initialized, need its current path, and
            // move its checkout.
            let dir = git_dir.strip_prefix(git.root()).unwrap_or(&git_dir);
            let fetch = format!(
                "git --git-dir {} --work-tree . fetch --no-recurse-submodules",
                dir.display()
            );
            return Err(ReadError::MissingCommit {
                fetch,
                submodule: path,
                commit: commit.to_owned(),
            });
        }

        Ok(Repo {
            git_dir: Some(git_dir),
            worktree: self.worktree.join(inner),
            outermost: Some(self.outermost.unwrap_or_else(|| path.clone())),
            path,
            commit: commit.to_owned(),
        })
    }

    /// The git directory of the submodule at `inner`, if this worktree has
    /// one.
    async fn find_git_dir(
        &self,
        git: &Git,
        inner: &str,
        gitmodules: Option<&str>,
    ) -> anyhow::Result<Option<PathBuf>> {
        let name = match gitmodules {
            Some(blob) => self.name_of(git, blob, inner).await?,
            None => None,
        };
        // git names a submodule after its path unless .gitmodules says
        // otherwise.
        let name = name.as_deref().unwrap_or(inner);

        let module = format!("modules/{name}");
        let mut out = self.git(git, &["rev-parse", "--git-path", &module]).await?;
        if out.last() == Some(&b'\n') {
            out.pop();
        }
        // A relative answer is relative to the directory that git ran in.
        let dir = git.root().join(OsString::from_vec(out));
        if dir.is_dir() {
            return Ok(Some(dir));
        }

        let embedded = self.worktree.join(inner).join(".git");
        Ok(embedded.exists().then_some(embedded))
    }

    /// The name that `.gitmodules` gives to the submodule at `inner`.
    async fn name_of(&self, git: &Git, blob: &str, inner: &str) -> anyhow::Result<Option<String>> {
        let args = self.args(
            git,
            &[
                "config",
                "--blob",
                blob,
                "-z",
                "--get-regexp",
                r"^submodule\..*\.path$",
            ],
        );
        // Exit 1: no submodule has a path.
        let (_, out) = git.output(&args).await?;

        let name = out.split('\0').find_map(|record| {
            let (key, value) = record.split_once('\n')?;
            let name = key.strip_prefix("submodule.")?.strip_suffix(".path")?;
            (value == inner).then_some(name)
        });
        // git refuses such a name too: `modules/<name>` would leave the git
        // directory.
        Ok(name
            .filter(|name| !name.split('/').any(|p| p == ".."))
            .map(str::to_owned))
    }
}

/// An entry of `git ls-tree -z`.
struct Entry {
    mode: String,
    kind: String,
    hash: String,
    name: Vec<u8>,
}

impl Entry {
    /// `<mode> SP <type> SP <hash> TAB <name>`.
    fn parse(raw: &[u8]) -> Option<Self> {
        let tab = raw.iter().position(|&b| b == b'\t')?;
        let meta = std::str::from_utf8(&raw[..tab]).ok()?;
        let mut words = meta.split(' ');
        Some(Self {
            mode: words.next()?.to_owned(),
            kind: words.next()?.to_owned(),
            hash: words.next()?.to_owned(),
            name: raw[tab + 1..].to_vec(),
        })
    }
}

/// The arguments of a call in the repository at `git_dir`.
fn args_in(git: &Git, git_dir: Option<&Path>, args: &[&str]) -> Vec<OsString> {
    let mut all = Vec::new();
    if let Some(dir) = git_dir {
        all.push(OsString::from("--git-dir"));
        all.push(dir.into());
        // Its `core.worktree` may name a directory that is gone.
        all.push(OsString::from("--work-tree"));
        all.push(git.root().into());
    }
    all.extend(args.iter().map(OsString::from));
    all
}

fn find<'a>(entries: &'a [Entry], name: &str) -> Option<&'a Entry> {
    entries.iter().find(|e| e.name == name.as_bytes())
}

fn short(commit: &str) -> &str {
    commit.get(..12).unwrap_or(commit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{Repo as Fixture, build_repo, clone_repo, commit};

    async fn read(repo: &Fixture, rev: &str, path: &str) -> Result<File, ReadError> {
        let git = Git::discover(repo.path()).await.unwrap();
        let commit = resolve(&git, rev).await?;
        read_file(&git, &commit, path).await
    }

    /// A superproject with `lib` at `vendor/lib`.
    async fn with_lib() -> (Fixture, Fixture) {
        let lib = build_repo(&[commit("lib").file("src/lib.c", "int lib;\n")]).await;
        let top = build_repo(&[commit("add lib")
            .file("main.c", "int main;\n")
            .submodule("vendor/lib", &lib)])
        .await;
        (lib, top)
    }

    /// `with_lib`, then a commit of `lib` that the submodule never fetched,
    /// which the gitlink of a new commit of `top` names.
    async fn with_unfetched_lib() -> (Fixture, Fixture, String) {
        let (lib, top) = with_lib().await;
        lib.commit(&commit("two").file("src/lib.c", "int two;\n"))
            .await;
        let two = lib.sha("HEAD").await;
        top.commit(&commit("bump lib").gitlink("vendor/lib", &two))
            .await;
        (lib, top, two)
    }

    /// Run the command that a `MissingCommit` names, from the top level.
    async fn run_the_fetch(top: &Fixture, err: &ReadError) {
        let ReadError::MissingCommit { fetch, .. } = err else {
            panic!("not a missing commit: {err:?}");
        };
        let args: Vec<&str> = fetch.strip_prefix("git ").unwrap().split(' ').collect();
        top.git(&args).await;
    }

    #[tokio::test]
    async fn a_file_at_head() {
        let repo = build_repo(&[commit("first").file("src/a.c", "int a;\n")]).await;

        let file = read(&repo, "HEAD", "src/a.c").await.unwrap();

        assert_eq!(
            file,
            File {
                bytes: b"int a;\n".to_vec(),
                repo: String::new(),
                commit: repo.sha("HEAD").await,
            }
        );
    }

    #[tokio::test]
    async fn a_file_at_an_older_commit() {
        let repo = build_repo(&[
            commit("first").file("a.c", "old\n"),
            commit("second").file("a.c", "new\n"),
        ])
        .await;

        let file = read(&repo, "HEAD~1", "a.c").await.unwrap();

        assert_eq!(file.bytes, b"old\n");
        assert_eq!(file.commit, repo.sha("HEAD~1").await);
    }

    #[tokio::test]
    async fn a_file_over_the_limit_is_refused_and_one_at_the_limit_is_read() {
        let repo = build_repo(&[commit("first").file("a.c", "1234\n")]).await;
        let git = Git::discover(repo.path()).await.unwrap();
        let head = repo.sha("HEAD").await;

        let read = read_file_within(&git, &head, "a.c", Some(5)).await;
        let refused = read_file_within(&git, &head, "a.c", Some(4))
            .await
            .unwrap_err();

        assert_eq!(read.unwrap().bytes, b"1234\n");
        assert!(
            matches!(
                refused,
                ReadError::TooLarge {
                    size: 5,
                    limit: 4,
                    ..
                }
            ),
            "{refused:?}"
        );
        assert_eq!(
            refused.to_string(),
            format!("a.c holds 5 bytes at commit {}, more than 4", &head[..12])
        );
    }

    #[tokio::test]
    async fn a_name_with_pathspec_magic_is_read_as_it_is() {
        let repo = build_repo(&[commit("first").file("a*.c", "star\n").file("ab.c", "ab\n")]).await;

        assert_eq!(read(&repo, "HEAD", "a*.c").await.unwrap().bytes, b"star\n");
    }

    #[tokio::test]
    async fn a_file_in_a_submodule_is_read_at_the_commit_of_the_tree() {
        let lib = build_repo(&[
            commit("one").file("src/lib.c", "one\n"),
            commit("two").file("src/lib.c", "two\n"),
        ])
        .await;
        let top = build_repo(&[commit("add lib").submodule("vendor/lib", &lib)]).await;
        // The checkout of the submodule leaves the commit of the tree.
        top.git(&["-C", "vendor/lib", "checkout", "--quiet", "HEAD~1"])
            .await;
        let on_disk = std::fs::read_to_string(top.path().join("vendor/lib/src/lib.c")).unwrap();
        assert_eq!(on_disk, "one\n");

        let file = read(&top, "HEAD", "vendor/lib/src/lib.c").await.unwrap();

        assert_eq!(
            file,
            File {
                bytes: b"two\n".to_vec(),
                repo: "vendor/lib".to_owned(),
                commit: lib.sha("HEAD").await,
            }
        );
    }

    #[tokio::test]
    async fn a_submodule_is_found_by_the_name_that_gitmodules_gives_it() {
        let (lib, top) = with_lib().await;
        // The name stays when the submodule moves.
        std::fs::create_dir(top.path().join("third_party")).unwrap();
        top.git(&["mv", "vendor/lib", "third_party/lib"]).await;
        top.commit(&commit("move lib")).await;
        let name = top
            .git(&["config", "-f", ".gitmodules", "submodule.vendor/lib.path"])
            .await;
        assert_eq!(name, "third_party/lib");
        // Only the name leads to the repository once the worktree of the
        // submodule is gone.
        top.git(&["submodule", "deinit", "--quiet", "third_party/lib"])
            .await;
        assert!(!top.path().join("third_party/lib/.git").exists());

        let file = read(&top, "HEAD", "third_party/lib/src/lib.c")
            .await
            .unwrap();

        assert_eq!(file.repo, "third_party/lib");
        assert_eq!(file.commit, lib.sha("HEAD").await);
    }

    #[tokio::test]
    async fn a_submodule_cloned_in_place_is_read_in_its_own_git_directory() {
        let lib = build_repo(&[commit("lib").file("lib.c", "int lib;\n")]).await;
        let top = build_repo(&[commit("first").file("main.c", "int main;\n")]).await;
        let source = lib.path().to_str().unwrap();
        top.git_clone(&["clone", "--quiet", source, "vendor/lib"])
            .await;
        top.git_clone(&["submodule", "add", "--quiet", source, "vendor/lib"])
            .await;
        top.commit(&commit("add lib")).await;
        assert!(top.path().join("vendor/lib/.git").is_dir());

        let file = read(&top, "HEAD", "vendor/lib/lib.c").await.unwrap();

        assert_eq!(file.bytes, b"int lib;\n");
    }

    #[tokio::test]
    async fn a_file_in_a_nested_submodule() {
        let inner = build_repo(&[commit("inner").file("x.c", "int x;\n")]).await;
        let outer = build_repo(&[commit("outer").submodule("deps/inner", &inner)]).await;
        let top = build_repo(&[commit("add outer").submodule("vendor/outer", &outer)]).await;

        let file = read(&top, "HEAD", "vendor/outer/deps/inner/x.c")
            .await
            .unwrap();

        assert_eq!(
            file,
            File {
                bytes: b"int x;\n".to_vec(),
                repo: "vendor/outer/deps/inner".to_owned(),
                commit: inner.sha("HEAD").await,
            }
        );
    }

    #[tokio::test]
    async fn a_rev_that_names_no_commit_is_refused() {
        let repo = build_repo(&[commit("first").file("a.c", "a\n")]).await;
        let git = Git::discover(repo.path()).await.unwrap();

        for rev in ["no-such-branch", "HEAD@{99}", "HEAD^{tree}", "--all", ""] {
            let err = resolve(&git, rev).await.unwrap_err();
            assert!(matches!(err, ReadError::BadRev { .. }), "{rev}: {err:?}");
            assert_eq!(err.to_string(), format!("{rev} is not a commit"));
        }
    }

    #[tokio::test]
    async fn a_rev_resolves_to_the_full_hash() {
        let repo = build_repo(&[
            commit("first").file("a.c", "a\n"),
            commit("second").file("a.c", "b\n"),
        ])
        .await;
        let git = Git::discover(repo.path()).await.unwrap();
        let head = repo.sha("HEAD").await;

        assert_eq!(resolve(&git, "main").await.unwrap(), head);
        assert_eq!(resolve(&git, &head[..7]).await.unwrap(), head);
        // A suffix would become part of the text that `:/` searches for.
        assert_eq!(
            resolve(&git, ":/first").await.unwrap(),
            repo.sha("HEAD~1").await
        );
    }

    #[tokio::test]
    async fn a_git_that_cannot_run_is_not_a_bad_rev() {
        let repo = build_repo(&[commit("first").file("a.c", "a\n")]).await;
        let git = Git::discover(repo.path()).await.unwrap();
        std::fs::remove_dir_all(repo.path()).unwrap();

        let err = resolve(&git, "HEAD").await.unwrap_err();

        assert!(matches!(err, ReadError::Git(_)), "{err:?}");
        assert!(err.to_string().ends_with("does not exist"), "{err}");
    }

    #[tokio::test]
    async fn a_path_that_is_not_in_the_tree_is_refused() {
        let repo = build_repo(&[commit("first").file("src/a.c", "a\n")]).await;
        let short = &repo.sha("HEAD").await[..12];

        for path in ["src/b.c", "lib/a.c", "src/a.c/x", "", "src/", "./src/a.c"] {
            let err = read(&repo, "HEAD", path).await.unwrap_err();
            assert!(
                matches!(err, ReadError::NotInTree { .. }),
                "{path}: {err:?}"
            );
            assert_eq!(
                err.to_string(),
                format!("{path} is not a file at commit {short}")
            );
        }
    }

    #[tokio::test]
    async fn a_file_that_a_later_commit_adds_is_not_in_an_older_tree() {
        let repo = build_repo(&[
            commit("first").file("a.c", "a\n"),
            commit("second").file("b.c", "b\n"),
        ])
        .await;

        let err = read(&repo, "HEAD~1", "b.c").await.unwrap_err();

        assert!(matches!(err, ReadError::NotInTree { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn a_directory_is_refused() {
        let (_lib, top) = with_lib().await;
        let short = &top.sha("HEAD").await[..12];

        for path in ["vendor", "vendor/lib", "vendor/lib/src"] {
            let err = read(&top, "HEAD", path).await.unwrap_err();
            assert!(
                matches!(err, ReadError::Directory { .. }),
                "{path}: {err:?}"
            );
            assert_eq!(
                err.to_string(),
                format!("{path} is a directory at commit {short}")
            );
        }
    }

    #[tokio::test]
    async fn a_symbolic_link_is_refused() {
        let repo = build_repo(&[commit("first").file("a.c", "a\n")]).await;
        std::os::unix::fs::symlink("a.c", repo.path().join("link.c")).unwrap();
        repo.commit(&commit("link")).await;
        let short = &repo.sha("HEAD").await[..12];

        let err = read(&repo, "HEAD", "link.c").await.unwrap_err();

        assert!(matches!(err, ReadError::Symlink { .. }), "{err:?}");
        assert_eq!(
            err.to_string(),
            format!("link.c is a symbolic link at commit {short}")
        );
    }

    #[tokio::test]
    async fn a_commit_that_the_submodule_does_not_hold_names_the_submodule() {
        let (lib, top, two) = with_unfetched_lib().await;

        let err = read(&top, "HEAD", "vendor/lib/src/lib.c")
            .await
            .unwrap_err();

        assert!(matches!(err, ReadError::MissingCommit { .. }), "{err:?}");
        assert_eq!(
            err.to_string(),
            format!(
                "the submodule vendor/lib does not hold commit {}. \
                 Run git --git-dir .git/modules/vendor/lib --work-tree . \
                 fetch --no-recurse-submodules",
                &two[..12]
            )
        );

        // The command that the message names fixes it, and leaves the
        // checkout of the submodule where it was.
        run_the_fetch(&top, &err).await;
        let file = read(&top, "HEAD", "vendor/lib/src/lib.c").await.unwrap();
        assert_eq!(file.bytes, b"int two;\n");
        let checked_out = top.git(&["-C", "vendor/lib", "rev-parse", "HEAD"]).await;
        assert_eq!(checked_out, lib.sha("HEAD~1").await);
    }

    #[tokio::test]
    async fn a_commit_that_only_another_branch_names_is_fetched() {
        let (lib, top) = with_lib().await;
        lib.commit(&commit("two").file("src/lib.c", "int two;\n"))
            .await;
        let two = lib.sha("HEAD").await;
        top.git(&["checkout", "--quiet", "-b", "next"]).await;
        top.commit(&commit("bump lib").gitlink("vendor/lib", &two))
            .await;
        top.git(&["checkout", "--quiet", "main"]).await;

        let err = read(&top, "next", "vendor/lib/src/lib.c")
            .await
            .unwrap_err();

        // `git submodule update` would fetch the commit of the index only.
        run_the_fetch(&top, &err).await;
        let file = read(&top, "next", "vendor/lib/src/lib.c").await.unwrap();
        assert_eq!(file.bytes, b"int two;\n");
    }

    #[tokio::test]
    async fn a_commit_of_a_deinitialized_submodule_is_fetched() {
        let (_lib, top, _two) = with_unfetched_lib().await;
        top.git(&["submodule", "deinit", "--quiet", "--force", "vendor/lib"])
            .await;

        let err = read(&top, "HEAD", "vendor/lib/src/lib.c")
            .await
            .unwrap_err();

        // `git submodule update` skips a submodule that is not initialized.
        run_the_fetch(&top, &err).await;
        let file = read(&top, "HEAD", "vendor/lib/src/lib.c").await.unwrap();
        assert_eq!(file.bytes, b"int two;\n");
    }

    #[tokio::test]
    async fn a_commit_of_a_submodule_that_moved_since_is_fetched() {
        let (_lib, top, _two) = with_unfetched_lib().await;
        std::fs::create_dir(top.path().join("third_party")).unwrap();
        top.git(&["mv", "vendor/lib", "third_party/lib"]).await;
        top.commit(&commit("move lib")).await;

        let err = read(&top, "HEAD~1", "vendor/lib/src/lib.c")
            .await
            .unwrap_err();

        // The index knows no submodule at vendor/lib any more.
        run_the_fetch(&top, &err).await;
        let file = read(&top, "HEAD~1", "vendor/lib/src/lib.c").await.unwrap();
        assert_eq!(file.bytes, b"int two;\n");
    }

    #[tokio::test]
    async fn a_submodule_whose_worktree_is_gone_is_read_in_its_repository() {
        let (_lib, top, _two) = with_unfetched_lib().await;
        top.git(&["rm", "--quiet", "--force", "vendor/lib"]).await;
        top.commit(&commit("drop lib")).await;
        // git keeps the repository, which names the gone worktree.
        assert!(!top.path().join("vendor/lib").exists());
        assert!(top.path().join(".git/modules/vendor/lib").is_dir());

        let file = read(&top, "HEAD~2", "vendor/lib/src/lib.c").await.unwrap();
        assert_eq!(file.bytes, b"int lib;\n");

        let err = read(&top, "HEAD~1", "vendor/lib/src/lib.c")
            .await
            .unwrap_err();
        run_the_fetch(&top, &err).await;
        let file = read(&top, "HEAD~1", "vendor/lib/src/lib.c").await.unwrap();
        assert_eq!(file.bytes, b"int two;\n");
    }

    #[tokio::test]
    async fn a_partial_clone_fetches_nothing() {
        let lib = build_repo(&[commit("one").file("src/lib.c", "one\n")]).await;
        let top = build_repo(&[commit("first").file("main.c", "old\n")]).await;
        // A file:// URL: the clone of the submodule below must be partial.
        let url = format!("file://{}", lib.path().display());
        top.git_clone(&["submodule", "add", "--quiet", &url, "vendor/lib"])
            .await;
        top.commit(&commit("second").file("main.c", "new\n")).await;
        for repo in [&lib, &top] {
            repo.git(&["config", "uploadpack.allowFilter", "true"])
                .await;
        }
        let clone = clone_repo(
            &top,
            &[
                "--filter=blob:none",
                "--recurse-submodules",
                "--also-filter-submodules",
            ],
        )
        .await;
        // The remote of the submodule holds a commit that its clone does not.
        lib.commit(&commit("two").file("src/lib.c", "two\n")).await;
        let two = lib.sha("HEAD").await;
        clone
            .commit(&commit("bump lib").gitlink("vendor/lib", &two))
            .await;
        let old = top.sha("HEAD~1:main.c").await;
        let local = |dir: &'static str| {
            let clone = &clone;
            async move {
                clone
                    .git(&[
                        "-C",
                        dir,
                        "cat-file",
                        "--batch-all-objects",
                        "--batch-check=%(objectname)",
                    ])
                    .await
            }
        };

        // The checkout fetched the blobs of HEAD only.
        let file = read(&clone, "HEAD~1", "vendor/lib/src/lib.c")
            .await
            .unwrap();
        assert_eq!(file.bytes, b"one\n");
        let err = read(&clone, "HEAD~2", "main.c").await.unwrap_err();
        assert!(matches!(err, ReadError::Git(_)), "{err:?}");
        assert!(!local(".").await.contains(&old), "main.c was fetched");

        let err = read(&clone, "HEAD", "vendor/lib/src/lib.c")
            .await
            .unwrap_err();
        assert!(matches!(err, ReadError::MissingCommit { .. }), "{err:?}");
        assert!(
            !local("vendor/lib").await.contains(&two),
            "{two} was fetched"
        );
    }

    #[tokio::test]
    async fn a_submodule_that_is_not_checked_out_is_refused() {
        let (lib, top) = with_lib().await;
        // A linked worktree starts with no submodule checked out.
        let elsewhere = tempfile::tempdir().unwrap();
        let tree = elsewhere.path().join("tree");
        let tree_arg = tree.to_str().unwrap();
        top.git(&["worktree", "add", "--quiet", tree_arg]).await;
        let git = Git::discover(&tree).await.unwrap();
        let head = resolve(&git, "HEAD").await.unwrap();

        let err = read_file(&git, &head, "vendor/lib/src/lib.c")
            .await
            .unwrap_err();

        assert!(matches!(err, ReadError::NotCheckedOut { .. }), "{err:?}");
        assert_eq!(
            err.to_string(),
            "the submodule vendor/lib is not checked out. \
             Run git submodule update --init vendor/lib"
        );

        // The command that the message names fixes it. git keeps the
        // submodule of a linked worktree in the git directory of that
        // worktree.
        top.git_clone(&[
            "-C",
            tree_arg,
            "submodule",
            "update",
            "--quiet",
            "--init",
            "vendor/lib",
        ])
        .await;
        let file = read_file(&git, &head, "vendor/lib/src/lib.c")
            .await
            .unwrap();
        assert_eq!(file.commit, lib.sha("HEAD").await);
    }

    #[tokio::test]
    async fn a_nested_submodule_names_the_outermost_one_to_update() {
        let inner = build_repo(&[commit("inner").file("x.c", "int x;\n")]).await;
        let outer = build_repo(&[commit("outer").submodule("deps/inner", &inner)]).await;
        let top = build_repo(&[commit("add outer").submodule("vendor/outer", &outer)]).await;
        let elsewhere = tempfile::tempdir().unwrap();
        let tree = elsewhere.path().join("tree");
        let tree_arg = tree.to_str().unwrap();
        top.git(&["worktree", "add", "--quiet", tree_arg]).await;
        // Without --recursive: the inner submodule stays out.
        top.git_clone(&["-C", tree_arg, "submodule", "update", "--quiet", "--init"])
            .await;
        let git = Git::discover(&tree).await.unwrap();
        let head = resolve(&git, "HEAD").await.unwrap();

        let err = read_file(&git, &head, "vendor/outer/deps/inner/x.c")
            .await
            .unwrap_err();

        assert_eq!(
            err.to_string(),
            "the submodule vendor/outer/deps/inner is not checked out. \
             Run git submodule update --init --recursive vendor/outer"
        );
    }
}
