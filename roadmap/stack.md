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

## 2026-10-29 — Holes found by the image

Section 4.6 says where an image comes from, the five types, and that a
URL with a scheme is refused. The code needed more answers.

**Decisions.**

- **The page draws an SVG from an `<img>`.** Section 4.6 said that an SVG
  goes through DOMPurify. A browser runs no script of an `<img>`, loads
  nothing for it, and keeps its styles out of the page. DOMPurify would
  put the SVG in the page, where a hole in its list reaches the cookie.
- **The media route** sends the bytes with the type of the name,
  `X-Content-Type-Options: nosniff`, and
  `Cache-Control: private, max-age=31536000, immutable`. Its policy is
  `default-src 'none'; style-src 'unsafe-inline'; img-src data:; sandbox`:
  an SVG that the learner opens in a tab of its own draws, and runs no
  script. The token guards it, as every route. A name that is not 64 hex
  digits, a dot and an extension of the list answers 404, so no name
  leaves `media/`.
- **The block on the wire** carries the name of the copy, the alt text,
  the path, the repository and the commit, as an excerpt does, the type
  as a media type, and the width and the height. Section 4.6 holds the
  type.
- **The size** comes from the header of a PNG, a GIF, a JPEG or a WebP. An
  SVG gives its `width` and `height` in pixels, or its `viewBox`. The size
  is `null` when the file does not say it, such as `width="100%"`. The
  page uses it to keep the room of the image before the image loads.
- **The type comes from the bytes**: the magic bytes of each raster type,
  and UTF-8 text whose first element is `<svg` for an SVG. An extension
  that names another type than the bytes is refused. A file with another
  extension is refused before the read.
- **An SVG that declares entities is refused.** An entity that expands to
  itself many times over makes a small file a large one to draw. A file
  named `.svg` gets the reason of its refusal: entities in its doctype,
  or text that is not UTF-8. A drawing program often declares entities.
- **An image holds 5 MB at most.** git gives the size first, so a larger
  file never reaches memory.
- **The path** follows the rules of an excerpt: from the top level
  directory of the worktree, with no leading `/`, no `.` and no `..`. A
  link with a host, such as `//example.com/a.png`, counts as a URL.
- **The path is percent-decoded** before these checks, since a link is a
  URL: `docs/my%20figure.png` names `docs/my figure.png`, as on GitHub.
  `<docs/my figure.png>` names it too. A `%` that starts no escape
  stays. A path that is not UTF-8 once decoded is refused.
- **The alt text is required.** A screen reader reads it in place of the
  image. Its Markdown comes out as plain text.
- **An image alone in a list item, a quote, a callout or a table cell is
  refused**, as a fence of a block kind is. An image inside a line of text
  stays text, as before: the page shows its alt text, and the path as its
  title. A title in `![alt](path "title")` is left out.
- **The copy is written with the page.** The parse keeps the bytes, and
  the store writes them into `media/` before the page file, so a page
  never names an image that is not there. A name that is there already
  holds the same bytes and is not written again. A push that fails after
  this write removes the images that it added.
- **The same rules hold in a debrief.**

**Why.** An image of the repository is read where the excerpt is read, so
a page shows only what the repository holds at `HEAD`. A file whose name
lies is most often a mistake of the agent, and the route sends the type
of the bytes. 5 MB holds any screenshot or diagram, and the store keeps
one copy of each. An alt text is the only part of an image that a screen
reader can give.

**Rejected:**

- **An SVG inlined after DOMPurify.** It would run in the page, and its
  styles would reach the page.
- **An image read in the worktree.** The page would show a file that no
  commit holds.
- **An image inside a line made a block.** The line would break around
  it, and the agent wrote a line.
- **The type from the extension alone.** A PNG named `.svg` would be
  served as an SVG.

## 2026-10-29 — Holes found by the findings

Sections 3, 8, 10 and 11 give the type of a finding, its route, its
event and its command. The code needed more answers.

**Decisions.**

- **A finding is written whole.** `PUT /api/findings/:id` takes
  `{ title, body, checked }`. It creates the finding, or replaces the one
  with its id in place, so the list keeps the order of the first writes.
  `checked` left out is `false`: a finding written again with no
  `--checked` is to check again. The route answers the finding as stored,
  and pushes a `finding` event of the agent with the id in `id` and no
  page.
