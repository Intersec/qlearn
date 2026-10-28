//! The excerpt block: lines of the repository, read at the push.
//!
//! The agent names a path, lines and a rev. The server reads the lines in
//! the object database, submodules included, so a page never shows code
//! that the repository does not hold. See section 4.2 of the design.
//!
//! The keys are checked first, with no git call, so every error of the
//! fence comes at once. Only a fence whose keys pass is read.

use std::collections::{BTreeSet, HashMap};

use super::Fault;
use crate::git::exec::Git;
use crate::git::object::{self, ReadError};
use crate::highlight::{Highlighter, offsets};
use crate::model::{Excerpt, Line};

/// The most lines that an excerpt shows. A longer one is a file to read,
/// not a piece of code to teach.
pub const MAX_LINES: u32 = 60;

const KIND: &str = "excerpt";

/// What a fence asks for, with the line of each key in the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    path: String,
    from: u32,
    to: u32,
    rev: String,
    marked: Vec<u32>,
    hide: Option<u32>,
    /// The line of the fence, of `path`, of `lines` and of `rev`. A key
    /// left out takes the line of the fence.
    at: Lines,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Lines {
    fence: usize,
    path: usize,
    lines: usize,
    rev: usize,
}

/// The commits that the revs of one push resolve to. Two excerpts at
/// `HEAD` show the same commit, even when a commit lands during the push.
pub type Revs = HashMap<String, Result<String, String>>;

