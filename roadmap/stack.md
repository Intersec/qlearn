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

## 2026-10-28 — Holes found by the review of step 1

The reviews of step 1, and the gaps between the procedure of a terminal
course and the design, left eight questions.

**Decisions.**

- **Two copies of one block.** The hash of each later copy also takes its
  rank in the page, so the two copies get two ids. A block that appears once
  keeps the id of section 4.1.
- **The token on the machine.** The URL with the token is in the command
  line of the browser, and a cookie of `127.0.0.1` goes to every port.
  qlearn accepts both.
- **The current page** travels in `GET /api/session`, as `current`.
- **A hidden line on a page with no quiz** stays hidden.
- **The recall quiz at a resume** is a checkpoint page.
- **A partial result.** A point takes `ok`, `partial` or `missed`. A
  `partial` counts 0.5 in the score, and To revisit lists it.
- **A course is deleted** with `qlearn delete <slug>`, with no server on the
  course.
- **A skill of an organization** asks nothing about the page when `qlearn`
  is on the PATH.

**Why.** Two copies with one id would carry the threads of both. The token
stays on one workstation, for one session; a one-time code would change the
URL of section 1 for little gain. The page cannot open the current page if
nothing names it. The agent pushes a page before its quiz, so the rule of
section 4.2 would show the hidden line at once. A checkpoint is already a
quiz on earlier lessons. The exercise "Your fix" has answers that are half
right, and a binary result loses that. The server is the only writer of the
store, so a course is deleted when no server writes it.

**Rejected:**

- **Refuse a page with two copies of one block.** It blocks an agent that
  repeats a short block.
- **A one-time code in the URL.** It is more code for a risk that qlearn
  already accepts.
- **A page kind for the recall quiz.** It adds a kind and an id format for
  a page that a checkpoint already is.
- **Count `partial` as `missed`.** The learner would lose the part they got
  right.
- **Delete a course through the server.** The server would remove its own
  store under itself.
- **SQLite to keep old courses.** A course already stays in `.learn/`
  until `qlearn delete` removes it, and `qlearn list` lists it.

## 2026-10-28 — Holes found by the quiz format

The parser of the quiz, and the review of it, left three questions that
section 5.1 did not answer.

**Decisions.**

- **Four more refusals of a quiz.** A point id holds lowercase letters,
  digits and hyphens. An open question names at least one point. An entry
  for a known point repeats its label and its lesson, or gives neither. An
  entry that no question names is refused. Section 5.1 lists them.
- **An option holds one line.** A paragraph on two lines, or a line
  break, is refused like a list.
- **A corrupt page file stays as it is.** A push of the page reads the
  file first to keep its quizzes, so it refuses a file that it cannot
  read. The error says to repair it by hand. The procedure tells the agent
  to stop and show the error to the learner.

**Why.** The point id goes into `--point <id>=ok`, so a space or a `=`
would break the command. The answer to an open question is graded on its
points, so a question with none cannot be graded. A new label for a known
point would change the label of the results it already holds. An entry
that no question names is a typo of the agent, and it would add a point
that no quiz checks. An option is one line of a list of options, and a
break in it reads as two options. A page file holds the quizzes, and soon
the answers of the learner: a push that writes over it loses them.

**Rejected:**

- **Accept a new label for a known point.** The results of the point
  would change their meaning.
- **Write a push over a corrupt page file**, as step 2 did. The answers of
  the learner would be lost without a word.
- **Move a corrupt page file aside, as `<id>.json.bad`, and push.** It
  adds a file to the layout of section 10 for a case that a repair by hand
  covers.

## 2026-10-28 — Holes found by the answers

The routes of the answers, section 8, left four questions that sections
5.2 and 5.3 did not answer.

**Decisions.**

- **Each choice takes an option.** The text of the "Other" box comes with
  the option, never in its place.
- **A part is sent once.** A part sent again, or the choices before the
  open answer, gets 409 with the code `conflict`. An empty open answer, a
  pick for each choice missing, and an option out of range get 400.
- **The bodies.** The open answer is `{ text }`. The choices are
  `{ choices: [{ option, other }] }`, one pick per choice, in order. Both
  routes answer the page. Section 8 says so.
