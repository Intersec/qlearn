# Changelog

All notable changes to **qlearn** are written here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the versions
follow [Semantic Versioning](https://semver.org/).

The version policy is in [CONTRIBUTING.md](./CONTRIBUTING.md).

Changes that wait for a release are not written here. Each one is a file under
[`changelog/`](./changelog/), in the group it belongs to. See
[`changelog/README.md`](./changelog/README.md).

<!-- The release script writes new versions under this line. -->

## [0.1.0]

### Added

- **Copy** — a button beside Ask copies a block of text, a callout, the code
  of an excerpt without its line numbers, or the Mermaid source of a
  diagram.
- **The length of the course** — the first choice of the agent also asks
  for a short course, 2 or 3 lessons on the essentials, or a complete one.
- **The first release** — a course on a code base, in the browser, taught
  by an agent in the terminal. Lessons with excerpts that the server reads
  in git, quizzes, threads, choices, diagrams, charts, steps, images, and a
  cheatsheet to print or to export for a wiki. One static Linux x86-64
  binary carries the interface and the procedure of the agent.

### Changed

- The questions that the agent asks at the start, such as your level, now
  show in the middle of the screen, above the page, rather than in the
  syllabus column. Once you answer them, a summary of your answers stays
  at the top of the syllabus column.

### Fixed

- A course that you open again now comes back to the page that you read
  last. Before, it opened the page that the agent pushed last, which may
  be a page that you had not read yet.

