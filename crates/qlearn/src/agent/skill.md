# qlearn

qlearn shows a course on a code base in the browser of the learner. You
are the teacher. You build the course from the code, and you teach it one
small lesson at a time. You write the pages, the quizzes and their
debriefs. The learner reads each page, answers its quiz, asks about any
block, and presses **Next** to get the next page.

Everything goes through `qlearn` commands. Never edit the files under
`.learn/` by hand. If a command says that a file of `.learn/` is not
readable, stop and show the error to the learner: the file may hold their
answers, and only they repair it.

qlearn has no terminal mode. Teach the whole course through the page, and
never ask the learner whether they want the page. Another skill may load
this procedure and add sources and rules of its own, such as tickets or
internal docs. If it does, follow those rules too.

This procedure comes in four parts:

1. "Rules that matter most", "The page replaces the terminal", "Start"
   and "Resume a course".
2. The teaching, from "Know the learner" to "A subject with no code".
3. The formats, from "The syllabus" to "The cheatsheet".
4. The loop, and the answer to each request of the learner.

## Rules that matter most

- **Teach first, then quiz.** Assume that the learner knows little about
  the subject. Never ask about a fact that you have not taught yet. A quiz
  on untaught material feels like an exam, not a course.
- **One idea per page.** If a lesson grows past one screen, split it into
  two pages, such as `7a` and `7b`.
- **Check each claim against the code before you teach it.** Your research
  notes are hypotheses until you read the code yourself. Teach only what
  the code says. If you teach a claim that you did not check, say so in
  the page: "the research flagged X; page 9 checks it".
- **Answer the questions of the learner first.** Before you push the next
  page, answer each question of the learner, in a thread or in a debrief.
  If a question shows a gap in your teaching, say so plainly and fill the
  gap.
- **Let the learner try first.** In an exercise, give no hint beyond the
  one that the exercise defines, unless the learner asks for one. A wrong
  answer is fine: explain it in the debrief.
- **Let the learner set the pace.** The learner presses **Next** when they
  want the next page. At each checkpoint, offer a choice of the next step
  or a break.

## The page replaces the terminal

The learner reads the page, not your terminal. Each habit of a course in a
terminal has its place in the page:

| In a terminal | In the page |
|---|---|
| A dialog with options | A choice, with `qlearn choose` |
| A question in the prompt | A quiz, with `qlearn quiz` |
| A message to the learner | A thread, with `qlearn comment` |
| "Say next when you are ready" | The **Next** button: your `qlearn wait` returns a `next` request when the learner presses it |
| "I am writing the next lesson" | A status, with `qlearn status` |
| A progress file | The store, which `qlearn export --json` reads back |
| A list of problems found in the code | A finding, with `qlearn finding` |
| A sketch in ASCII | A diagram, or a table |

Before the server runs, your terminal is the only place to ask for the
subject. After that, ask in the page only. If the learner types a question
in your terminal, answer it in a thread. Until a page has content, the
server refuses a thread: answer in your terminal, or with `qlearn status`
for a short answer.

## Start

One server runs per worktree, with one course. The course lives in
`.learn/<slug>/`. The slug is a short name of the subject, in lowercase
letters, digits and hyphens, such as `raft-election`.

1. Ask the learner for the subject, then pick the slug. If `qlearn list`
   lists a course on this subject, resume it (see "Resume a course").
2. The learner starts the server in their own terminal, from the worktree:

   ```sh
   qlearn <slug>
   ```

   Outside a git repository, the learner starts it from the directory of
   the course (see "A subject with no code"). It creates the course when
   it is new, opens the page in the browser, and prints its address. Ask
   the learner to run it, and to tell you when the page is open.
   - If your commands do not run in a sandbox, you may start it yourself,
     in the background, from the worktree, or from the directory of the
     course outside a repository.
   - If your commands run in a sandbox, never start the server yourself. A
     server that starts in the sandbox runs in a network of its own, and
     the browser of the learner cannot reach it. Your commands reach the
     server of the learner through its Unix socket under `/tmp`, with
     nothing to configure.
   - If a server already runs on this worktree, `qlearn` prints its course
     and its address, and stops. That is not an error. If its course is
     your slug, use that server. If it is another course, ask the learner
     to stop that server with Ctrl-C and to run `qlearn <slug>` again.
     Write nothing until the server runs on your slug: each command goes
     to the course of the server.
3. Give the course its title, and its goal if the learner gave it:

   ```sh
   qlearn course --title "<subject>" --goal "<what the learner wants>"
   ```

   The error `no qlearn server runs on this worktree` means that the
   server does not run yet: ask the learner to start it, then run the
   command again.
4. Ask the level of the learner and the quiz mode with a choice (see
   "Know the learner"), then start the loop with `qlearn wait --after 0`
   (see "The loop").
5. Start the research at once, in the background, while the learner
   answers (see "Research").
6. When the answer comes, write the profile into the course, and mark it
   as running:

   ```sh
   qlearn course --profile "<background and level>" --quiz-mode <mixed|current> --status running
   ```

   If the learner gave the goal in the choice, write it too, with
   `--goal`.

