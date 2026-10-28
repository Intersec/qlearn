//! Syntax colors, computed in the server.
//!
//! The page receives code that already carries its classes, so it never
//! downloads a grammar. The output is a CSS class, never a color: a theme is
//! a stylesheet, and the light and dark pair costs one pass, not two.

pub mod lang;

use std::sync::LazyLock;

use syntect::parsing::{
    ParseState, ScopeStack, SyntaxDefinition, SyntaxReference, SyntaxSet, SyntaxSetBuilder,
};
use syntect::util::LinesWithEndings;

use crate::model::Span;

/// Above this size, a text is shown without colors.
///
/// Highlighting is linear, but a very large text still costs seconds and
/// megabytes of spans that nobody reads.
pub const MAX_BYTES: usize = 512 * 1024;

/// The scopes that say nothing about a token. `source.c` is on every line.
const GENERIC: [&str; 3] = ["source", "text", "meta"];

/// The grammars that qlearn ships. `two-face` has no IOP grammar.
///
/// They stay out of the `two-face` set, because adding one grammar to it
/// links all 199 again: 750 ms at every start, against 9 ms for this set.
static OURS: LazyLock<SyntaxSet> = LazyLock::new(|| {
    let text = include_str!("../../grammars/iop.sublime-syntax");
    // A test loads this set, so a broken grammar fails the gate first.
    let iop = SyntaxDefinition::load_from_str(text, true, None).expect("the IOP grammar loads");

    let mut builder = SyntaxSetBuilder::new();
    builder.add(iop);
    builder.build()
});

pub struct Highlighter {
    syntaxes: SyntaxSet,
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::new()
    }
}

impl Highlighter {
    pub fn new() -> Self {
        Self {
            syntaxes: two_face::syntax::extra_newlines(),
        }
    }

    /// The highlighter of the process. The grammars take a moment to load,
    /// so they load once, at the first fence.
    pub fn shared() -> &'static Self {
        static SHARED: LazyLock<Highlighter> = LazyLock::new(Highlighter::new);
        &SHARED
    }

    /// The spans of every line of a text, in bytes of the line.
    ///
    /// `language` is the info string of a fence: a name or an extension.
    /// A language that no grammar knows gives lines with no spans.
    pub fn spans(&self, text: &str, language: &str) -> Vec<Vec<Span>> {
        if text.len() > MAX_BYTES {
            return Vec::new();
        }
        let (ours, syntax) = self.syntax_of(language);
        let set = match ours {
            true => &*OURS,
            false => &self.syntaxes,
        };

        // One parse from the first line, because a block comment or a
        // multi-line string needs the lines before it.
        let mut state = ParseState::new(syntax);
        let mut stack = ScopeStack::new();
        let mut lines = Vec::new();
        for line in LinesWithEndings::from(text) {
            match state.parse_line(line, set) {
                Ok(ops) => lines.push(spans_of(line, &ops, &mut stack)),
                // A grammar that fails on one line still colors the rest.
                Err(_) => lines.push(Vec::new()),
            }
        }
        lines
    }

    /// The grammar of a language, and whether it comes from `OURS`.
    fn syntax_of(&self, language: &str) -> (bool, &SyntaxReference) {
        // A house extension, such as `blk`, names its language in the map.
        let language = lang::of(language).unwrap_or(language);
        if language.is_empty() {
            return (false, self.syntaxes.find_syntax_plain_text());
        }

        self.find(|set| set.find_syntax_by_token(language))
            .unwrap_or_else(|| (false, self.syntaxes.find_syntax_plain_text()))
    }

    /// Ask the `two-face` set first, then the grammars that qlearn ships.
    fn find<'a>(
        &'a self,
        lookup: impl Fn(&'a SyntaxSet) -> Option<&'a SyntaxReference>,
    ) -> Option<(bool, &'a SyntaxReference)> {
        lookup(&self.syntaxes)
            .map(|syntax| (false, syntax))
            .or_else(|| lookup(&OURS).map(|syntax| (true, syntax)))
    }
}

