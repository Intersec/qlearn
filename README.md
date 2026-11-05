# qlearn

A course on your code base, in your browser, taught by an agent in your
terminal.

One binary, no runtime to install, nothing leaves your machine.

## Install

One link, always the newest release, for Linux x86-64:

```sh
curl -fLO https://github.com/Intersec/qlearn/releases/latest/download/qlearn-linux-x86_64.gz
gzip -d qlearn-linux-x86_64.gz
install -m 755 qlearn-linux-x86_64 ~/.local/bin/qlearn
qlearn --version
```

On another system, [build it from source](#build-from-source).

## Why

An agent such as Claude Code can teach a subject of a code base as a
course: short lessons, quizzes, code tours, exercises. In the terminal, a
lesson is plain text, a quiz scrolls away with the conversation, and the
learner never sees the syllabus or the score. Nothing proves that an excerpt
in a lesson is the code of the repository.

qlearn starts a small web server on the loopback address, on one course,
and opens the browser. The agent teaches the course through commands of
the binary.

## Use

```sh
qlearn raft-election            # open the course, and create it when it is new
qlearn                          # open the only course of .learn/
qlearn list                     # list the courses of .learn/
qlearn export                   # the syllabus, the scores, the points to revisit
qlearn export --json            # the whole course, as an agent reads it to resume
qlearn delete raft-election     # delete a course, once its server is stopped
qlearn export cheatsheet        # the cheatsheet, as Markdown for a wiki
```

| Option | What it does |
|---|---|
| `--no-open` | Print the URL, open no browser |
| `--port <n>` | Use a fixed port |
| `-v` | Print the course before the URL |
| `--trace` | Print on standard error what the work costs |

What the browser shows:

- The syllabus on the left, in parts and lessons, with the state and the
  score of each lesson.
- The lesson: text, tables, diagrams, charts, step-by-step views, images,
  and excerpts of the code with their colors. The server reads each
  excerpt and each image of the repository in the repository, submodules
  included. It refuses a page whose excerpt does not match the code. An
  image can also come from the notes of the course, where the agent puts
  a figure that it drew.
- The quiz under the lesson: the open question first, then the
  multiple-choice questions, then the debrief of the agent. They stay on the
  page.
- An **Ask** button on each block of a lesson, and one at the end of the
  page for a question about the whole page. Your question opens a thread,
  and the agent answers in it.
- The choices that the agent asks you to make, such as the next step, at
  the end of a page or at the top of the syllabus.
- A line in the top bar that says what the agent does now.
- The grade of each point of an open question, under the debrief of its
  quiz.
- The findings of the course, under the syllabus: what the agent found in
  the code that looks wrong or undocumented, for you to check with the
  team.
- At the end, a cheatsheet of one page. The agent writes it, and qlearn adds
  the map of the code and the points that you missed. Save it as one HTML
  file, as a PDF from the print dialog, or as Markdown for a wiki.

The course lives in `.learn/<slug>/` at the top level of the repository.
If git does not ignore `.learn/`, qlearn adds it to
`.git/info/exclude`. A course stays there until `qlearn delete` removes it.
Deleting the course deletes its cheatsheet, so export the cheatsheet
first. `qlearn delete` names what it removes and asks before it removes
it. `--yes` skips the question.

A subject with no code, such as a paper, needs no repository. Outside a
git repository, the course lives in the nearest directory, from the
current directory up, that holds `.learn/`, or in the current directory
when there is none. Such a course shows no excerpt and no image of a
repository. It shows images of its notes. Without git on the PATH, every directory is outside a
repository.

`qlearn export` asks the server of the worktree for the course. With no
server, it reads the only course of `.learn/`.

`qlearn export cheatsheet` prints the cheatsheet as Markdown on standard
output, the same way. Save it before a delete:

```sh
qlearn export cheatsheet > raft-cheatsheet.md
```

The excerpts come as fenced code under their `path:from-to` and commit,
the diagrams as `mermaid` fences, the charts as tables, and the steps as a
numbered list. The command fails until the agent writes the cheatsheet.

In the page, **Save as HTML** saves the cheatsheet as one file that opens
in any browser, with no server and no network. It holds the diagrams, the
charts, the colored code and the images, and it prints on A4 as the page
does.

## Learn with an agent

An agent in a terminal, such as Claude Code, teaches the course. For Claude
Code, install the skill in [`skills/qlearn`](skills/qlearn/SKILL.md):

```sh
mkdir -p ~/.claude/skills
cp -r skills/qlearn ~/.claude/skills/
```

The skill holds no procedure of its own. It runs `qlearn skill`,
which prints the procedure of the binary on your PATH: how to teach the
course, and how to drive the page. So the skill and the binary are always
the same release. A new release of qlearn needs no new copy of the skill.

Then ask Claude to teach you a subject of your code base. qlearn has no
terminal mode: the agent teaches the course through the page.

An organization can wrap qlearn in a skill of its own. That skill adds its
sources, such as tickets, a review server or internal docs, and its rules.
Then it follows `qlearn skill`.

The agent writes through these commands. Each one finds the server of
the worktree on its own:

```sh
# change the course: its title, its goal, your profile, the quiz mode...
qlearn course --goal "..." --profile "..." --quiz-mode mixed
qlearn syllabus --file syllabus.json  # the parts and the pages
qlearn page 3 --file lesson.md        # a lesson, checked before it is stored
qlearn quiz 3 --file quiz.json        # a quiz under the lesson
# the debrief of quiz 1 of page 3, with the grade of each point
qlearn debrief 3 1 --file debrief.md --point split-vote=ok
qlearn reply <thread> --body "..."    # answer your question in a thread
qlearn comment 3 --body "..."         # open a thread about page 3
qlearn choose --file choice.json      # ask you to choose the next step
# something in the code that looks wrong, for you to check with the team
qlearn finding seed-race --title "..." --body "..."
qlearn status "Writing lesson 4"      # a line in the top bar of the page
qlearn points                         # the points, the missed ones first
# the cheatsheet of the course, checked before it is stored
qlearn cheatsheet --file cheatsheet.md
qlearn wait                           # block until you act, print it as JSON
qlearn wait --after 7 --server <id>   # the next request after the last wait
qlearn export --json                  # the whole course, to resume it
```

A `--file` or a `--body` of `-` reads standard input. A command that
writes prints its result as JSON: what it wrote, or the id of a new quiz
or a new choice. If the server refuses a command, the command prints the
errors on standard error and exits with 1.

If Claude Code runs commands in a sandbox, start `qlearn` yourself, in your
own terminal. The agent commands reach your server through its Unix socket
under `/tmp`, which the sandbox shares with your machine.

A directory of `.learn/` without `course.json` is not a course of qlearn,
and qlearn refuses it. Start a new course with a new slug.

## Build from source

Rust and Node. Node builds the interface. It is not needed to run the tool.

```sh
git clone https://github.com/Intersec/qlearn.git
cd qlearn
make setup         # the interface dependencies, once
make build         # the interface, then the binary
make check         # the gate: lint, format, tests, build
make test          # the Rust and Vitest tests alone
make e2e           # the browser tests alone
make fmt           # correct the format of everything
make dev           # the server and Vite together, with hot reload
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
| [`CONTRIBUTING.md`](./CONTRIBUTING.md) | The commits, the changelog, the versions, the release |
| [`CHANGELOG.md`](./CHANGELOG.md) | What each release changed |
| [`CLAUDE.md`](./CLAUDE.md) | What a Claude session must do |

## License

Apache-2.0. See [`LICENSE`](./LICENSE).
