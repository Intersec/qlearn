//! qlearn — a course on a code base, in the browser.
//!
//! The binary is a thin shell: it reads the arguments, opens the course,
//! and serves the interface. Everything else lives in the library.

mod cli;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let _cli = cli::Cli::parse();

    Ok(())
}