- **The id** is lowercase words of letters and digits, joined by single
  hyphens, 40 characters at most, such as `vote-race`. A hyphen at an end
  or twice in a row is refused: it is most often a typo, which would write
  a second finding.
- **The title** is trimmed, not empty, on one line, and 120 characters at
  most. The page escapes it. **The body** is Markdown, trimmed, and may be
  empty.
- **The refusals.** A bad id, a bad title and a field that is missing,
  unknown or of the wrong type come back at once, one line each with its
  field, as `invalidFinding`, and store nothing. The route walks the body
  by hand, as for a syllabus, because serde stops at the first fault. An
  empty id is refused as any other bad id. A body that is not JSON gets
  `badRequest`. `qlearn finding` prints each line on standard error and
  exits 1.
- **The body on the wire.** `course.json` keeps the Markdown that the
  agent wrote. The session sends, in `body`, the HTML that the server
  writes from it, with the HTML of the source escaped as in section 4.1,
  as for a comment. The route, the export and every other answer to the
  agent keep the Markdown. The export holds the findings in `course`, so
  section 8.4 needs no new field.
- **The page** shows the findings at the bottom of the syllabus column,
  under a button "Findings" with their count and the count of those to
  check. The button opens the list in the column: each finding with its
  title, its body, and a badge "To check" or "Checked". The column shows
  nothing of the findings while the course has none. A `finding` event
  reads the session again. The learner edits no finding, and the address
  does not name the list.
- **A path segment of a command** writes a space as `%20`. It wrote `+`,
  which a path keeps, so a refused id named another id.

**Why.** One write for a new finding and a changed one keeps the command
of section 11 as it is, and an id that the agent picks lets it mark the
finding checked later. A short id and a title on one line fit the narrow
column. The page draws Markdown with no parser of its own, and the agent
must read back what it wrote. The column already lists what the course
holds, and the list closed by default keeps the syllabus in sight. A list
in the column needs no new place in the address of section 8.1.

**Rejected:**

- **A route for `checked` alone.** Section 8 names one route of the
  findings.
- **A view of the findings in the place of the page.** Section 8.1 would
  need a new place in the address, and the learner would leave the page.
- **The HTML in `course.json`.** The agent would get HTML in its export.
- **The findings in the summary of `qlearn export`.** Section 11 says what
  the summary prints. The page and the JSON export hold the findings.

## 2026-10-29 — Holes found by the cheatsheet

Sections 8, 9, 10 and 11 give the routes of the cheatsheet, its two
sections, its file and its command. The code needed more answers.

**Decisions.**

- **The source is a page source** with its own place in the parser. It
  takes every block of a page, images included. A `hide` is refused, with
  the other errors: the learner keeps the sheet to read, and answers no
  quiz on it. Its block ids hash the word `cheatsheet` too, as those of a
  debrief hash their quiz. `PUT /api/cheatsheet` takes the source as its
  body, as a push of a page does, replaces the cheatsheet before it, and
  pushes a `cheatsheet` event of the agent with no page and no id. A
  refused source gets `invalidCheatsheet`, one line per error with the
  file `cheatsheet.md`, and stores nothing.
- **The blocks are stored in `cheatsheet.json`**, beside `cheatsheet.md`,
  as `pages/<id>.json` sits beside its source. A read never parses the
  source again: a parse resolves `rev` again, and an excerpt would move to
  another commit. The blocks file is written last, so a cheatsheet with
  no blocks file reads as not written. Its corrupt file is an error that a
  new write repairs: it holds nothing of the learner. The file is new, and
  an earlier release ignores it, so `version` stays 1. Section 10 names it.
- **The two sections of the server are built at each read**, from the
  page files and `points.json`. A result that the learner gets after the
  write shows at the next read.
- **Map of the code** takes every excerpt of the lessons and of their
  debriefs, never those of the cheatsheet. Excerpts of one path at one
  commit that overlap or touch merge into one range, which names each page
  that showed a part of it, once, in the order of the syllabus. One path
  at two commits gives a range for each commit: the same line numbers may
  name other lines. The ranges come by path, then by commit in the order
  that the course first showed it, then by line.
