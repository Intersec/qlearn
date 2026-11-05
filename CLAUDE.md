# qlearn — working notes for Claude

qlearn shows a course on a code base in the browser. An agent in a terminal
builds the course and teaches it through commands of the binary. Rust for
the binary, Vue 3 for the interface, embedded in that binary. It ships as
one file and needs no runtime.

The code follows the steps of section 14 of
[`roadmap/design.md`](./roadmap/design.md), and [README.md](./README.md)
says how to use the tool.
[`roadmap/`](./roadmap/README.md) holds the design. This file holds what a
session must do.

## Start of a session

1. Take the task the user names. The order of the work is section 14 of
   [`roadmap/design.md`](./roadmap/design.md).
2. Read the part of `design.md` that the task touches.
3. Do the task.

Do not start a task that the user did not ask for.

## Before you finish

Leave this green:

```sh
make check   # eslint + prettier + vitest + vite build
             # + cargo fmt + clippy + cargo test + release build
             # + playwright
```

`check` is the whole gate and the only command to run. It prints one line per
step and stays quiet until a step fails. Do not pipe it through `tail` or
`grep`. A Prettier failure carries no word that you can grep for.

Fix the format with `make fmt`.

**You cannot see the interface. `make shots` can.** It writes screenshots of
both themes into `web/e2e/.shots`, from the real binary on a real repository.
Look at them before saying that a visual change works.

## The design is a contract

[`roadmap/design.md`](./roadmap/design.md) holds the course model, the format
of a page, the storage format, the API routes, the commands of the agent and
the export format. Other code depends on them.

If the task needs a different design, stop and report. Name what breaks and
propose the change. Once the user agrees, write the decision in
[`roadmap/stack.md`](./roadmap/stack.md) with the date, then write the code.
Never change the design in silence.

Four parts are contracts with something outside the code:

- **The storage format** is read by a future version of the tool. A change
  owes a `version` bump and a migration.
- **The format of a page** is written by an agent. A change breaks the
  procedure that the agent follows.
- **The procedure of the agent** says how to teach a course and how to
  drive the page. It names the commands of the binary. A test fails when it
  names a command that the binary does not have.
- **The Markdown export of the cheatsheet** is read by a wiki. A snapshot
  test pins it.

## Tests

Read [`roadmap/testing.md`](./roadmap/testing.md). The rules that a session
breaks most often:

- A unit test sits in the file it tests, in `#[cfg(test)] mod tests`.
- **No network in a test.**
- **No mock of the `git` binary.** Git code runs against a real repository,
  with a submodule, built by `testutil::build_repo` in a temporary directory.
- A bug gets a test before the fix. The commit carries both.
- Read a changed `insta` snapshot before you accept it.

## Code style

`rustfmt` and `clippy` own the Rust style, ESLint and Prettier own the
interface. `make check` gates all four, and clippy runs with `-D warnings`, so
a warning is an error.

Never silence a lint with an `#[allow]` without a comment on the line above
that says why.

Two rules that no tool enforces:

- Code comments **in English** and **short**. One line where it fits.
- A comment says **why**, not what. The code says what.

## Rules that hold from the first line

- **The server writes under `.learn/` only.** Its one other write is the line
  `.learn/` in `.git/info/exclude`, when git does not ignore the directory.
  It never runs a command that changes the repository.
- **The server is the only writer of the store**, except for `notes/`. The
  agent writes through the commands of the binary.
- **The server binds `127.0.0.1` only**, with a session token. Beside it, a
  Unix socket in a directory that only the user can enter, for an agent in a
  sandbox, with the same token.
- **An excerpt is read, never typed.** The server reads its lines in the
  object database, submodules included, and refuses a page whose excerpt
  does not match the code.
- **No script of the agent in the page.** The server escapes the HTML of a
  source, and the page passes every HTML string through DOMPurify. If steps
  are not enough, add a widget of the page, with data in JSON.
- **Nothing in the page gives a quiz away.** The right option of a choice
  stays on the server until the learner sends the choices. A hidden line
  stays there until the learner answers every quiz of the page, and while
  the page has no quiz.
- **An answer of the learner is never lost.** A syllabus that drops a page
  with content is refused. A thread whose block is gone moves to the end of
  the page.
- **`git` is a child process**, never a library. The tool must agree with what
  the developer sees in the terminal.
- **The binary carries the interface and the procedure.** A release build
  embeds `web/dist` and `agent/skill.md`. `skills/qlearn/SKILL.md` holds no
  procedure: it runs `qlearn skill`.
- **qlearn never starts an agent and never calls a network service.**

## Established patterns

This section fills as the code grows. Add a line when a pattern becomes the
way the project does a thing. Keep each line short.

- **A refusal of the server lists every fault at once.** A page and a
  cheatsheet give each with its line (`lesson::Fault`), a quiz and a
  choice with its place in the JSON. Each refusal of a page, a quiz or a
  choice gets a `refused-*` case in `crates/qlearn/tests/data/`.
- **A file of the store goes through `write_atomic`.** A write of several
  files returns an `Undo`. A route that fails after it calls
  `undo.after(error)`, so a refused push leaves the store as it was.
- **Each route that answers JSON gets an `insta` snapshot** in
  `api/snapshots/`. It pins the shape that `web/src/api/types.ts` copies
  by hand.
- **Ids.** A block id is 12 hex characters of a SHA-256 of its kind and
  its source (`lesson::block_id`), so an unchanged block keeps its threads.
  A thread is `<page>-<n>`, a choice `choice-<n>`, and neither is removed.
- **The model takes the time as `now`.** Only a route or `main.rs` reads
  the clock, so a test passes a fixed date.
- **A command of the agent is a thin client.** It sends its request
  through `agent/client.rs` and prints the JSON. The checks live on the
  server.
- **Every `v-html` shows a string cleaned by DOMPurify**: `clean()` in
  `web/src/html.ts`, or `diagram.ts` for Mermaid. Its ESLint comment says
  where.
- **The e2e agent is `web/e2e/agent.ts`.** It runs the real commands of the
  release binary. A browser test drives the course through it, never by a
  write to `.learn/`.

## Git workflow

- **Never push, never merge.** The user publishes. A rebase of the current
  branch is expected.
- **Commit at every task**, so the user tests each step alone.
- **Amend rather than stack** while the commit is only on your branch. A
  commit that is on `main` is public. Add a new commit instead.

## Commits

- Say the **why**. A subject alone is enough only for a trivial change.
- English, imperative. Subject 72 characters or less, body wrapped at 72.
- A Conventional Commits prefix in the subject: `feat`, `fix`, `docs`, `test`,
  `refactor`, `perf`, `chore`, `ci`, `style`.
- End with a `Co-Authored-By: Claude …` trailer.

## Changelog

A change that a user can see drops one file under `changelog/`, in the
commit of the change. [`changelog/README.md`](./changelog/README.md) says
how to write it. Never edit `CHANGELOG.md`: `cargo xtask release` writes
it, and the user cuts the release.

## Documentation

`roadmap/` holds the design, the decisions, and the test policy.
`README.md` holds the use.

**This is an open-source project.** Write for a reader who has never seen
your code base. Never commit the name of an internal host, repository,
branch, or service of one organization. A public repository may be named,
such as qlearn itself: <https://github.com/Intersec/qlearn>. Examples use
`myproject`.

Write in English, in short sentences, with one term per concept.
