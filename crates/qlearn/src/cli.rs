//! The command line.

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "qlearn", version = qlearn::version::LONG, about, long_about = None)]
// A slug before a subcommand would be dropped without a word.
#[command(args_conflicts_with_subcommands = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// The course, `.learn/<slug>/`. It is created when it is new. The only
    /// course of `.learn/` by default.
    pub slug: Option<String>,

    /// Print the URL, open no browser.
    #[arg(long)]
    pub no_open: bool,

    /// Print the course before the URL.
    #[arg(short, long)]
    pub verbose: bool,

    /// Print, on standard error, what every piece of work costs.
    #[arg(long)]
    pub trace: bool,

    /// The port to listen on. 0 asks the system for a free one.
    #[arg(long, default_value_t = 0)]
    pub port: u16,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// List the courses of `.learn/`.
    List,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_arguments_are_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn a_slug_and_the_list_command_parse() {
        let cli = Cli::try_parse_from(["qlearn", "raft"]).unwrap();
        assert_eq!(cli.slug.as_deref(), Some("raft"));
        assert!(cli.command.is_none());

        let cli = Cli::try_parse_from(["qlearn", "list"]).unwrap();
        assert!(matches!(cli.command, Some(Command::List)));

        assert!(Cli::try_parse_from(["qlearn", "raft", "list"]).is_err());
    }

    #[test]
    fn the_server_options_parse() {
        let cli = Cli::try_parse_from(["qlearn"]).unwrap();
        assert_eq!(cli.port, 0, "the system picks a free port by default");
        assert!(!cli.no_open && !cli.verbose && !cli.trace);

        let cli = Cli::try_parse_from(["qlearn", "raft", "--no-open", "-v", "--trace"]).unwrap();
        assert!(cli.no_open && cli.verbose && cli.trace);

        let cli = Cli::try_parse_from(["qlearn", "--port", "7420"]).unwrap();
        assert_eq!(cli.port, 7420);
    }
}