- **The page gets the answers in the quiz.** The open question carries
  `sent`, the answer of the learner. Each choice carries `graded`, with
  `picked`, `answer`, `ok` and `other`. Both are `null` until sent, so
  `answer` reaches the page only after the choices.

**Why.** A choice is `ok` or `missed`, and the server grades the option.
Free text in place of an option has no grade. A second answer would add a
second result to the points, and the learner would answer a quiz whose
answer they saw. The page needs what the learner sent to show the quiz
after a reload. A field that stays `null` until the choices keeps the
right option off the wire, as the page type of a quiz did before.

**Rejected:**

- **Other in place of an option.** The server could not grade it.
- **Accept a second answer and keep the last one.** The learner would
  answer again after seeing the right option.
- **Grade the choices in the page.** The right option would reach the page
  before the choices, against section 5.2.

## 2026-10-29 — Holes found by the debrief

The debrief and the points, sections 5.3, 5.4 and 8, left seven questions
that the design did not answer.

**Decisions.**

- **The page gets the debrief in its quiz.** A quiz carries `debrief`:
  `null` until the agent writes it, then its `blocks` and the grade of each
  point of the open question, as `points: [{ id, label, result }]`.
- **A debrief hides no line.** The server refuses `hide` in it, with the
  line of the key. It keeps the hidden lines of the lesson: until the
  learner answers every quiz of the page, a line of a debrief excerpt that
  holds the text of a hidden line goes as `null`.
- **A debrief waits for its answer.** A debrief of a quiz that the learner
  has not answered in full gets 409 with the code `conflict`. It grades
  each point of the open question once, and no other point. A quiz with no
  open question takes a debrief with no points. An empty source is
  refused. A refused debrief gets every fault at once, with the code
  `invalidDebrief`: those of the source in the format of section 4.7, then
  those of the points.
- **A debrief written again replaces the first one.** Its source, its
  blocks and its grades replace those of the first one, so the agent can
  fix a broken diagram. It keeps the stamp of the first one. Each open
  result carries that stamp, so the server grades it again in its place.
  An open result stays in `points.json` even when no page file names its
  stamp.
- **A broken block of a debrief names its quiz.** The `broken` request
  carries the number of the quiz in `id`, so the agent writes that debrief
  again rather than the page.
- **The score of a page.** Each choice and each point of an open question
  counts 1 in `total` once it has a result. The score is `null` while
  nothing has a result. `right` is a fraction in `course.json`, and a
  whole number stays an integer, so the format keeps version 1. At its
  start, the server computes the score of a page that an earlier release
  answered. An earlier release cannot open a course with a fractional
  score, and one that writes a page file drops its debriefs: going back
  to an earlier release is not supported.
- **The points route.** `GET /api/points` answers an array of points.
  Inside each group of section 5.3, the oldest last result comes first. A
  point with no result is the oldest of the others.

**Why.** The page draws the debrief, so it needs the blocks and the grades
in the quiz that it shows. A hidden line in a debrief waits for an answer
that is already sent. An open answer that the learner has not sent cannot
be graded. A grade for a point that the open question does not name would
add a result that no answer gave. A debrief shows before the other
quizzes of its page are answered, and a quiz pushed later makes the page
wait again, so its excerpts must not show a hidden line before the
lesson does. A debrief written twice must not count one answer twice, nor
date it again: the order of section 5.3 reads the last result of a point.
Without the number of the quiz, the agent would look for the broken
diagram in the page and not find it. An integer in an earlier
`course.json` must still read. No release of qlearn is published yet, so
no store of an earlier release needs a way back.

**Rejected:**

- **A field `debrief` in the detail of `broken`.** The `id` of an event
  already names the quiz, as section 8.3 says.
- **A second debrief beside the first.** The page would show two debriefs
  for one answer, and the points would hold two results for it.
- **Refuse a debrief whose excerpt shows a hidden line.** A quiz pushed
  after the debrief makes the page wait again, and the debrief would show
  the line then.
- **Version 2 for the fractional score.** It needs a migration for stores
  that no published release wrote.
- **Store the score as halves in an integer.** The wire would need a
  conversion on each side, for a number that a float holds exactly.
