# Design

The architecture, the data model, and the contracts. Read the part that you
touch before you write code. If the code must contradict this document, stop
and report.

## 1. Process model

```
qlearn [<slug>] [options]
    |
    |-- find the repository and the worktree
    |-- read .learn/<slug>/, or create the course
    |-- start the server          127.0.0.1:<random port>
    |                             /tmp/qlearn-<uid>/<worktree-id>.sock
    `-- open the browser          http://127.0.0.1:<port>/?t=<token>
```

One process, one binary, no runtime to install. The interface is inside the
binary. The process stops with Ctrl-C. A closed page does not stop it, so
the agent can go on and the learner can open the page again.

**One server per worktree, one course per server.** `qlearn <slug>` opens
the course `.learn/<slug>/`, and creates it when it does not exist. Without
a slug, `qlearn` opens the only course of `.learn/`. A new course whose
slug is the name of a command of the binary, such as `list` or `wait`, is
refused: `qlearn list` runs the command, so that course could never open.
The binary reads the names from its parser of the arguments, so a new
command is refused too. A course made before its slug became a command
still opens with `qlearn -- <slug>`. If `.learn/` holds several courses, it
lists them and stops. If a server already answers on the worktree, `qlearn`
prints its course and its URL, and stops. To open another course, the
learner stops that server first.

**The token and the socket.** The server binds the
loopback address only. It makes a random token at startup. The first page
request carries the token in the query string, and the server answers with
a session cookie. A request without the cookie or the token gets 401.

The token can leak to another user of the machine. The URL is in the
command line of the browser that `qlearn` opens, and a browser sends a
cookie of `127.0.0.1` to every port. qlearn accepts this risk, for the
reasons in `stack.md`.
A browser sends that cookie from every port of 127.0.0.1, so a write whose
`Origin` is not the page of the server, or whose `Sec-Fetch-Site` is
`same-site` or `cross-site`, gets 403.

The server also listens on `/tmp/qlearn-<uid>/<worktree-id>.sock`, for the
commands of the agent. An agent in a sandbox runs each command in a network
of its own, but it shares `/tmp`. The directory has mode `0700`, and the
server refuses it when another user owns it or others can enter it. The
socket has mode `0600`. The token is asked for there as on the port.
`<worktree-id>` is the first 16 hexadecimal characters of the SHA-256 of
the real path of the top level directory of the worktree.

**In a sandbox, the learner starts the server.** A server that the agent
starts inside a sandbox cannot be reached by the browser. The procedure of
the agent (section 12) says how to detect it.

**What the server reads and writes.** It writes under `.learn/` only, and
nowhere else in the working tree. At startup, if git does not ignore
`.learn/`, it adds `.learn/` to `.git/info/exclude`. It reads the code from
the object database, so an excerpt names a commit and not
the state of a file on disk. It never runs a command that changes the
repository.

On a terminal, a status line says how the course stands: the page (not
open yet, open, closed) and the agent (absent, connected, working on a page,
left). The line is redrawn in place under the messages of the server, on
a terminal only. In a file or a pipe, the messages are all there is.

## 2. Layout

```
Cargo.toml            the workspace
crates/
  qlearn/
    src/
      main.rs         the binary: arguments, server start, browser
      cli.rs          the arguments, clap
      lib.rs          everything below, so it can be tested without a process
      git/            the child process wrapper, the rev, the submodules
      lesson/         the parser of a page, the blocks, the checks
      quiz/           the quiz format, the grading, the points
      highlight/      the extension map, syntect, the class output
      store/          the files of .learn/, the format version
      cheatsheet/     the sections that the server builds, the Markdown export
      agent/          the address file, the socket, the client
        skill.md      the procedure of the agent, printed by qlearn skill
      api/            the axum routes
      model.rs        the types that cross the wire
      testutil.rs     the repository fixture, test builds only
  xtask/              the release and changelog chores
web/                  the Vue application, built by Vite
  src/
  dist/               the Vite output, embedded by rust-embed
skills/
  qlearn/SKILL.md     the skill: it runs qlearn skill and follows it
Makefile
```

`model.rs` holds every type that crosses the wire, with
`#[serde(rename_all = "camelCase")]`. The TypeScript shapes in this document
are the contract. They are written by hand in `web/src/api/types.ts` and
pinned by one snapshot test per route.

`highlight/` colors the code with `syntect`, a language map and an IOP
grammar.

## 3. The course model

A course is a syllabus of pages. A page is a lesson, a checkpoint, or a
follow-up quiz. A page holds blocks, quizzes, and threads.