/// Read the keys of a fence. `fence` is the line of its opening, so the
/// first key is on the next line.
pub fn spec(body: &str, fence: usize) -> Result<Spec, Vec<Fault>> {
    let mut faults = Vec::new();
    let mut fault = |line: usize, message: String| {
        faults.push(Fault {
            line,
            kind: KIND,
            message,
        })
    };

    // Each key with its value and its line.
    let mut keys: HashMap<&str, (&str, usize)> = HashMap::new();
    for (index, raw) in body.lines().enumerate() {
        let line = fence + 1 + index;
        let text = raw.trim();
        if text.is_empty() {
            continue;
        }
        let Some((key, value)) = text.split_once(':') else {
            fault(line, format!("expected `key: value`, not {text:?}"));
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if !["path", "lines", "rev", "mark", "hide"].contains(&key) {
            fault(
                line,
                format!("unknown key {key:?}: an excerpt takes path, lines, rev, mark and hide"),
            );
            continue;
        }
        if keys.contains_key(key) {
            fault(line, format!("{key} is given twice"));
            continue;
        }
        if value.is_empty() {
            fault(line, format!("{key} is empty"));
        }
        keys.insert(key, (value, line));
    }
    let key = |name: &str| {
        keys.get(name)
            .copied()
            .filter(|(value, _)| !value.is_empty())
    };

    let path = match keys.get("path") {
        None => {
            fault(fence, "path is missing".to_owned());
            None
        }
        Some((path, line)) if !path.is_empty() && !is_tree_path(path) => {
            fault(
                *line,
                format!(
                    "path {path:?} must name a file from the top level directory of the \
                     worktree, with no leading `/`, no `.` and no `..`"
                ),
            );
            None
        }
        Some(_) => key("path"),
    };

    let shown_lines = match keys.get("lines") {
        None => {
            fault(fence, "lines is missing".to_owned());
            None
        }
        Some(("", _)) => None,
        Some((value, line)) => match range(value) {
            None => {
                fault(
                    *line,
                    format!("lines takes <from>-<to> or one line, from 1, not {value:?}"),
                );
                None
            }
            Some((from, to)) if to - from + 1 > MAX_LINES => {
                fault(
                    *line,
                    format!(
                        "lines {from}-{to} holds {} lines, and an excerpt shows {MAX_LINES} \
                         at most",
                        to - from + 1
                    ),
                );
                None
            }
            Some(range) => Some(range),
        },
    };

    // The marks and the hidden line are checked against a range only when
    // the range is good: a bad one already has its error.
    let mut marked = BTreeSet::new();
    if let Some((value, line)) = key("mark") {
        for item in value.split(',').map(str::trim) {
            match (range(item), shown_lines) {
                (None, _) => fault(
                    line,
                    format!("mark takes lines and ranges, such as 127, 130-131, not {item:?}"),
                ),
                (Some((first, last)), Some((from, to))) if first < from || last > to => fault(
                    line,
                    format!("mark {item} is outside lines {}", shown(from, to)),
                ),
                // Only a range inside good lines is counted: it holds 60 lines
                // at most, where an unchecked one can hold billions.
                (Some((first, last)), Some(_)) => marked.extend(first..=last),
                (Some(_), None) => {}
            }
        }
    }

    let mut hide = None;
    if let Some((value, line)) = key("hide") {
        match (range(value), shown_lines) {
            (Some((one, other)), _) if one != other => {
                fault(line, format!("hide takes one line, not {value:?}"))
            }
            (None, _) if value.contains([',', '-']) => {
                fault(line, format!("hide takes one line, not {value:?}"))
            }
            (None, _) => fault(line, format!("hide takes a line from 1, not {value:?}")),
            (Some((one, _)), Some((from, to))) if one < from || one > to => fault(
                line,
                format!("hide {one} is outside lines {}", shown(from, to)),
            ),
            (Some((one, _)), _) => hide = Some(one),
        }
    }

    let rev = key("rev");
    match (faults.is_empty(), path, shown_lines) {
        (true, Some((path, path_at)), Some((from, to))) => Ok(Spec {
            path: path.to_owned(),
            from,
            to,
            rev: rev.map_or("HEAD", |(rev, _)| rev).to_owned(),
            marked: marked.into_iter().collect(),
            hide,
            at: Lines {
                fence,
                path: path_at,
                lines: keys.get("lines").map_or(fence, |(_, line)| *line),
                rev: rev.map_or(fence, |(_, line)| line),
            },
        }),
        _ => Err(faults),
    }
}

/// Read the lines of a spec in the repository, with their colors.
///
/// Answers the excerpt with the hidden line as None, and the text of that
/// line apart: the page gets it only once the learner may see it.
pub async fn read(
    spec: &Spec,
    git: &Git,
    highlighter: &Highlighter,
    revs: &mut Revs,
) -> Result<(Excerpt, Option<Line>), Fault> {
    let fault = |line: usize, message: String| Fault {
        line,
        kind: KIND,
        message,
    };

    let commit = match revs.get(&spec.rev) {
        Some(known) => known.clone(),
        None => {
            let resolved = object::resolve(git, &spec.rev)
                .await
                .map_err(|e| e.to_string());
            revs.insert(spec.rev.clone(), resolved.clone());
            resolved
        }
    }
    .map_err(|message| fault(spec.at.rev, message))?;

    let file = object::read_file(git, &commit, &spec.path)
        .await
        .map_err(|e| {
            let line = match &e {
                ReadError::BadRev { .. } => spec.at.rev,
                ReadError::Git(_) => spec.at.fence,
                _ => spec.at.path,
            };
            fault(line, e.to_string())
        })?;
    let Ok(text) = String::from_utf8(file.bytes) else {
        return Err(fault(
            spec.at.path,
            format!("{} is not UTF-8 text at {}", spec.path, spec.rev),
        ));
    };

    let all: Vec<&str> = text.split_inclusive('\n').collect();
    if spec.to as usize > all.len() {
        return Err(fault(
            spec.at.lines,
            format!(
                "{} has {} at {}, the excerpt asks for {}",
                spec.path,
                match all.len() {
                    1 => "1 line".to_owned(),
                    count => format!("{count} lines"),
                },
                spec.rev,
                shown(spec.from, spec.to)
            ),
        ));
    }

    // The parse runs from the first line, because a comment or a string
    // may open above the excerpt. It stops at the last line shown.
    let lang = highlighter.language_of_path(&spec.path);
    let upto: usize = all[..spec.to as usize].iter().map(|line| line.len()).sum();
    let spans = highlighter.spans(&text[..upto], &lang);

    let mut hidden = None;
    let mut lines = Vec::new();
    for number in spec.from..=spec.to {
        let at = number as usize - 1;
        let raw = all[at];
        let raw = raw.strip_suffix('\n').unwrap_or(raw);
        let mut line = Line {
            text: raw.strip_suffix('\r').unwrap_or(raw).to_owned(),
            spans: spans.get(at).cloned().unwrap_or_default(),
        };
        // The page slices the line, so the offsets leave in its units.
        offsets::to_utf16(&mut line);
        match spec.hide == Some(number) {
            true => {
                hidden = Some(line);
                lines.push(None);
            }
            false => lines.push(Some(line)),
        }
    }

    let excerpt = Excerpt {
        path: spec.path.clone(),
        repo: file.repo,
        commit: file.commit,
        from: spec.from,
        to: spec.to,
        lang,
        lines,
        marked: spec.marked.clone(),
    };
    Ok((excerpt, hidden))
}

/// `<from>-<to>` or one line, each from 1, in order.
fn range(text: &str) -> Option<(u32, u32)> {
    let number = |text: &str| {
        let text = text.trim();
        // `parse` takes a `+`, which no line number has.
        match !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) {
            true => text.parse::<u32>().ok().filter(|n| *n >= 1),
            false => None,
        }
    };

    match text.split_once('-') {
        Some((from, to)) => {
            let (from, to) = (number(from)?, number(to)?);
            (from <= to).then_some((from, to))
        }
        None => number(text).map(|line| (line, line)),
    }
}

