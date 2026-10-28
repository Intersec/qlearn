//! The files of `.learn/`.
//!
//! `.learn/` sits at the top level of the worktree, one directory per
//! course. One server per worktree, and `.learn/` belongs to one worktree,
//! so no lock is needed.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use time::OffsetDateTime;

use crate::git::exec::Git;
use crate::model::{self, Course};

/// The name of the store, in the worktree and in `.git/info/exclude`.
const DIR: &str = ".learn";

pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// The store of the worktree whose top level is `root`.
    pub fn at(root: &Path) -> Self {
        Self {
            dir: root.join(DIR),
        }
    }

    /// The directory of a course.
    pub fn course_dir(&self, slug: &str) -> PathBuf {
        self.dir.join(slug)
    }

    /// Open a course, as `qlearn [<slug>]` does.
    ///
    /// With a slug, the course is created when it does not exist. Without
    /// one, `.learn/` must hold exactly one course. A course that cannot be
    /// read is an error, and is never written over.
    pub fn open(&self, slug: Option<&str>, now: OffsetDateTime) -> Result<Course> {
        let slug = match slug {
            Some(slug) => {
                check_slug(slug)?;
                slug.to_owned()
            }
            None => self.only_slug()?,
        };

        if let Some(course) = self.load(&slug)? {
            return Ok(course);
        }
        let course = Course::new(&slug, now);
        self.save(&course)?;

        Ok(course)
    }

    fn only_slug(&self) -> Result<String> {
        let mut slugs = self.slugs()?;
        match slugs.len() {
            0 => bail!("{DIR}/ holds no course yet. Give it a name: `qlearn <slug>`"),
            1 => Ok(slugs.remove(0)),
            _ => bail!(
                "{DIR}/ holds several courses: {}. Name one: `qlearn <slug>`",
                slugs.join(", ")
            ),
        }
    }

    /// The slugs of the courses of `.learn/`, in order. A directory
    /// without `course.json` is not a course, so it is left out.
    pub fn slugs(&self) -> Result<Vec<String>> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(e).with_context(|| format!("cannot read {}", self.dir.display()));
            }
        };

        let mut slugs: Vec<_> = entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                if !entry.file_type().ok()?.is_dir() {
                    return None;
                }
                let name = entry.file_name().into_string().ok()?;
                (is_slug(&name) && entry.path().join("course.json").exists()).then_some(name)
            })
            .collect();

        slugs.sort();
        Ok(slugs)
    }

    /// Read a course. None when it has no directory yet.
    pub fn load(&self, slug: &str) -> Result<Option<Course>> {
        let dir = self.course_dir(slug);
        let path = dir.join("course.json");

        match self.find(slug)? {
            Found::Course(course) => Ok(Some(*course)),
            Found::Nothing => Ok(None),
            Found::Foreign => bail!(
                "{} has no course.json, so it is not a course of qlearn. \
                 Start a new course with a new slug: `qlearn <slug>`",
                dir.display()
            ),
            Found::Link => bail!(
                "{} is a symbolic link, and qlearn writes under {DIR}/ only. \
                 Start a new course with a new slug: `qlearn <slug>`",
                dir.display()
            ),
            Found::Newer(version) => bail!(
                "{} was written by a newer qlearn (format {version}, this one \
                 reads {}). Upgrade qlearn",
                path.display(),
                model::VERSION
            ),
            Found::Corrupt(e) => Err(e).with_context(|| {
                format!(
                    "{} is not readable. It is left as it is: repair it by hand \
                     rather than lose the course",
                    path.display()
                )
            }),
        }
    }

    /// What the directory of a course holds. `load()` turns it into an
    /// error, `listing()` into a line, so each case keeps its own advice.
    fn find(&self, slug: &str) -> Result<Found> {
        let dir = self.course_dir(slug);
        let path = dir.join("course.json");

        // A link would take every write of the course out of `.learn/`.
        if fs::symlink_metadata(&dir).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Ok(Found::Link);
        }
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                // Another tool may have written the directory, and its
                // files are not a contract: they are never read or written.
                if dir.exists() {
                    return Ok(Found::Foreign);
                }
                return Ok(Found::Nothing);
            }
            Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
        };

        // The version is read alone first: a newer format may not parse as
        // this one, and the learner must be told to upgrade, not to repair.
        #[derive(Deserialize)]
        struct Head {
            version: u32,
        }
        let head: Head = match serde_json::from_str(&text) {
            Ok(head) => head,
            Err(e) => return Ok(Found::Corrupt(e)),
        };
        if head.version > model::VERSION {
            return Ok(Found::Newer(head.version));
        }

        Ok(match serde_json::from_str::<Course>(&text) {
            // The directory names the course, and every write goes back to
            // it. A copied directory is a course of its own.
            Ok(mut course) => {
                course.slug = slug.to_owned();
                Found::Course(Box::new(course))
            }
            Err(e) => Found::Corrupt(e),
        })
    }

    pub fn save(&self, course: &Course) -> Result<()> {
        let mut text = serde_json::to_string_pretty(course)?;
        text.push('\n');

        write_atomic(&self.course_dir(&course.slug).join("course.json"), &text)
    }

    /// The courses as `qlearn list` prints them.
    pub fn listing(&self) -> Result<String> {
        let slugs = self.slugs()?;
        if slugs.is_empty() {
            return Ok(format!("{DIR}/ holds no course yet.\n"));
        }

        let mut out = String::new();
        for slug in slugs {
            // A course of a newer qlearn is not to be repaired, so it is
            // not called unreadable.
            let line = match self.find(&slug) {
                Ok(Found::Course(course)) => {
                    format!("{}  {}", course.status.as_str(), course.title)
                }
                // Its course.json went away since `slugs()`.
                Ok(Found::Nothing | Found::Foreign | Found::Link) => continue,
                Ok(Found::Newer(_)) => "(newer qlearn)".to_owned(),
                Ok(Found::Corrupt(_)) | Err(_) => "(unreadable)".to_owned(),
            };
            out.push_str(&format!("{slug}  {line}\n"));
        }
        Ok(out)
    }
}

