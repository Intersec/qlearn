//! A course outside a git repository: the built binary, in a temporary
//! directory that git sees outside any repository.

mod common;

use std::path::{Path, PathBuf};
use std::process::Output;

use serde_json::Value;

use common::{interrupt, qlearn, read_url};

/// A directory for a course on a paper, with a directory below it. The
/// ceiling stops the search of git at the temporary directory, whatever
/// lies above it.
struct Paper {
    _top: tempfile::TempDir,
    ceiling: PathBuf,
    dir: PathBuf,
    below: PathBuf,
}

fn paper() -> Paper {
    let top = tempfile::tempdir().unwrap();
    let ceiling = top.path().canonicalize().unwrap();
    let dir = ceiling.join("paper");
    let below = dir.join("notes");
    std::fs::create_dir_all(&below).unwrap();
    Paper {
        _top: top,
        ceiling,
        dir,
        below,
    }
}

impl Paper {
    fn qlearn(&self, cwd: &Path, args: &[&str]) -> tokio::process::Command {
        let mut command = qlearn(cwd, args);
        command.env("GIT_CEILING_DIRECTORIES", &self.ceiling);
        command
    }

    async fn run(&self, cwd: &Path, args: &[&str]) -> Output {
        self.qlearn(cwd, args).output().await.unwrap()
    }
}

fn json_of(out: &Output) -> Value {
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{out:?}"))
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Every `.git` under `dir`.
fn dot_gits(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(at) = todo.pop() {
        for entry in std::fs::read_dir(&at).unwrap() {
            let path = entry.unwrap().path();
            if path.file_name().is_some_and(|name| name == ".git") {
                found.push(path.clone());
            }
            if path.is_dir() {
                todo.push(path);
            }
        }
    }
    found
}

const QUIZ: &str = r#"{ "choices": [{ "header": "Vote", "prompt": "Who wins?",
    "options": ["The first to time out", "The highest id"], "answer": 0,
    "point": "random-timeout" }],
    "points": { "random-timeout": { "label": "Why the timeout is random" } } }"#;

#[tokio::test]
async fn a_course_outside_a_repository_takes_every_block_but_the_code() {
    let paper = paper();
    let mut server = paper
        .qlearn(&paper.dir, &["raft", "--no-open"])
        .spawn()
        .unwrap();
    read_url(&mut server).await;
    assert!(paper.dir.join(".learn/raft/course.json").is_file());

    // The agent works from a directory below, as it does in a worktree.
    let page = "# Election\n\nSection 5.2 of the paper.\n\n```c\nif (now > deadline)\n    \
                become_candidate();\n```\n\n```mermaid\ngraph LR\n  Follower --> Candidate\n```\n";
    std::fs::write(paper.below.join("page.md"), page).unwrap();
    let pushed = json_of(
        &paper
            .run(&paper.below, &["page", "1", "--file", "page.md"])
            .await,
    );
    let kinds: Vec<_> = pushed["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|block| block["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["text", "diagram"], "{pushed}");
    std::fs::write(paper.below.join("quiz.json"), QUIZ).unwrap();
    let number = json_of(
        &paper
            .run(&paper.below, &["quiz", "1", "--file", "quiz.json"])
            .await,
    );
    assert_eq!(number["n"], 1, "{number}");

    let code = "# Code\n\n```excerpt\npath: a.c\nlines: 1\n```\n\n![The states](states.svg)\n";
    std::fs::write(paper.below.join("code.md"), code).unwrap();
    let refused = paper
        .run(&paper.below, &["page", "2", "--file", "code.md"])
        .await;
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let errors = stderr(&refused);
    let lines: Vec<_> = errors.lines().collect();
    assert_eq!(lines.len(), 2, "{errors}");
    assert!(
        lines[0].starts_with("code.md:3: excerpt: show the code as a sketch"),
        "{errors}"
    );
    assert!(
        lines[1].starts_with("code.md:8: image: take this image out"),
        "{errors}"
    );
    assert!(!paper.dir.join(".learn/raft/pages/2.md").exists());

    let listing = paper.run(&paper.below, &["list"]).await;
    assert!(listing.status.success(), "{listing:?}");
    assert!(
        String::from_utf8_lossy(&listing.stdout).starts_with("raft  "),
        "{listing:?}"
    );

    assert!(interrupt(server).await.success());
    assert!(
        !paper.below.join(".learn").exists(),
        "one store, in the directory of the course"
    );
    assert_eq!(
        dot_gits(&paper.ceiling),
        Vec::<PathBuf>::new(),
        "a write to git"
    );
}

/// With no git on the PATH, every directory is outside a repository.
#[tokio::test]
async fn a_course_opens_with_no_git() {
    let paper = paper();
    let empty = paper.ceiling.join("bin");
    std::fs::create_dir(&empty).unwrap();

    let mut server = paper
        .qlearn(&paper.dir, &["raft", "--no-open"])
        .env("PATH", &empty)
        .spawn()
        .unwrap();
    read_url(&mut server).await;

    assert!(paper.dir.join(".learn/raft/course.json").is_file());
    assert!(interrupt(server).await.success());
}

/// A command that fails leaves no `.learn/`: outside a repository, a
/// course started below would take a stray one above as its own.
#[tokio::test]
async fn a_failed_command_leaves_no_store() {
    let paper = paper();

    for args in [
        &["--no-open"][..],
        &["Bad Slug", "--no-open"],
        &["export"],
        &["export", "--json"],
        &["export", "cheatsheet"],
    ] {
        let out = paper.run(&paper.dir, args).await;

        assert_eq!(out.status.code(), Some(1), "{args:?}: {out:?}");
        assert!(!paper.dir.join(".learn").exists(), "{args:?} made .learn/");
    }

    let mut server = paper
        .qlearn(&paper.below, &["raft", "--no-open"])
        .spawn()
        .unwrap();
    read_url(&mut server).await;
    assert!(paper.below.join(".learn/raft/course.json").is_file());
    assert!(interrupt(server).await.success());
}
