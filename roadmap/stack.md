# Stack and decisions

This document records the technology choices and the reason for each one. A
decision that changes gets a new entry with a date. Nothing is deleted.

## 2026-10-28 — The stack

**Decision.** The stack below, with `pulldown-cmark` to
split a page into blocks, and Mermaid to draw the diagrams.

| Layer | Choice |
|---|---|
| Command line | Rust, `clap` |
| Server | `axum` on `tokio`, bound to `127.0.0.1`, and a Unix socket |
| Git access | The `git` binary, run as a child process |
| Page source | `pulldown-cmark`, then the checks of the server |
| Colors | `syntect`, CSS classes as output |
| Assets | `rust-embed`, the Vite output inside the binary |
| Interface | Vue 3, TypeScript, Vite, Tailwind CSS, Pinia, DOMPurify |
| Diagrams | Mermaid, bundled, `securityLevel: 'strict'` |
| Tests | `cargo test`, `insta` for snapshots, Vitest, Playwright |
| Style | `rustfmt`, `clippy`, ESLint, Prettier |

**Why.** One static binary, no runtime to install, and the colors
computed on the server.

**What Mermaid costs.** It is the largest dependency of the interface. The
page loads it in a chunk of its own, the first time that a page holds a
diagram. The server cannot run Mermaid, so the page reports a diagram that
fails, and the agent fixes it.

## 2026-10-28 — The agent model

**Decision.** The agent model of qlearn:

- The server binds the loopback address only, with a session token.
- The server also listens on a Unix socket under `/tmp`, for an agent in a
  sandbox.
- `qlearn wait` is a long poll that returns on a request of the learner.
- The server is the only writer of the store. The agent writes through the
  commands of the binary.
- The binary carries the procedure of the agent. `qlearn skill` prints it.

**Why.** The loopback and the token keep the course on the machine. The
socket reaches an agent in a sandbox, which shares `/tmp` but not the
network. One writer lets the page hear of every write. The procedure in
the binary names only the commands that the binary has. One rule of
presence serves the lessons. A request that a `wait`
handed over keeps the agent present for 30 minutes at most. A lesson takes
minutes to write, and the agent must not look gone meanwhile.

## 2026-10-28 — The server reads each excerpt in the repository

**Decision.** An excerpt names a path, lines and a rev. The server reads the
lines in the object database. If the file does not hold them, the server
refuses the page. It resolves the rev to a commit once, at the push, and
stores the hash with the lines. A path inside a submodule is read in the
repository of the submodule, at the commit that the tree names. If that
commit is not in the submodule, the server refuses the page and names the
submodule to update.

**Why.** A course has a rule that the terminal cannot enforce: teach only
what the code says. In the terminal, an excerpt is text that the agent
typed, and nothing proves that the file holds these lines. On the page, the
agent cannot show code that does not exist. The stored hash shows the code
as the lesson taught it, even after the code changes.

The server reads inside submodules too. Without that, a lesson on a library
in a submodule could show no code.

**Rejected: an excerpt as text that the agent writes.** It is what the
terminal does now, and the reason for this decision.

## 2026-10-28 — A small chart format of qlearn

**Decision.** A `chart` fence holds JSON in a format of qlearn: a bar or a
line chart, labeled axes, 8 series at most, one value per `x` value. The
server checks these rules, and the page draws the chart in SVG with the
palette of the page.

**Why.** The server checks the whole format. Every chart of every course has
one style, and the agent cannot write a chart that the page cannot draw.

**Rejected: Vega-Lite.** It weighs more than the rest of the interface, and
the server cannot check a spec in full.

## 2026-10-28 — No script of the agent in the page

**Decision.** A lesson holds no script. To show how a state changes over
time, the agent writes a `steps` fence: columns, then one row set per step,
and the learner plays the steps one by one. The page marks each cell that
changed since the previous step.

**Why.** A script of the agent in the page is a surface that the server
cannot check. The concept says that a lesson holds no script, and the
server escapes the HTML of every source.

