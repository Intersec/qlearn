//! What the binary says about itself, beyond the version Cargo knows.
//!
//! A reader who reports a problem gives a version. Between two releases that
//! version names a hundred commits, so the commit is baked in here, once, at
//! build time. A build outside a checkout of qlearn says nothing, which is
//! why every path below falls back to an empty string rather than failing.

use std::path::Path;
use std::process::Command;

fn main() {
    // A packager who builds outside a checkout can say it instead of git.
    println!("cargo::rerun-if-env-changed=QLEARN_GIT_SHA");

    let sha = match std::env::var("QLEARN_GIT_SHA") {
        Ok(given) => given,
        Err(_) => head(),
    };
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let long = match sha.is_empty() {
        true => version,
        false => format!("{version} ({sha})"),
    };

    println!("cargo::rustc-env=QLEARN_VERSION_LONG={long}");

    watch_the_interface();
}

/// The binary embeds `web/dist`, and the tests do too. Cargo tracks each
/// embedded file, but not a directory that is not there yet, so this script
/// waits for it. A release without it would start and show no page, so it
/// fails here, where the cause is plain.
fn watch_the_interface() {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let index = Path::new(&crate_dir).join("../../web/dist/index.html");
    if index.exists() {
        return;
    }
    // A path that does not exist runs this script, then the build of the
    // crate, again on every build, until it does.
    println!("cargo::rerun-if-changed={}", index.display());

    if std::env::var("PROFILE").as_deref() == Ok("release") {
        eprintln!("web/dist is missing: build the interface first, with `make build`");
        std::process::exit(1);
    }
}

/// The commit being built, short. Empty when git cannot say.
fn head() -> String {
    if !in_a_checkout_of_qlearn() {
        return String::new();
    }
    let Some(sha) = git(&["rev-parse", "--short=9", "HEAD"]) else {
        return String::new();
    };
    watch_the_head();

    sha
}

/// Whether git works on the checkout of this workspace. git walks up from
/// the crate, so a tarball unpacked inside another checkout would bake in
/// the commit of that checkout.
fn in_a_checkout_of_qlearn() -> bool {
    let Ok(crate_dir) = std::env::var("CARGO_MANIFEST_DIR") else {
        return false;
    };
    let Some(top) = git(&["rev-parse", "--show-toplevel"]) else {
        return false;
    };
    let workspace = Path::new(&crate_dir).join("../..").canonicalize();

    matches!((Path::new(&top).canonicalize(), workspace), (Ok(a), Ok(b)) if a == b)
}

/// Build again when HEAD moves.
///
/// Cargo reruns this script only when a file it was told about changes.
/// `HEAD` covers a checkout. A commit leaves `HEAD` alone: it appends to
/// the reflog of HEAD, and writes the branch when the branch has a loose
/// ref. A linked worktree keeps its branches in the common directory, so
/// git names each path.
fn watch_the_head() {
    let mut files = vec!["HEAD".to_owned(), "logs/HEAD".to_owned()];
    if let Some(reference) = git(&["symbolic-ref", "--quiet", "HEAD"]) {
        files.push(reference);
    }

    for file in files {
        let Some(path) = git(&["rev-parse", "--path-format=absolute", "--git-path", &file]) else {
            continue;
        };
        // Cargo reruns on every build for a path that does not exist.
        if Path::new(&path).exists() {
            println!("cargo::rerun-if-changed={path}");
        }
    }
}

/// Run git and take its answer, or nothing at all.
fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?.trim().to_owned();

    (!text.is_empty()).then_some(text)
}
