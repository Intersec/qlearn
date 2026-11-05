# Pending changelog entries

One file per entry. A change that a user can see drops a Markdown file in the
group it belongs to:

```
changelog/added/    -> ### Added     a new feature
changelog/changed/  -> ### Changed   behavior or wording that moved
changelog/fixed/    -> ### Fixed     a bug that is gone
changelog/removed/  -> ### Removed   something taken away
```

`cargo xtask release` collects them into the version that it cuts, writes
them under `## [x.y.z]` in [CHANGELOG.md](../CHANGELOG.md), and deletes the
files. So do not edit `CHANGELOG.md` by hand.

Why one file per entry: a shared `Unreleased` section conflicts on every
rebase. A fragment is a file that only its own branch has, so there is nothing
to merge.

## How to write one

The file name is a kebab-case slug of the change. It must be distinctive, so
that two branches do not pick the same one: `notes-images.md`,
`quiz-other-box.md`. Not `fix.md`.

The content is the bullet, exactly as it will read: one `- ` bullet,
continuation lines indented by two spaces, wrapped at 80 columns. Nothing
reflows it. An `Added` entry opens with the feature in bold.

```markdown
- **Images of the notes** — a page shows a figure that the agent drew in the
  notes of the course.
```

One file is one entry. Two unrelated changes are two files, even in the same
group and the same branch.

Skip the file when no user can see the change.
