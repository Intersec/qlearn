//! Offsets the browser can slice with.
//!
//! Rust counts bytes, JavaScript counts UTF-16 code units. A line with one
//! accent in it makes every span after that point land in the wrong place, so
//! the offsets are converted once, where a line leaves the server.

use crate::model::Line;

/// Convert every span of a line from byte offsets to UTF-16 offsets.
pub fn to_utf16(line: &mut Line) {
    // An ASCII line is the common case and needs no work at all.
    if line.text.is_ascii() {
        return;
    }

    let map = Map::of(&line.text);
    for span in &mut line.spans {
        span.start = map.at(span.start);
        span.end = map.at(span.end);
    }
}

/// Byte offset to UTF-16 offset, for one line.
struct Map {
    /// One entry per character boundary: the byte index and the UTF-16 index.
    points: Vec<(usize, usize)>,
    end: usize,
}

impl Map {
    fn of(text: &str) -> Self {
        let mut points = Vec::with_capacity(text.len() + 1);
        let mut utf16 = 0;

        for (byte, ch) in text.char_indices() {
            points.push((byte, utf16));
            utf16 += ch.len_utf16();
        }
        points.push((text.len(), utf16));

        Self { points, end: utf16 }
    }

    fn at(&self, byte: usize) -> usize {
        match self.points.binary_search_by_key(&byte, |(b, _)| *b) {
            Ok(index) => self.points[index].1,
            // An offset inside a character can only come from a bug. Round to
            // the boundary before it rather than move the whole line.
            Err(0) => 0,
            Err(index) => self
                .points
                .get(index - 1)
                .map(|(_, u)| *u)
                .unwrap_or(self.end),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Span;

    fn line(text: &str, spans: &[(usize, usize)]) -> Line {
        Line {
            text: text.to_owned(),
            spans: spans
                .iter()
                .map(|(start, end)| Span {
                    start: *start,
                    end: *end,
                    cls: "tok-string".to_owned(),
                })
                .collect(),
        }
    }

    #[test]
    fn an_ascii_line_is_unchanged() {
        let mut l = line("let a = 1;", &[(4, 5)]);
        to_utf16(&mut l);

        assert_eq!((l.spans[0].start, l.spans[0].end), (4, 5));
    }

    #[test]
    fn an_accent_moves_the_offsets_after_it() {
        // "é" is two bytes and one UTF-16 unit.
        let text = "héllo world";
        let byte_start = text.find("world").unwrap();
        let mut l = line(text, &[(byte_start, text.len())]);
        to_utf16(&mut l);

        assert_eq!(byte_start, 7, "the byte offset counts é as two");
        assert_eq!(l.spans[0].start, 6, "the browser counts it as one");
        assert_eq!(l.spans[0].end, 11);
        assert_eq!(l.spans[0].cls, "tok-string");
    }

    #[test]
    fn an_emoji_counts_as_two_in_the_browser() {
        // A character outside the basic plane is one char, four bytes, and
        // two UTF-16 units.
        let text = "x 🚀 y";
        let byte_start = text.find(" y").unwrap();
        let mut l = line(text, &[(byte_start, text.len())]);
        to_utf16(&mut l);

        assert_eq!(byte_start, 6);
        assert_eq!(l.spans[0].start, 4, "one x, one space, two for the rocket");
    }

    #[test]
    fn an_offset_inside_a_character_rounds_down() {
        let mut l = line("héllo", &[(2, 6)]);
        to_utf16(&mut l);

        // Byte 2 is the second byte of é. It becomes the boundary before it.
        assert_eq!(l.spans[0].start, 1);
    }
}