- **To revisit** holds every point whose last result is `missed` or
  `partial`, in the order of section 5.3, with its id, its label, its
  result, its lesson, and the title of that lesson. The title is `null`
  when the syllabus does not hold the lesson.
- **`GET /api/cheatsheet` answers `{ written, blocks, map, revisit }`.**
  Before the write, `written` is `false` and every list is empty, with no
  other read of the store. The page asks at each load, and a 404 would be
  an error in its console. `PUT` answers the same shape.
- **A line that a page still hides stays hidden in the cheatsheet**, as
  in a debrief: the server puts `null` in place of each line of an excerpt
  whose text is the hidden line of a page with a quiz to answer.
- **The page** puts an entry "Cheatsheet" at the end of the syllabus, under
  the pages. It is quiet and opens nothing until the agent writes the
  cheatsheet. A course that is finished shows the note "Cheatsheet ready"
  under the bar. The cheatsheet takes the place of the page, and the
  address says `#cheatsheet` (section 8.1), so Back and a reload find it.
  It lays the blocks of the agent, then Map of the code, then To revisit,
  in two columns on the width of an A4 sheet, with no Ask button: it is no
  page of the course. Each lesson that it names opens. A course that is
  finished shows the note that deleting `.learn/<slug>/` deletes the
  cheatsheet, and a button that opens the print dialog. The exports of
  step 4.6 join that button.
- **Print** shows the page alone: no bar, no column, no note, no button,
  and no thread. It sets A4 with margins of 12 mm, keeps a block in one
  column, and keeps a heading with the block under it. The dark tokens
  hold on the screen only, so print is dark ink on white in both themes.
  Mermaid writes its colors into the SVG, so print inverts a diagram that
  it drew in the dark theme.
- **A diagram of the cheatsheet that Mermaid refuses** shows its error in
  place, with no promise of a fix, and sends no `broken` request: that
  request names a page, and section 8 has no route for the cheatsheet.
  The procedure tells the agent to take the diagrams of the lessons, which
  drew already.

**Why.** The cheatsheet is a page source, so the agent writes one format
and the server checks it with the same code. Stored blocks keep the
excerpts at the commit that the agent saw. The records are true at each
read because they are built at each read. A place in the address costs
one fragment and gives Back and a reload for free.

**Rejected:**

- **The rendered form built at each read from the source.** A `rev` such
  as `HEAD` would name another commit after a new commit.
- **The two sections written into `cheatsheet.json`.** They would drift
  from the store with each answer after the write.
- **One range for one path at two commits.** It would name lines that one
  of the commits does not hold.
- **A 404 before the write.** The page would log an error at each load.
- **The print colors said again for print.** A third copy of the light
  tokens, where `@media screen` on the dark ones gives the same result.

## 2026-10-29 — Holes found by the Markdown export

Sections 8, 9 and 11 give the route, the command and the outline of the
Markdown export of the cheatsheet. The code needed more answers.

**Decisions.**

- **One function renders the Markdown**, `cheatsheet::markdown`. The
  route `GET /api/cheatsheet.md` and `qlearn export cheatsheet` with no
  server both call it. It renders the cheatsheet as the page gets it:
  the stored blocks, with each line that a page still hides as `null`,
  then Map of the code and To revisit, built at the read.
- **The Markdown of a text, a callout and a step body comes from
  `cheatsheet.md`.** The blocks hold them as HTML only. The server splits
  the source and finds each block by its id, as a thread finds the source
  of its block. The split reads no repository, so no `rev` moves: the
  excerpts, the charts and the images come from the blocks. A block that
  the source does not hold fails the export, and the error says to write
  the cheatsheet again. So does a missing `cheatsheet.md`.
- **A text block** is its source, as the agent wrote it, tables and fences
  of code included. Raw HTML stays as written: the page never renders the
  file, and a wiki applies its own rules. The source of a block starts at
  the start of its first line, where the parser starts it after the
  indent: an indented code block stays code.