```ts
type Course = {
  version: 1;
  slug: string;
  title: string;            // the subject, as the learner named it
  goal: string;             // Markdown
  profile: string;          // Markdown: the background and the level
  quizMode: 'mixed' | 'current';
  status: 'new' | 'running' | 'finished';
  followUp: string | null;  // a date, YYYY-MM-DD
  parts: Part[];
  findings: Finding[];
  created: string;          // RFC 3339
  updated: string;
};

type Part = { title: string; pages: PageRef[] };

type PageRef = {
  id: string;               // "1", "7a", "c1", "f1"
  kind: 'lesson' | 'checkpoint' | 'followup';
  title: string;
  state: 'planned' | 'taught' | 'answered' | 'debriefed';
  score: { right: number; total: number } | null; // right: in steps of 0.5
};

type Finding = {
  id: string;
  title: string;
  body: string;             // Markdown
  checked: boolean;         // false: "to check"
};
```

A page id matches `[0-9]+[a-z]?` for a lesson, `c[0-9]+` for a checkpoint,
and `f[0-9]+` for a follow-up. The procedure splits a long lesson into `7a`
and `7b`, so the ids follow that habit.

**The state of a page.** `planned`: in the syllabus, with no content.
`taught`: the agent pushed blocks or a quiz. `answered`: the learner
answered every quiz of the page. `debriefed`: the agent wrote the debrief
of every quiz. A quiz pushed on a page that is `answered` or `debriefed`
takes it back to `taught`. The syllabus draws the state of each page, with
its score.

**The current page** is the page that the agent pushed last. When the agent
pushes a page and the learner reads another one, the page says that the new
one is ready. It does not move the learner. `GET /api/session` gives its id
as `current`, or `null` before the first push. After a restart, the server
finds it from the newest page source, because only a push writes one.

**A course with no parts** is one implicit part with no title. The syllabus
then shows a flat list.

### 3.1 The syllabus

The agent writes the syllabus as a whole:

```json
{
  "parts": [
    { "title": "Why", "pages": [
      { "id": "1", "kind": "lesson", "title": "Two leaders, one term" },
      { "id": "c1", "kind": "checkpoint", "title": "Checkpoint" }
    ] }
  ]
}
```

A new syllabus can add, rename, and reorder pages. The server refuses a
syllabus that drops a page with content, because that would delete answers
of the learner. The state and the score of a page come from the store, never
from the agent.

**A page outside the syllabus.** If the agent pushes a page that the
syllabus does not hold, the server adds it at the end of the last part. Its
kind comes from its id. Its title is its first heading, or its id when it
has no heading. A later syllabus moves it. So a course of step 1, with no
syllabus, still works.

## 4. The format of a page

The agent writes a page in Markdown, with typed blocks. A model writes
Markdown well, and the server checks every block before it stores the page.

### 4.1 The blocks

The server splits the source into blocks with `pulldown-cmark`:

| Block | Source | What the page draws |
|---|---|---|
| `text` | Paragraphs, lists, tables, headings, inline code, fences of code | Markdown, with the tables styled and the fences colored |
| `callout` | `> [!hook]`, `> [!teaser]`, `> [!pitfall]`, `> [!note]` | A box with its marker: 🧠 for a hook, 🔭 for a teaser, ⚠️ for a pitfall, ℹ️ for a note |
| `excerpt` | A fence with the info string `excerpt` | Lines of the repository, with colors, a header `path:from-to`, and a short commit hash |
| `diagram` | A fence with the info string `mermaid` | An SVG drawn by Mermaid |
| `chart` | A fence with the info string `chart` | An SVG chart (section 4.4) |
| `steps` | A fence with the info string `steps` | A view that plays step by step (section 4.5) |
| `image` | `![alt](path)` alone in a paragraph | An image of the repository |

Consecutive text forms one `text` block. A heading starts a new one, so a
thread can name a section.

The marker of a callout opens the first line of a quote. The server refuses
any other marker in the `[!word]` form, such as `[!tip]`, so that a typo
does not become a plain quote. It also refuses a callout inside a list or a
quote, and a callout with only its marker. An escaped `\[!hook]` stays text.

**A fence of code.** A fence whose info string names no block kind stays in
the `text` block, for a command or a sketch of code. The server colors it
with the language of its info string, as it colors an excerpt. The agent
typed it, so the page shows it with no path and no commit. The procedure
says to show the code of the repository with `excerpt` only.

**No raw HTML.** The server escapes the HTML of the source, and the page
passes every HTML string through DOMPurify. A lesson holds no script, no
style, and no link that loads anything from the network. A link to the web
stays a link that the learner clicks.

**The identity of a block** is the first 12 hexadecimal characters of the
SHA-256 of its kind and its source. If a page holds the same kind and
source more than once, the hash of each later copy also takes its rank in
the page: 2 for the second copy, 3 for the third. When the agent pushes a
page again, an unchanged block keeps its id, and its threads stay on it
(section 6).

```ts
type Block =
  | { id: string; kind: 'text'; html: string }
  | { id: string; kind: 'callout'; marker: 'hook' | 'teaser' | 'pitfall' | 'note'; html: string }
  | { id: string; kind: 'excerpt'; excerpt: Excerpt }
  | { id: string; kind: 'diagram'; source: string }
  | { id: string; kind: 'chart'; chart: Chart }
  | { id: string; kind: 'steps'; steps: Steps }
  | { id: string; kind: 'image' } & Image;
```

