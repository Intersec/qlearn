# qlearn

qlearn shows a course on a code base in the browser of the learner. You
write the pages of the course. The learner reads them and presses **Next**
when they want the next page. Everything goes through `qlearn` commands.
Never edit the files under `.learn/` by hand.

This release knows pages of text, callouts, excerpts of the code, and the
**Next** button. It has no quiz and no syllabus yet.

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
4. Push the first page (see "Push a page"), then start the loop.

## The loop

Repeat until the learner says that the course is over:

1. Run `qlearn wait --after <next>` in the background. Use the `next`
   number of the last `wait`. The first time, use `--after 0`, so that a
   **Next** that the learner presses before your first `wait` is not lost.
2. When it returns, read the JSON:
   - `events`: the requests of the learner. A request of kind `next` says
     that the learner pressed **Next**. Its `detail.left` names the page
     that they left, with its id and its title.
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
3. Answer each request. For `next`, push the next page.
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

Number the pages 1, 2, 3, in the order of the course. A page that you push
again with the same id replaces the old one. The first heading of a page is
its title.

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

This release refuses these blocks, with an error at their line:

- A fence with the info string `mermaid`, `chart` or `steps`.
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
  until the learner answers the quiz of the page. This release has no
  quiz, so a hidden line never shows: do not use `hide` yet.
- Put an excerpt at the top level of the page, not inside a list or a
  quote.
- Read the file before you write the fence, so that `lines` names the code
  that you explain.

## Next

The learner presses **Next** at the end of a page. Your `wait` returns a
request of kind `next`. Push the next page, then wait again. Before you
push it, check each claim of the page against the code of the repository.
