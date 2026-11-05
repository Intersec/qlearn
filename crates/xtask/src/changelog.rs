//! Collecting the pending changelog entries.
//!
//! One file per entry, in the group it belongs to. A shared section would
//! conflict on every rebase; a file only its own branch has does not.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The groups, in the order they are printed.
const GROUPS: [(&str, &str); 4] = [
    ("added", "Added"),
    ("changed", "Changed"),
    ("fixed", "Fixed"),
    ("removed", "Removed"),
];

pub struct Collected {
    /// The Markdown of the entries, ready to go under a version heading.
    pub text: String,
    /// The files that were read, so a release can delete them.
    pub files: Vec<PathBuf>,
}

pub fn collect(root: &Path) -> Result<Collected> {
    let mut text = String::new();
    let mut files = Vec::new();

    for (dir, title) in GROUPS {
        let path = root.join("changelog").join(dir);
        let mut entries = read_group(&path)?;
        if entries.is_empty() {
            continue;
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        text.push_str(&format!("### {title}\n\n"));
        for (file, body) in entries {
            text.push_str(body.trim_end());
            text.push('\n');
            files.push(file);
        }
        text.push('\n');
    }

    if text.is_empty() {
        text.push_str("Nothing is waiting for a release.\n");
    }
    Ok(Collected { text, files })
}

fn read_group(dir: &Path) -> Result<Vec<(PathBuf, String)>> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let body = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        out.push((path, body));
    }
    Ok(out)
}