fn shown(from: u32, to: u32) -> String {
    match from == to {
        true => from.to_string(),
        false => format!("{from}-{to}"),
    }
}

/// Whether a path names an entry from the top level directory. git would
/// find no `.` or `..` in a tree, and the error would not say why.
fn is_tree_path(path: &str) -> bool {
    path.split('/')
        .all(|part| !part.is_empty() && part != "." && part != "..")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{build_repo, commit};

    fn messages(body: &str) -> Vec<String> {
        match spec(body, 10) {
            Ok(spec) => panic!("{spec:?}"),
            Err(faults) => faults
                .iter()
                .map(|fault| format!("{}: {}", fault.line, fault.message))
                .collect(),
        }
    }

    #[test]
    fn the_keys_give_the_spec_with_the_line_of_each() {
        let spec = spec(
            "path: src/raft.c\nlines: 120-135\nrev: main\nmark: 127, 130-131, 127\nhide: 128\n",
            4,
        )
        .unwrap();

        assert_eq!(
            spec,
            Spec {
                path: "src/raft.c".to_owned(),
                from: 120,
                to: 135,
                rev: "main".to_owned(),
                marked: vec![127, 130, 131],
                hide: Some(128),
                at: Lines {
                    fence: 4,
                    path: 5,
                    lines: 6,
                    rev: 7,
                },
            }
        );
    }

    #[test]
    fn the_rev_is_head_by_default_and_one_line_is_a_range() {
        let spec = spec("lines: 7\npath: a.c\n", 1).unwrap();

        assert_eq!((spec.from, spec.to), (7, 7));
        assert_eq!(spec.rev, "HEAD");
        assert_eq!(spec.at.rev, 1, "a key left out takes the line of the fence");
    }

    #[test]
    fn sixty_lines_pass_and_sixty_one_do_not() {
        assert!(spec("path: a.c\nlines: 1-60\n", 1).is_ok());
        assert_eq!(
            messages("path: a.c\nlines: 41-101\n"),
            ["12: lines 41-101 holds 61 lines, and an excerpt shows 60 at most"]
        );
    }

    #[test]
    fn a_bad_range_is_refused() {
        for value in ["0", "5-3", "a-b", "1-", "-4", "+3", "1-2-3", "1,2", "0-4"] {
            let body = format!("path: a.c\nlines: {value}\n");
            assert_eq!(
                messages(&body),
                [format!(
                    "12: lines takes <from>-<to> or one line, from 1, not {value:?}"
                )],
                "{value}"
            );
        }
    }

    #[test]
    fn every_error_of_a_fence_comes_at_once() {
        let body = "\
            path: /etc/passwd\n\
            colour: red\n\
            hide: 3, 4\n\
            mark: 1-2, 9\n\
            no colon here\n\
            path: again.c\n\
            rev:\n";

        assert_eq!(
            messages(body),
            [
                "12: unknown key \"colour\": an excerpt takes path, lines, rev, mark and hide",
                "15: expected `key: value`, not \"no colon here\"",
                "16: path is given twice",
                "17: rev is empty",
                "11: path \"/etc/passwd\" must name a file from the top level directory of the \
                 worktree, with no leading `/`, no `.` and no `..`",
                "10: lines is missing",
                "13: hide takes one line, not \"3, 4\"",
            ]
        );
    }

    #[test]
    fn mark_and_hide_stay_inside_lines() {
        assert_eq!(
            messages("path: a.c\nlines: 5-9\nmark: 4, 6-7, 8-10\nhide: 10\n"),
            [
                "13: mark 4 is outside lines 5-9",
                "13: mark 8-10 is outside lines 5-9",
                "14: hide 10 is outside lines 5-9",
            ]
        );
        assert_eq!(
            messages("path: a.c\nlines: 5-9\nhide: 6-7\n"),
            ["13: hide takes one line, not \"6-7\""]
        );
        assert_eq!(
            messages("path: a.c\nlines: 5-9\nhide: 6\nhide: 7\n"),
            ["14: hide is given twice"]
        );
    }

    /// One push could ask for billions of marked lines, and hold the server
    /// while it counts them.
    #[test]
    fn a_huge_mark_with_bad_lines_costs_nothing() {
        assert_eq!(
            messages("path: a.c\nlines: 1-100\nmark: 1-4000000000\n"),
            ["12: lines 1-100 holds 100 lines, and an excerpt shows 60 at most"]
        );
    }

    #[test]
    fn a_path_with_a_dot_or_an_empty_part_is_refused() {
        for path in ["./a.c", "src/../a.c", "src//a.c", "src/"] {
            let body = format!("path: {path}\nlines: 1\n");
            assert_eq!(messages(&body).len(), 1, "{path}");
        }
    }

    async fn read_one(
        repo: &crate::testutil::Repo,
        body: &str,
    ) -> Result<(Excerpt, Option<Line>), Fault> {
        let git = Git::discover(repo.path()).await.unwrap();
        let spec = spec(body, 1).unwrap();
        read(&spec, &git, Highlighter::shared(), &mut Revs::new()).await
    }

    /// The page cuts each line at the bounds of its spans, in UTF-16
    /// units, so the pieces between and inside the spans must give the
    /// text back, unit for unit.
    #[tokio::test]
    async fn the_spans_of_a_line_cover_its_text_exactly() {
        let text = "/* A comment that opens\n   above the excerpt, é */\n\
                    int x = 1;\r\n\tchar *s = \"héllo\"; // ünïcode\n}\n";
        let repo = build_repo(&[commit("code").file("a.c", text)]).await;

        let (excerpt, _) = read_one(&repo, "path: a.c\nlines: 2-5\n").await.unwrap();

        let first = excerpt.lines[0].as_ref().unwrap();
        assert!(
            first.spans.iter().all(|s| s.cls.starts_with("tok-comment")),
            "the comment opened above the excerpt: {first:?}"
        );
        for line in excerpt.lines.iter().flatten() {
            let units: Vec<u16> = line.text.encode_utf16().collect();
            let mut pieces = Vec::new();
            let mut last = 0;
            for span in &line.spans {
                assert!(last <= span.start && span.start < span.end, "{line:?}");
                assert!(span.end <= units.len(), "{line:?}");
                pieces.push(&units[last..span.start]);
                pieces.push(&units[span.start..span.end]);
                last = span.end;
            }
            pieces.push(&units[last..]);
            // A piece cut inside a character would not decode.
            let rebuilt: String = pieces
                .iter()
                .map(|piece| String::from_utf16(piece).unwrap())
                .collect();
            assert_eq!(rebuilt, line.text);
        }
        let accented = excerpt.lines[2].as_ref().unwrap();
        let comment = accented.spans.last().unwrap();
        assert!(comment.cls.starts_with("tok-comment"), "{accented:?}");
        assert_eq!(
            String::from_utf16(&accented.text.encode_utf16().collect::<Vec<_>>()[comment.start..])
                .unwrap(),
            "// ünïcode",
            "the offsets count UTF-16 units, as the page slices"
        );
        assert_eq!(excerpt.lines[1].as_ref().unwrap().text, "int x = 1;");
    }

    #[tokio::test]
    async fn the_hidden_line_is_none_and_its_text_comes_apart() {
        let repo = build_repo(&[commit("code").file("a.c", "one;\ntwo;\nthree;\n")]).await;

        let (excerpt, hidden) = read_one(&repo, "path: a.c\nlines: 1-3\nhide: 2\n")
            .await
            .unwrap();

        assert!(excerpt.lines[0].is_some() && excerpt.lines[2].is_some());
        assert_eq!(excerpt.lines[1], None);
        assert_eq!(hidden.unwrap().text, "two;");
    }

    #[tokio::test]
    async fn a_file_shorter_than_lines_is_refused_at_the_line_of_lines() {
        let repo = build_repo(&[commit("code").file("a.c", "one;\ntwo;\nthree")]).await;

        let fault = read_one(&repo, "path: a.c\nlines: 2-4\n")
            .await
            .unwrap_err();

        assert_eq!(fault.line, 3);
        assert_eq!(
            fault.message,
            "a.c has 3 lines at HEAD, the excerpt asks for 2-4"
        );
        assert!(read_one(&repo, "path: a.c\nlines: 3\n").await.is_ok());
    }

    #[tokio::test]
    async fn a_file_that_is_not_text_is_refused() {
        let repo = build_repo(&[commit("first")]).await;
        std::fs::write(repo.path().join("logo.png"), [0x89, b'P', 0xff, b'\n']).unwrap();
        repo.commit(&commit("logo")).await;

        let fault = read_one(&repo, "path: logo.png\nlines: 1\n")
            .await
            .unwrap_err();

        assert_eq!(fault.line, 2);
        assert_eq!(fault.message, "logo.png is not UTF-8 text at HEAD");
    }

    #[tokio::test]
    async fn an_unknown_extension_is_plain_text() {
        let repo = build_repo(&[commit("data").file("table.xyz", "a = 1;\n")]).await;

        let (excerpt, _) = read_one(&repo, "path: table.xyz\nlines: 1\n")
            .await
            .unwrap();

        assert_eq!(excerpt.lang, "");
        assert_eq!(
            excerpt.lines,
            [Some(Line {
                text: "a = 1;".to_owned(),
                spans: Vec::new(),
            })]
        );
    }

    /// The rev resolves once per push, so two excerpts at `HEAD` never show
    /// two commits.
    #[tokio::test]
    async fn a_rev_resolves_once_per_push() {
        let repo = build_repo(&[commit("one").file("a.c", "one;\n")]).await;
        let git = Git::discover(repo.path()).await.unwrap();
        let spec = spec("path: a.c\nlines: 1\n", 1).unwrap();
        let mut revs = Revs::new();

        let (before, _) = read(&spec, &git, Highlighter::shared(), &mut revs)
            .await
            .unwrap();
        repo.commit(&commit("two").file("a.c", "two;\n")).await;
        let (after, _) = read(&spec, &git, Highlighter::shared(), &mut revs)
            .await
            .unwrap();

        assert_eq!(after, before);
    }
}
