//! The command line.

use clap::{ArgGroup, CommandFactory, Parser, Subcommand, ValueEnum};

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

    /// Print the procedure that an agent follows to teach a course with
    /// this release.
    Skill,

    /// Print the course: its syllabus with the state and the score of each
    /// page, and the points to revisit. Reads the course of the server of
    /// the worktree, or, with no server, the only course of `.learn/`.
    // `qlearn export cheatsheet` comes later as a subcommand, so `export`
    // takes no positional argument that would capture the word `cheatsheet`.
    Export {
        /// Print the whole course as JSON, as an agent reads it to resume
        /// the course.
        #[arg(long)]
        json: bool,
    },

    /// Delete a course: remove `.learn/<slug>/`, with the answers and the
    /// cheatsheet. Refused while a server serves the course.
    Delete {
        /// The course to delete.
        slug: String,
        /// Delete with no question. Without it, qlearn asks on the
        /// terminal.
        #[arg(long)]
        yes: bool,
    },

    /// Change the course, as the agent. Name at least one field.
    #[command(group(ArgGroup::new("field").required(true).multiple(true)))]
    Course {
        /// The subject, as the learner named it.
        // A text may open with a hyphen, such as a list item.
        #[arg(long, group = "field", allow_hyphen_values = true)]
        title: Option<String>,
        /// What the learner wants to get from the course, in Markdown.
        #[arg(long, group = "field", allow_hyphen_values = true)]
        goal: Option<String>,
        /// The background and the level of the learner, in Markdown.
        #[arg(long, group = "field", allow_hyphen_values = true)]
        profile: Option<String>,
        /// Whether a quiz also asks about the earlier pages.
        #[arg(long, value_enum, group = "field")]
        quiz_mode: Option<QuizMode>,
        /// The course runs, or is finished. Only the creation of a course
        /// makes it new.
        #[arg(long, value_enum, group = "field")]
        status: Option<Status>,
        /// The date of the follow-up quiz, as YYYY-MM-DD, or `none`.
        #[arg(long, value_name = "DATE|none", group = "field")]
        follow_up: Option<String>,
    },

    /// Write the syllabus, as the agent: its parts and their pages, as JSON.
    /// A refused syllabus prints every error and exits 1.
    Syllabus {
        /// The JSON file. `-` reads it from standard input.
        #[arg(long, value_name = "PATH")]
        file: String,
    },

    /// Push a page, as the agent. A refused page prints every error, each
    /// with its line, and exits 1.
    Page {
        /// The page: `1`, `7a`, `c1` or `f1`.
        id: String,
        /// The Markdown source. `-` reads it from standard input.
        #[arg(long, value_name = "PATH")]
        file: String,
    },

    /// Add a quiz under a page, as the agent. A refused quiz prints every
    /// error, each with its place in the JSON, and exits 1.
    Quiz {
        /// The page, which the agent pushed already.
        id: String,
        /// The JSON file. `-` reads it from standard input.
        #[arg(long, value_name = "PATH")]
        file: String,
    },

    /// Write the debrief of a quiz that the learner answered, as the agent,
    /// with the grade of each point of its open question. A refused
    /// debrief prints every error and exits 1.
    Debrief {
        /// The page of the quiz.
        id: String,
        /// The number of the quiz in its page, from 1.
        n: usize,
        /// The Markdown source. `-` reads it from standard input.
        #[arg(long, value_name = "PATH")]
        file: String,
        /// The grade of a point of the open question: ok, partial or
        /// missed. Once for each point.
        #[arg(long = "point", value_name = "POINT=ok|partial|missed")]
        points: Vec<String>,
    },

    /// Open a thread in the page, as the agent: on a block, or about the
    /// whole page. Prints the thread, with its id.
    Comment {
        /// The page, then `#` and the block: `3#6495234d4654`. The page
        /// alone opens a thread about the whole page.
        #[arg(value_name = "ID[#BLOCK]")]
        target: String,
        /// The comment, in Markdown. `-` reads it from standard input.
        // A text may open with a hyphen, such as a list item.
        #[arg(long, allow_hyphen_values = true)]
        body: String,
    },

    /// Write in a thread, as the agent. Prints the thread.
    Reply {
        /// The id of the thread, such as `3-1`.
        thread: String,
        /// The comment, in Markdown. `-` reads it from standard input.
        #[arg(long, allow_hyphen_values = true)]
        body: String,
    },

    /// Ask the learner to choose, in the page, as the agent: at the end of a
    /// page, or at the top of the syllabus column. Prints the id of the
    /// choice. A refused choice prints every error, each with its place in
    /// the JSON, and exits 1.
    Choose {
        /// The JSON file. `-` reads it from standard input.
        #[arg(long, value_name = "PATH")]
        file: String,
    },

    /// Print the points of the course and their results, the missed ones
    /// first, then the others by the date of their last result.
    Points,

    /// Say in the page what the agent does, until its next wait. An empty
    /// text clears it.
    Status {
        // A text may open with a hyphen, such as "- two pages left".
        #[arg(allow_hyphen_values = true)]
        text: String,
    },

    /// Wait for the next request of the learner, and print it as JSON.
    /// Exits 1 when the time runs out.
    Wait {
        /// The `next` number of the last wait. Now by default.
        #[arg(long)]
        after: Option<u64>,
        /// The `server` id of the last wait. The `after` number of another
        /// server counts other events, so the wait answers `reset` then.
        #[arg(long, value_name = "ID")]
        server: Option<String>,
        /// Seconds to wait.
        #[arg(long, default_value_t = 600)]
        timeout: u64,
    },
}