### 4.2 The excerpt

The excerpt is the block that only qlearn can give: the server reads the
lines in the repository. If the file does not hold them, the server refuses
the page. The agent cannot show code that does not exist.

````
```excerpt
path: vendor/libcore/src/core/el.c
lines: 120-135
rev: HEAD
mark: 127, 130-131
hide: 127
```
````

| Key | Required | What it does |
|---|---|---|
| `path` | yes | A path from the top level directory of the worktree |
| `lines` | yes | `<from>-<to>`, or one line. 60 lines at most |
| `rev` | no | A rev that git resolves to a commit. `HEAD` by default |
| `mark` | no | Lines to mark, inside `lines`. A list of lines and ranges |
| `hide` | no | One line to hide, for "Complete the code" |

The server resolves `rev` to a commit hash once, at the push, and stores
the hash with the lines. The excerpt then shows the code as the lesson
taught it, even after the code changes.

**A path inside a submodule.** The server walks the path one directory at a
time. When the tree holds a submodule there (mode `160000`), it goes on in
the repository of the submodule, at the commit that the tree names. If that
commit is not in the submodule, the server refuses the page and names the
submodule to update.

**A hidden line.** The page shows the line as a blank that spans the
width of the excerpt. The page never gets the length of the line, so the
blank gives none of it away. The server sends its text only after the
learner answers every quiz of the page. A page with no quiz keeps the line
hidden, because the agent pushes the quiz after the page. The developer
tools of the browser then cannot give the answer away.

```ts
type Excerpt = {
  path: string;
  repo: string;             // "" for the main repository, or the submodule path
  commit: string;           // the full hash, in the repository that holds the file
  from: number;
  to: number;
  lang: string;
  lines: (Line | null)[];   // null: the hidden line, before the answer
  marked: number[];
};

type Line = { text: string; spans: Span[] }; // offsets in UTF-16 units
```

### 4.3 The diagram

The page draws a `mermaid` fence with Mermaid, bundled in the binary, with
`securityLevel: 'strict'` and the colors of the page theme. The server
cannot run Mermaid, so it checks only that the source is not empty and has
200 lines at most.

**A diagram that fails comes back to the agent.** When Mermaid refuses a
source, the page shows the error in place of the diagram, and sends a
`broken` request (section 8.3). The agent gets the block and the error from
its `wait`, and pushes the page again. The learner sees a broken diagram for
the time of one fix.

### 4.4 The chart

A `chart` fence holds JSON, in a small format of qlearn:

```json
{
  "kind": "bar",
  "title": "Election time, by timeout range",
  "x": { "label": "Timeout range (ms)", "values": ["150-300", "150-151"] },
  "y": { "label": "Median election time", "unit": "ms" },
  "series": [{ "name": "5 servers", "values": [283, 1520] }]
}
```

`kind` is `bar` or `line`. Each series has one value for each `x` value.
There are 8 series at most. The server checks these rules, and the page
draws the chart in SVG with the palette of the page.

```ts
type Chart = {
  kind: 'bar' | 'line';
  title: string;
  x: { label: string; values: string[] };   // a number of the fence comes as its text
  y: { label: string; unit: string };       // "" when the values have no unit
  series: { name: string; values: (number | null)[] }[]; // null: a gap, in a line chart only
};
```

The limits of each field are in `stack.md`, entry "Holes found by the
chart".

Rejected: Vega-Lite. It weighs more than the rest of the interface, and the
server cannot check a spec in full. A small format keeps every chart of
every course in one style, and the agent cannot write a chart that the page
cannot draw.

### 4.5 Steps

A `steps` fence holds JSON. It shows how a state changes over time: a
timeline, an election, an array that a loop fills. The learner plays the
steps one by one.

```json
{
  "title": "Two candidates in term 6",
  "columns": ["Server", "Term", "State", "Voted for"],
  "steps": [
    { "label": "t = 0 ms", "body": "S1 times out first.",
      "rows": [["S1", "6", "candidate", "S1"], ["S2", "5", "follower", "-"]] },
    { "label": "t = 2 ms", "body": "S2 gets the vote request.",
      "rows": [["S1", "6", "candidate", "S1"], ["S2", "6", "follower", "S1"]] }
  ]
}
```

Each step has a `label`, a `body` in Markdown with no blocks, and `rows`
that match `columns`. The page marks each cell that changed since the
previous step. A step may hold a `diagram`, a Mermaid source, in place of
`rows`.

```ts
type Steps = {
  title: string;
  columns: string[];          // [] when every step holds a diagram
  steps: Step[];
};

type Step = {
  label: string;
  html: string;               // the body, as HTML that the server wrote
  rows: string[][] | null;    // a number of the fence comes as its text
  diagram: string | null;     // a Mermaid source, in place of rows
};
```