- **A `syllabus` event after a debrief.** The page reads the course again
  on the `debrief` event, which also names the page to read again.

## 2026-10-29 — Holes found by the threads

The threads, sections 6, 8 and 11, left eight questions that the design
did not answer.

**Decisions.**

- **The id of a thread** is `<page>-<n>`, such as `3-2`: the page, and the
  rank of the thread in it, from 1. A page id holds no hyphen, so the id
  names its page. Threads are never removed, so the id stays unique in the
  course.
- **The body of a comment.** The store keeps the Markdown that its author
  wrote. The page gets, in `body`, the HTML that the server writes from it,
  with the HTML of the source escaped as in section 4.1, as for an option
  of a choice. The agent gets the Markdown in the detail of `ask`. Both
  routes answer the thread as the page sees it.
- **Who writes.** Both routes take `author` in the body, and a body with no
  `author` is a write of the learner. A write of the learner is an `ask`
  request. A write of the agent is a `thread` event when it opens a thread,
  and a `reply` event when it writes in one. Neither is a request.
- **The refusals.** An unknown page, a block that the page and its
  debriefs do not hold, and an unknown thread get 404. An empty body, and
  a quote on a thread with no block, get 400. An empty quote is no quote.
  A refusal stores nothing.
- **A block of a debrief** takes threads, as a block of the lesson does.
  Its id also hashes the number of its quiz: the SHA-256 takes
  `\ndebrief <n>` after the rank. A debrief may repeat a block of the
  lesson, or of another debrief, and the page places a thread by the id of
  its block alone. A debrief written again keeps the ids of its unchanged
  blocks.
- **A thread whose block is gone.** The server decides it at each read,
  from the blocks of the page and of its debriefs, and sends those threads
  last. The thread keeps its shape: `block` still names the block that is
  gone. The page shows a thread whose block it does not hold under "On an
  earlier version of this page". A block that a later push brings back
  takes its threads back. The thread still takes a reply.
- **The detail of `ask`** is `{ thread, block, earlier }`: the thread with
  its comments in Markdown, the block as the agent wrote it, and whether
  the block is gone. The store keeps the source of the block in the
  thread, from the moment the thread opened.
- **A thread about the whole page.** The learner asks on a block. The
  agent opens a thread about the whole page with `qlearn comment <id>`, and
  the page shows it at its end, under "About this page".

The page file holds the threads in a new field, `threads`, which an
earlier page file reads as empty. The format keeps version 1.

The address gets `&block=<id>` when the learner presses Ask on a block.
Ask replaces the entry of the history, because back and forward walk the
pages. A reload scrolls to the block, and opens its Ask box again when the
block has no thread yet.

**Why.** An id that names its page lets a route find the thread with no
scan of the course, and the agent reads it at a glance. The agent must read
the question as the learner wrote it, and the page must draw it with no
Markdown parser of its own. One body for both parties would hand one of
them the wrong text. Section 8 gives both routes to both parties, and
section 8.3 counts `ask` among the requests and `thread` and `reply` among
the writes. A debrief is a page source, and its excerpts and diagrams raise
questions as the lesson does. The agent must answer a question on a block
that a push took away, and the page no longer holds its source.

**Rejected:**

- **A random id.** A route would read every page file to
  find a thread.
- **A field `earlier` on the thread of the page.** It changes a type of
  section 6 for a fact that the page reads from the blocks that it shows.
- **A flag in the store, set by the push.** A block that comes back would
  leave its threads at the end, and a debrief written again would need the
  same code.
- **The HTML in the store.** The agent would get HTML in its detail.
- **Refuse a reply in a thread whose block is gone.** The thread holds
  answers of the learner.
- **A field on the thread that names the list of its block**, lesson or
  quiz `n`. It changes a type of section 6, where an id that differs says
  the same thing.

## 2026-10-29 — Holes found by the choices

The choices, sections 7, 8, 8.3, 10 and 11, left ten questions that the
design did not answer.

**Decisions.**

