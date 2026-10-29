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

## 2026-10-29 — Decisions on the open points of steps 2 and 3

The reviews of steps 2 and 3 left six open points. The user decided them
on this date.

**Decisions.**

- **The earlier decisions stand.** The five entries above on the quiz
  format, the answers, the debrief, the threads and the choices stay as
  written. So does the text of `design.md` on the 403 of a write from
  another site (section 1), on the refusals of `PATCH /api/course`
  (section 8), and on the callouts (section 4.1).
- **A slug that names a command is refused.** `qlearn list` runs the
  command `list`, so a course with the slug `list` could never open. The
  store refuses such a slug for a new course, and the error says to pick
  another one. The binary reads the names from its parser of the
  arguments, `help` included. A course made before its slug became a
  command, by an earlier release or before a new command, still opens
  with `qlearn -- <slug>`.
- **A part of a quiz tells the other tabs.** The open answer and the
  choices each write an `answered` event, with the page and the number of
  the quiz. The other tabs of the page read the page again. `answered` is
  not a request, so `qlearn wait` does not return on it. The `answer`
  request stays as it was, after the last part.
- **The hidden line spans the excerpt.** The page draws it as a blank of
  the full width of the excerpt. The wire carries no length of the line.
  Section 4.2 said "a blank of the same width", which the page cannot draw
  with no length.
- **Each server draws an id.** The server draws a random id at each start.
  The answers of `/api/events` and `GET /api/session` carry it as
  `server`. A party that gets another id takes it as a reset, and reads
  the state again. `qlearn wait` prints the id, and takes it back with
  `--server`. If the id differs, the wait answers `"reset": true` with
  `next` at 0.
- **Two limits are accepted.**
  - The server counts one change of presence for all the tabs, because no
    wait names its tab. With several tabs between two waits, a tab can
    show a presence that is up to 30 seconds old.
  - Mermaid makes `web/dist` about 5 MB, and the release binary about
    12 MB.

**Why.** The reviews found no fault in the earlier decisions. A course that
`qlearn <slug>` cannot open is a course that the learner loses. A list of
names written by hand misses the next command. A second tab showed an open
answer only after a reload, because only the last part made an event. The
length of a hidden line can give the answer away, such as a short name. A
new server numbers its events from 1 again: once it counts past the
`next` of the old one, an `after` of the old one names events of the new
one, and the wait skips a request that came before them. The address of
the server does not tell this to a new `qlearn wait`, which only has its
arguments. A presence that is late by one wait costs less than an id for
each tab. The page loads Mermaid in a chunk of its own, so a page with no
diagram does not read it.

**Rejected:**

- **A fixed list of reserved slugs.** A new command would be missing
  from it.
- **Refuse a course that exists.** It would open by no route at all, and
  the learner would lose its answers.
- **An `answer` request for each part.** The agent would wake for an
  answer that it cannot debrief yet.
- **A blank with the length of the line.** The length is part of the
  answer.
- **A count of events that goes on across starts.** The server keeps its
  events in memory only. A random first number could still fall at or
  below the `after` of the old server, where two ids never match.
- **Compare the address of the server.** One wait sees the address
  change, but the next `qlearn wait` has only its arguments.
- **A count of presence for each tab.** Each wait of the page would need
  an id of its tab, for a delay of 30 seconds at most.

## 2026-10-29 — Holes found by the export

The export, the resume and the delete, sections 8, 10, 11 and 12, left
seven questions that the design did not answer.

**Decisions.**

- **The shape of the export.** `GET /api/export` answers the course, the
  current page, each page with content in the order of the syllabus, the
  choices, the points in the order of section 5.3, the names of the
  notes, and the release. A page holds its source, its quizzes as the
  agent wrote them with the answers, grades and debriefs, and its threads
  as the detail of `ask` gives them. Section 8.4 holds the type. The store
  version is `course.version`, so the export has no field of its own for
  it.
- **The export gives no quiz away.** The right option of a choice stays
  in `graded`, `null` until the learner sends the choices, as in the
  page. A source names a hidden line by its number only.