The limits of each field are in `stack.md`, entry "Holes found by the
steps".

A step view also serves the exercise Predict: the learner stops on a step,
answers the quiz, then plays the rest.

Rejected: simulations that the agent writes in JavaScript. A script of the
agent in the page is a surface that the server cannot check, and the
concept says that a lesson holds no script. If steps are not enough, a new
widget of the page, with data in JSON, is the way.

### 4.6 The image

`![alt](path)` alone in a paragraph is an image of the repository, at
`HEAD`. The server reads it at the push in the object database,
submodules included, as it reads an excerpt, and copies it into `media/`,
under the hash of its content. It takes PNG, JPEG, GIF, WebP, and SVG, and
reads the type in the bytes of the file. A URL with a scheme is refused.

The page draws every image from an `<img>`, so an SVG runs no script and
loads nothing. `GET /api/media/:name` sends it with its type and a
`sandbox` policy, for a learner who opens it in a tab of its own.

```ts
type Image = {
  alt: string;              // the text for a screen reader, never empty
  media: string;            // <sha256>.<ext>, the name of the file in media/
  path: string;
  repo: string;             // "" for the main repository, or the submodule path
  commit: string;           // the full hash of HEAD, in the repository that holds the file
  type: 'image/png' | 'image/jpeg' | 'image/gif' | 'image/webp' | 'image/svg+xml';
  width: number | null;     // in pixels, when the header of the file says it
  height: number | null;
};
```

The limits and the other rules are in `stack.md`, entry "Holes found by
the image".

### 4.7 The checks

The server checks the whole page before it stores anything. If a check
fails, it stores nothing and answers with every error, each one with its
line in the source:

```
lesson.md:14: excerpt: vendor/libcore/src/core/el.c has 98 lines at HEAD, the excerpt asks for 120-135
lesson.md:31: chart: series[0].values: holds 3 values, and x holds 2. Give one value for each x value
```

The command exits with 1, and the agent fixes the source.

## 5. Quizzes

A page holds zero or more quizzes, in order. A lesson usually has one. A
lesson that teaches a missed point again adds a second one.

### 5.1 The format

```json
{
  "open": {
    "prompt": "In 2 sentences: why is the timeout random?",
    "points": ["random-timeout"]
  },
  "choices": [
    {
      "header": "Split vote",
      "prompt": "Two candidates get 2 votes each out of 5. What happens next?",
      "options": [
        "The candidate with the higher id wins",
        "Both time out and start a new election in a new term",
        "The leader of the previous term breaks the tie"
      ],
      "answer": 1,
      "point": "split-vote"
    }
  ],
  "points": {
    "random-timeout": { "label": "Why the election timeout is random" },
    "split-vote": { "label": "How a split vote ends", "lesson": "3" }
  }
}
```

- `open` is one open question, or `null`. An exercise (Predict, Your fix,
  Spot the bug) is a quiz with an `open` and no `choices`.
- `choices` holds 0 to 4 questions, with 2 to 4 options each. `answer` is
  the index of the right option. The options are inline Markdown, on one
  line.
- `points` gives a label to each new point. A point that the course already
  knows needs no entry. `lesson` names the lesson that taught the point, and
  the page of the quiz is the default. A question on an earlier lesson, in
  the mixed mode, names that lesson.

The server refuses a quiz that is empty, a point with no label, and an
`answer` out of range. It also refuses:

- a point id with a character other than a lowercase letter, a digit or a
  hyphen, because the id goes into `--point <id>=ok`;
- an open question that names no point;
- an entry for a point that the course knows, with another label or
  another lesson;
- an entry that no question of the quiz names.

### 5.2 What the learner sees

1. The open question, alone, with a text box. The choices are hidden, so
   their options cannot give its answer away.
2. Once the learner sends the open answer, the choices appear. Each choice
   also has an "Other" box, for free text.
3. Once the learner sends the choices, the server grades them, and the page
   shows right or wrong at once. The page says that the agent writes the
   debrief.
4. The debrief of the agent appears under the quiz.

A quiz with no open question starts at step 2. A quiz with no choices sends
the answer at step 1.

The page never receives `answer` before the learner sends the choices.

### 5.3 The answer and the points

When the learner sends the last part of a quiz, the server writes an
`answer` request (section 8.3). The agent gets the open answer, the option
that the learner picked for each choice, whether it is right, and the
"Other" text.

The server keeps the result of each point in `points.json`:

```ts
type Point = {
  id: string;
  label: string;
  lesson: string;
  results: { at: string; result: 'ok' | 'partial' | 'missed'; how: 'choice' | 'open' }[];
};
```

A choice adds its result at once. The open question has no automatic
grade: the agent gives it in the debrief, with `--point <id>=ok`,
`--point <id>=partial` or `--point <id>=missed`. A choice is `ok` or
`missed`. `partial` is for an open answer that is half right, such as a fix
that works but misses a case.

