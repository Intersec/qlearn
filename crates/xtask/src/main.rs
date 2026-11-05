//! Release and changelog chores.
//!
//! Not shipped. `cargo xtask <task>` runs one of these from the workspace.

mod changelog;
mod release;

use anyhow::{Result, bail};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);

    match args.next().as_deref() {
        Some("changelog") => {
            print!("{}", changelog::collect(&root())?.text);
            Ok(())
        }
        Some("release") => match args.next() {
            Some(version) => release::run(&root(), &version),
            None => bail!("usage: cargo xtask release <version>"),
        },
        Some(task) => bail!("unknown task {task:?}. Try changelog or release"),
        None => {
            println!("usage: cargo xtask <task>");
            println!();
            println!("tasks:");
            println!("  changelog          print what the next release would say");
            println!("  release <version>  cut that version and tag it");
            Ok(())
        }
    }
}

/// The top of the workspace, which is two directories above this crate.
fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}