- **A link reference definition** gives no event, so a definition outside
  every block, such as one at the end of the file, belongs to no block. The
  export writes these definitions after the last block, in their order in
  the source, so a reference link of a text or a callout still resolves.
- **A callout** is its quote, with the marker replaced by a bold label of
  the page, such as `**🧠 Hook.**`. Every wiki draws a quote. The alerts of
  GitHub have five types of their own, few wikis draw them, and a wiki
  that does not shows `[!NOTE]` as text.
- **An excerpt** is a line `` `path:from-to` at `<commit>` ``, with the 7
  characters of the commit that the page shows. A file of a submodule
  adds `` in submodule `<repo>` ``. A fence in the language of the
  excerpt follows. A hidden line shows `▒▒▒ hidden line ▒▒▒`, which gives
  no length away. A fence has no marks, so the head line names the marked
  lines: `, lines 3-4 marked`.
- **A diagram** is a `mermaid` fence of its source.
- **A chart** is its title in bold, a line `Values: <y label>, in
  <unit>.`, and a table: the x values as rows, a column for each series,
  the numbers aligned to the right. A gap is an empty cell. The kind, bar
  or line, is lost: a table has none.
- **Steps** are the title in bold, then a numbered list. Each item holds
  the label in bold and the body, then the table of its rows, or a
  `mermaid` fence of its diagram. A full table per step keeps every state
  readable alone. The page marks the changed cells, and the Markdown does
  not.
- **An image** is a line `` **Image**: <alt> (`path` at `<commit>`) ``. A wiki
  cannot load a media file of the server, and the path names the file in
  the repository.
- **The two sections** are the headings `## Map of the code` and `## To
  revisit`. A range is a list item with its place and each page that
  showed it, as `(The timer, page 1; The vote, page 2)`. A point is
  `- **Missed**: <label> (<title>, page <id>)`, with no title when the
  syllabus does not hold the page. An empty section says what the page
  says: "The course showed no code." and "Nothing to revisit.".
- **Plain text is escaped.** A title, a label, a cell, an alt and a page
  title are text in the page, never Markdown, so their Markdown signs get
  a backslash. A fence and a code span take more backticks than their
  text holds. A `|` in a cell is escaped. A code span never goes in a
  cell, and outside a table a backslash shows in a code span, so a code
  span keeps its `|`.
- **Before the write, the route answers 404**, with `notFound` and a
  message that names `qlearn cheatsheet`. The page asks only from its
  button, which shows once the cheatsheet is written, so its console
  stays clean. `qlearn export cheatsheet` fails with the same message,
  through the server or with none.
- **The answer** has `Content-Type: text/markdown; charset=utf-8`,
  `X-Content-Type-Options: nosniff`, and `Content-Disposition: attachment;
  filename="<slug>-cheatsheet.md"`. A slug holds no quote, so the name
  needs no escape. The token guard checks it, as every route.
- **`qlearn export cheatsheet`** is a subcommand of `export`, and it
  conflicts with `--json`. It reads as `qlearn export` reads: the server
  of the worktree, or with no server the only course of `.learn/`,
  settled in memory under the claim. It writes nothing, and prints the
  Markdown on standard output.
- **The page** puts a link **Save as Markdown** beside **Print or save as
  PDF**. A link of the same origin carries the cookie, and its `download`
  attribute names the file. Print hides it with the bar.

**Why.** The source holds the Markdown that the agent wrote, and the HTML
of the server cannot become Markdown again without loss. The blocks hold
what the server read in the repository at the write. A quote and a table
render on every wiki. A file that a wiki stores must not point to the
server of one session.

**Rejected:**

- **Markdown written back from the HTML of the blocks.** It loses the
  Markdown of the agent, and needs a converter.
- **A new parse of `cheatsheet.md`.** It resolves `rev` again, and an
  excerpt would move to another commit.
- **The alerts of GitHub for a callout.** They have no hook, teaser or
  pitfall, and render as text on most wikis.
- **A comment in the code for a marked line.** Its syntax depends on the
  language, and it changes the code.
- **An empty answer before the write.** A saved file with no content
  would look like a cheatsheet.

## 2026-10-29 — Holes found by the HTML export