- **The limits** are those of a dialog of a terminal. A choice holds 1 to
  4 questions. A question holds a `header`, a `prompt`, and 2 to 4
  options. An option holds a `label` and a `description`. The header, the
  prompt and the label are not empty, and the header and the label hold
  one line. Two labels of one question differ. The description may be
  left out, and reads as `""`. `multi` may be left out, and reads as
  `false`. Every text is plain text. A refused choice gets every fault at
  once, each with its place in the JSON, with the code `invalidChoice`.
- **The page of a choice** is a page with content. A page that the
  syllabus does not hold is refused, and so is a planned page: the learner
  cannot open it, so the choice would never show.
- **The id of a choice** is `choice-<n>`, such as `choice-2`: its rank in
  the course, from 1. A choice may name no page, so its id names none.
  Choices are never removed, so the id stays unique in the course.
- **`POST /api/choices`** answers `{ id }`. The agent writes a `choose`
  event, with the page or `null`, and the id.
- **The answer.** `POST /api/choices/:id` takes
  `{ answers: [{ picked, other }] }`, one per question, in order. `picked`
  holds the indices of the options, one at most when `multi` is false.
  `other` is the free text, and empty reads as none. A question with no
  pick takes a free text that is not empty. An unknown choice gets 404. A
  pick out of range, two picks on a question that takes one, one option
  picked twice, a missing or an extra answer, and an empty answer get
  400. A choice answered already gets 409 with the code `conflict`. A
  refusal stores nothing. The route answers the choice as the page sees
  it.
- **The detail of `chosen`** is `{ questions, answers }`: the questions as
  the agent wrote them, and for each answer its `header`, its `picked`
  indices, the `labels` of those options, and `other`. The event names
  the page and the choice.
- **The page finds a choice after a reload.** The wire type is
  `Choosing`: `{ id, page, questions, answers }`, with `answers` `null`
  until the learner sends them. `GET /api/pages/:id`, and every route
  that answers a page, carries `choices`: the choices of that page.
  `GET /api/session` carries `choices`: the choices that name no page.
  Both lists hold the open and the answered choices, in the order that
  the agent asked them.
- **`choices.json`** holds `{ choices: [...] }`. Each choice keeps its id,
  its page, its questions, the stamp of the question in `at`, and, once
  answered, `answered: { answers, at }`. A file that cannot be read is an
  error, and is never written over. The file is new, and a course with no
  such file has no choice, so the format keeps version 1.
- **An unreadable `choices.json` refuses only the choices.** The two
  routes of the choices refuse with the error of the file. The session,
  the pages, a push, a quiz answer and a debrief go on with no choices.
  `GET /api/session` carries `choicesError`: the error of the file, or
  `null`. The page shows it above the course.
- **The page shows** a choice at the end of its page, after the quizzes,
  and a choice with no page at the top of the syllabus column, the open
  ones first. On a narrow screen, the button of the column shows a dot
  while a choice there waits.

**Why.** The agent asks through the page what it asked in its terminal, so
the limits of that dialog are enough, and a terminal dialog the agent
already knows maps one to one. A choice on a page that the learner cannot
open would wait forever. An id with no page fits a choice of the column,
and a rank keeps it short for the agent to read. The agent acts on the
answer as soon as it comes, so a second answer would come too late. The
labels in the detail save the agent a second command, as the detail of
every request does. A reload must show an open choice again, and the page
already reads the page and the session. Every command of the agent
probes the session to find the server, and a quiz holds answers of the
learner, so a broken file of the choices must not hide the server or
refuse a quiz.

**Rejected:**

- **A choice on a planned page, shown once the page has content.** The
  agent would wait for an answer that the learner cannot give.
- **A route `GET /api/choices`.** The page reads the page and the session
  already, and a third read on each event adds a request for a list that
  holds a few entries.
- **Indices alone in the detail.** The agent would read its own file again
  to know what the learner picked.
- **Accept a second answer and keep the last one.** The agent may have
  acted on the first one.
- **Markdown in a label.** A label is a short name, and the agent reads it
  back in the detail as it wrote it.
- **Refuse the session and the pages over an unreadable `choices.json`.**
  The agent would read that no server runs, and the page would stay
  blank.

## Open questions

- **The URL of the repository.** The host is decided: a public repository
  on GitHub. Until the repository exists, no document holds its URL.