In the score of a page, `ok` counts 1, `partial` counts 0.5, and `missed`
counts 0.

`qlearn points` prints the points, the missed ones first (last result
`missed` or `partial`), then the others by the date of their last result,
the oldest first.
That is the order in which the procedure picks the point of a question on
an earlier lesson.

### 5.4 The debrief

The debrief is a page source in the format of section 4, so it can hold an
excerpt or a diagram that explains a wrong answer. Writing it takes the
page to `debriefed` when no quiz of the page is left without one.

## 6. Threads

The learner can ask a question about any block, or about the whole page.
The page shows an **Ask** button on each block, and an **Ask about this
page** button at the end of the page, before Next. If the learner selects
text inside a block and presses its Ask, the selection becomes the quote
of the thread. A question about the whole page has no quote. The page
shows its thread at its end, under "About this page".

```ts
type Thread = {
  id: string;
  page: string;
  block: string | null;     // null: about the whole page
  quote: string | null;     // the text that the learner selected
  comments: { author: 'learner' | 'agent'; body: string; at: string }[];
};
```

A question of the learner writes an `ask` request. The agent answers with
`qlearn reply`. A reply of the learner in the thread asks again.

The agent opens a thread too, with `qlearn comment`. It does so when it
must say something outside a lesson: the summary of where the course
stopped, a recap of the points that the learner is not sure of, the answer
to a question typed in the terminal.

**A thread whose block is gone.** When a page pushed again no longer holds
the block of a thread, the thread moves to the end of the page, with its
quote, under "On an earlier version of this page".

## 7. Choices

The agent asks the learner to choose with `qlearn choose`. In the page, a
choice replaces the dialogs that the agent opens in its terminal: the level
and the quiz mode at the start, the next step at a checkpoint, the
follow-up quiz at the end.

```json
{
  "page": "c1",
  "questions": [
    {
      "header": "Next step",
      "prompt": "Part 1 is done. What next?",
      "options": [
        { "label": "Part 2", "description": "The code tour of the election" },
        { "label": "A break", "description": "The course waits here" }
      ],
      "multi": false
    }
  ]
}
```

The page shows the choice at the end of its page, or at the top of the
syllabus without `page`. Each question also takes free text. The answer
writes a `chosen` request. A choice is never graded.

## 8. HTTP API

All routes are under `/api`, all answers but a media file are JSON, and
all errors carry `{ "error": { "code": "...", "message": "..." } }`. A
write of the agent carries `"author": "agent"`. It names the
writer, and grants no permission.

| Method and route | Who | What it does |
|---|---|---|
| `GET /api/session` | page | The course, the syllabus, the current page, the tool version, the id of the server |
| `GET /api/pages/:id` | page | The blocks, the quizzes without their answers until answered, the threads |
| `POST /api/pages/:id/next` | page | Ask the agent for the next page. Answers the request |
| `POST /api/pages/:id/quizzes/:n/open` | page | Send the open answer. Body: `{ text }`. Answers the page |
| `POST /api/pages/:id/quizzes/:n/choices` | page | Send the choices. Body: `{ choices: [{ option, other }] }`, one per choice. The server grades them, and answers the page |
| `POST /api/pages/:id/threads` | both | Open a thread. Body: `{ block, quote, body }` |
| `POST /api/threads/:id` | both | Reply in a thread |
| `POST /api/choices/:id` | page | Answer a choice |
| `POST /api/pages/:id/broken` | page | Report a block that the page cannot draw. Body: `{ block, error }` |
| `GET /api/points` | both | The points, in the order of section 5.3 |
| `GET /api/cheatsheet` | page | The cheatsheet, as blocks, with the sections that the server builds |
| `GET /api/cheatsheet.md` | both | The cheatsheet as Markdown (section 9) |
| `GET /api/media/:name` | page | An image of `media/`. Raw bytes, with its type |
| `GET /api/events?after=<n>&who=<page\|agent>` | both | Wait for the next writes. See section 8.3 |
| `PATCH /api/course` | agent | Change the title, the goal, the profile, the quiz mode, the status, the follow-up date. Refuses the whole change for an unknown field, a `null` but for the follow-up, a bad value, the status `new`, or a body that names no field. Trims the title. Answers the course |
| `PUT /api/syllabus` | agent | Write the syllabus (section 3.1) |
| `PUT /api/pages/:id` | agent | Push the blocks of a page. Body: the source. Answers the errors of section 4.7 |
| `POST /api/pages/:id/quizzes` | agent | Add a quiz to a page |
| `PUT /api/pages/:id/quizzes/:n/debrief` | agent | Write the debrief. Body: `{ source, points: { id: ok } }` |
| `POST /api/choices` | agent | Ask a choice. Answers its id |
| `PUT /api/findings/:id` | agent | Write a finding |
| `PUT /api/status` | agent | Say what the agent does. Body: `{ text }`. Answers `{ status }` |
| `PUT /api/cheatsheet` | agent | Write the source of the cheatsheet |
| `GET /api/export` | agent | The whole course as JSON, for a resume after a reset (section 8.4) |