Section 9 says that the page builds the HTML export: one file with inline
CSS, the diagrams as inline SVG, and the code with its colors. The code
needed more answers. The screenshots of the cheatsheet also showed two
faults of its layout.

**Decisions.**

- **The file is a copy of the sheet on the screen.** The button **Save as
  HTML** sits beside **Print or save as PDF** and **Save as Markdown**. The
  page copies the element of the sheet into a new document, then changes
  the copy. It downloads the file through a `Blob` and a link with
  `download`. The name is `<slug>-cheatsheet.html`, by the rule of the
  Markdown file. The title of the document is `Cheatsheet: <course title>`.
- **The CSS is the rules of the page, filtered.** A style rule stays when
  its selector matches an element of the file. A pseudo-element counts as
  its element. A rule that the browser cannot test, such as `:host`,
  stays. The blocks `@media`, `@supports`, `@layer` and `@container` keep
  the rules inside them that stay, and `@page` stays. A `@media` block
  whose condition names the dark color scheme is left out, with
  `@keyframes`, `@property` and `@font-face`.
- **The file is light only.** It keeps the light tokens on `:root`, and a
  `color-scheme` meta of `light`. Mermaid writes its colors into the SVG,
  so a dark file would need a second drawing of each diagram. The file is
  a document to read and to print, and print is light already.
- **Each diagram is drawn again, in the light theme.** The page may show
  the dark theme. The light palette is the custom properties of the
  `:root` rules that no `@media` holds. A new drawing gets a new id, and
  Mermaid prefixes its markers and its style by that id, so two diagrams
  never share an id in the file. The `<style>` inside the SVG stays. A
  diagram that Mermaid refuses shows its error, as on the screen, even
  when the page had not drawn it yet.
- **Mermaid draws one diagram at a time.** Its config is global:
  `initialize` sets it at once, and a queued render reads it when it
  runs. `diagram.ts` chains every set-up, parse and render on one
  promise, so the light drawing of the file and a drawing of the page
  never take each other's palette.
- **A chart keeps the SVG that the page drew.** The colors of its series
  are written as values in its inline styles. The rest of its look comes
  from the rules of the file. The targets of the pointer and the tooltip
  are left out. The chart keeps the width of its column, and scales down
  in a narrower box.
- **The cheatsheet shows every step of a steps block, one after the
  other**, on the screen too. A file runs no script, and print shows one
  state of the player only. Each step shows its number, its label, its
  body, then its table with the changed cells, or its diagram. The sheet
  on the screen is then what print and the file show. A long steps block
  may go on in the next column, but a step stays whole.
- **An excerpt keeps the colored spans of the page**, with the class rules
  of the syntax copied. A hidden line stays a blank: the page never had
  its text.
- **An image is a `data:` URI**, from the bytes of `GET /api/media/:name`,
  with the type that the server read in the bytes. The images of a file
  hold 8 MB at most before base64. An image that does not fit, or that the
  server does not send, leaves a note with its alt text in its place. An
  image of the server holds 5 MB at most, so the budget takes the largest
  one.
- **The file runs nothing and loads nothing.** It holds no `<script>`, no
  button, no comment and no event attribute. Every link becomes its text:
  a page of the course or of the server does not exist outside the app. A
  web link of the agent loses its address too, and the Markdown export
  keeps it. An `<img>` with no `data:` source is left out. The `xmlns`
  attributes of an SVG are left out: they name a namespace by an `http`
  URL, and HTML needs none. A URL that the agent wrote as plain text stays
  text.
- **Every HTML string of the agent went through DOMPurify** before it
  reached the sheet on the screen. The copy keeps that HTML as it is.
- **A wide diagram spans both columns.** Mermaid draws its text at 14px. A
  diagram wider than 400px, from its `viewBox`, would draw that text below
  11px in one column. The page marks it with `data-wide`, and its block
  takes `column-span: all`, on the screen and in print. So does a block
  that ends with a heading just before it. A chart stays in its column: the
  page draws it at the width of the column, with text at 12px.
