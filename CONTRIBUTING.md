# Contributing

The rules of the project. The design is in [`roadmap/`](./roadmap/README.md),
and the test policy in [`roadmap/testing.md`](./roadmap/testing.md).

## The gate

```sh
make check
```

Run it before every commit. GitHub Actions runs the same command on every
push to `main` and on every pull request. Never push to `main` with a red
`make check`.

## Commits

- One task, one commit. The task is the unit that a reader can test alone.
- Say the **why**. A subject alone is enough only for a trivial change.
- English, imperative, subject 72 characters or less, body wrapped at 72.
- A [Conventional Commits](https://www.conventionalcommits.org) prefix in the
  subject, counted in the 72: `feat`, `fix`, `docs`, `test`, `refactor`,
  `perf`, `chore`, `ci`, `style`.
- If the commit answers an issue, name it in the body, on a line of its
  own: `Closes #12` when the commit does the whole of it, `Refs #12`
  otherwise.

## Changelog

Do not edit `CHANGELOG.md`. It holds the released versions, and
`cargo xtask release` is the only thing that writes it.

A change that a user can see drops one file in `changelog/added`,
`changelog/changed`, `changelog/fixed`, or `changelog/removed`. One file per
entry, named after the change. See
[`changelog/README.md`](./changelog/README.md).

`cargo xtask changelog` prints what the next release would say.

## Versions

[Semantic versioning](https://semver.org), read for a tool before 1.0:

- **`0.x`** — while more than one person does not yet use the tool every
  day.
- **Minor** (`0.1` → `0.2`) — a notable feature. Most releases.
- **Patch** (`0.1.0` → `0.1.1`) — fixes only.
- **`1.0`** — no open issue blocks daily use, and a second person uses the
  tool.
- **Major** — a change of the storage format that the tool cannot migrate.

The storage format carries a `version` field. A change of the format owes a
migration, as [`roadmap/design.md`](./roadmap/design.md) says.

The version lives in the workspace `Cargo.toml`, alone. The `package.json`
of the interface is private and carries no version.

## Release

```sh
make check
cargo xtask release 0.2.0   # collects the changelog entries, writes
                            # CHANGELOG.md, bumps Cargo.toml, commits, tags
git push origin main v0.2.0
```

`cargo xtask release` prints the push line, with the tag that it just
wrote. It commits only the files that it writes, and refuses a tree with an
uncommitted change.

Push the tag by name. Do not use `--follow-tags`: it sends a tag only beside
a branch that it pushes. If the branch is up to date, it sends nothing, and
the release workflow never starts.

The tag starts the `release` workflow. It runs the gate again on the commit
that the tag names, builds the static Linux x86-64 binary, and creates the
GitHub release. The release carries the notes of that version and the
binary. If the gate is red, the tag publishes nothing. The token that
publishes is the one that Actions gives to the job, so there is nothing to
configure.

The `check` workflow skips a commit named `chore(release): v…`. The release
workflow runs the same gate on the same commit, so one run is enough.

`make dist` packs the same file on your machine, under `dist/`, beside the
binary. Use it to hand a colleague a build before a tag, or when no pipeline
is available.

The file name carries no version: `qlearn-linux-x86_64.gz`, on your machine
and on the release page. The release page says which version it is, and
`qlearn --version` says it too. A name that never changes is what the link
to the newest release points at.
