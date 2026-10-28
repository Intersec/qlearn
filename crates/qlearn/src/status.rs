//! One line at the bottom of the terminal that says how the course stands.
//!
//! The messages of the server scroll away, and a learner who comes back to
//! the terminal wants the state, not the history. The line is redrawn in
//! place under the messages, on a terminal only: in a file or a pipe, the
//! messages are all there is.

use std::io::{IsTerminal, Write};

use crate::events::Events;

/// Clear the line the cursor is on.
const CLEAR: &str = "\r\x1b[2K";

/// What draws the status line again over the one drawn before. With no
/// wrap, a line wider than the terminal is cut at its edge: a wrapped line
/// would take two rows, and `CLEAR` erases only the last one.
fn redraw(text: &str) -> String {
    format!("{CLEAR}\x1b[?7l{text}\x1b[?7h")
}

/// The state that the line shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// None before the first page.
    pub page: Option<bool>,
    pub agent: Agent,
}

impl Snapshot {
    pub fn of(events: &Events) -> Self {
        let agent = if events.agent_works() {
            Agent::Working
        } else if events.agent_listens() {
            Agent::Connected
        } else if events.agent_left() {
            Agent::Left
        } else {
            Agent::Absent
        };

        Self {
            page: events.page_open(),
            agent,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Agent {
    /// No agent came yet.
    Absent,
    Connected,
    /// A wait handed over a request, and the agent has not waited again.
    Working,
    Left,
}

/// The line for a state, with colors when `color` is true.
pub fn line(state: &Snapshot, color: bool) -> String {
    let page = match state.page {
        None => (Grey, "page not open yet"),
        Some(true) => (Green, "page open"),
        Some(false) => (Red, "page closed"),
    };
    let agent = match state.agent {
        Agent::Absent => (Grey, "no agent"),
        Agent::Connected => (Green, "agent connected"),
        Agent::Working => (Blue, "agent working on a page"),
        Agent::Left => (Orange, "agent left: ask it to run `qlearn wait`"),
    };

    [page, agent]
        .iter()
        .map(|(dot, text)| format!("{} {text}", dot.paint(color)))
        .collect::<Vec<_>>()
        .join("   ")
}

#[derive(Clone, Copy)]
enum Dot {
    Grey,
    Green,
    Red,
    Orange,
    Blue,
}
use Dot::*;

impl Dot {
    fn paint(self, color: bool) -> String {
        let (code, mark) = match self {
            Grey => ("90", "○"),
            Green => ("32", "●"),
            Red => ("31", "●"),
            Orange => ("33", "●"),
            Blue => ("34", "●"),
        };
        if color {
            format!("\x1b[{code}m{mark}\x1b[0m")
        } else {
            mark.to_owned()
        }
    }
}

/// The terminal the server writes to.
pub struct Terminal {
    /// A status line is drawn. False in a file, a pipe, or under `--trace`,
    /// whose lines would break into it.
    live: bool,
    color: bool,
    drawn: Option<String>,
}

impl Terminal {
    pub fn new() -> Self {
        Self {
            live: std::io::stdout().is_terminal() && !crate::trace::on(),
            color: std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()),
            drawn: None,
        }
    }

    /// Print a message above the status line.
    pub fn say(&mut self, message: &str) {
        let mut out = std::io::stdout().lock();
        if self.live && self.drawn.is_some() {
            let _ = write!(out, "{CLEAR}");
        }
        let _ = writeln!(out, "{message}");
        if let Some(drawn) = self.drawn.as_ref().filter(|_| self.live) {
            let _ = write!(out, "{}", redraw(drawn));
        }
        let _ = out.flush();
    }

    /// Draw the status line again. Every time, and not only on a change:
    /// a line that another writer broke comes back whole.
    pub fn draw(&mut self, state: &Snapshot) {
        if !self.live {
            return;
        }
        let text = line(state, self.color);
        let mut out = std::io::stdout().lock();
        let _ = write!(out, "{}", redraw(&text));
        let _ = out.flush();
        self.drawn = Some(text);
    }
}

impl Default for Terminal {
    fn default() -> Self {
        Self::new()
    }
}

/// Clear the status line, so the shell prompt starts on a clean line.
pub fn clear() {
    let mut out = std::io::stdout();
    if out.is_terminal() && !crate::trace::on() {
        let _ = write!(out, "{CLEAR}");
        let _ = out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_state_has_its_line() {
        let cases = [
            (None, Agent::Absent, "○ page not open yet   ○ no agent"),
            (
                Some(true),
                Agent::Connected,
                "● page open   ● agent connected",
            ),
            (
                Some(true),
                Agent::Working,
                "● page open   ● agent working on a page",
            ),
            (
                Some(false),
                Agent::Left,
                "● page closed   ● agent left: ask it to run `qlearn wait`",
            ),
        ];

        for (page, agent, expected) in cases {
            assert_eq!(line(&Snapshot { page, agent }, false), expected);
        }
    }

    /// A line wider than the terminal would wrap, and the next redraw would
    /// clear only its last row.
    #[test]
    fn a_redraw_never_wraps_the_line() {
        let text = "x".repeat(300);

        assert_eq!(redraw(&text), format!("\r\x1b[2K\x1b[?7l{text}\x1b[?7h"));
    }

    #[test]
    fn a_dot_is_colored_unless_told_not_to() {
        let state = Snapshot {
            page: Some(false),
            agent: Agent::Left,
        };

        assert!(line(&state, true).starts_with("\x1b[31m●\x1b[0m page closed"));
        assert!(!line(&state, false).contains('\x1b'));
    }

    #[tokio::test(start_paused = true)]
    async fn the_snapshot_names_the_work_before_the_presence() {
        use crate::model::{Author, EventKind};

        let events = Events::default();
        assert_eq!(
            Snapshot::of(&events),
            Snapshot {
                page: None,
                agent: Agent::Absent,
            }
        );

        events.push(EventKind::Next, Author::Learner, Some("1"), None);
        let _page = events.page_waits();
        let mut waits = events.agent_waits();
        assert_eq!(Snapshot::of(&events).agent, Agent::Connected);

        waits.answered(&events.after(Some(0), crate::events::HOLD).await);
        drop(waits);
        assert_eq!(
            Snapshot::of(&events),
            Snapshot {
                page: Some(true),
                agent: Agent::Working,
            }
        );

        tokio::time::advance(std::time::Duration::from_secs(30 * 60)).await;
        assert_eq!(Snapshot::of(&events).agent, Agent::Left);
    }
}
