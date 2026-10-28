# Test policy

The policy is strict on purpose. qlearn holds the answers of a learner and
the record of a course, which exist nowhere else. It also promises that an
excerpt shows code that the repository holds. A silent failure loses the
answers or shows false code.

## The gate

```sh
make check
```

One command. It runs, in this order: ESLint, Prettier, Vitest, the interface
build, `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, the
release build, and the browser tests. It prints one line per step and stays
quiet until a step fails. A red `check` means the task is not done.

Do not pipe `check` through `tail` or `grep`. A Prettier failure carries no
word that you can grep for.

## Rules

1. **A unit test sits in the file it tests**, in a `#[cfg(test)] mod tests`
   block. An integration test that drives the binary sits in `tests/`.
2. **Shared fixtures live in `crates/qlearn/src/testutil.rs`**, behind
   `#[cfg(test)]`. Corpora of page sources are files under
   `crates/qlearn/tests/data/`.
3. **No network in a test. Ever.** qlearn calls no network service, so no
   test needs one. A route is tested in the process, with no port.
4. **Git code is tested against a real repository.** `testutil::build_repo`
   makes one in a temporary directory with real `git` commands, a submodule
   included. No mock of the `git` binary. A mock agrees with the code that
   wrote it, not with git.
5. **A bug gets a test before the fix.** The test fails, then the fix makes it
   pass. The commit carries both.
6. **A parser owes a corpus.** The parser of a page, the formats of a chart,
   of steps, of a quiz and of a choice are pure functions from text to
   structure. Every shape and every error becomes a case in the corpus,
   pinned with `insta`.
7. **An expectation records what the code does**, not what it must do. A
   case that is wrong on purpose carries a defect code and says so in its
   name. So an unexpected result turns the run red, and a fix turns its own
   case red, which is the reminder to update the expectation.
8. **No coverage number.** A percentage rewards a test on a getter and says
   nothing about the check that refuses a false excerpt. What we count is in
   the table below.
9. **`insta` snapshots cover the shapes that cross the wire.** One per API
   route, and one for the Markdown export of the cheatsheet. Review a changed
   snapshot with `cargo insta review`. Never accept one without reading it.
10. **The procedure names only commands that exist.** A test reads
    `agent/skill.md` and fails when it names a command that the binary does
    not have.

## What must be covered

| Area | The invariant a test must hold |
|---|---|
| Excerpt | The lines come from the object database, at the commit of the push. A file shorter than `lines` is refused, with the line of the source. A range over 60 lines is refused |
| Excerpt in a submodule | A path inside a submodule is read at the commit that the tree names. A commit that the submodule does not hold is refused, and the error names the submodule |
| Hidden line | The page gets `null` for the line until the learner answers every quiz of the page, then gets its text. A page with no quiz keeps `null` |
| Page checks | A page with several errors gets them all, each with its line. A refused page stores nothing |
| Raw HTML | The HTML of a source comes out escaped. An image with a URL scheme is refused |
| Block identity | An unchanged block keeps its id when the agent pushes the page again. Two copies of one block get two ids. A thread whose block is gone moves to the end of the page with its quote |
| Chart and steps | Each rule of sections 4.4 and 4.5 of `design.md` has a case that breaks it |
| Quiz | Each refusal of section 5.1 of `design.md` has a case, with every fault at once. The page never gets `answer` before the learner sends the choices |
| Page state | Each step from `planned` to `debriefed`. A quiz pushed on an answered page takes it back to `taught` |
| Syllabus | A syllabus that drops a page with content is refused. The state and the score come from the store, never from the agent |
| Points | `qlearn points` gives the `missed` and `partial` points first, then the others by the date of their last result. A `partial` point counts 0.5 in the score |
| Storage | A write is atomic. A read of a corrupt file reports an error and never deletes the file. A push of a page over a corrupt page file is refused. A `version` above the one of the binary is refused |
| Delete | `qlearn delete` refuses a course that a server serves, and removes nothing |
| Directory without `course.json` | A directory of `.learn/` without `course.json` is refused, and the error says to start a new course with a new slug |
| Working tree | The server writes under `.learn/` only, and adds `.learn/` to `.git/info/exclude` when git does not ignore it |
| Events | `wait` returns on a request only. An `after` that the server no longer holds answers `"reset": true` |
| Token and socket | A request without the token gets 401, on the port and on the socket. A socket directory that another user owns, or that others can enter, is refused |
| Cheatsheet | Map of the code merges excerpts that overlap. To revisit holds every point whose last result is `missed` or `partial`, and no other |
| Highlighting | The spans of a line cover its text exactly. An unknown extension falls back to plain text and does not fail |
| API routes | Every route answers with the documented shape |
| Procedure | Every command that `agent/skill.md` names exists in the binary |

## The repository fixture

`testutil::build_repo` writes a real repository in a temporary directory,
runs `git init`, sets a fixed author and a fixed date, and removes the
directory when the value is dropped. A fixed date matters: a test that
depends on the wall clock is a test that fails on a Monday.

The builder makes a submodule, because the excerpt walks into one. It also
makes a superproject whose tree names a commit that the submodule does not
hold, because the server must refuse that excerpt.

## The interface

Vitest covers what the browser does alone: the step view, the chart, the
steps of a quiz, the body of a thread.

The types in `web/src/api/types.ts` are written by hand. One `insta` snapshot
per route pins the Rust shape, so a struct that drifts from the TypeScript
turns a Rust test red.

## The browser

`web/e2e/` drives a real browser with Playwright, against the real binary on
a real repository. It is part of `make check`.

- **The browser is the one on the machine.** `QLEARN_BROWSER` names one
  outright.
- **No browser, no lie.** `make check` prints `skip playwright` when it finds
  none, rather than passing quietly.
- **The agent is a script.** It drives the course with the real commands of
  the binary: `qlearn page`, `qlearn quiz`, `qlearn wait`. Nothing in the
  tool changes for it.
- **The token is read from what the binary prints**, never forced in, so the
  test walks the same path as a learner.
- **The store is in the fixture.** The course lives in `.learn/` of the
  temporary repository, so nothing of the person who runs the tests is
  touched.
- **A console error fails a test.** One suite watches the console and expects
  it silent.

`make shots` writes screenshots of both themes into `web/e2e/.shots`. They are
not a test. They are how a person, or an agent with no eyes on the screen,
sees what the interface looks like: a lesson, a quiz, a diagram, a chart, the
cheatsheet.

## Before the CI exists

Run `make check` before every commit. The day a pipeline lands, it runs the
same command, so nothing in this document changes.