- **The notes go by name.** The export lists the files under `notes/`,
  relative to it, and never reads their content. A link is listed, never
  followed.
- **A file that cannot be read fails the export**, with its error, a
  broken `choices.json` included. A file of a page that `course.json`
  backs and that is missing fails it too.
- **`qlearn export` works with no server.** It asks the server of the
  worktree when one answers. Otherwise it holds the claim of the worktree,
  so that no server starts and writes the store, and reads the store: the
  only course of `.learn/`, as `qlearn` with no slug finds it. A server
  that holds the claim and does not answer makes it refuse. It settles the course in memory, as a server does at its
  start, and writes nothing. `export` takes no slug, so `qlearn export
  cheatsheet` stays free for step 4.
- **The summary of `qlearn export`** is the outline of `qlearn -v`, then
  "To revisit" with the label, the page and the last result of each
  point whose last result is `missed` or `partial`, or "Nothing to
  revisit.".
- **`qlearn delete` runs on the store.** It holds the claim of the
  worktree until the course is gone. A server of another course does not
  block it, and the command checks again after the answer of the learner,
  since that server may stop and one of this course start meanwhile. A
  server that does not answer blocks it. It names the
  directory, the title, the number of pages, the cheatsheet and the
  number of notes, then asks `[y/N]`. With no terminal and no `--yes`, it
  refuses. A "no" exits with 1. It refuses a slug with no directory, a
  directory with no `course.json`, and a link. It deletes a course whose
  `course.json` cannot be read.

The procedure gets the resume: the export, what waits, then a recall
checkpoint `c<n>` on the first points of `qlearn points`. It gets the end
too: the status `finished`, a choice to keep or delete the course, and the
learner stops the server before `qlearn delete`. It asks for no export of
the cheatsheet before the delete until step 4 adds `qlearn export
cheatsheet`: an agent would send the learner to a command that fails.

**Why.** An agent whose context was reset knows nothing of the course, and
one document is one command. The page has the cookie of the server, so a
route of the agent is a route of the page too: the developer tools would
show what the page hides. The agent does not need a right option before
the answer, because the `answer` request carries it. The notes are files
of the agent, and section 10 says that the server does not read them. A
course with a part missing would mislead the agent into asking a choice or
a quiz again. The learner wants to see where a course stands with no
server running, and a server that does not run writes nothing. A positional
slug would take the word `cheatsheet`. A summary that reuses `qlearn -v`
reads the same in both places. The claim keeps a server off the course
while it goes, and a server writes its own course only. A command with no
terminal cannot ask, and a delete that nobody confirmed loses answers of
the learner.

**Rejected:**

- **The right options in the export.** The page could read them with its
  cookie.
- **The content of small notes.** The server would read files that the
  agent writes, against section 10, for a read that the agent does with
  its own tools.
- **`choicesError` in the export, as in the session.** The agent would
  resume a course with its choices missing, and might ask them again.
- **`qlearn export <slug>`.** The slug would take the word `cheatsheet`.
- **An export through the server only.** The learner could not read the
  course of a stopped server, before a delete for instance.
- **Delete with no claim.** A server could open the course between the
  check and the removal.
- **Refuse a delete while any server runs on the worktree.** A server of
  another course never writes this one.

## 2026-10-29 — The learner asks about the whole page

The entry "Holes found by the threads" let the learner ask on a block
only. Only the agent opened a thread about the whole page. The user
changed this on this date.

**Decisions.**

- **The learner asks about the whole page.** The page shows an "Ask
  about this page" button at its end, before Next. The button opens the
  Ask box under "About this page", after the threads already there. The
  box takes the place of the button until the learner sends the question
  or cancels it.
- **The route stays as it is.** `POST /api/pages/:id/threads` already
  takes `block: null` from both parties, and still refuses a quote with
  no block. A question of the learner about the page is an `ask` request.
  Its detail keeps the shape `{ thread, block, earlier }`, with
  `block: null` and `earlier: false`. A reply of the learner in a thread
  of the agent about the page already had this detail.
