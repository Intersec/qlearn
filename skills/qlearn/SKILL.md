---
name: qlearn
description: >-
  Teach the user a subject of a code base (a mechanism, a module, an
  algorithm, a protocol) as a course in qlearn, a local page in the
  browser with short lessons, quizzes, code tours and exercises. Run only
  when the user asks for a course, runs /qlearn, or asks to resume a
  course of `.learn/`. If the user wants to learn a subject in depth,
  propose a course. Never start one on your own.
---

# qlearn

The procedure of this skill ships in the qlearn binary, so it always
matches the commands of the release that runs. It says how to teach the
course and how to drive the page. Read it first:

```sh
qlearn skill
```

Then follow what it prints.

If `qlearn` is not on the PATH, it is not installed. If it does not know
`skill`, it is a release from before this command. In both cases, tell the
user to install a newer release of qlearn, and stop.