### 8.1 The place in the fragment

The address bar says where the learner is: `#page=<id>`, and
`&block=<id>` when the learner opened a thread. A question about the whole
page writes `&block=page`. A block id is 12 hex digits, so it never reads
`page`. Back and forward walk the pages that the learner read. A reload
comes back to the same place.

### 8.2 Status

`qlearn status "<text>"` shows a line in the top bar of the page, for
example "Writing lesson 4: the code tour". The agent sets it before a long
piece of work. The next `wait` of the agent clears it, and so does an empty
text. The server trims the text.

### 8.3 Events

For the events, the server is the only writer:
it numbers its writes, keeps the last few hundred in memory, and holds a
request on `/api/events` until a write comes, for 30 seconds at most. An
`after` that the server no longer holds answers `"reset": true`.

**The id of the server.** The server draws a random id at each start. Each
answer of `/api/events` carries it as `server`, and so does
`GET /api/session`. A new server numbers its events from 1 again, so an
`after` of the old server can name events of the new one. A party that
gets an id other than the one it had takes it as `"reset": true`, and reads
the state again.

```ts
type Event = {
  seq: number;
  kind: 'course' | 'syllabus' | 'page' | 'quiz' | 'debrief' | 'status'
      | 'thread' | 'reply' | 'choose' | 'finding' | 'cheatsheet' | 'idle'
      | 'answered'
      // the requests of the learner:
      | 'next' | 'answer' | 'ask' | 'chosen' | 'broken';
  author: 'learner' | 'agent';
  page: string | null;
  id: string | null;        // the quiz, the thread, or the choice
};
```

**A part of a quiz.** Each time that the learner sends the open answer or
the choices of a quiz, the server writes an `answered` event, with the page
and the number of the quiz in `id`. The other tabs of the page read the
page again. `answered` is not a request. When the part is the last one, an
`answer` request follows it.

**A request** is an event of the learner that needs the agent: `next`,
`answer`, `ask`, `chosen`, `broken`. `qlearn wait` returns on a request
only. Each request in its output carries `detail`, so the agent needs no
second command:

| Request | `detail` |
|---|---|
| `next` | The page that the learner left |
| `answer` | The quiz, the open answer, each choice with the option picked, `ok`, and the "Other" text |
| `ask` | The thread, the id and the source of its block, or no block for a thread about the whole page |
| `chosen` | The choice and the answers |
| `broken` | The block, its source, and the error of the page |

**The presence of the agent.** An agent is present while a `qlearn wait`
is open, for 30 seconds after a wait that got its answer, and for 5
seconds after it hung up on one. A request that a `wait` handed over
keeps the agent present and
working until its next `wait`, for 30 minutes at most. A lesson takes
minutes to write, and the agent must not look gone while it writes one.
The page shows "the agent prepares the next page" while it works, with the
text of the status.

### 8.4 The export

`GET /api/export` gives an agent that lost its context what it needs to
resume the course: where it stopped, what the learner answered, and which
points to ask again. `qlearn export --json` prints it.

```ts
type Export = {
  version: string;          // the release, as qlearn --version says
  course: Course;           // its version is the format of the store
  current: string | null;   // the page that the agent pushed last
  pages: ExportPage[];      // the pages with content, in the order of the syllabus
  choices: Choosing[];      // every choice, open or answered, as the page gets it
  points: Point[];          // in the order of section 5.3
  notes: string[];          // the files under notes/, relative to it
};

type ExportPage = {
  id: string;
  source: string;           // as the agent pushed it last
  quizzes: ExportQuiz[];    // quiz n is at n - 1
  threads: ExportThread[];  // in the order of the page
};

type ExportQuiz = {
  open: { prompt: string; points: string[]; sent: string | null } | null;
  choices: {
    header: string;
    prompt: string;
    options: string[];      // inline Markdown, as the agent wrote it
    point: string;
    graded: GradedChoice | null; // as in the page
  }[];
  debrief: { source: string; points: GradedPoint[] } | null;
};

type ExportThread = {
  id: string;
  block: { id: string; kind: string; source: string } | null;
  earlier: boolean;         // the block is gone
  quote: string | null;
  comments: Comment[];      // the bodies in Markdown
};
```

The page may read this route too, with its cookie. So the export follows
the rules of the page: the right option of a choice stays in `graded`,
which is `null` until the learner sends the choices. A source names a
hidden line by its number, never by its text. An agent that resumes needs
neither: the `answer` request carries the right options.

The export lists the names of the notes, never their content. The agent
reads its notes with its own tools.

A file of the store that cannot be read fails the whole export, with the
error of the file. So does a file of a page that `course.json` says the
agent pushed, when it is missing. A part of the course left out would read
as a part that never happened.

## 9. The cheatsheet

