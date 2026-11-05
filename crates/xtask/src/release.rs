//! Cutting a version.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::changelog;

pub fn run(root: &Path, version: &str) -> Result<()> {
    check_version(version)?;
    check_clean(root)?;

    let collected = changelog::collect(root)?;
    if collected.files.is_empty() {
        bail!("nothing is waiting for a release. See changelog/README.md");
    }

    bump(root, version)?;
    write_changelog(root, version, &collected.text)?;

    for file in &collected.files {
        std::fs::remove_file(file).with_context(|| format!("cannot remove {}", file.display()))?;
    }

    commit(root, version)?;
    tag(root, version, &collected.text)?;

    let name = format!("v{version}");
    println!("{name} is cut and tagged. Nothing is pushed: that is yours to do.");
    println!();
    // The tag by name, not `--follow-tags`: that one carries tags only
    // beside refs it is already pushing, so it sends nothing at all when the
    // branch is up to date, and no release pipeline ever starts.
    println!("    git push origin {} {name}", branch(root));
    Ok(())
}

/// Commit what the release wrote, and nothing else: an untracked file of
/// the developer, such as the settings of an editor, stays out.
fn commit(root: &Path, version: &str) -> Result<()> {
    git(
        root,
        &[
            "add",
            "--",
            "Cargo.toml",
            "Cargo.lock",
            "CHANGELOG.md",
            "changelog",
        ],
    )?;
    git(
        root,
        &[
            "commit",
            "--quiet",
            "-m",
            &format!("chore(release): v{version}"),
        ],
    )
}

/// Tag the release commit, with the notes of that version in the tag.
///
/// The tag is annotated. `git push --follow-tags` pushes an annotated tag
/// and skips a lightweight one, so a lightweight tag stays on the machine
/// that cut it and no release pipeline ever starts.
fn tag(root: &Path, version: &str, entries: &str) -> Result<()> {
    let name = format!("v{version}");
    let subject = format!("qlearn {name}");

    git(
        root,
        &["tag", "-a", "-m", &subject, "-m", entries.trim(), &name],
    )
}

/// The branch to push, for the line that says how to push.
fn branch(root: &Path) -> String {
    let out = Command::new("git")
        .current_dir(root)
        .args(["branch", "--show-current"])
        .output();
    let name = out
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .unwrap_or_default();

    match name.trim().is_empty() {
        true => "HEAD".to_owned(),
        false => name.trim().to_owned(),
    }
}

fn check_version(version: &str) -> Result<()> {
    let parts: Vec<_> = version.split('.').collect();
    let ok = parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));

    if !ok {
        bail!("{version} is not a version like 0.1.0");
    }
    Ok(())
}

/// A change to a tracked file must be committed first. An untracked file
/// counts only under `changelog/`, where it would go into the notes of a
/// version that does not hold its code.
fn check_clean(root: &Path) -> Result<()> {
    for args in [
        &["status", "--porcelain", "--untracked-files=no"][..],
        &["status", "--porcelain", "--", "changelog"][..],
    ] {
        let out = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .context("cannot run git")?;

        if !out.stdout.is_empty() {
            bail!("the working tree is not clean. Commit or stash first");
        }
    }
    Ok(())
}