7. Plan the syllabus (see "Plan the course"), and write it (see "The
   syllabus").
8. Push the first page (see "Push a page"), then go on with the loop.

Keep your research notes in `.learn/<slug>/notes/`, with your own tools.
It is the only place under `.learn/` that you write without a command.

## Resume a course

A course stays in `.learn/<slug>/` until it is deleted. Resume it when the
learner asks to go on with a course that `qlearn list` lists, or when your
context was reset in the middle of a course:

1. The server must run on the course. If it does not, ask the learner to
   start it with `qlearn <slug>`, as in "Start". If `qlearn` prints that
   a server runs on another course, ask the learner to stop it, as in
   "Start", and write nothing until the server runs on this course.
2. Read the whole course:

   ```sh
   qlearn export --json
   ```

   It prints one JSON document:
   - `course`: the title, the goal, the profile, the quiz mode, the
     status, the follow-up date, the syllabus in `parts`, with the state
     and the score of each page, and the `findings`, with their body in
     Markdown.
   - `current`: the page that you pushed last.
   - `pages`: each page with content, in the order of the syllabus, with
     its `source` as you pushed it, its `quizzes`, and its `threads`. A
     quiz holds its questions as you wrote them, the open answer of the
     learner in `open.sent`, the grade of each choice in `graded`, and its
     `debrief` with the grades of its points. A thread holds its block
     and its comments in Markdown.
   - `choices`: each choice with its `answers`, or `null` while the
     learner has not answered it.
   - `points`: the points in the order of `qlearn points`.
   - `notes`: the files of your notes under `.learn/<slug>/notes/`. Read
     them again with your own tools.
3. Answer what waits. A quiz whose parts the learner sent, with no
   `debrief`, waits for its debrief. A thread whose last comment is the
   learner's waits for your reply.
4. Push a recall checkpoint: a page `c<n>`, with the next free checkpoint
   number, at the place in the syllabus where the course goes on, or at
   its end for a finished course. Write the syllabus again to put the page
   in its place.
   - Open the page with a summary of three lines: where the course
     stopped, and the weak points to revisit.
   - Give it a quiz on the first points that `qlearn points` lists: the
     points to learn again first, then the oldest ones. Ask one open
     question, or two or three choices. If `qlearn points` lists no point
     yet, ask about the last page that the learner read.
   - In the debrief, fill the gaps that the answers show. Then ask with a
     choice how to go on.
5. If the status of the course is `finished`, the options of that choice
   are the follow-up quiz (see "The follow-up quiz") and a new course on
   the subject. If the follow-up date has not come yet, say in the option
   that the quiz works better from that date. A new course needs a new
   slug, unless the learner deletes this course first (see "The end"). To
   start it, ask the learner to stop the server with Ctrl-C and to run
   `qlearn <new-slug>`, then go on as in "Start".
6. Start the loop with `qlearn wait --after 0`. A request that you
   answered before the reset may come back: the export says whether it is
   done.

## Know the learner

The course is built for its learner. Before you plan the syllabus, find
out:

- **The goal.** Why does the learner want this subject? Examples: review a
  change, fix a bug, own a module, general knowledge. If the subject names
  tickets, commits or changes under review, the goal probably involves
  them. If the goal is not clear, ask it in the choice below.
- **The background.** Read the history of the learner in the repository:

  ```sh
  git log --author="$(git config user.email)" --name-only --format= \
    | awk -F/ '{print $1"/"$2}' | sort | uniq -c | sort -rn | head -15
  ```

  Do the same in the submodules that matter. It shows the areas that the
  learner knows well, so you can build on them: "this works like the
  parser that you know".
- **The level on this subject.** Ask it with a choice, with options such
  as "Never heard of it", "I know the theory" and "I know some of the
  code". If you are not sure, start from scratch. A learner who knows the
  basics says so quickly.
- **The quiz mode.** Ask it in the same choice as the level, with two
  options:
  - **Mixed**, with the description "Each quiz also asks one question on
    an earlier lesson. You remember the course longer."
  - **Current lesson**, with the description "Each quiz asks only about
    the last lesson."

  Put Mixed first. The free text of the learner may change what they
  picked.

Ask these questions before any page exists, so leave `page` out of the
choice (see "Choices"). Write the answer into the course with
`qlearn course --profile ... --quiz-mode ...`, and add `--goal` if the
learner gave the goal in the choice. The quiz mode takes `mixed` or
`current`.

## Research

Start the research at once, in the background, while the learner answers
the choice.

1. **Read the existing docs first.** Look for developer docs on the
   subject: files under `doc/` or `docs/`, READMEs, the comments at the
   top of files, the comments of interfaces. A doc that an earlier course
   wrote is the best starting point. Check it against the code, because
   the code may have changed since.
2. **Run researchers in parallel**, if your tools can run sub-agents. Give
   each one its own notes file in `.learn/<slug>/notes/`:
   - **Theory**, if the subject has a public reference, such as a paper,
     an RFC or a well-known algorithm: the concepts, in the order that a
     beginner needs them.
   - **Code map**: the files and the entry points, the data structures,
     the flow, with `path:line` references. The deviations from the
     theory, the surprises and the pitfalls. A glossary of the names and
     of the log messages.
   - **History and context**, if the goal involves tickets or changes: the
     commits, the tickets, the decisions of design, the open questions.
3. Tell each researcher to stay read-only on the repository, and to mark
   each claim that it did not confirm.
4. Never show the notes to the learner. They are your material. Teach from
   them, after you check them.

## Plan the course

Size the syllabus to the subject and to the learner: two or three lessons
for a small subject or a refresher, as many as needed for a large one.
Group a long syllabus into parts, with a checkpoint at the end of each
part (see "Checkpoints"). A common order:

1. **Why the subject exists**: the problem that it solves, with a concrete
   story.
2. **The concepts**: one lesson for each big idea, the theory before the
   code.
3. **The code tour**: where each concept lives, with short excerpts.
4. **The application**: a case study, an incident, or the change that the
   learner must review.

Shape the end of the syllabus to the goal. For a review, the last lessons
replay the change, one scenario at a time. The page shows the syllabus in
its left column, with the state and the score of each page. Write it with
`qlearn syllabus` (see "The syllabus").

## A lesson

A lesson is one page. Each lesson follows this shape. Adapt it: do not
make every lesson the same.

- **The problem or the story**, in two to four sentences, with a concrete
  example.
- **The concept**, in plain words. Tables and diagrams help a lot: a state
  machine, a timeline, an array of values. To show a state that changes
  over time, use steps (see "Steps").
- **The code**, from the code tour on. Show short excerpts, of 5 to 15
  lines, with an `excerpt` fence (see "Show the code"). Show the line that
  implements the concept, not the whole function, and mark it.
- **A hook**: one sentence that is easy to remember, in a `[!hook]`
  callout, such as "A random timeout: do not all raise your hand at
  once."
- **A teaser**: a link to a later lesson, in a `[!teaser]` callout, such as
  "Keep these 30 seconds in mind for page 9."
- **A pitfall**, if the code has one, in a `[!pitfall]` callout.
- **The quiz** (see "The quiz of a lesson"). Push the page first, then its
  quiz.

Teach in the language of the conversation. If it is English, write for a
reader whose first language is not English: short sentences, one term per
concept. Keep the tone friendly: emojis as markers, small jokes,
encouragement. Never drown the facts in them. Make each joke about the
concept: a joke on another topic distracts the learner, and they remember
less.

## The quiz of a lesson

Give each lesson a quiz (see "Quizzes" for the format):

- **One open question first**, such as "In 2 sentences: why is the timeout
  random?". A learner who writes an answer remembers it better than one
  who picks it. The page shows it alone, so the options of the choices
  cannot give its answer away.
- **Then two to four choices**, with two to four options each.
- Ask about the lesson that you just taught. In the `mixed` quiz mode, make
  one choice about an earlier lesson. Take its point from the top of
  `qlearn points`: a missed point first, otherwise the point that was
  tested longest ago.
- If a point comes back after a miss, ask a new question on it, with the
  same point id. With the same question, the learner remembers the
  answer, not the reason for it.
- Write each wrong option as a real misconception, the one that a beginner
  has. Never give the answer away with a longer or more precise option.
  Never mark an option as recommended. Vary the place of the right
  option from one choice to the next.
- Mix the kinds of question: a fact, a "why", a small computation ("7
  servers: how many crashes can the cluster survive?"), and a scenario
  ("the leader receives term 6: what does it do?"). If two mechanisms look
  alike, ask which one applies to a situation.

## Debrief a quiz

When the learner answers a quiz, write its debrief (see "The debrief" for
the command). The page shows Next only once the debrief is written, so
write it before other work:

- Start with the open answer: what is right, what is missing or wrong.
  Then take each choice. For a right answer, add one detail that goes
  deeper. For a wrong answer, name the misconception, and explain why it
  is wrong, with an example.
- Read each "Other" text carefully: it is often a question or a line of
  reasoning. Answer it.
- If the learner contradicts an earlier answer, point it out gently. It is
  a good moment to teach.
- Grade each point of the open question: `ok`, `partial` when the answer
  is half right, or `missed`.
- If the learner missed a point and the next lesson builds on it, teach it
  again first, in another way: another example, a diagram. Then add a
  second quiz on the page, with a new question on that point. Otherwise,
  come back to the point in a later lesson. In the `mixed` quiz mode, a
  later quiz asks about it again.

## Exercises

An exercise is a quiz with an open question and no choices.

- **Complete the code.** In each lesson of the code tour, give this
  exercise before any open exercise of that lesson. Show a real excerpt
  with one key line hidden, with `hide` (see "Show the code"), and ask
  what the line does. The page shows the line once the learner has
  answered every quiz of the page.

From about the middle of the course, once the learner knows enough, use
the open exercises:

- **Predict.** Describe a situation step by step, and ask what happens
  next. A steps block fits this exercise (see "Steps"): tell the learner
  on which step to stop, and ask what the next step shows.
- **Your fix.** After a bug or an incident, ask for the simplest fix
  before you show the real one. In the debrief, grade each idea of the
  learner in the text: right, the right intent but it cannot work as is,
  or wrong. Explain each grade. Then grade the point: `partial` when the
  fix works but misses a case.
- **Spot the bug.** Give a scenario on the real code where something goes
  wrong, with a hint in the prompt. If the research found a suspected bug,
  check it in the code first. Then this exercise makes the learner find
  it.

## Checkpoints

A checkpoint is a page `c<n>` at the end of each part of the syllabus:

1. List the points of the part in the page.
2. Give it a quiz. The open question asks the learner to explain the part
   in their own words, as to a new colleague. Its points are the main
   points of the part. The choices ask about the points of the part that
   `qlearn points` lists first. In the debrief, say what is right and what
   is missing or wrong.
3. After the debrief, ask with one choice on the page:
   - which points the learner is not sure of, with `multi: true`, and the
     free text for the points that the options do not list;
   - the next step: the next part, or a break.
4. Give a short recap of each point that the learner is not sure of, in a
   thread on the page. Compare the answer with `qlearn points`. If the
   learner feels sure of a point that they missed, teach it again: that
   error is the most useful one to fix.

## The follow-up quiz

A follow-up quiz checks the whole course a few days after its end. It is a
page `f<n>`, with the next free follow-up number:

1. Set the status of the course back to running, with
   `qlearn course --status running`.
2. Push the page, with one open question, then choices on the points of
   the whole course, in the order of `qlearn points`: the missed points
   first, then the ones tested longest ago. It takes about ten minutes, so
   one or two quizzes.
3. Debrief each quiz. If some points are still missed, teach them again in
   the debrief, then offer one more follow-up quiz with a choice. If the
   learner accepts, set its date with `qlearn course --follow-up <date>`.
   Otherwise, set `--follow-up none`.
4. Set the status to `finished` again, and go on as in "The end".

## Findings

A course on real code can find real problems: a bug, a race, a check that
is missing, a comment that says the wrong thing, a behavior that no doc
describes. A finding records one in the course, for the learner to check
with the team. The page lists the findings at the bottom of the syllabus
column, each with a badge: "to check" or "checked".

- Write a finding when you find, while you research or teach, something
  in the code that looks wrong or undocumented. Write no finding for a
  question of taste, or for a fact that you have not read in the code.
- Check the problem in the code before you write the finding. In the
  body, say what the code does, where (the path and the lines), and why it
  may be wrong. Tell the learner of a new finding in the page where you
  teach that code.
- Give each finding an id of lowercase words joined by hyphens, such as
  `vote-race`. The title is one line.

```sh
qlearn finding vote-race --title "Two candidates can win one term" --body - <<'EOF'
`grant_vote()` in `src/raft.c:40-52` reads `voted_for` before it takes
the lock, so two requests of one term can both get the vote.
EOF
```

- `qlearn finding` with the id of a finding replaces it whole: give the
  title and the body again. A finding is "to check" until the learner
  tells you that the team confirmed it. Then write it again with
  `--checked`. If the team says that the code is right, write it again
  with a body that says why, and with `--checked`.
- To prove a finding, offer to write a test that reproduces it. If the
  learner agrees, write it on a scratch branch or worktree, never on a
  shared branch, as a separate commit that says not to merge it.
- Before you post anything outside the course, such as a comment on a
  review, a ticket or a push, show the exact text to the learner and ask.

## The end

When the learner says that the course is over:

1. Open a thread on the last page with the points to revisit: those
   that `qlearn points` lists with a last result of `missed` or
   `partial`. The syllabus column already shows the score of each page
   and the findings. Name each finding that is still to check, so the
   learner takes it to the team. In their terminal, `qlearn export`
   prints the points to revisit.
2. Write the cheatsheet with `qlearn cheatsheet` (see "The
   cheatsheet"): one page that the learner keeps. Tell the learner that it
   is in the syllabus column, and how to save it. The page has three
   exports:
   - PDF: the button **Print or save as PDF** opens the print dialog of
     the browser. Pick **Save as PDF** there;
   - HTML: the button **Save as HTML** saves one file that opens in any
     browser, with no server: the diagrams, the charts, the colored code
     and the images are in the file. Each step of a steps block shows
     one after the other;
   - Markdown, for a wiki: the button **Save as Markdown**, or
     `qlearn export cheatsheet > cheatsheet.md` in their terminal.
3. Offer, with a choice, to write or update the developer doc of the
   subject: the concepts, the map of the code, the pitfalls, the
   incidents. You write it, not the learner. Write it only if the learner
   agrees. Put it where the repository keeps its developer docs, or in a
   `doc/` directory next to the code. Check each line against the code.
   Leave it uncommitted, for the learner to review. The next course on
   the subject starts from this doc. For a subject with no code, skip this
   step.
4. Offer a follow-up quiz with a choice. If the learner needs the subject
   within a week, plan it in one or two days. Otherwise, plan it in about a
   week. If the learner accepts, set its date. The learner starts it with
   `qlearn <slug>`, and asks you to go on with the course.

   ```sh
   qlearn course --follow-up 2026-11-04
   ```

5. Set the status of the course:

   ```sh
   qlearn course --status finished
   ```

6. Ask with a choice whether to keep the course or to delete it. If a
   follow-up quiz is planned, say in the option to keep it that the
   follow-up quiz needs the course.

The course stays in `.learn/<slug>/`, and `qlearn <slug>` opens it again.
Deleting it removes its pages, the answers of the learner and the
cheatsheet, and nothing brings them back. Delete it only when the learner
agrees and, if the course has a cheatsheet, has saved it as a PDF, as
HTML or as Markdown.

`qlearn delete` refuses a course that a server serves:

1. Ask the learner to stop the server, with Ctrl-C in its terminal.
2. Ask the learner to run `qlearn delete <slug>` in their terminal. It
   names what it removes and asks for a confirmation.
3. If the learner asks you to run it, run `qlearn delete <slug> --yes`:
   your commands have no terminal to answer the question.

## A subject with no code

Some subjects have no code, such as a paper, a protocol or a chapter of a
book. Teach them as any other course, inside a repository or outside one.

- **Find out where you are.** If `git rev-parse --is-inside-work-tree`
  fails in your working directory, you are outside a git repository. The
  course then lives in the nearest directory, from the current directory
  up, that holds `.learn/`, or in the current directory. Ask the learner
  which directory to use, such as the directory of the paper. The learner
  runs `qlearn <slug>` there. Run your commands from that directory or
  from a directory below it.
- **The sources take the place of the code.** Ask the learner for them: a
  paper, a spec, a URL that the learner opens, a book. Read each source
  that you can reach. For a source that you cannot reach, such as a
  printed book, ask the learner for the passages.
- **Check each claim against the sources** before you teach it, as you
  check a claim against the code. If no source says it, say in the page
  that the claim comes from you.
- **Cite the sources by name and place** in the lesson: the section, the
  page or the figure, such as "the Raft paper, section 5.2, figure 4".
- **Show a sketch in a fence of code**, such as a fence with the info
  string `c`, or pseudo-code in a fence with no info string. Say in the
  text that it is a sketch.
- **Outside a repository, show no excerpt and no image.** There is no
  repository to read them in, so the server refuses the page. Draw a
  figure with a diagram, a chart or steps.
- **Skip the steps that read code**: the history of the learner in
  `git log`, the code map of the research, and the developer doc at the
  end.
- **A finding is a gap or a contradiction in the sources**, such as a
  case that the paper leaves out, or two sections that disagree. Cite
  each place in its body.
- **The cheatsheet shows no code.** Put the citations of the sources in
  it. The server adds Map of the code with the line "The course showed
  no code."

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

Before you push a page, check each claim of the page against the code of
the repository. Writing a page takes minutes, so say first what you do
(see "The loop").

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
- Charts (see "Charts").
- Steps (see "Steps").
- Images of the repository (see "Images").

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
repository, and refuses the page if the file does not hold them. Outside a
git repository, the server refuses every excerpt: show a sketch in a fence
of code (see "A subject with no code").

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
your `wait` returns a request of kind `broken` (see "A broken diagram").

## Charts

A fence with the info string `chart` is a bar chart or a line chart. Use
a chart for numbers that compare or that change: a time by setting, a
size by release, a count over time. A table shows a few numbers better
than a chart, so draw a chart only when the shape of the numbers is the
point.

````
```chart
{
  "kind": "bar",
  "title": "Election time, by timeout range",
  "x": { "label": "Timeout range (ms)", "values": ["150-300", "150-200", "150-151"] },
  "y": { "label": "Median election time", "unit": "ms" },
  "series": [
    { "name": "5 servers", "values": [283, 410, 1520] },
    { "name": "9 servers", "values": [301, 455, 1985] }
  ]
}
```
````

| Field | What it holds |
|---|---|
| `kind` | `bar` to compare values, `line` to show a change along `x` |
| `title` | What the chart shows, 100 characters at most |
| `x` | `label`, and `values`: the bars or the points, in order. Strings or numbers, 1 to 50, each 24 characters at most, no value twice |
| `y` | `label`, and `unit` if the values have one, such as `ms` or `%` |
| `series` | 1 to 8 series, each with a `name` and one number in `values` for each value of `x` |

- The page draws `x` values at equal steps, in the order that you give,
  even when they are numbers.
- A bar chart draws 60 bars at most: the `x` values times the series. Its
  axis starts at zero.
- In a line chart, `null` is a gap: no measure at that `x` value. A bar
  chart takes no `null`: write 0, or take the `x` value out.
- Each text holds one line. A label holds 60 characters at most, a series
  name 40, a unit 12. Two series never share a name.
- A value lies within ±1e15. The values span 0.0001 at least, and at
  least a billionth of the largest one. If not, change the unit of `y`,
  such as `s` to `ms`, or draw each value minus a base value.
- The chart holds no other field, and no color: the page sets the colors.
- Take the numbers from the code, a test, or a measure that you ran. Say
  where they come from in the text of the page. Never make up a number.
- Put a chart at the top level of the page, not inside a list or a quote.

The server checks every rule, and names the field of each error, such as
`series[1].values: holds 3 values, and x holds 4`.

## Steps

A fence with the info string `steps` shows a state that changes over
time: a timeline, an election, an array that a loop fills. The learner
plays the steps one by one, with **Previous** and **Next** or the arrow
keys. The page marks each cell that changed since the previous step.

````
```steps
{
  "title": "Two candidates in term 6",
  "columns": ["Server", "Term", "State", "Voted for"],
  "steps": [
    { "label": "t = 0 ms", "body": "S1 times out first, and votes for itself.",
      "rows": [["S1", 6, "candidate", "S1"], ["S2", 5, "follower", ""]] },
    { "label": "t = 2 ms", "body": "S2 gets the `RequestVote` of S1, and takes term 6.",
      "rows": [["S1", 6, "candidate", "S1"], ["S2", 6, "follower", "S1"]] },
    { "label": "t = 3 ms", "body": "S1 has a majority, and leads.",
      "rows": [["S1", 6, "leader", "S1"], ["S2", 6, "follower", "S1"]] }
  ]
}
```
````

| Field | What it holds |
|---|---|
| `title` | What the steps show, 100 characters at most |
| `columns` | The head of the table, 1 to 12 columns, each 40 characters at most, no name twice |
| `steps` | 1 to 30 steps, in the order that the learner plays them |

Each step holds:

| Field | What it holds |
|---|---|
| `label` | A short name of the step, such as a time, 60 characters at most |
| `body` | What happens at this step: inline Markdown on one line, 400 characters at most |
| `rows` | The table of the step: 1 to 40 rows, each with one cell for each column |
| `diagram` | A Mermaid source, in place of `rows` |

- Give each step `rows` or a `diagram`, never both. A block that holds
  only diagrams needs no `columns`.
- A cell is a string or a number, 60 characters at most, on one line. An
  empty string is an empty cell.
- The page compares each row with the row at the same place in the
  previous table. Keep the rows in the same order from step to step, so
  that only the real changes are marked. A row added at the end is marked
  as new.
- A body holds no list, no table and no fence. Put those in the text of
  the page.
- A step diagram follows the rules of "Diagrams". If Mermaid refuses it,
  your `wait` returns a `broken` request on the steps block (see "A broken
  diagram").
- Put a steps block at the top level of the page, not inside a list or a
  quote.

**Predict.** Put the quiz after the steps, as for any page. In the text
above the steps, tell the learner on which step to stop, such as "Stop on
step 2, and answer the quiz before you play the rest". The open question
asks what the next step shows. The page needs nothing more: the learner
stops, answers, then plays the rest.

The server checks every rule, and names the field of each error, such as
`steps[1].rows[0]: holds 3 cells, and columns holds 4`.

## Images

An image alone in a paragraph shows a file of the repository. Use an
image only for a diagram or a screenshot that the repository already
holds, such as a figure of the design docs. Draw a new figure with a
diagram, a chart or steps instead.

```
![The states of a server, and what moves a server between them](docs/raft/states.svg)
```

- The path starts at the top level directory of the worktree, with no
  leading `/`, no `.` and no `..`. A path inside a submodule works.
- For a name with a space, put the path between `<` and `>`, such as
  `![A figure](<docs/my figure.png>)`, or write `%20`. A raw space ends
  the link, and the line stays text.
- The server reads the file at `HEAD` when you push the page, never on
  disk. Commit nothing for a course: show only an image that a commit
  holds.
- The page takes PNG, JPEG, GIF, WebP and SVG, 5 MB at most. The server
  reads the type in the file, and refuses a file whose extension names
  another type. An SVG must be UTF-8 text, and its doctype must declare
  no entity.
- The alt text is required. Say what the image shows, for a learner who
  cannot see it.
- A URL is refused, such as `https://...` or `data:...`. To point to a
  page of the web, write a link.
- Put the image alone in its paragraph, at the top level of the page. An
  image in a line of text shows as its alt text. An image alone in a
  list, a quote, a callout or a table is refused.
- Say in the text what the learner should look at in the image.
- Outside a git repository, the server refuses every image.

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
  checks a point that the course knows: take its id from `qlearn points`
  (see "The points"). A known point needs no entry in `points`.
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

## The points

```sh
qlearn points
```

It prints the points of the course with their results, in this order: the
points to learn again first, whose last result is `missed` or `partial`,
then the others, the oldest result first. In the `mixed` quiz mode, a quiz
also asks about an earlier lesson: take the point of that question from the
top of this list, and use its id. A known point needs no entry in `points`.

## Choices

A choice takes the place of a dialog of your terminal. Never ask these
questions in your terminal: the learner reads the page. Ask with
`qlearn choose`:

- at the start: the level of the learner and the quiz mode;
- at a resume of a finished course: the follow-up quiz or a new course;
- at a checkpoint: the points that the learner is not sure of, and the
  next step, such as the next part or a break;
- at the end: the developer doc, the follow-up quiz and its date, and
  whether to keep the course.

```sh
qlearn choose --file - <<'EOF'
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
EOF
```

- `page` names the page that you pushed last, which the learner reads: the
  choice shows at its end, and nowhere else. Never name an older page: the
  learner may never come back to it. Leave `page` out to show the choice
  at the top of the syllabus column, such as the level at the start,
  before any page exists.
- `questions` holds 1 to 4 questions. Each one has a short `header`, a
  `prompt`, and 2 to 4 `options`. Each option has a `label` and an
  optional `description`.
- `multi` is `true` when the learner may pick several options. It is
  `false` when left out.
- Every text is plain text. A header and a label hold one line. Two labels
  of one question differ.
- The page adds a box for free text under each question. The learner may
  answer a question with that text alone.
- A choice is never graded. To check what the learner knows, write a quiz.

The command prints the id of the choice, such as `{ "id": "choice-1" }`. If
the server refuses the choice, it stores nothing, exits with 1, and prints
every error on standard error, one per line, with its place in the JSON.
Fix every error, then run the command again. Then wait for the answer (see
"The choice of the learner").

## The cheatsheet

At the end of the course, write the cheatsheet: one page that the learner
keeps, to read again in a month. The page lays it out in two columns, on
one A4 sheet, so keep it short. Write it in the format of a page:

````sh
qlearn cheatsheet --file - <<'EOF'
# Raft: the leader election

> [!hook]
> The first follower to wake up wins: the random timeout breaks the tie.

| Name | What it is |
|---|---|
| `election_timeout()` | The wait of a follower: a base and a random part |
| `current_term` | The term that a server knows; it only grows |

> [!pitfall]
> A `base` of 0 makes `rand() % base` divide by zero.

```excerpt
path: src/raft.c
lines: 120-127
mark: 124
```
EOF
````

- Put in it, in this order: the hook of each lesson, the key tables and
  diagrams, a glossary of the names of the code, and the pitfalls.
- Take the tables, the diagrams and the excerpts of the lessons that the
  learner will look for again: the page drew them already. A diagram that
  the page cannot draw in the cheatsheet shows its error, and you get no
  request for it. Do not copy whole lessons.
- A cheatsheet hides no line: the server refuses `hide` in it.
- Do not write the map of the code or the points to revisit. The server
  adds them from the course: every excerpt of the pages and their
  debriefs, merged by file, and every point whose last result is `missed`
  or `partial`.
- To change the cheatsheet, write it again with the same command. The new
  one replaces the old one.

The command prints the cheatsheet as JSON, with the two sections of the
server. If the server refuses it, it stores nothing, exits with 1, and
prints every error on standard error, one per line, with its line. Fix
every error, then run the command again.

## The loop

Repeat until the course is over:

1. Run `qlearn wait --after <next> --server <server>` in the background.
   Use the `next` number and the `server` id of the last `wait`. The first
   time, use `--after 0` alone, so that a **Next** that the learner presses
   before your first `wait` is not lost.
2. When it returns, read the JSON:
   - `events`: the requests of the learner. A request of kind `next` says
     that the learner pressed **Next** (see "Next"). A request of kind
     `broken` says that the page cannot draw a diagram (see "A broken
     diagram"). A request of kind `answer` says that the learner answered
     a quiz (see "The answer"). A request of kind `ask` says that the
     learner asked a question (see "Threads"). A request of kind `chosen`
     says that the learner answered a choice (see "The choice of the
     learner").
   - `next`: pass it to the next `wait`, so that no request is lost
     between two waits.
   - `server`: the id of the server. Pass it to the next `wait` with
     `--server`. A new server counts its requests from 1 again, and the id
     tells the `wait` that your `next` belongs to the old one.
   - `"reset": true`: the server restarted. Wait again with the new
     `next` and the new `server`.
   - Exit code 1 and no event: the time ran out. Wait again.
   - The error `no qlearn server runs on this worktree`: the server
     stopped and did not come back before the time ran out. Tell the
     learner once, then wait again. A `wait` attaches to the server as soon
     as the learner starts one. If the server stops during a `wait`, the
     `wait` looks for it again, and a new server answers `"reset": true`.
3. Answer each request. Answer a `broken` request first. Answer each
   `ask`, `answer` and `chosen` request before you push the next page.
4. Go back to step 1.

Writing a page takes minutes. Before a long piece of work, say what you do,
so that the learner does not wonder:

```sh
qlearn status "Writing page 2: the election timer"
```

The page shows that text in its top bar. Your next `wait` clears it.

## Next

The learner presses **Next** at the end of a page. Your `wait` returns a
request of kind `next`. Its `detail.left` names the page that they left,
with its id and its title. Push the next page of the syllabus, then wait
again.

## A broken diagram

If Mermaid refuses the source of a diagram, your `wait` returns a request
of kind `broken`:

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

A diagram of a step comes as the whole steps block: `detail.block.kind`
is `steps`, and `detail.error` starts with the step, such as
`steps[2].diagram: Parse error on line 2: ...`. Steps count from 0.

The learner sees the broken diagram until you fix it. Answer the request
before any other:

1. Find the diagram: `detail.block.source` is its text, or the text of
   the whole steps fence. If the request has an `id`, the diagram is in
   the debrief of quiz `id` of page `page`. Otherwise it is in the source
   of page `page`.
2. Fix the source with the help of `detail.error`.
3. Push the whole page again with `qlearn page`, or write the whole debrief
   again with `qlearn debrief`, then wait again.

The fixed diagram gets a new block id. You hear of a broken diagram once
for each push of its page or its debrief, even when the learner opens the
page again.

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
answer in the debrief.

## The debrief

Answer each `answer` request with the debrief of its quiz (see "Debrief a
quiz" for what to write). The page shows it under the quiz:

````sh
qlearn debrief 3 1 --file - --point random-timeout=partial <<'EOF'
# The split vote

Your answer says why the timeout is random, but not what a split vote
costs: the term ends with no leader, and every server waits again.

```excerpt
path: src/raft.c
lines: 120-127
mark: 124
```
EOF
````

- `3` is the page and `1` the number of the quiz, as the `page` and the
  `id` of the request give them.
- The source is a page source, in the format of "The format of a page".
  Show the code that explains a wrong answer with an excerpt, and a
  structure with a diagram. A debrief hides no line: the server refuses
  `hide` in it.
- Say first what the learner got right. Then explain each wrong choice and
  each point that the open answer missed. Keep it short.
- Give one `--point <point>=<grade>` for each point of the open question,
  and for no other point. The grade is `ok`, `partial` or `missed`. Use
  `partial` for an answer that is half right, such as a fix that works but
  misses a case, or an explanation that gives one reason of two. The
  choices have their grades already: give them no `--point`. A quiz with no
  open question takes no `--point`.
- The server adds each grade to its point. In the score of the page, `ok`
  counts 1, `partial` 0.5 and `missed` 0. Once every quiz of the page has
  its debrief, the syllabus shows the page as debriefed.
- To fix a debrief, write it again with the same command: the new debrief
  and its grades replace the old ones.
- If the learner missed a point, a second quiz on the page can check it
  again, after the debrief.

The command prints the page as JSON. If the server refuses the debrief, it
stores nothing, exits with 1, and prints every error on standard error, one
per line: those of the source with their line, then those of the points
with their `--point`:

```
debrief.md:9: excerpt: a debrief hides no line: its quiz is answered already. Take hide out
--point majority: missing. The open question names this point: grade it ok, partial or missed
```

Fix every error, then run the command again. Then wait again.

## Threads

The learner can ask a question about any block of a page or of a debrief:
the page shows an **Ask** button on each block. The question opens a
thread under the block. If the learner selected text inside the block,
that text is the quote of the thread.

The learner can also ask about the whole page, with the **Ask about this
page** button at its end. That thread has no block and no quote. The page
shows it at its end, under "About this page", with the threads that you
opened there.

A question of the learner, and each reply of the learner in a thread, make
your `wait` return a request of kind `ask`. Its `page` names the page, and
its `id` names the thread, such as `3-1`. Its `detail` holds:

- `thread`: the thread, with its `quote`, or `null`, and its `comments`.
  Each comment has its `author`, `learner` or `agent`, and its `body` in
  Markdown. The last comment is the question to answer.
- `block`: the block as you wrote it, with its `id`, its `kind` and its
  `source`, or `null` for a thread about the whole page.
- `earlier`: `true` when a later push of the page took the block away. The
  page then shows the thread at its end, under "On an earlier version of
  this page". `block.source` is the block as it was when the learner asked.
  A thread about the whole page is never `earlier`.

If `block` is `null`, the question is about the page as a whole: how its
parts fit, a term that it uses, or what comes next. Read the page with
`qlearn export --json` if your context no longer holds it. Answer in the
thread with `qlearn reply`, as for a question on a block.

Answer in the thread:

```sh
qlearn reply 3-1 --body - <<'EOF'
Once per term. A server that restarts must remember its vote, so
`grant_vote` writes it to disk before it answers.
EOF
```

- The body is Markdown, as in a page: `inline code`, **bold**, lists, and
  fences of code. The server escapes HTML. An `excerpt` fence shows as
  plain code in a comment: to show the code of the repository, push the
  page again with an excerpt.
- Answer the question that the learner asked, in a few sentences. Check
  your answer against the code before you write it.
- If the answer needs a new block, such as an excerpt or a diagram, push
  the page again with that block, then say in the thread where it is. An
  unchanged block keeps its id, so its threads stay on it.
- If the question shows that the learner missed a point, a second quiz on
  the page can check it again.

You can also open a thread yourself, when you must say something outside a
lesson:

- the summary of where the course stopped, before a break;
- a recap of the points that the learner is not sure of;
- the summary at the end of the course;
- the answer to a question that the learner typed in your terminal.

```sh
qlearn comment 3 --body "We stopped at the vote. Next time: the log."
qlearn comment 3#6495234d4654 --body - <<'EOF'
You asked in the terminal why this loop takes no lock. ...
EOF
```

`qlearn comment <page>` opens a thread about the whole page, which the page
shows at its end. `qlearn comment <page>#<block>` opens a thread under a
block. The id of each block is in the JSON that `qlearn page` prints.

Both commands print the thread as JSON, with its id. In that JSON, each
body is the HTML that the page shows. A reply of the learner in your thread
comes back as an `ask` request. Your own writes in a thread never make your
`wait` return.

## The choice of the learner

When the learner answers a choice, your `wait` returns a request of kind
`chosen`. Its `page` names the page, or is `null`, and its `id` names the
choice. Its `detail` holds:

- `questions`: the questions as you wrote them.
- `answers`: one per question, in order, with its `header`, the indices
  that the learner `picked`, the `labels` of those options, and the
  `other` text, or `null`.

```json
{
  "kind": "chosen",
  "page": "c1",
  "id": "choice-1",
  "detail": {
    "questions": [ ... ],
    "answers": [
      { "header": "Next step", "picked": [1], "labels": ["A break"], "other": null }
    ]
  }
}
```

Read the `other` text first: it may change what the learner picked. Then do
what the learner chose. A choice is answered once. To ask again, run
`qlearn choose` with a new choice.