fn spans_of(
    line: &str,
    ops: &[(usize, syntect::parsing::ScopeStackOp)],
    stack: &mut ScopeStack,
) -> Vec<Span> {
    // The newline is not part of the text that the page shows.
    let end_of_text = line.trim_end_matches(['\n', '\r']).len();
    let mut spans: Vec<Span> = Vec::new();
    let mut last = 0usize;

    for (pos, op) in ops {
        let pos = (*pos).min(end_of_text);
        if pos > last
            && let Some(cls) = class_of(stack)
        {
            push(&mut spans, last, pos, cls);
        }
        if stack.apply(op).is_err() {
            break;
        }
        last = last.max(pos);
    }

    if last < end_of_text
        && let Some(cls) = class_of(stack)
    {
        push(&mut spans, last, end_of_text, cls);
    }
    spans
}

/// Add a span, merging it into the one before when the class is the same.
fn push(spans: &mut Vec<Span>, start: usize, end: usize, cls: String) {
    match spans.last_mut() {
        Some(last) if last.end == start && last.cls == cls => last.end = end,
        _ => spans.push(Span { start, end, cls }),
    }
}

/// The class of the most specific scope that says something.
///
/// Every class is prefixed. A scope atom is an ordinary English word, and
/// the interface is built on a framework whose utilities are ordinary
/// English words too: `comment.block.documentation` became `comment block`,
/// and `block` sets `display: block`, so every doc comment broke its own
/// line in two.
fn class_of(stack: &ScopeStack) -> Option<String> {
    for scope in stack.as_slice().iter().rev() {
        let name = scope.build_string();
        let mut atoms = name.split('.');
        let Some(first) = atoms.next() else {
            continue;
        };
        if GENERIC.contains(&first) {
            continue;
        }
        return match atoms.next() {
            Some(second) => Some(format!("tok-{first} tok-{second}")),
            None => Some(format!("tok-{first}")),
        };
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(text: &str, language: &str) -> Vec<Vec<Span>> {
        Highlighter::new().spans(text, language)
    }

    fn classes(line: &[Span], text: &str) -> Vec<(String, String)> {
        line.iter()
            .map(|s| (s.cls.clone(), text[s.start..s.end].to_owned()))
            .collect()
    }

    #[test]
    fn a_keyword_and_a_string_get_their_classes() {
        let text = "int main(void) {\n    return \"hi\";\n}\n";
        let out = spans(text, "c");

        let second: Vec<_> = out[1].iter().map(|s| s.cls.clone()).collect();
        assert!(
            second.iter().any(|c| c.starts_with("tok-keyword")),
            "no keyword class: {second:?}"
        );
        assert!(
            second.iter().any(|c| c.starts_with("tok-string")),
            "no string class: {second:?}"
        );
    }

    /// The page cuts each line at the bounds of its spans. A span that
    /// overlaps the one before, or leaves the line, loses or repeats text.
    #[test]
    fn the_spans_of_a_line_stay_in_order_inside_its_text() {
        let text = "int x = 1;\nchar *s = \"héllo\";\n// é\n";
        let out = spans(text, "c");

        for (index, line) in text.lines().enumerate() {
            let mut last = 0;
            for span in &out[index] {
                assert!(span.start >= last, "line {index}: {:?}", out[index]);
                assert!(span.start < span.end, "line {index}: {span:?}");
                assert!(
                    line.get(span.start..span.end).is_some(),
                    "line {index} span {span:?} does not slice {line:?}"
                );
                last = span.end;
            }
        }
    }

    #[test]
    fn the_newline_is_never_inside_a_span() {
        let out = spans("// a comment\n", "c");
        let longest = out[0].iter().map(|s| s.end).max().unwrap_or(0);

        assert_eq!(longest, "// a comment".len());
    }

    #[test]
    fn a_block_comment_stays_a_comment_on_the_second_line() {
        let out = spans("/* one\n   two */\nint x;\n", "c");
        let second = classes(&out[1], "   two */");

        assert!(
            second.iter().any(|(cls, _)| cls.starts_with("tok-comment")),
            "the parser must carry the state across lines: {second:?}"
        );
    }

    #[test]
    fn a_language_is_found_by_its_name_or_its_extension() {
        let h = Highlighter::new();

        assert_eq!(h.syntax_of("rust").1.name, "Rust");
        assert_eq!(h.syntax_of("rs").1.name, "Rust");
        assert_eq!(h.syntax_of("Python").1.name, "Python");
    }

    #[test]
    fn a_house_extension_takes_the_language_of_the_map() {
        // No bundled grammar claims .blk. The map says it is C.
        let out = spans("int x = 1;\n", "blk");
        let first: Vec<_> = out[0].iter().map(|s| s.cls.clone()).collect();

        assert!(
            first
                .iter()
                .any(|c| c.starts_with("tok-storage") || c.starts_with("tok-keyword")),
            "{first:?}"
        );
    }

    #[test]
    fn an_iop_fence_gets_the_grammar_we_ship() {
        let h = Highlighter::new();
        let (ours, syntax) = h.syntax_of("iop");

        assert!(ours, "no grammar of the two-face set claims .iop");
        assert_eq!(syntax.name, "IOP");
    }

    /// The parse must run against the set that holds the grammar.
    #[test]
    fn an_iop_fence_is_painted_with_our_set() {
        let out = spans("struct User {\n    1: int id;\n};\n", "iop");
        let field = classes(&out[1], "    1: int id;");

        for (cls, token) in [
            ("tok-constant tok-numeric", "1"),
            ("tok-storage tok-type", "int"),
            ("tok-variable tok-other", "id"),
        ] {
            assert!(
                field.contains(&(cls.to_owned(), token.to_owned())),
                "{token} is not {cls}: {field:?}"
            );
        }
    }

    /// One line drove one regex of the IOP grammar to its backtrack limit:
    /// 70 ms a line, minutes for a text of them.
    #[test]
    fn a_crafted_iop_line_does_not_backtrack_for_long() {
        let line = format!("{}{}b\n", "a.".repeat(30), " ".repeat(180));
        let started = std::time::Instant::now();
        spans(&line.repeat(20), "iop");

        let took = started.elapsed();
        assert!(took < std::time::Duration::from_secs(3), "{took:?}");
    }

    /// Once 100 contexts are open, syntect searches every push regex again
    /// at each token. `c"` was one, and in the slow engine: a crafted line
    /// of 16 KiB took 16 s.
    #[test]
    fn a_string_at_the_depth_cap_stays_cheap() {
        let text = format!("@x{}\n{}c\"\n", "(".repeat(120), "a,".repeat(2048));
        let started = std::time::Instant::now();
        spans(&text, "iop");

        let took = started.elapsed();
        assert!(took < std::time::Duration::from_secs(3), "{took:?}");
    }

    #[test]
    fn an_unknown_language_is_plain_and_does_not_fail() {
        for language in ["no-such-language", ""] {
            let out = spans("some text\n", language);

            assert_eq!(out.len(), 1, "{language:?}");
            assert!(out[0].is_empty(), "{language:?}: {:?}", out[0]);
        }
    }

    #[test]
    fn a_text_above_the_cap_is_not_highlighted() {
        let text = "int x = 1;\n".repeat(MAX_BYTES / 5);

        assert!(spans(&text, "c").is_empty(), "a huge text has no colors");
    }

    #[test]
    fn an_empty_text_has_no_lines() {
        assert!(spans("", "c").is_empty());
    }

    /// Every class the highlighter emits is prefixed.
    ///
    /// A scope atom is an English word, and so is a utility class. One that
    /// slips through unprefixed can set `display`, `float` or `position` on
    /// a piece of code and break the line it sits on.
    #[test]
    fn every_class_carries_the_prefix() {
        let text = "/** A doc comment.\n *\n * More words.\n */\nint main(void) { return 0; }\n";

        let mut seen = 0;
        for line in spans(text, "c") {
            for span in line {
                for class in span.cls.split_whitespace() {
                    assert!(class.starts_with("tok-"), "{class} is not prefixed");
                    seen += 1;
                }
            }
        }
        assert!(seen > 0, "the fixture must produce some classes");
    }
}