- **The address writes `&block=page`.** A block id is 12 hex digits, so
  it never reads `page`. A reload scrolls to "About this page". If the
  page has no thread about it yet, the reload opens the Ask box again, as
  for a block. Cancel takes `&block=page` out of the address under the
  same rule.
- **The threads of an earlier version stay last.** The button sits
  between "About this page" and "On an earlier version of this page".
- **The procedure** says that an `ask` request may have no block. The
  agent answers it with `qlearn reply`, as any other question.

**Why.** A learner asks about the page as a whole: how its parts fit, or
what comes next. No block holds such a question. With no button, the
learner had to pick a block, and the agent read a source that the
question was not about. Sections 6 and 8 already let both parties open a
thread with no block, so only the page lacked a button. A reserved value
keeps the rule of a reload that the threads of a block follow.

**Rejected:**

- **No `&block` for the page.** A reload could not tell a question about
  the page from no open thread, and would not open the box again.
- **A new key, such as `&about=page`.** Two keys for one fact: an
  address could name a block and the page at once.
- **The button in the footer, beside Next.** While the agent works, the
  footer shows its status in place of Next. The box opens under "About
  this page", far from the footer.
- **The button after the threads of an earlier version.** Those threads
  come last on the page, as the entry "Holes found by the threads" says.

## 2026-10-29 — Holes found by the chart

Section 4.4 names the fields of a chart and two rules: one value per `x`
value, and 8 series at most. The code needed more answers.

**Decisions.**

- **The page gets the chart as JSON.** The block is
  `{ id, kind: "chart", chart }`, and `chart` has the fields of the fence.
  Section 4.4 holds the type. The texts stay plain text, and the page
  escapes them when it draws them.
- **Every field is checked at once.** A fault names the line of the fence
  and the JSON path of the field, such as
  `series[2].values: holds 3 values, and x holds 4`. A JSON syntax error
  names its own line in the page. A field that the format does not name
  is refused, at every level.
- **`x` values are categories.** A string or a number, which the server
  turns into its text. The page draws them at equal steps, in order, even
  in a line chart. No value comes twice.
- **`null` is a gap in a line chart.** A bar chart refuses it: a missing
  bar would read as a zero. A series of `null` only is refused.
- **`unit` is optional.** It is `""` on the wire when the values have
  none, such as a count.
- **The limits.** `x` holds 1 to 50 values. A bar chart draws 60 bars at
  most, `x` values times series. A title holds 100 characters, an axis
  label 60, an `x` value 24, a series name 40, a unit 12. Each text holds
  one line. Two series never share a name, because the legend tells them
  apart by name.
- **The size of a value.** A value lies within ±1e15. The y axis spans
  0.0001 at least, and at least a billionth of the largest value: from 0
  in a bar chart, from the lowest value in a line. A fault asks for
  another unit of y.
- **A tick in the thousands** is short, such as `1.5K`, when the step
  allows two decimals at most. A finer step writes the whole number, such
  as `1,505`, so that two ticks never share a text.
- **The drawing.** A bar chart starts its axis at zero. A line chart
  takes the range of its values. The ticks are round numbers, 1, 2 or 5
  times a power of ten. The page shows a legend for 2 series or more, the
  values of an `x` value under the pointer, and a table of the values for
  a screen reader.
- **The colors.** Eight tokens of the page, `--series-1` to `--series-8`,
  in a fixed order, with their own values in the dark theme. Each pair of
  neighbors stays apart for a reader with a color deficiency. A series
  always takes the color of its rank in the fence.

**Why.** The page draws every chart that the server takes. A chart of more
than 50 points or 60 bars does not fit a narrow screen. A label that is
too long overlaps the next one. A number as an `x` value is natural for a
count of servers or a second, and the agent should not need to quote it.
A gap is how a line shows a server that was down. A bar has no such
reading. The page writes a tick with 6 decimals at most and computes in
floats: a narrower axis, or a larger value, would show the same text at
each tick or no tick at all.