- **A long line of an excerpt wraps in the cheatsheet**, on the screen as
  in print. It scrolled in its box, but a system that hides scroll bars
  showed the line cut at the edge of the column. The sheet is read whole.
  The cells of a step table wrap too.

**Why.** A copy of the sheet on the screen keeps one source of the layout:
the file follows each change of `main.css` with no second stylesheet to
keep in step. Only the page has the drawn diagrams and charts, as section
9 says. A file that loads nothing opens years later, offline, on any
machine. The budget keeps a file that a mail or a wiki takes.

**Rejected:**

- **A stylesheet of the file, kept beside the view.** A third copy of the
  tokens and of the rules of the blocks, which drifts from the page.
- **The dark theme in the file.** Each diagram would come twice, or print
  would need the filter that inverts it.
- **The player in the file.** It needs a script, and the file has none.
- **Every diagram, chart and steps block across both columns.** A small
  diagram and a chart read well in one column, and the sheet must fit on
  one page.
- **An image left out whole.** A small figure is often the point of the
  sheet.

## 2026-10-30 — The page waits for the agent where the agent owes an answer

**Decision.** A thread whose last comment is the learner's shows "The
agent writes its answer here." and no box to reply. A page with a quiz
that the learner answered and that has no debrief shows no Next: the end
of the page says that the next page comes after the debrief. If no agent
waits on the server, it says to run `qlearn wait`. Both waits show the
dot of the other waits of the agent.

**Why.** A second message before the answer only asked again, and a
learner who moved on before the debrief skipped the end of the lesson.
The procedure writes the debrief of every quiz, so Next always comes
back.

**Rejected:** a box to reply that is there but disabled. It looks like a
fault, and it says nothing of the wait.

## 2026-10-30 — A course outside a git repository

**Decision.** qlearn opens a course outside the work tree of a git
repository. The directory of the course takes the place of the worktree:
the nearest directory, from the current directory up, that holds
`.learn/`, or the current directory. Its id comes from its real path, as
the id of a worktree does. The server writes nothing to git. An excerpt
or an image is refused with its line, since there is no object database
to read. A fence of code stays legal. Inside a repository, nothing
changes. The procedure gets a case for a subject with no code: the
sources of the learner take the place of the code.

**Why.** A course on a paper or a protocol has no code. A course taught
in the terminal of an agent runs in any directory, but qlearn refused to
start outside a repository, so those subjects lost the page. Every
widget of a page but the excerpt and the image needs no git.

**Rejected:**

- **An empty repository as the way to do it.** It works today, but the
  learner must know the trick, and the course still offers excerpts that
  can show nothing.
- **A store in the home of the user**, such as `~/.local/share/qlearn/`.
  It changes the lock, the address of the socket and `qlearn list` for
  every course. A `.learn/` in the directory where the learner works keeps
  one model.
- **The current directory alone, with no walk up.** An agent command run
  from a directory below would not find the server, where a repository
  finds it.

**Code.** `Root` in `root.rs` is the outcome of the discovery: a
`Repository` with its `Git`, or a `Directory`. The parser takes the `Git`
as an `Option`, so no code reads the object database without a
repository. Outside one, each excerpt and each image is refused with no
check of its keys, since the fix is to take it out.

A broken repository stays an error. qlearn falls back to a `Directory`
only when `git rev-parse --show-toplevel` exits with 128 and says
`fatal: not a git repository`, or when git is not on the PATH. git says
the same of a `.git` that it cannot read, such as an empty directory or
a file that names no directory. So qlearn also looks for a `.git` from
the current directory up, where git looks: it stops below a directory of
`GIT_CEILING_DIRECTORIES`, and at the edge of the file system. A `.git`
found there, or a `GIT_DIR` that is set, is an error. Any other message
of git, such as a `safe.directory` refusal, is an error too.

git may write a trace or a warning before its fatal line, so qlearn
looks for that line in all of stderr, not at its start.

Outside a repository, qlearn refuses a `.learn/` that another user owns.
git refuses a repository of another user for the same reason: in a
directory such as `/tmp`, that user could hold the claim, or write the
`server.json` that the agent commands trust.

A stray empty `.learn/` above a directory would take every course below
it. So a command that fails with no `.learn/` does not create one:
`qlearn` checks its slug, and `qlearn export` looks for a course, before
the claim makes the directory.