At the end of the course, the learner keeps a cheatsheet of one page. The
agent writes it, and the learner has nothing to write. The developer doc of
the subject stays a separate text, for the team, that the agent writes in
the repository.

**The agent writes the source** with `qlearn cheatsheet`, in the format of
section 4, with no hidden line. The procedure asks for: the hooks of the
lessons, the key tables and diagrams, a glossary of the names of the code,
and the pitfalls.

**The server adds two sections** that it builds from the store. They are
records, so they are true by construction:

- **Map of the code.** Every excerpt of the course, by `path:from-to`, with
  the lesson that showed it. Excerpts that overlap merge.
- **To revisit.** Every point whose last result is `missed` or `partial`,
  with its label and its lesson.

**The page lays it out** for an A4 sheet, in two columns, with a print
stylesheet.

**Three exports:**

| Export | How | What it holds |
|---|---|---|
| HTML | A button in the page | One file with inline CSS, the diagrams as inline SVG, the code with its colors. The page builds it, because only the page has the drawn diagrams |
| PDF | The print dialog of the browser | The print stylesheet |
| Markdown | A button, or `qlearn export cheatsheet` | Excerpts as fenced code with their `path:from-to`, diagrams as `mermaid` fences, charts as tables, steps as a numbered list. A wiki renders it |

The cheatsheet lives in the course directory. The page says so when the
course is finished: deleting `.learn/<slug>/` deletes it, so the learner
saves an export first.

## 10. Storage

```
.learn/
  server.json               where the server listens, mode 0600, removed at stop
  <slug>/
    course.json             the course, the syllabus, the findings
    points.json             the points and their results
    pages/
      3.md                  the source of the page, as pushed
      3.json                the blocks, the quizzes and their answers, the threads
    choices.json
    cheatsheet.md
    media/
      <sha256>.<ext>        the images, by the hash of their content
    notes/                  the research notes of the agent
```

```json
{ "pid": 41207, "port": 38412, "token": "...", "socket": "/tmp/qlearn-1000/c710c85489f3b4c3.sock" }
```

**The server is the only writer**, except for `notes/`. The agent writes its
research notes there with its own tools, and the server never reads or
writes them. The export lists their names only. Every other file goes
through the commands of section 11. `qlearn delete` removes a course only
when no server serves it.

Each write goes to a temporary file in the same directory, then a rename,
so a crash never leaves half a file. One server per worktree, and
`.learn/` belongs to one worktree, so no lock is needed.

**`course.json` carries `version`.** A server that reads a version above
its own refuses the course and says to upgrade qlearn. A change of the
format owes a version bump and a migration.

**A directory without `course.json` is not a course of qlearn.** `qlearn`
refuses it, and says to start a new course with a new slug. Another tool
may have written that directory. Its files are not a contract. `qlearn
list` and a bare `qlearn` skip it.

## 11. Command line

```
qlearn [<slug>]               open a course, and create it when it is new
qlearn --no-open              start the server, print the URL, open nothing
qlearn --port <n>             use a fixed port
qlearn -v                     print the course before the URL
qlearn --trace                print on standard error what the work costs
qlearn list                   list the courses of .learn/
qlearn delete <slug> [--yes]  delete a course, with no server on it
qlearn skill                  print the procedure of the agent, for this release
qlearn export [--json]        the whole course
qlearn export cheatsheet      the cheatsheet as Markdown
```

**`qlearn delete <slug>`** removes `.learn/<slug>/`. If a server serves
that course, it refuses and says to stop that server first. It names what
it removes and asks for a confirmation, which `--yes` skips. Until then, a
course stays in `.learn/`, and `qlearn <slug>` opens it again.

- It works with no server, on the store. It holds the claim of the
  worktree (the lock on `.learn/`) until the course is gone, so no server
  opens the course meanwhile. A server that holds the claim and serves
  another course does not block it: a server writes its own course only.
  That server may stop while the learner reads the question, and a server
  of this course start, so the command checks again after the answer, and
  takes the claim if it is free. Only the instant between that check and
  the removal stays open. A server that holds the claim and does not
  answer blocks it, because nothing tells which course it serves. A
  `server.json` left by a server that died blocks nothing.
- It names the directory, the title of the course, the number of pages,
  whether a cheatsheet exists, and the number of notes. It asks
  `[y/N]` on the terminal. With no terminal on standard input and no
  `--yes`, it refuses. An answer other than `y` or `yes` deletes nothing,
  and the command exits with 1.
- It refuses a slug that names no directory, a directory with no
  `course.json`, and a symbolic link. A course whose `course.json` cannot
  be read is deleted. The removal follows no link inside the course.