/// The names that `qlearn <name>` runs as a command, so no course may
/// take one as its slug. Read from the parser, so the list holds every new
/// command.
pub fn command_names() -> Vec<String> {
    let mut cli = Cli::command();
    // The build adds `help`, which runs as a command too.
    cli.build();
    cli.get_subcommands()
        .flat_map(|command| std::iter::once(command.get_name()).chain(command.get_all_aliases()))
        .map(str::to_owned)
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum QuizMode {
    Mixed,
    Current,
}

/// The status that the agent may set. Only the creation of a course makes
/// it new.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Status {
    Running,
    Finished,
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn every_command_is_a_name_that_no_course_may_take() {
        let names = command_names();
        for word in ["list", "skill", "course", "page", "wait", "help"] {
            assert!(names.iter().any(|name| name == word), "{word}: {names:?}");
        }
        for command in Cli::command().get_subcommands() {
            assert!(names.iter().any(|name| name == command.get_name()));
        }

        // `qlearn <command>` runs the command. Only `--` gets the name to
        // the store as a slug, which refuses it.
        let cli = Cli::try_parse_from(["qlearn", "--", "list"]).unwrap();
        assert_eq!(cli.slug.as_deref(), Some("list"));

        let dir = tempfile::tempdir().unwrap();
        let store = qlearn::store::Store::at(dir.path()).refusing(command_names());
        let now = time::OffsetDateTime::UNIX_EPOCH;
        for name in &names {
            let error = store.open(Some(name), now).unwrap_err().to_string();
            assert!(error.contains("is a command of qlearn"), "{name}: {error}");
        }
        assert!(!store.dir().exists(), "a refused slug creates nothing");
    }

    /// The subcommands that the procedure names in its code: inline code
    /// that opens with `qlearn `, and the lines of a code block.
    fn named_commands(text: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut in_block = false;

        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                in_block = !in_block;
                continue;
            }
            let calls: Vec<&str> = match in_block {
                // A block in a list item is indented.
                true => line
                    .trim_start()
                    .strip_prefix("qlearn ")
                    .into_iter()
                    .collect(),
                false => line.split("`qlearn ").skip(1).collect(),
            };
            for call in calls {
                let word: String = call
                    .chars()
                    .take_while(|c| c.is_ascii_lowercase() || *c == '-')
                    .collect();
                // An option or a placeholder such as `<slug>` is no command.
                if !word.is_empty() && !word.starts_with('-') {
                    found.push(word);
                }
            }
        }
        found
    }

    #[test]
    fn the_procedure_names_only_commands_of_this_release() {
        let cli = Cli::command();
        let named = named_commands(qlearn::agent::SKILL);

        assert!(named.contains(&"wait".to_owned()), "{named:?}");
        for word in named {
            assert!(
                cli.find_subcommand(&word).is_some(),
                "the procedure names `qlearn {word}`, which this release does not have"
            );
        }
    }

    #[test]
    fn a_placeholder_or_an_option_is_no_command() {
        let text = "Run `qlearn <slug>` or `qlearn --no-open`.\n```sh\nqlearn wait\n```\n";

        assert_eq!(named_commands(text), ["wait"]);
    }

    #[test]
    fn a_block_in_a_list_item_is_checked_too() {
        let text = "1. Run:\n\n   ```sh\n   qlearn bogus --x\n   ```\n";

        assert_eq!(named_commands(text), ["bogus"]);
    }

    /// A status or a title may open with a hyphen, such as "- two pages
    /// left", and is still a text.
    #[test]
    fn a_text_that_opens_with_a_hyphen_is_a_text() {
        let cli = Cli::try_parse_from(["qlearn", "status", "- two pages left"]).unwrap();
        assert!(
            matches!(cli.command, Some(Command::Status { ref text }) if text == "- two pages left"),
            "{cli:?}"
        );

        let cli = Cli::try_parse_from(["qlearn", "course", "--title", "-1 page", "--goal", "-x"])
            .unwrap();
        let Some(Command::Course { title, goal, .. }) = cli.command else {
            panic!("{cli:?}");
        };
        assert_eq!(title.as_deref(), Some("-1 page"));
        assert_eq!(goal.as_deref(), Some("-x"));
    }

    #[test]
    fn a_change_of_the_course_names_at_least_one_field() {
        assert!(Cli::try_parse_from(["qlearn", "course"]).is_err());
        assert!(Cli::try_parse_from(["qlearn", "course", "--status", "new"]).is_err());

        let cli = Cli::try_parse_from([
            "qlearn",
            "course",
            "--title",
            "Raft",
            "--quiz-mode",
            "current",
            "--follow-up",
            "none",
        ])
        .unwrap();
        let Some(Command::Course {
            title,
            quiz_mode,
            follow_up,
            status,
            ..
        }) = cli.command
        else {
            panic!("{cli:?}");
        };
        assert_eq!(title.as_deref(), Some("Raft"));
        assert_eq!(quiz_mode, Some(QuizMode::Current));
        assert_eq!(follow_up.as_deref(), Some("none"));
        assert_eq!(status, None);
    }

    #[test]
    fn a_debrief_takes_a_point_once_for_each_point() {
        let cli = Cli::try_parse_from([
            "qlearn",
            "debrief",
            "3",
            "1",
            "--file",
            "-",
            "--point",
            "split-vote=partial",
            "--point",
            "random-timeout=ok",
        ])
        .unwrap();
        let Some(Command::Debrief {
            id,
            n,
            file,
            points,
        }) = cli.command
        else {
            panic!("{cli:?}");
        };
        assert_eq!((id.as_str(), n, file.as_str()), ("3", 1, "-"));
        assert_eq!(points, ["split-vote=partial", "random-timeout=ok"]);

        let cli = Cli::try_parse_from(["qlearn", "debrief", "3", "2", "--file", "a.md"]).unwrap();
        assert!(
            matches!(cli.command, Some(Command::Debrief { ref points, .. }) if points.is_empty())
        );
        assert!(Cli::try_parse_from(["qlearn", "debrief", "3", "x", "--file", "-"]).is_err());
        assert!(Cli::try_parse_from(["qlearn", "debrief", "3", "--file", "-"]).is_err());
    }

    #[test]
    fn a_comment_names_its_page_and_its_block_and_takes_a_body() {
        let cli =
            Cli::try_parse_from(["qlearn", "comment", "3#6495234d4654", "--body", "- a list"])
                .unwrap();
        assert!(
            matches!(cli.command, Some(Command::Comment { ref target, ref body })
                if target == "3#6495234d4654" && body == "- a list"),
            "{cli:?}"
        );

        let cli = Cli::try_parse_from(["qlearn", "reply", "3-1", "--body", "-"]).unwrap();
        assert!(
            matches!(cli.command, Some(Command::Reply { ref thread, ref body })
                if thread == "3-1" && body == "-"),
            "{cli:?}"
        );
        assert!(Cli::try_parse_from(["qlearn", "reply", "3-1"]).is_err());
        assert!(Cli::try_parse_from(["qlearn", "comment", "--body", "x"]).is_err());
    }

    #[test]
    fn a_choice_takes_a_file() {
        let cli = Cli::try_parse_from(["qlearn", "choose", "--file", "-"]).unwrap();
        assert!(
            matches!(cli.command, Some(Command::Choose { ref file }) if file == "-"),
            "{cli:?}"
        );
        assert!(Cli::try_parse_from(["qlearn", "choose"]).is_err());
    }

    #[test]
    fn an_export_takes_json_and_no_other_word() {
        let cli = Cli::try_parse_from(["qlearn", "export"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Export { json: false })));
        let cli = Cli::try_parse_from(["qlearn", "export", "--json"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Export { json: true })));

        // The word of `qlearn export cheatsheet`, which a later step adds,
        // must not parse as something else meanwhile.
        assert!(Cli::try_parse_from(["qlearn", "export", "cheatsheet"]).is_err());
    }

    #[test]
    fn a_delete_names_its_course() {
        let cli = Cli::try_parse_from(["qlearn", "delete", "raft"]).unwrap();
        assert!(
            matches!(cli.command, Some(Command::Delete { ref slug, yes: false }) if slug == "raft"),
            "{cli:?}"
        );
        let cli = Cli::try_parse_from(["qlearn", "delete", "raft", "--yes"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Delete { yes: true, .. })
        ));
        assert!(Cli::try_parse_from(["qlearn", "delete"]).is_err());
        assert!(Cli::try_parse_from(["qlearn", "delete", "--yes"]).is_err());
    }

    #[test]
    fn a_wait_waits_ten_minutes_from_now_by_default() {
        let cli = Cli::try_parse_from(["qlearn", "wait"]).unwrap();

        assert!(matches!(
            cli.command,
            Some(Command::Wait {
                after: None,
                server: None,
                timeout: 600
            })
        ));

        let cli =
            Cli::try_parse_from(["qlearn", "wait", "--after", "7", "--server", "0a1b"]).unwrap();
        assert!(
            matches!(cli.command, Some(Command::Wait { after: Some(7), ref server, .. })
                if server.as_deref() == Some("0a1b")),
            "{cli:?}"
        );
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
