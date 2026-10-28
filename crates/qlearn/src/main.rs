//! qlearn — a course on a code base, in the browser.
//!
//! The binary is a thin shell: it reads the arguments, opens the course,
//! and serves the interface. Everything else lives in the library.

mod cli;

use anyhow::{Context, Result};
use clap::Parser;
use time::OffsetDateTime;

use qlearn::git::exec::Git;
use qlearn::store::{self, Store};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = cli::Cli::parse();

    let cwd = std::env::current_dir().context("cannot read the working directory")?;
    let git = Git::discover(&cwd).await?;
    let store = Store::at(git.root());

    if let Some(cli::Command::List) = cli.command {
        print!("{}", store.listing()?);
        return Ok(());
    }

    let course = store.open(cli.slug.as_deref(), OffsetDateTime::now_utc())?;
    store::exclude(&git).await?;

    // Until the server exists, the course directory is all there is to show.
    println!("{}", store.course_dir(&course.slug).display());
    Ok(())
}