/// The version lives in the workspace manifest, alone.
fn bump(root: &Path, version: &str) -> Result<()> {
    let path = root.join("Cargo.toml");
    let text = std::fs::read_to_string(&path)?;
    let mut out = String::with_capacity(text.len());
    let mut done = false;

    for line in text.lines() {
        if !done && line.starts_with("version = ") {
            out.push_str(&format!("version = \"{version}\"\n"));
            done = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }

    if !done {
        bail!("no version line in {}", path.display());
    }
    std::fs::write(&path, out)?;

    // Keep Cargo.lock in step, so the release commit holds both.
    Command::new("cargo")
        .current_dir(root)
        .args(["update", "--workspace", "--offline"])
        .status()
        .context("cannot update Cargo.lock")?;

    Ok(())
}

fn write_changelog(root: &Path, version: &str, entries: &str) -> Result<()> {
    let path = root.join("CHANGELOG.md");
    let text = std::fs::read_to_string(&path)?;
    let marker = "<!-- The release script writes new versions under this line. -->";

    let Some(at) = text.find(marker) else {
        bail!("no marker in {}. See CHANGELOG.md", path.display());
    };
    let cut = at + marker.len();
    let section = format!("\n\n## [{version}]\n\n{}", entries.trim_end());

    let mut out = String::with_capacity(text.len() + section.len());
    out.push_str(&text[..cut]);
    out.push_str(&section);
    // A blank line before the version below. A heading that opens right
    // under a bullet is a heading a Markdown reader has to squint at.
    out.push_str("\n\n");
    out.push_str(text[cut..].trim_start_matches('\n'));

    std::fs::write(&path, out)?;
    Ok(())
}

fn git(root: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .with_context(|| format!("cannot run git {}", args.join(" ")))?;

    if !status.success() {
        bail!("git {} failed", args.join(" "));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A repository with one commit, which is what a tag needs.
    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        for args in [
            vec!["init", "--quiet", "--initial-branch=main", "."],
            vec!["config", "user.name", "Test"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "commit.gpgsign", "false"],
            vec!["config", "tag.gpgsign", "false"],
            vec!["commit", "--quiet", "--allow-empty", "-m", "first"],
        ] {
            git(root, &args).unwrap();
        }
        dir
    }

    fn text(root: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();

        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    /// The files that a release writes, committed, beside a file of the
    /// developer that git does not track.
    fn released(root: &Path) {
        for (path, text) in [
            ("Cargo.toml", "version = \"0.1.0\"\n"),
            ("Cargo.lock", "# lock\n"),
            ("CHANGELOG.md", "# Changelog\n"),
            ("changelog/added/thing.md", "- A thing.\n"),
        ] {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        git(root, &["add", "-A"]).unwrap();
        git(root, &["commit", "--quiet", "-m", "work"]).unwrap();
        std::fs::write(root.join("notes.txt"), "mine\n").unwrap();
    }

    #[test]
    fn the_release_commit_leaves_an_untracked_file_out() {
        let dir = repo();
        let root = dir.path();
        released(root);

        std::fs::write(root.join("Cargo.toml"), "version = \"0.2.0\"\n").unwrap();
        std::fs::write(root.join("Cargo.lock"), "# lock 0.2.0\n").unwrap();
        std::fs::write(root.join("CHANGELOG.md"), "# Changelog\n\n## [0.2.0]\n").unwrap();
        std::fs::remove_file(root.join("changelog/added/thing.md")).unwrap();
        commit(root, "0.2.0").unwrap();

        let files = text(root, &["show", "--name-only", "--format=", "HEAD"]);
        assert_eq!(
            files,
            "CHANGELOG.md\nCargo.lock\nCargo.toml\nchangelog/added/thing.md"
        );
        assert_eq!(text(root, &["status", "--porcelain"]), "?? notes.txt");
    }

    #[test]
    fn an_untracked_file_outside_the_changelog_is_clean() {
        let dir = repo();
        let root = dir.path();
        released(root);

        check_clean(root).unwrap();
    }

    #[test]
    fn an_untracked_entry_of_the_changelog_is_not_clean() {
        let dir = repo();
        let root = dir.path();
        released(root);
        std::fs::write(root.join("changelog/added/other.md"), "- Other.\n").unwrap();

        assert!(check_clean(root).is_err());
    }

    #[test]
    fn a_changed_tracked_file_is_not_clean() {
        let dir = repo();
        let root = dir.path();
        released(root);
        std::fs::write(root.join("Cargo.toml"), "version = \"9.9.9\"\n").unwrap();

        assert!(check_clean(root).is_err());
    }

    #[test]
    fn the_line_to_push_names_the_branch_that_is_out() {
        let dir = repo();

        assert_eq!(branch(dir.path()), "main");
    }

    #[test]
    fn the_tag_is_annotated_so_follow_tags_pushes_it() {
        let dir = repo();
        let root = dir.path();

        tag(root, "1.2.3", "### Fixed\n\n- A thing that was broken.\n").unwrap();

        // A lightweight tag names the commit. `git push --follow-tags` skips
        // one, so the release pipeline never starts.
        assert_eq!(text(root, &["cat-file", "-t", "v1.2.3"]), "tag");
    }

    #[test]
    fn the_tag_carries_the_notes_of_the_version() {
        let dir = repo();
        let root = dir.path();

        tag(root, "1.2.3", "### Fixed\n\n- A thing that was broken.\n").unwrap();
        let message = text(root, &["tag", "-l", "--format=%(contents)", "v1.2.3"]);

        assert!(message.starts_with("qlearn v1.2.3"), "{message}");
        assert!(message.contains("- A thing that was broken."), "{message}");
    }
}
