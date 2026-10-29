# qlearn

A course on your code base, in your browser, taught by an agent in your
terminal.

One binary, no runtime to install, nothing leaves your machine.

> **Status.** Nothing is coded yet. This README says what qlearn will do.
> The design is in [`roadmap/`](./roadmap/README.md).

## Why

An agent such as Claude Code can teach a subject of a code base as a
course: short lessons, quizzes, code tours, exercises. In the terminal, a
lesson is plain text, a quiz scrolls away with the conversation, and the
learner never sees the syllabus or the score. Nothing proves that an excerpt
in a lesson is the code of the repository.

qlearn will start a small web server on the loopback address, on one course,
and open the browser. The agent will teach the course through commands of
the binary.

## Use

```sh
qlearn raft-election            # open the course, and create it when it is new
qlearn                          # open the only course of .learn/
qlearn list                     # list the courses of .learn/
qlearn export                   # the syllabus, the scores, the points to revisit
qlearn export --json            # the whole course, as an agent reads it to resume
qlearn delete raft-election     # delete a course, once its server is stopped
qlearn export cheatsheet        # the cheatsheet, as Markdown
```

| Option | What it does |
|---|---|
| `--no-open` | Print the URL, open no browser |
| `--port <n>` | Use a fixed port |
| `-v` | Print the course before the URL |
| `--trace` | Print on standard error what the work costs |

What the browser will show:

- The syllabus on the left, in parts and lessons, with the state and the
  score of each lesson.
- The lesson: text, tables, diagrams, charts, step-by-step views, and
  excerpts of the code with their colors. The server reads each excerpt in
  the repository, submodules included. It refuses a page whose excerpt does
  not match the code.
- The quiz under the lesson: the open question first, then the
  multiple-choice questions, then the debrief of the agent. They stay on the
  page.
- An **Ask** button on each block of a lesson. Your question opens a thread,
  and the agent answers in it.
- The points of the course, with the result of each one, so you see what to
  revisit.
- At the end, a cheatsheet of one page. The agent writes it, and qlearn adds
  the map of the code and the points that you missed. Save it as HTML, as a
  PDF, or as Markdown.

The course will live in `.learn/<slug>/` at the top level of the
repository. If git does not ignore `.learn/`, qlearn adds it to
`.git/info/exclude`. A course stays there until `qlearn delete` removes it.
Deleting the course deletes its cheatsheet, so export the cheatsheet
first. `qlearn delete` names what it removes and asks before it removes
it. `--yes` skips the question.

`qlearn export` asks the server of the worktree for the course. With no
server, it reads the only course of `.learn/`.

## Learn with an agent

An agent in a terminal, such as Claude Code, teaches the course. For Claude
Code, install the skill in [`skills/qlearn`](skills/qlearn/SKILL.md):

```sh
mkdir -p ~/.claude/skills
cp -r skills/qlearn ~/.claude/skills/
```

The skill will hold no procedure of its own. It will run `qlearn skill`,
which prints the procedure of the binary on your PATH: how to teach the
course, and how to drive the page. So the skill and the binary are always
the same release. A new release of qlearn needs no new copy of the skill.

Then ask Claude to teach you a subject of your code base. qlearn has no
terminal mode: the agent teaches the course through the page.

An organization can wrap qlearn in a skill of its own. That skill adds its
sources, such as tickets, a review server or internal docs, and its rules.
Then it follows `qlearn skill`.

The agent will write through these commands. Each one finds the server of
the worktree on its own:

```sh
qlearn syllabus --file syllabus.json  # the parts and the pages
qlearn page 3 --file lesson.md        # a lesson, checked before it is stored
qlearn quiz 3 --file quiz.json        # a quiz under the lesson
qlearn debrief 3 1 --file debrief.md --point split-vote=ok
qlearn reply <thread> --body "..."    # answer your question in a thread
qlearn comment 3 --body "..."         # say something outside a lesson
qlearn choose --file choice.json      # ask you to choose the next step
qlearn cheatsheet --file cheatsheet.md
qlearn wait                           # block until you act, print it as JSON
```

If Claude Code runs commands in a sandbox, start `qlearn` yourself, in your
own terminal. The agent commands reach your server through its Unix socket
under `/tmp`, which the sandbox shares with your machine.

A directory of `.learn/` without `course.json` is not a course of qlearn,
and qlearn refuses it. Start a new course with a new slug.

## Build from source

Rust and Node. Node builds the interface. It is not needed to run the tool.

```sh
make setup         # the interface dependencies, once
make build         # the interface, then the binary
make check         # the gate: lint, format, tests, build
make e2e           # the browser tests alone
make shots         # screenshots of both themes, into web/e2e/.shots
make install       # into ~/.local/bin
make dist          # one static x86_64-unknown-linux-musl binary
```

## Documentation

| Document | What it holds |
|---|---|
| [`roadmap/concept.md`](./roadmap/concept.md) | The problem, the users, the non-goals, the prior art |
| [`roadmap/design.md`](./roadmap/design.md) | The architecture, the course model, the format of a page, the API |
| [`roadmap/stack.md`](./roadmap/stack.md) | The technology decisions, with a date each |
| [`roadmap/testing.md`](./roadmap/testing.md) | The test policy |
| [`skills/qlearn/SKILL.md`](./skills/qlearn/SKILL.md) | The skill for Claude Code. It runs `qlearn skill` |
| [`CLAUDE.md`](./CLAUDE.md) | What a Claude session must do |

## License

Apache-2.0. See [`LICENSE`](./LICENSE).