**Rejected:**

- **A numeric `x` axis for a line chart.** The format would need a second
  kind of axis, and the server could not check the spacing that the agent
  means.
- **A color in the fence.** Every chart of every course keeps one style,
  as section 4.4 wants.
- **A chart library.** The page draws two kinds of chart, and a library
  weighs more than the code that draws them.

## 2026-10-29 — Holes found by the steps

Section 4.5 names the fields of a steps block and two rules: the rows
match the columns, and a step holds a diagram in place of rows. The code
needed more answers.

**Decisions.**

- **The page gets the steps as JSON.** The block is
  `{ id, kind: "steps", steps }`. Section 4.5 holds the type. The body of
  a step crosses the wire as `html`, written by the server as an option of
  a choice: inline Markdown, the HTML of the source escaped. The title,
  the columns, the labels and the cells stay plain text, and the page
  escapes them.
- **Every field is checked at once.** A fault names the line of the fence
  and the JSON path of the field, such as
  `steps[1].rows[0]: holds 3 cells, and columns holds 4`. A JSON syntax
  error names its own line in the page. A field that the format does not
  name is refused, at every level.
- **A step holds `rows` or `diagram`, never both.** A field at `null`
  counts as left out. `columns` is required when a step holds rows. A
  block that holds only diagrams takes no `columns`, and the wire then
  carries `[]`.
- **A body** holds one line of inline Markdown: no list, no heading, no
  fence, no second paragraph. It is never empty.
- **A cell** is a string or a number, which the server turns into its
  text. An empty string is an empty cell, such as no vote yet. The server
  trims each text.
- **A step diagram** follows the checks of a `mermaid` fence: not empty,
  200 lines at most.
- **The limits.** 1 to 30 steps, 1 to 12 columns, 1 to 40 rows per step.
  A title holds 100 characters, a label 60, a column name 40, a cell 60,
  a body 400. Each text holds one line. Two columns never share a name.
- **A changed cell.** The page compares each row with the row at the same
  index in the table of the nearest earlier step that holds rows. A row
  that the earlier table did not have is new, and each of its cells is
  marked. The first table marks nothing. A changed cell is bold, on the
  color of a marked line, with a dot before its text, and a screen reader
  reads "changed, was ..." after it.
- **A broken step diagram.** The page reports the steps block through
  `POST /api/pages/:id/broken`, with no new field. The error starts with
  the JSON path of the step, such as `steps[2].diagram: Parse error ...`,
  so the agent finds the step in the fence. The `broken` request hands
  the agent the whole fence as the source of the block. A steps block
  with no diagram cannot break, and the route refuses it. The page checks
  every step diagram when it shows the block, so the agent hears of a
  broken one before the learner plays up to it.
- **The player.** The first step shows at load. Previous and Next play
  one step, and a counter reads `2 / 5`. When the view has focus, the
  left and right arrows play one step, and Home and End go to the first
  and the last step.
- **Predict needs no code.** The procedure puts the quiz after the steps,
  as on any page, and tells the learner on which step to stop.

**Why.** The page plays every steps block that the server takes. More than
30 steps is a story that the learner loses track of, more than 12 columns
does not fit a narrow screen, and more than 40 rows hides a change below
the fold. A body that holds a block would break the line of the step. Rows
compared by index need no key column, and the agent controls the order.
Bold and a dot mark a change for a reader who does not see the color. A
report with no new field keeps section 8 as it is.

**Rejected:**

- **Rows compared by their first cell.** The format would need a key
  column, and a key that changes would read as a row removed and a row
  added.
- **A field of the step in the broken body.** It changes the route of
  section 8 for one case, where the text of the error carries the step.
- **The changed cells computed by the server.** The page draws the
  tables, and the rule stays in one place.

## Open questions

- **The URL of the repository.** The host is decided: a public repository
  on GitHub. Until the repository exists, no document holds its URL.