The cheatsheet keeps Map of the code outside a repository, with the line
"The course showed no code.". The page, the HTML file and the Markdown
export then show the same sections in every course, and nothing that
crosses the wire changes.

## 2026-10-30 — An image of the notes of the course

**Decision.** An image whose path starts with `.learn/<slug>/notes/`,
where `<slug>` is the course of the page, is read on disk at the push.
Every other image stays an image of the repository, read at `HEAD`. The
rule holds inside a repository and outside one. The agent draws the
figure, makes it with a program, or takes it from the web with its own
tools. qlearn fetches nothing.

**Why.** A course outside a repository could show no image, and a
gesture or a shape is hard to teach with a diagram alone, such as a
stitch of crochet. Inside a repository, the agent could show only a
figure that a commit holds, and a course commits nothing. `notes/` is
already the place where the agent writes with its own tools.

**Rules.**

- **The path is written from the top level directory**, as the path of an
  image of the repository. No new syntax: it is the real path of the file.
- **Only the notes of the course of the page.** A path under
  `.learn/<other>/notes/` is refused, and so is any other path under
  `.learn/`.
- **The file must be a regular file inside the notes.** The server
  resolves the real path of the file and of `notes/`, and refuses a file
  whose real path leaves `notes/`, such as a symbolic link to another
  directory. It refuses a directory or a device.
- **The size is checked before the read**, from the metadata of the file:
  5 MB at most, as for an image of the repository.
- **Every other rule holds**: the type from the bytes, the refusal of an
  SVG that declares entities, the alt text, the copy into `media/`.
- **The block on the wire** carries `origin: 'notes'`, with `repo` and
  `commit` at `null`. A page stored before this change has no `origin`,
  and reads as `repository`. The format of the store keeps its version:
  a server that reads an older page gets the default.
- **The header of the image** shows its path, with no commit. The
  Markdown export of the cheatsheet shows the path with no commit too.
- **The procedure** says when an image beats a diagram, a chart or
  steps, how to draw an SVG that reads on the white ground of the page,
  to look at the figure before the push, and to name the source of a
  figure of the web under it.

**Rejected:**

- **An image read anywhere in the worktree.** The page would show a file
  that the learner changes, and that no commit holds. The notes belong to
  the course, and the page says so with no commit in the header.
- **A path relative to `notes/`, or a prefix such as `notes:`.** A path
  `notes/a.svg` already names a file of a repository. A prefix is a new
  syntax that the agent must learn, where the real path needs none.
- **An image fetched by the server from a URL.** qlearn never calls a
  network service, and the page loads nothing from the network.

## 2026-11-05 — The repository is github.com/Intersec/qlearn

**Decision.** The public repository is
<https://github.com/Intersec/qlearn>. `Cargo.toml` names it, and the
README clones it.

**Why.** The repository now exists, so the open question of its URL is
closed. Intersec publishes qlearn, so the docs may name it.

## 2026-11-05 — A tag publishes a release

**Decision.** `cargo xtask release <version>` cuts a version on the
machine of the developer. It collects the entries of `changelog/`, writes
them in `CHANGELOG.md`, bumps `Cargo.toml`, commits, and makes an
annotated tag. The developer pushes the tag. The `release` workflow of
GitHub Actions runs the gate on the tagged commit, builds the binary of
`make dist`, and publishes `qlearn-linux-x86_64.gz` with the notes of the
version. The `check` workflow runs the gate on every push to `main` and
on every pull request.

**Why.** The README can now give one link to the newest binary, which
needs neither Rust nor Node. One file per changelog entry does not
conflict on a rebase, where a shared `Unreleased` section does. The
pipeline runs `make check`, so it cannot disagree with the gate of the
developer.

**Rules.**

- **The release commit holds only what the release writes**: `Cargo.toml`,
  `Cargo.lock`, `CHANGELOG.md` and `changelog/`. An untracked file of the
  developer stays out.
- **A tag whose gate is red publishes nothing.**
- **The file name carries no version**, so the link to the newest release
  never changes.

## Open questions

None.