**`qlearn export`** prints the course for the learner: the line of the
course, the syllabus with the state and the score of each page, as
`qlearn -v` prints it, then "To revisit" with each point whose last result
is `missed` or `partial`, in the order of section 5.3, or "Nothing to
revisit.". `--json` prints the export of section 8.4. Both ask the server
of the worktree. With no server, the command holds the claim of the
worktree, so no server starts and writes the store meanwhile, and reads
the store: the course is the only course of `.learn/`, as `qlearn` with no
slug finds it. It writes nothing, and shows what a server would show after
its start. With several courses, it says to open the course to export with
`qlearn <slug>`. A server that holds the claim and does not answer in the
few seconds that a second `qlearn` waits makes the command refuse. `cheatsheet` stays the word of
`qlearn export cheatsheet`, so `export` takes no slug.

**The commands of the agent.** They write as the agent, through the server
of the worktree:

```
qlearn course [--title <text>] [--goal <text>] [--profile <text>]
              [--quiz-mode mixed|current] [--status running|finished]
              [--follow-up <date>|none]
qlearn syllabus --file <path>
qlearn page <id> --file <path>
qlearn quiz <id> --file <path>
qlearn debrief <id> <n> --file <path> [--point <point>=ok|partial|missed]...
qlearn comment <id>[#<block>] --body <text>
qlearn reply <thread> --body <text>
qlearn choose --file <path>
qlearn finding <id> --title <text> --body <text> [--checked]
qlearn status <text>
qlearn points
qlearn cheatsheet --file <path>
qlearn wait [--after <n>] [--server <id>] [--timeout <s>]
```

A `--file` or a `--body` of `-` is read from standard input. Every command
prints what it wrote as JSON, its id included. A command that the server
refuses exits with 1 and prints the errors on standard error.

`wait` prints
`{ "events": [...], "next": <n>, "server": <id> }`, returns on a request
only, exits with 1 when the time runs out (600 seconds by default), and
stays attached across a restart of the server, with `"reset": true`. The
agent passes `next` and `server` to the next `wait`. If the server that
answers has another id than `--server`, the `wait` answers
`"reset": true` at once, with `next` at 0. So an `after` of the old server
never skips a request of the new one.

None of these commands starts a server. Each one says to run `qlearn` when
no server answers.

## 12. The procedure of the agent

`qlearn skill` prints the procedure, from `agent/skill.md` in the binary.
A test fails when the procedure names a command that the binary does
not have.

The procedure covers the teaching and the page. For the teaching, it says:

- How to know the learner: the profile, the level, the quiz mode.
- How to prepare the course: the research, the syllabus, and the check of
  each claim against the code.
- How to teach: the lessons, the quizzes and their debrief, the exercises,
  the checkpoints, the follow-up quiz, and the findings. The recall quiz at
  a resume is a checkpoint page.
- How to end: the cheatsheet, the developer doc of the subject, which the
  agent writes in the repository if the learner agrees, and `qlearn delete`
  once the learner agrees and has saved an export of the cheatsheet.

For the page, it says:

- How to start: the server, the sandbox case, the resume with
  `qlearn export --json`. After a reset, the agent reads the export,
  answers what waits, and pushes a recall checkpoint whose quiz asks
  about the first points of `qlearn points`.
- The loop: `wait` in the background, then the answer to each request.
- The format of a page, a quiz, a choice, and the cheatsheet, with one
  example of each.
- What replaces each habit of the terminal: a choice for a dialog, a quiz
  for a question in the prompt, a thread for a message.

qlearn has no terminal mode. The agent teaches the course through the page.

**The skill.** `skills/qlearn/SKILL.md` holds no procedure. Its description
tells the agent when to load it. Its body runs `qlearn skill` and follows
it, so the skill and the binary are always the same release. If `qlearn` is
not on the PATH, or does not know `skill`, the skill tells the user to
install a newer release, and stops.

**A skill of an organization.** An organization can wrap qlearn in a skill
of its own. That skill adds its sources, such as tickets, a review server
or internal docs, and its rules. Then it follows `qlearn skill`. If
`qlearn` is on the PATH, that skill teaches through the page and asks the
learner nothing about it. Without `qlearn`, it may keep a terminal mode of
its own.

## 13. Build and distribution

`make` is the entry point, `make check` is the
whole gate, `make shots` takes screenshots of both themes, and `make dist`
builds one static `x86_64-unknown-linux-musl` binary.

The interface uses Vue 3, Pinia, Tailwind, DOMPurify, and Mermaid.
Mermaid is the largest dependency of the interface. The page
loads it in a chunk of its own, the first time that a page holds a diagram.

## 14. The order of the work

1. **The loop.** The server, the token, the socket, `wait`, a page of
   `text` blocks, and Next. Done when an agent teaches a lesson of two pages
   through the page.
2. **The lesson.** The excerpt with its checks and the submodules, the
   callouts, the diagram with `broken`, the syllabus.
3. **The quiz.** The quiz, the grading, the points, the debrief, the
   choices, the threads, the resume. `qlearn skill` with the whole
   procedure, and `skills/qlearn`.
4. **The record.** The chart, the steps, the images, the cheatsheet and its
   three exports, the findings.