/// What `Store::find()` sees in the directory of a course.
enum Found {
    Course(Box<Course>),
    /// No directory: the course is new.
    Nothing,
    /// A directory without `course.json`: not a course of qlearn.
    Foreign,
    /// A symbolic link in place of the directory.
    Link,
    /// A format above `model::VERSION`.
    Newer(u32),
    Corrupt(serde_json::Error),
}

/// Write a file of the store.
///
/// The write is atomic: a full file appears, or the old one stays. A
/// half-written course is worse than none, because nobody notices.
fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let dir = path
        .parent()
        .context("a file of the store has a directory")?;
    fs::create_dir_all(dir).with_context(|| format!("cannot make {}", dir.display()))?;

    // A name of its own for each write: two writers of one file must not
    // share a temporary file, or one truncates what the other wrote.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut temp = path.as_os_str().to_owned();
    temp.push(format!(
        ".{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let temp = PathBuf::from(temp);

    let written = fs::File::create(&temp)
        .and_then(|mut handle| {
            handle.write_all(text.as_bytes())?;
            handle.sync_all()
        })
        .with_context(|| format!("cannot write {}", temp.display()))
        .and_then(|()| {
            fs::rename(&temp, path)
                .with_context(|| format!("cannot put {} in place", path.display()))
        });
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

/// A slug names a directory, so it must never reach outside `.learn/`.
fn is_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn check_slug(slug: &str) -> Result<()> {
    if !is_slug(slug) {
        bail!("`{slug}` is not a slug. Use lowercase letters, digits and hyphens");
    }
    Ok(())
}

/// Make git ignore `.learn/`, so a course is never committed by accident.
///
/// When git does not ignore it yet, the line goes into `info/exclude`,
/// which no commit carries. This is the one write outside `.learn/`.
pub async fn exclude(git: &Git) -> Result<()> {
    let pattern = format!("{DIR}/");
    let (ignored, _) = git.output(&["check-ignore", "-q", &pattern]).await?;
    if ignored {
        return Ok(());
    }

    // A linked worktree has no `.git` directory: git names the file.
    let path = git
        .text(&["rev-parse", "--git-path", "info/exclude"])
        .await?;
    let path = git.root().join(path.trim());

    // The file is the user's: an append never loses their lines, even
    // when the write fails half way.
    let before = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
    };
    // git still says no when a .gitignore un-ignores the store, or when
    // the store is tracked. The line cannot change that, so one is enough.
    if String::from_utf8_lossy(&before)
        .lines()
        .any(|line| line.trim() == pattern)
    {
        return Ok(());
    }
    let last = before.last().copied();
    let mut line = String::new();
    if last.is_some_and(|b| b != b'\n') {
        line.push('\n');
    }
    line.push_str(&pattern);
    line.push('\n');

    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("cannot make {}", dir.display()))?;
    }
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| file.write_all(line.as_bytes()))
        .with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CourseStatus;
    use crate::testutil::{Repo, build_repo, commit};

    fn at_new_year() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_767_225_600).unwrap()
    }

    async fn repo() -> Repo {
        build_repo(&[commit("first").file("a.txt", "a\n")]).await
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// Every file under `dir`, relative to it, `.git` left out.
    fn files(dir: &Path) -> Vec<String> {
        fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let name = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                if name == ".git" {
                    continue;
                }
                if path.is_dir() {
                    walk(base, &path, out);
                } else {
                    out.push(name);
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out.sort();
        out
    }

    #[tokio::test]
    async fn a_new_course_is_written_and_read_back() {
        let repo = repo().await;
        let store = Store::at(repo.path());

        let course = store.open(Some("raft"), at_new_year()).unwrap();
        assert_eq!(course.title, "raft");
        assert_eq!(course.status, CourseStatus::New);
        assert_eq!(course.created, "2026-01-01T00:00:00Z");
        assert!(repo.path().join(".learn/raft/course.json").is_file());

        let mut changed = course.clone();
        changed.title = "Raft elections".to_owned();
        store.save(&changed).unwrap();

        let later = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
        let back = store.open(Some("raft"), later).unwrap();
        assert_eq!(back, changed, "an open course is read, not created again");
    }

    #[tokio::test]
    async fn a_write_leaves_no_temporary_file_behind() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();

        assert_eq!(files(repo.path()), [".learn/raft/course.json", "a.txt"]);
    }

    #[tokio::test]
    async fn a_newer_format_is_refused_with_a_word_to_upgrade() {
        let repo = repo().await;
        let path = repo.path().join(".learn/raft/course.json");
        // A shape that this qlearn cannot parse: the version alone decides.
        let text = r#"{"version":2,"slug":"raft","parts":"elsewhere"}"#;
        write(&path, text);

        let error = Store::at(repo.path())
            .open(Some("raft"), at_new_year())
            .unwrap_err()
            .to_string();

        assert!(error.contains("newer qlearn"), "{error}");
        assert!(error.contains("Upgrade qlearn"), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }

    #[tokio::test]
    async fn a_directory_without_a_course_file_is_refused_and_left_alone() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        write(&repo.path().join(".learn/raft/notes.txt"), "elections\n");

        let error = store
            .open(Some("raft"), at_new_year())
            .unwrap_err()
            .to_string();

        assert!(error.contains("not a course of qlearn"), "{error}");
        assert!(error.contains("new course with a new slug"), "{error}");
        assert_eq!(files(repo.path()), [".learn/raft/notes.txt", "a.txt"]);
        assert_eq!(
            fs::read_to_string(repo.path().join(".learn/raft/notes.txt")).unwrap(),
            "elections\n"
        );
    }

    #[tokio::test]
    async fn a_directory_without_a_course_file_is_skipped() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        write(&repo.path().join(".learn/bully/notes.txt"), "bully\n");
        fs::create_dir_all(repo.path().join(".learn/empty")).unwrap();

        assert_eq!(store.listing().unwrap(), ".learn/ holds no course yet.\n");
        let error = store.open(None, at_new_year()).unwrap_err().to_string();
        assert!(error.contains("holds no course yet"), "{error}");

        store.open(Some("raft"), at_new_year()).unwrap();
        assert_eq!(store.open(None, at_new_year()).unwrap().slug, "raft");
        assert_eq!(store.listing().unwrap(), "raft  new  raft\n");
        assert_eq!(
            files(repo.path()),
            [".learn/bully/notes.txt", ".learn/raft/course.json", "a.txt"]
        );
    }

    #[tokio::test]
    async fn a_link_in_place_of_a_course_is_refused_and_left_alone() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        let elsewhere = tempfile::tempdir().unwrap();
        fs::create_dir_all(repo.path().join(".learn")).unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), repo.path().join(".learn/raft")).unwrap();

        let error = store
            .open(Some("raft"), at_new_year())
            .unwrap_err()
            .to_string();

        assert!(error.contains("symbolic link"), "{error}");
        assert!(error.contains("new course with a new slug"), "{error}");
        assert_eq!(files(elsewhere.path()), Vec::<String>::new());
        assert_eq!(store.listing().unwrap(), ".learn/ holds no course yet.\n");
    }

    #[tokio::test]
    async fn a_copied_course_is_named_after_its_directory() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();
        let original = repo.path().join(".learn/raft/course.json");
        let text = fs::read_to_string(&original).unwrap();
        write(&repo.path().join(".learn/raft-2/course.json"), &text);

        let mut copy = store.open(Some("raft-2"), at_new_year()).unwrap();
        assert_eq!(copy.slug, "raft-2");
        copy.title = "The copy".to_owned();
        store.save(&copy).unwrap();

        assert_eq!(fs::read_to_string(&original).unwrap(), text);
        assert_eq!(store.open(Some("raft-2"), at_new_year()).unwrap(), copy);
    }

    #[tokio::test]
    async fn two_writers_of_one_course_both_succeed() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        let course = store.open(Some("raft"), at_new_year()).unwrap();

        std::thread::scope(|scope| {
            for _ in 0..2 {
                scope.spawn(|| {
                    for _ in 0..200 {
                        store.save(&course).unwrap();
                    }
                });
            }
        });

        assert_eq!(files(repo.path()), [".learn/raft/course.json", "a.txt"]);
        assert_eq!(store.open(Some("raft"), at_new_year()).unwrap(), course);
    }

    #[tokio::test]
    async fn a_corrupt_course_is_an_error_and_stays_on_disk() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();
        let path = repo.path().join(".learn/raft/course.json");
        fs::write(&path, "{ this is not json").unwrap();

        let error = store
            .open(Some("raft"), at_new_year())
            .unwrap_err()
            .to_string();
        assert!(error.contains("repair it by hand"), "{error}");
        assert_eq!(store.listing().unwrap(), "raft  (unreadable)\n");

        assert_eq!(fs::read_to_string(&path).unwrap(), "{ this is not json");
    }

    #[tokio::test]
    async fn a_slug_with_another_character_is_refused() {
        let repo = repo().await;
        let store = Store::at(repo.path());

        for slug in ["", "Raft", "raft_1", "../escape", "a/b", "."] {
            let error = store.open(Some(slug), at_new_year()).unwrap_err();
            assert!(error.to_string().contains("not a slug"), "{slug}: {error}");
        }
        store.open(Some("raft-2"), at_new_year()).unwrap();
        assert_eq!(files(repo.path()), [".learn/raft-2/course.json", "a.txt"]);
    }

    #[tokio::test]
    async fn without_a_slug_the_only_course_opens() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();
        // A file of the store is not a course.
        write(&repo.path().join(".learn/server.json"), "{}");

        assert_eq!(store.open(None, at_new_year()).unwrap().slug, "raft");
    }

    #[tokio::test]
    async fn without_a_slug_several_courses_are_listed() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();
        store.open(Some("paxos"), at_new_year()).unwrap();

        let error = store.open(None, at_new_year()).unwrap_err().to_string();
        assert!(error.contains("several courses: paxos, raft"), "{error}");
    }

    #[tokio::test]
    async fn without_a_slug_and_no_course_a_slug_is_asked_for() {
        let repo = repo().await;
        let store = Store::at(repo.path());

        let error = store.open(None, at_new_year()).unwrap_err().to_string();
        assert!(error.contains("qlearn <slug>"), "{error}");
        assert_eq!(files(repo.path()), ["a.txt"], "nothing is created");
    }

    #[tokio::test]
    async fn the_listing_gives_the_slug_the_status_and_the_title() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        assert_eq!(store.listing().unwrap(), ".learn/ holds no course yet.\n");

        let mut raft = store.open(Some("raft"), at_new_year()).unwrap();
        raft.title = "Raft elections".to_owned();
        raft.status = CourseStatus::Running;
        store.save(&raft).unwrap();
        store.open(Some("paxos"), at_new_year()).unwrap();
        write(&repo.path().join(".learn/bully/notes.txt"), "bully\n");
        write(
            &repo.path().join(".learn/gossip/course.json"),
            r#"{"version":2}"#,
        );
        write(&repo.path().join(".learn/zab/course.json"), "{ not json");

        assert_eq!(
            store.listing().unwrap(),
            "gossip  (newer qlearn)\n\
             paxos  new  paxos\n\
             raft  running  Raft elections\n\
             zab  (unreadable)\n"
        );
    }

    fn exclude_file(repo: &Repo) -> String {
        fs::read_to_string(repo.path().join(".git/info/exclude")).unwrap_or_default()
    }

    fn count(text: &str) -> usize {
        text.lines().filter(|line| *line == ".learn/").count()
    }

    #[tokio::test]
    async fn the_store_is_excluded_once() {
        let repo = repo().await;
        let git = Git::discover(repo.path()).await.unwrap();
        // A last line of the user without its newline.
        let before = format!("{}*.log", exclude_file(&repo));
        fs::write(repo.path().join(".git/info/exclude"), &before).unwrap();

        exclude(&git).await.unwrap();
        exclude(&git).await.unwrap();

        assert_eq!(exclude_file(&repo), format!("{before}\n.learn/\n"));

        Store::at(repo.path())
            .open(Some("raft"), at_new_year())
            .unwrap();
        assert_eq!(repo.git(&["status", "--porcelain"]).await, "");
    }

    #[tokio::test]
    async fn a_store_that_git_ignores_already_is_left_alone() {
        let repo = build_repo(&[commit("first").file(".gitignore", ".learn/\n")]).await;
        let git = Git::discover(repo.path()).await.unwrap();
        let before = exclude_file(&repo);

        exclude(&git).await.unwrap();

        assert_eq!(exclude_file(&repo), before);
    }

    #[tokio::test]
    async fn a_store_that_git_cannot_ignore_is_excluded_once() {
        // A .gitignore that un-ignores the store wins over info/exclude, and
        // git never calls a tracked path ignored.
        for spec in [
            commit("first").file(".gitignore", "!.learn/\n"),
            commit("first").file(".learn/raft/course.json", "{}\n"),
        ] {
            let repo = build_repo(&[spec]).await;
            let git = Git::discover(repo.path()).await.unwrap();

            exclude(&git).await.unwrap();
            exclude(&git).await.unwrap();

            assert_eq!(count(&exclude_file(&repo)), 1);
        }
    }

    #[tokio::test]
    async fn a_linked_worktree_is_excluded_through_the_common_directory() {
        let repo = repo().await;
        let elsewhere = tempfile::tempdir().unwrap();
        let tree = elsewhere.path().join("second-worktree");
        repo.git(&["worktree", "add", tree.to_str().unwrap(), "-b", "other"])
            .await;

        let git = Git::discover(&tree).await.unwrap();
        exclude(&git).await.unwrap();

        assert_eq!(count(&exclude_file(&repo)), 1);
        assert_eq!(
            files(&tree),
            ["a.txt"],
            "nothing is written in the worktree"
        );
    }

    #[tokio::test]
    async fn every_write_stays_under_the_store() {
        let repo = repo().await;
        let git = Git::discover(repo.path()).await.unwrap();
        let config = fs::read_to_string(repo.path().join(".git/config")).unwrap();

        exclude(&git).await.unwrap();
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();
        store.open(Some("paxos"), at_new_year()).unwrap();

        let outside: Vec<_> = files(repo.path())
            .into_iter()
            .filter(|f| !f.starts_with(".learn/"))
            .collect();
        assert_eq!(outside, ["a.txt"]);
        assert_eq!(
            fs::read_to_string(repo.path().join(".git/config")).unwrap(),
            config
        );
    }
}
