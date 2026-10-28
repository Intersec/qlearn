# Roadmap

This directory holds the design of qlearn. It is the memory of the project
across sessions. Read it before you write code.

| Document | What it holds |
|---|---|
| [`concept.md`](./concept.md) | The problem, the users, the non-goals, the prior art |
| [`stack.md`](./stack.md) | The technology decisions and the rejected alternatives |
| [`design.md`](./design.md) | The architecture, the course model, the format of a page, the API, the commands of the agent |
| [`testing.md`](./testing.md) | The test policy. It is strict on purpose |

The order of the work is the four steps of section 14 of `design.md`.

`design.md` and `stack.md` are a contract. If a change contradicts the design,
stop and report. Do not change the design in silence.

## Outside this directory

| File | What it holds |
|---|---|
| [`../README.md`](../README.md) | What the tool is and how to use it |
| [`../CLAUDE.md`](../CLAUDE.md) | What a Claude session must do |
| [`../skills/qlearn/`](../skills/qlearn/SKILL.md) | The skill for Claude Code. It runs `qlearn skill` and follows it |
| [`../LICENSE`](../LICENSE) | Apache-2.0 |