**Rejected: simulations that the agent writes in JavaScript.** If steps are
not enough, the way is a new widget of the page, with data in JSON.

## 2026-10-28 — The agent writes the cheatsheet, the server adds the record

**Decision.** At the end of the course, the agent writes the source of the
cheatsheet: the hooks of the lessons, the key tables and diagrams, a
glossary of the names of the code, the pitfalls. The server adds two
sections that it builds from the store:

- **Map of the code.** Every excerpt of the course, with the lesson that
  showed it.
- **To revisit.** Every point whose last result is wrong.

**Why.** The learner has nothing to write. The two sections of the server
are records, so they are true by construction.

**Rejected: the agent writes the two sections too.** The store already holds
them. A copy by the agent can drift from the record.

## 2026-10-28 — Open source, and the binary carries the procedure of a course

**Decision.** qlearn is open source, under Apache-2.0, in a public
repository on GitHub. The binary carries the whole
procedure: `qlearn skill` prints how to teach a course and how to drive the
page. `skills/qlearn/SKILL.md` holds no procedure. Its description tells the
agent when to load it, and its body runs `qlearn skill` and follows it.
qlearn has no terminal mode. The agent teaches the course through the page.

An organization can wrap qlearn in a skill of its own. That skill adds its
sources, such as tickets, a review server or internal docs, and its rules.
Then it follows `qlearn skill`.

**Why.** A learner who installs qlearn gets the teacher with the surface.
The skill and the binary are the same release by construction, whatever
installed the binary. A test fails when the procedure names a command that
the binary does not have.

**Rejected: depending on a private skill for the teaching.** Users outside
the organization would get a surface without a teacher.

**Rejected: the pedagogy in the file of the skill.** A skill copied from
another release names commands that this release does not have.

## 2026-10-28 — A directory without `course.json` is not a course

**Decision.** A directory of `.learn/` without `course.json` is not a course
of qlearn. `qlearn` refuses it, and says to start a new course with a new
slug.

**Why.** `course.json` carries the version of the store, and a change of
the format owes a migration. Another tool may have written a directory
without it. Its files are not a contract.

**Rejected: resume a course from the Markdown files of another tool.** A
model wrote them. Parsing them is not a contract.

## 2026-10-28 — The work comes in four steps

**Decision.** The work follows section 14 of `design.md`:

1. **The loop.** The server, the token, the socket, `wait`, a page of `text`
   blocks, and Next.
2. **The lesson.** The excerpt and its checks, the callouts, the diagram,
   the syllabus.
3. **The quiz.** The quiz, the points, the debrief, the choices, the
   threads, the resume, `qlearn skill` with the whole procedure, and
   `skills/qlearn`.
4. **The record.** The chart, the steps, the images, the cheatsheet and its
   exports, the findings.

**Why.** Each step adds to a loop that already works. Step 1 is done when
an agent teaches a lesson of two pages through the page.

## 2026-10-28 — A fence of code is colored, and a page may come before the syllabus

Two holes of the design, found before the code of step 1.

**Decision.** A fence whose info string names no block kind stays in the
`text` block, and the server colors it as it colors an excerpt. If the agent
pushes a page that the syllabus does not hold, the server adds it at the end
of the last part, with the kind of its id and the title of its first
heading.

**Why.** A lesson needs a command or a sketch of code that is not in the
repository. The page shows such a fence with no path and no commit, so it
does not pass for an excerpt. The syllabus comes in step 2, and a page of
step 1 must still have a place.

**Rejected: refuse every fence that is not a block kind.** A shell command
would have no place in a lesson. **Rejected: show the fence without
colors.** It would look broken beside an excerpt. **Rejected: refuse a page
outside the syllabus.** The syllabus would then move into step 1.

## Open questions

- **The URL of the repository.** The host is decided: a public repository
  on GitHub. Until the repository exists, no document holds its URL.
