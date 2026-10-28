# Concept

qlearn shows a course on a codebase in the browser. An agent in a terminal
builds the course and teaches it. The learner reads the lessons, answers the
quizzes, and asks questions in the page. At the end, the learner keeps a
cheatsheet.

## The problem

An agent such as Claude Code can teach a subject of a codebase as a course:
short lessons, quizzes, code tours, exercises. It teaches in its terminal,
and the terminal limits it:

- **A lesson is plain text.** A state machine or a timeline is ASCII art. A
  table wider than the terminal wraps and breaks.
- **A quiz is a dialog.** The open question and the multiple-choice
  questions go through the prompt. Once the debrief is over, the quiz
  scrolls away with the rest of the conversation.
- **The course has no shape on the screen.** The syllabus, the score and the
  weak points live in the conversation, or in a file that the learner never
  sees.
- **Nothing stays at the end.** The course ends with the conversation. A
  developer doc that the agent writes is for the team. The learner has
  nothing short to keep for themselves.
- **The visual pages leave the machine.** The agent can publish a page on
  claude.ai for a diagram. That page holds names and excerpts of the code.

A course also has a rule that the terminal cannot enforce: teach only what
the code says. An excerpt in a lesson is text that the agent typed. Nothing
proves that the file holds these lines.

## What qlearn is

A command that starts a small web server on the loopback address, on one
course, and opens the browser:

```
qlearn raft-election
```

The agent runs in its own terminal. It writes the course through commands of
the binary, and it waits for the learner with `qlearn wait`. qlearn never
starts the agent.

The browser shows:

- The syllabus on the left, in parts and lessons, with the state of each
  lesson and its score.
- The lesson: text, tables, diagrams, charts, and excerpts of the code. The
  server reads each excerpt in the repository, with its colors. An excerpt
  of a file that does not hold these lines is refused, so the agent cannot
  show code that does not exist.
- The quiz under the lesson: the open question first, then the
  multiple-choice questions, then the debrief of the agent. They stay on
  the page.
- A question on any block of a lesson, as a thread. The agent answers in
  the thread.
- The points of the course, with the result of each one and its date, so
  the learner sees what to revisit.
- At the end, a cheatsheet of one page: the hooks of the lessons, the key
  tables and diagrams, the map of the code, the pitfalls, and the points
  that this learner missed. The learner saves it as HTML, as a PDF, or as
  Markdown.

The course lives in `.learn/<slug>/` at the root of the repository, as it
does now. Nothing leaves the machine.

## How the binary and the skill split

The binary carries the procedure and the surface. `qlearn skill` prints the
procedure. It says how to teach a course: the profile of the learner, the
research, the syllabus, the check of each claim against the code, the
lessons, the quizzes and their debrief, the exercises, the checkpoints, the
follow-up quiz, the findings, and the developer doc of the subject. It also
says how to drive the page: the loop, the commands, the format of a lesson
and of a quiz. The page shows the course, it collects the answers, and it
keeps the record.

The skill in `skills/qlearn` is the entry point. Its description tells the
agent when to load it. Its body runs `qlearn skill` and follows it, so the
skill and the binary are always the same release. qlearn has no terminal
mode: the agent teaches the course through the page.

An organization can wrap qlearn in a skill of its own. That skill adds its
sources, such as tickets, a review server or internal docs, and its rules.
Then it follows `qlearn skill`.

## Who it is for

Developers who learn a subject of a codebase with an agent: a mechanism, a
module, an algorithm, a protocol. One learner, one machine.

## Non-goals

- **Not a course catalog.** There are no saved courses. Each course is
  built for its learner. What a course finds goes into the developer doc of
  the subject, and the next course starts from that doc.
- **Not an agent runner.** qlearn never starts `claude` and never calls a
  network service. It depends on no vendor.
- **Not a learning platform.** No accounts, no teams, no grades for a
  manager. The score is for the learner alone.
- **Not a sandbox for code.** A lesson holds no script of the agent. The
  page draws diagrams, charts and step-by-step views from data.
- **Not a documentation tool.** The developer doc of the subject lives in
  the repository, and the agent writes it there. The cheatsheet is personal.
- **Not multi-user.** No authentication beyond the loopback address and a
  session token.

## Prior art

| Tool | What it gives | Why it does not fit |
|---|---|---|
| A course taught in the terminal of an agent | Lessons and quizzes on the code | Plain text, quizzes in the prompt, no record on the screen |
| Artifact pages on claude.ai | Rich pages that the agent writes | The page leaves the machine. One page per course, apart from the quizzes |
| [CodeTour](https://github.com/microsoft/codetour) | A guided walk through the code in VS Code | A fixed tour that a person writes. No quiz, no agent, one editor |
| [Swimm](https://swimm.io) | Docs linked to the code, checked when the code changes | Team documentation on a service. Not a course for one learner |
| [Anki](https://apps.ankiweb.net) | Spaced repetition | Cards that the learner writes. No link to the code |

Three ideas shape the model of qlearn:

- An agent in its terminal is a second party, reached through commands of
  the binary, with no network service.
- The server is the only writer of the store.
- The binary carries the procedure of the agent, so the procedure and the
  commands are always the same release.
