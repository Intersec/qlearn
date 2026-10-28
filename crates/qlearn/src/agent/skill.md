# qlearn

qlearn shows a course on a code base in the browser of the learner. You
write the pages of the course. The learner reads them and presses **Next**
when they want the next page. Everything goes through `qlearn` commands.
Never edit the files under `.learn/` by hand. If a command says that a file
of `.learn/` is not readable, stop and show the error to the learner: the
file may hold their answers, and only they repair it.

This release knows the syllabus, pages of text, callouts, excerpts of the
code, diagrams, quizzes, and the **Next** button. The learner answers a
quiz in the page, and you get the answer. You cannot write the debrief yet.

## Start

One server runs per worktree, with one course. The course lives in
`.learn/<slug>/`. The slug is a short name of the subject, in lowercase
letters, digits and hyphens, such as `raft-election`.

1. Ask the learner for the subject, then pick the slug.
2. The learner starts the server in their own terminal, from the worktree:

   ```sh
   qlearn <slug>
   ```

   It creates the course when it is new, opens the page in the browser,
   and prints its address. Ask the learner to run it, and to tell you when
   the page is open.
   - If your commands do not run in a sandbox, you may start it yourself,
     in the background, from the worktree.
   - If your commands run in a sandbox, never start the server yourself. A
     server that starts in the sandbox runs in a network of its own, and
     the browser of the learner cannot reach it. Your commands reach the
     server of the learner through its Unix socket under `/tmp`, with
     nothing to configure.
   - If a server already runs on this worktree, `qlearn` prints its course
     and its address, and stops. That is not an error: use that server.
3. Give the course its title and its goal:

   ```sh
   qlearn course --title "<subject>" --goal "<what the learner wants>"
   ```

   `--profile` takes the background and the level of the learner. The
   error `no qlearn server runs on this worktree` means that the server
   does not run yet: ask the learner to start it, then run the command
   again.
4. Write the syllabus (see "The syllabus").
5. Push the first page (see "Push a page"), then start the loop.

## The syllabus

The syllabus is the plan of the course: its parts, and the pages of each
part. The page shows it in a column at the left, with the state of each
page. Write it before the first page:

```sh
qlearn syllabus --file - <<'EOF'
{
  "parts": [
    { "title": "Why", "pages": [
      { "id": "1", "kind": "lesson", "title": "Two leaders, one term" },
      { "id": "2", "kind": "lesson", "title": "The election timer" },
      { "id": "c1", "kind": "checkpoint", "title": "Checkpoint" }
    ] },
    { "title": "How", "pages": [
      { "id": "3", "kind": "lesson", "title": "The vote" }
    ] }
  ]
}
EOF
```

- A page takes `id`, `kind` and `title`, and nothing else.
- The id names the kind. `c1` and `c2` are checkpoints, with the kind
  `checkpoint`. `f1` is a follow-up quiz, with the kind `followup`. Any
  other id is a number with an optional letter, such as `7` or `7a`, with
  the kind `lesson`.
- Each page has a title, and each id comes once.
- If the course has one part, that part may have the title `""`: the page
  then shows a flat list. If the course has several parts, each part has a
  title.
- The server keeps the state and the score of each page. You never write
  them.

To change the syllabus, write the whole syllabus again. A new syllabus can
add, rename and reorder pages. The server refuses a syllabus that drops a
page with content, because the answers of the learner would be lost. You
may drop a page that you have not pushed yet.

The command prints the course as JSON. If the server refuses the
syllabus, it stores nothing, exits with 1, and prints every error on
standard error, one per line, with its place in the JSON:

```
parts[0].pages[2].kind: page c1 has the kind checkpoint, not lesson. A c id takes the kind checkpoint, an f id followup, and any other id lesson
```

Fix every error, then write the whole syllabus again.

If you push a page that the syllabus does not hold, the server adds it at
the end of the last part. To put it in its place, write the syllabus again.

## The loop

Repeat until the learner says that the course is over:

1. Run `qlearn wait --after <next>` in the background. Use the `next`
   number of the last `wait`. The first time, use `--after 0`, so that a
   **Next** that the learner presses before your first `wait` is not lost.
2. When it returns, read the JSON:
   - `events`: the requests of the learner. A request of kind `next` says
     that the learner pressed **Next**. Its `detail.left` names the page
     that they left, with its id and its title. A request of kind `broken`
     says that the page cannot draw a diagram (see "Diagrams"). A request
     of kind `answer` says that the learner answered a quiz (see "The
     answer").
   - `next`: pass it to the next `wait`, so that no request is lost
     between two waits.
   - `"reset": true`: the server restarted. Wait again with the new
     `next`.
   - Exit code 1 and no event: the time ran out. Wait again.
   - The error `no qlearn server runs on this worktree`: the server
     stopped and did not come back before the time ran out. Tell the
     learner once, then wait again. A `wait` attaches to the server as soon
     as the learner starts one. If the server stops during a `wait`, the
     `wait` looks for it again, and a new server answers `"reset": true`.
3. Answer each request. For `next`, push the next page. For `broken`,
   fix the diagram and push its page again. For `answer`, see "The
   answer".
4. Go back to step 1.

Writing a page takes minutes. Before a long piece of work, say what you do,
so that the learner does not wonder:

```sh
qlearn status "Writing page 2: the election timer"
```

The page shows that text in its top bar. Your next `wait` clears it.

## Push a page

```sh
qlearn page <id> --file <path>
```

`--file -` reads the source from standard input, so a page needs no file:

```sh
qlearn page 2 --file - <<'EOF'
# The election timer

Each follower waits for a random time.
EOF
```

Use the ids of the syllabus. A page that you push again with the same id
replaces the old one. The column shows the title of the syllabus, and the
page shows its first heading at the top.

The command prints the page as JSON. If the server refuses the page, it
stores nothing, exits with 1, and prints every error on standard error, one
per line, with the line of the source:

```
lesson.md:14: excerpt: src/raft.c has 98 lines at HEAD, the excerpt asks for 120-135
```

Fix every error, then push the whole page again.

## The format of a page

A page is Markdown. This release takes:

- Paragraphs, lists, **bold**, *italic*, links, and `inline code`.
- Headings. Each heading starts a new block of the page.
- Tables.
- Code fences. The info string names the language, such as `c` or
  `rust`, and the page colors the code. The page shows a fence with no
  path and no commit, so use it for a command or a sketch.
- Callouts (see "Callouts").
- Excerpts of the repository (see "Show the code").
- Diagrams (see "Diagrams").

This release refuses these blocks, with an error at their line:

- A fence with the info string `chart` or `steps`.
- An image, `![alt](path)`, alone in a paragraph.

The server escapes raw HTML, so it shows as text. Write no HTML.

## Callouts

A callout is a quote whose first line is a marker. The page draws it as a
box with the sign of its marker.

| Marker | Sign | What it holds |
|---|---|---|
| `[!hook]` | 🧠 | A short way to remember the idea of the page |
| `[!teaser]` | 🔭 | A link to a later lesson: what the learner will see there |
| `[!pitfall]` | ⚠️ | A trap of the code: what goes wrong, and when |
| `[!note]` | ℹ️ | Anything else that stands apart from the text |

```
> [!pitfall]
> A server that restarts forgets its vote, unless `grant_vote` wrote it
> to disk first. It can then vote twice in one term.
```

- A callout holds text, lists, `inline code` and fences of code. It holds
  no excerpt and no other block: put those at the top level of the page.
- Put a callout at the top level of the page, not inside a list or another
  quote.
- The server refuses any other marker, such as `[!tip]` or `[!WARNING]`.
  A quote with no marker stays a plain quote.
- Keep a callout short: one idea, two or three lines.

## Show the code

Show the code of the repository with an `excerpt` fence only. Never copy
the code into a code fence: the server reads the lines of an excerpt in the
repository, and refuses the page if the file does not hold them.

````
```excerpt
path: src/raft.c
lines: 120-135
rev: HEAD
mark: 127, 130-131
```
````

| Key | Required | What it does |
|---|---|---|
| `path` | yes | The file, from the top level directory of the worktree |
| `lines` | yes | `<from>-<to>`, or one line. 60 lines at most |
| `rev` | no | A branch, a tag or a commit. `HEAD` by default |
| `mark` | no | Lines to mark, inside `lines`. A list of lines and ranges |
| `hide` | no | One line to hide, inside `lines`, for "Complete the code" |

- The server reads the lines at the commit that `rev` names when you push
  the page, and keeps that commit. The page shows the same code after the
  code changes.
- A path inside a submodule works. If the submodule does not hold the
  commit that the tree names, the error gives the command that fetches it.
- The page shows a hidden line as a blank. The server keeps its text
  until the learner answers every quiz of the page. A page with no quiz
  keeps the line hidden, so add a quiz to a page that hides a line.
- Put an excerpt at the top level of the page, not inside a list or a
  quote.
- Read the file before you write the fence, so that `lines` names the code
  that you explain.

## Diagrams

A fence with the info string `mermaid` is a diagram. The page draws it with
Mermaid, in the colors of its theme.

````
```mermaid
flowchart LR
  F[Follower] -- times out --> C[Candidate]
  C -- wins the vote --> L[Leader]
```
````

- A diagram holds 200 lines at most. The server refuses an empty one.
- Put a diagram at the top level of the page, not inside a list or a quote.
- Write no `%%{init}%%` line and no style: the page sets the colors.
- A diagram loads nothing from the network: put no image in a shape. The
  page blocks the load, and the diagram comes back to you as broken.
- Mermaid runs in its strict mode, so a `click` does nothing. Write no
  HTML in a label.
- Use a diagram for a structure or a sequence of messages. Show code with
  an excerpt.

The server cannot run Mermaid, so it cannot check the syntax. If Mermaid
refuses the source, the page shows the error in place of the diagram, and
your `wait` returns a request of kind `broken`:

```json
{
  "kind": "broken",
  "page": "3",
  "detail": {
    "block": { "id": "6495234d4654", "kind": "diagram", "source": "graph LR\n  A -->\n" },
    "error": "Parse error on line 2: ..."
  }
}
```

The learner sees the broken diagram until you fix it. Answer the request
before any other:

1. Find the diagram in the source of page `page`: `detail.block.source` is
   its text.
2. Fix the source with the help of `detail.error`.
3. Push the whole page again with `qlearn page`, then wait again.

The fixed diagram gets a new block id. You hear of a broken diagram once
for each push of its page, even when the learner opens the page again.

## Quizzes

A quiz checks what the learner got from a page. Push the page first, then
add its quiz:

```sh
qlearn quiz 3 --file - <<'EOF'
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
    "split-vote": { "label": "How a split vote ends", "lesson": "2" }
  }
}
EOF
```

- `open` is one open question: a `prompt`, and the `points` that its
  answer shows. Give `null`, or leave it out, for a quiz of choices only.
  An exercise, such as "Predict" or "Spot the bug", is an open question
  with no choices.
- `choices` holds 0 to 4 questions. Each one has a short `header`, a
  `prompt`, 2 to 4 `options`, the `answer`, and the `point` that it
  checks. `answer` is the index of the right option, from 0.
- An option is inline Markdown on one line: `code`, **bold**, *italic*,
  or a link. It holds no list and no heading. To open an option with a
  number or a dash, escape it: `1\.` or `\-`.
- A prompt and a header are plain text: the page shows them as you wrote
  them, backticks included.
- A point id holds lowercase letters, digits and hyphens, such as
  `split-vote`. `points` gives a `label` to each point that the course
  does not know yet. A point that an earlier quiz labelled needs no entry.
  If you give one, it repeats the label and the lesson of that point.
- An open question names at least one point. Each entry of `points` is a
  point that a question of the quiz names.
- `lesson` names the page that taught the point. The page of the quiz is
  the default. In the `mixed` quiz mode, a question on an earlier lesson
  names that lesson.
- The page shows the open question first, alone. The learner sees the
  choices only after the open answer, so their options cannot give it
  away. The page never gets `answer` before the learner sends the choices.

A page can hold several quizzes, in order: a second quiz teaches a missed
point again. A quiz on a page that the learner answered makes the page
wait for an answer again. The command prints the number of the new quiz in
its page, such as `{ "page": "3", "n": 1 }`.

If the server refuses the quiz, it stores nothing, exits with 1, and
prints every error on standard error, one per line, with its place in the
JSON:

```
choices[0].answer: 3 is out of range. The choice has 3 options, so answer takes 0 to 2
```

Fix every error, then run the command again.

## The answer

The learner sends the open answer, then the choices. Each choice also has
an "Other" box for free text. The server grades the choices, and the page
shows at once which ones are right. Once the learner has sent the last part
of a quiz, your `wait` returns a request of kind `answer`. Its `page` and
its `id` name the page and the number of the quiz. Its `detail` holds:

- `quiz`: the number of the quiz in its page.
- `open`: the open question, its `points`, and the `text` of the learner,
  or `null` for a quiz of choices only.
- `choices`: each choice as you wrote it, with the option that the learner
  `picked`, the right `answer`, `ok`, and the `other` text, or `null`.

The server adds the result of each choice to its point. You grade the open
answer in the debrief, which this release cannot write yet. Wait again.

## Next

The learner presses **Next** at the end of a page. Your `wait` returns a
request of kind `next`. Push the next page, then wait again. Before you
push it, check each claim of the page against the code of the repository.
