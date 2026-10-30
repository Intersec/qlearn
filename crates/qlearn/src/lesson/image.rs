//! The image block: a file of the repository, read at the push.
//!
//! `![alt](path)` alone in a paragraph names an image of the repository at
//! `HEAD`. The server reads it in the object database, submodules included,
//! never in the worktree, so a page shows only what the repository holds.
//! See section 4.6 of the design.
//!
//! The type comes from the bytes. The media route sends that type, so a
//! file whose name lies is refused at the push rather than served as
//! something else.

use sha2::{Digest, Sha256};

use super::Fault;
use super::excerpt::{self, Revs};
use crate::git::exec::Git;
use crate::git::object::{self, ReadError};
use crate::model::{Block, ImageType};

/// The largest image, in bytes. A screenshot or a diagram fits well under
/// it, and the store keeps a copy of each image that a page shows.
pub const MAX_BYTES: u64 = 5 * 1024 * 1024;

const KIND: &str = "image";

/// What `![alt](path)` asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    alt: String,
    path: String,
    /// The type that the extension of the path names.
    named: ImageType,
    line: usize,
}

/// The error of an image outside a repository, at its line: there is no
/// object database to read it in.
pub fn outside(line: usize) -> Fault {
    Fault {
        line,
        kind: KIND,
        message: "take this image out, or draw it in a mermaid fence or a fence of code: a \
                  course outside a git repository shows no file of a repository"
            .to_owned(),
    }
}

/// Read the alt text and the path of an image at `line`, with every fault
/// at once.
pub fn spec(alt: &str, path: &str, line: usize) -> Result<Spec, Vec<Fault>> {
    let mut faults = Vec::new();
    let mut fault = |message: String| {
        faults.push(Fault {
            line,
            kind: KIND,
            message,
        })
    };

    let alt = alt.trim();
    if alt.is_empty() {
        fault(
            "the alt text is empty: say what the image shows, such as ![The states of a \
             server](docs/states.svg). A screen reader reads it in place of the image"
                .to_owned(),
        );
    }

    let found = if has_scheme(path) {
        fault(format!(
            "{path:?} is a URL, and an image comes from the repository: give the path of a \
             file from the top level directory of the worktree"
        ));
        None
    } else {
        match percent_decode(path) {
            None => {
                fault(format!(
                    "path {path:?} is not UTF-8 once its escapes are decoded: write the name \
                     of the file between `<` and `>`, such as ![A figure](<docs/my figure.png>)"
                ));
                None
            }
            Some(path) if !excerpt::is_tree_path(&path) => {
                fault(format!(
                    "path {path:?} must name a file from the top level directory of the \
                     worktree, with no leading `/`, no `.` and no `..`"
                ));
                None
            }
            Some(path) => {
                let named = ImageType::of_path(&path);
                if named.is_none() {
                    fault(format!(
                        "{path} is not a PNG, JPEG, GIF, WebP or SVG file: the page shows only \
                         those"
                    ));
                }
                named.map(|named| (path, named))
            }
        }
    };

    match (faults.is_empty(), found) {
        (true, Some((path, named))) => Ok(Spec {
            alt: alt.to_owned(),
            path,
            named,
            line,
        }),
        _ => Err(faults),
    }
}

/// A link destination with its `%XX` escapes decoded. A link is a URL, so
/// an agent writes a space in a name as `%20`, as GitHub does. A `%` that
/// starts no escape stays. None when the bytes are not UTF-8.
fn percent_decode(path: &str) -> Option<String> {
    let bytes = path.as_bytes();
    let hex = |at: usize| bytes.get(at).and_then(|b| char::from(*b).to_digit(16));
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%'
            && let (Some(high), Some(low)) = (hex(at + 1), hex(at + 2))
        {
            // Two hex digits fit a byte.
            out.push((high * 16 + low) as u8);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Read the image of a spec at `HEAD`. Answers its block, and the bytes to
/// store under the name that the block gives in `media`.
pub async fn read(
    id: String,
    spec: &Spec,
    git: &Git,
    revs: &mut Revs,
) -> Result<(Block, Vec<u8>), Fault> {
    let fault = |message: String| Fault {
        line: spec.line,
        kind: KIND,
        message,
    };

    let commit = excerpt::resolve("HEAD", git, revs).await.map_err(fault)?;
    let file = object::read_file_within(git, &commit, &spec.path, Some(MAX_BYTES))
        .await
        .map_err(|e| match e {
            ReadError::TooLarge { path, size, .. } => fault(format!(
                "{path} holds {} at HEAD, and an image holds 5 MB at most: show a smaller file",
                megabytes(size)
            )),
            e => fault(e.to_string()),
        })?;

    let Some(found) = sniff(&file.bytes) else {
        // A file named as an SVG gets the reason of its refusal, so the
        // agent knows what to change.
        let why = match (spec.named, svg_root(&file.bytes)) {
            (ImageType::Svg, Err(NotSvg::Entities)) => {
                "declares entities in its doctype at HEAD, and an entity can make a small SVG \
                 a large one to draw: show an SVG whose doctype declares none"
            }
            (ImageType::Svg, Err(NotSvg::NotUtf8)) => {
                "is not UTF-8 text at HEAD: show an SVG saved in UTF-8"
            }
            _ => "is not a PNG, JPEG, GIF, WebP or SVG image at HEAD",
        };
        return Err(fault(format!("{} {why}", spec.path)));
    };
    if found != spec.named {
        return Err(fault(format!(
            "{} holds {} image, and its name says {}: show a file whose name says what it \
             holds",
            spec.path,
            found.with_article(),
            spec.named.name()
        )));
    }

    let (width, height) = size(found, &file.bytes).unzip();
    let media = format!(
        "{}.{}",
        hex::encode(Sha256::digest(&file.bytes)),
        found.ext()
    );
    let block = Block::Image {
        id,
        alt: spec.alt.clone(),
        media,
        path: spec.path.clone(),
        repo: file.repo,
        commit: file.commit,
        format: found,
        width,
        height,
    };
    Ok((block, file.bytes))
}

/// Whether a link names a scheme, such as `https:` or `data:`, or a host,
/// as `//example.com/a.png` does. Neither is a file of the repository.
fn has_scheme(url: &str) -> bool {
    if url.starts_with("//") {
        return true;
    }
    let Some((scheme, _)) = url.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

fn megabytes(bytes: u64) -> String {
    // Rounded up, so a file just over the limit never reads "5.0 MB".
    let tenths = (bytes * 10).div_ceil(1024 * 1024);
    format!("{}.{} MB", tenths / 10, tenths % 10)
}

/// The type of an image, from its first bytes.
fn sniff(bytes: &[u8]) -> Option<ImageType> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageType::Png)
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(ImageType::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(ImageType::Gif)
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some(ImageType::Webp)
    } else if svg_root(bytes).is_ok() {
        Some(ImageType::Svg)
    } else {
        None
    }
}

/// Why bytes are not an SVG.
#[derive(Debug, PartialEq, Eq)]
enum NotSvg {
    NotUtf8,
    Entities,
    NoRoot,
}

/// The start tag of the root of an SVG, up to its `>`: UTF-8 text whose
/// first element is `<svg`, after an XML declaration, comments and a
/// doctype.
///
/// A doctype that declares entities is refused: an entity that expands to
/// itself many times over makes a small file a large one to draw.
fn svg_root(bytes: &[u8]) -> Result<&str, NotSvg> {
    let text = std::str::from_utf8(bytes).map_err(|_| NotSvg::NotUtf8)?;
    let mut rest = text.strip_prefix('\u{feff}').unwrap_or(text);
    let end_of = |text: &str, pattern: &str| text.find(pattern).ok_or(NotSvg::NoRoot);
    loop {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix("<?") {
            rest = &after[end_of(after, "?>")? + 2..];
        } else if let Some(after) = rest.strip_prefix("<!--") {
            rest = &after[end_of(after, "-->")? + 3..];
        } else if rest.starts_with("<!DOCTYPE") || rest.starts_with("<!doctype") {
            let end = end_of(rest, ">")?;
            if rest[..end].contains('[') {
                return Err(NotSvg::Entities);
            }
            rest = &rest[end + 1..];
        } else {
            break;
        }
    }

    let after = rest.strip_prefix("<svg").ok_or(NotSvg::NoRoot)?;
    let next = after.chars().next().ok_or(NotSvg::NoRoot)?;
    if !(next.is_ascii_whitespace() || next == '>' || next == '/') {
        return Err(NotSvg::NoRoot);
    }
    Ok(&after[..end_of(after, ">")?])
}

/// The width and the height of an image in pixels, when its header says
/// them. Only the header is read: the page draws the image.
fn size(kind: ImageType, bytes: &[u8]) -> Option<(u32, u32)> {
    let found = match kind {
        ImageType::Png => png_size(bytes),
        ImageType::Gif => {
            let width = u16::from_le_bytes(bytes.get(6..8)?.try_into().ok()?);
            let height = u16::from_le_bytes(bytes.get(8..10)?.try_into().ok()?);
            Some((u32::from(width), u32::from(height)))
        }
        ImageType::Jpeg => jpeg_size(bytes),
        ImageType::Webp => webp_size(bytes),
        ImageType::Svg => svg_size(svg_root(bytes).ok()?),
    };
    found.filter(|(width, height)| *width > 0 && *height > 0)
}

fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    Some((be32(bytes, 16)?, be32(bytes, 20)?))
}

/// The size in the first frame header of a JPEG, which comes before the
/// scan.
fn jpeg_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut at = 2;
    loop {
        if *bytes.get(at)? != 0xff {
            return None;
        }
        let marker = *bytes.get(at + 1)?;
        match marker {
            // A fill byte before a marker.
            0xff => at += 1,
            // The markers that carry no length.
            0x01 | 0xd0..=0xd8 => at += 2,
            // The scan or the end of the image: no frame header came.
            0xd9 | 0xda => return None,
            // A frame header, but for the tables of Huffman and of
            // arithmetic coding, which share the range.
            0xc0..=0xcf if !matches!(marker, 0xc4 | 0xc8 | 0xcc) => {
                let height = be16(bytes, at + 5)?;
                let width = be16(bytes, at + 7)?;
                return Some((u32::from(width), u32::from(height)));
            }
            _ => {
                let length = usize::from(be16(bytes, at + 2)?);
                if length < 2 {
                    return None;
                }
                at += 2 + length;
            }
        }
    }
}

/// The size in the first chunk of a WebP, in each of its three forms.
fn webp_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let le24 = |at: usize| -> Option<u32> {
        let b = bytes.get(at..at + 3)?;
        Some(u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16)
    };
    match bytes.get(12..16)? {
        // Lossy: a key frame starts with its tag, a start code, then the
        // sizes on 14 bits.
        b"VP8 " => {
            if bytes.get(23..26)? != [0x9d, 0x01, 0x2a] {
                return None;
            }
            let width = u16::from_le_bytes(bytes.get(26..28)?.try_into().ok()?) & 0x3fff;
            let height = u16::from_le_bytes(bytes.get(28..30)?.try_into().ok()?) & 0x3fff;
            Some((u32::from(width), u32::from(height)))
        }
        // Lossless: a signature, then the sizes minus one on 14 bits each.
        b"VP8L" => {
            if *bytes.get(20)? != 0x2f {
                return None;
            }
            let bits = u32::from_le_bytes(bytes.get(21..25)?.try_into().ok()?);
            Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        // Extended: the size of the canvas minus one, on 24 bits each.
        b"VP8X" => Some((le24(24)? + 1, le24(27)? + 1)),
        _ => None,
    }
}

/// The size of an SVG in pixels: its `width` and `height` in pixels, or
/// the size of its `viewBox`, scaled to the one of the two that it gives.
/// None for a size in another unit, such as `100%`.
fn svg_size(root: &str) -> Option<(u32, u32)> {
    let attributes = attributes(root);
    let get = |name: &str| {
        attributes
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| *value)
    };
    let pixels = |value: &str| {
        let value = value.trim();
        let number = value.strip_suffix("px").unwrap_or(value);
        number
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite() && *n > 0.0)
    };
    let width = get("width").and_then(pixels);
    let height = get("height").and_then(pixels);
    let view = get("viewBox").and_then(|value| {
        let numbers: Vec<f64> = value
            .split([' ', ',', '\t', '\n', '\r'])
            .filter(|part| !part.is_empty())
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        match numbers[..] {
            [_, _, w, h] if w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 => Some((w, h)),
            _ => None,
        }
    });

    let (width, height) = match (width, height, view) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, Some((vw, vh))) => (w, w * vh / vw),
        (None, Some(h), Some((vw, vh))) => (h * vw / vh, h),
        (None, None, Some(view)) => view,
        _ => return None,
    };
    // `as` saturates, and a size under one pixel still takes one.
    let round = |n: f64| (n.round() as u32).max(1);
    Some((round(width), round(height)))
}

/// The attributes of a start tag, as `name="value"` or `name='value'`. It
/// stops at the first one that it cannot read: the size is a hint.
fn attributes(tag: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut rest = tag;
    loop {
        rest = rest.trim_start();
        let Some(eq) = rest.find('=') else {
            return out;
        };
        let name = rest[..eq].trim();
        if name.is_empty() || name.contains(char::is_whitespace) {
            return out;
        }
        let value = rest[eq + 1..].trim_start();
        let Some(quote) = value.chars().next().filter(|c| matches!(c, '"' | '\'')) else {
            return out;
        };
        let Some(end) = value[1..].find(quote) else {
            return out;
        };
        out.push((name, &value[1..1 + end]));
        rest = &value[end + 2..];
    }
}

fn be16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{PNG, SVG};

    fn messages(alt: &str, path: &str) -> Vec<String> {
        match spec(alt, path, 3) {
            Ok(spec) => panic!("{spec:?}"),
            Err(faults) => faults.into_iter().map(|fault| fault.message).collect(),
        }
    }

    #[test]
    fn a_spec_takes_a_path_of_the_tree_with_an_image_extension() {
        let spec = spec("  The states ", "docs/States.SVG", 3).unwrap();

        assert_eq!(spec.alt, "The states");
        assert_eq!(spec.named, ImageType::Svg);
    }

    #[test]
    fn a_url_with_a_scheme_or_a_host_is_refused() {
        for url in [
            "https://example.com/a.png",
            "HTTP://example.com/a.png",
            "file:///etc/a.png",
            "data:image/png;base64,iVBORw0KGgo=",
            "javascript:alert(1)",
            "//example.com/a.png",
        ] {
            let found = messages("A figure", url);

            assert_eq!(found.len(), 1, "{url}");
            assert!(found[0].contains("is a URL"), "{url}: {found:?}");
        }
    }

    #[test]
    fn a_path_that_leaves_the_tree_is_refused() {
        for path in [
            "/etc/a.png",
            "../a.png",
            "docs/../../a.png",
            "./a.png",
            "docs//a.png",
            "",
        ] {
            let found = messages("A figure", path);

            assert_eq!(
                found,
                [format!(
                    "path {path:?} must name a file from the top level directory of the \
                     worktree, with no leading `/`, no `.` and no `..`"
                )],
            );
        }
    }

    #[test]
    fn a_path_is_percent_decoded_before_its_checks() {
        assert_eq!(
            spec("A figure", "docs/my%20figure%2epng", 3).unwrap().path,
            "docs/my figure.png"
        );
        assert_eq!(
            spec("A figure", "docs/100%.png", 3).unwrap().path,
            "docs/100%.png",
            "a % that starts no escape stays"
        );
        assert_eq!(
            messages("A figure", "docs/%2e%2e/%2E%2E/a.png"),
            [
                "path \"docs/../../a.png\" must name a file from the top level directory of the \
              worktree, with no leading `/`, no `.` and no `..`"
            ]
        );
        assert_eq!(
            messages("A figure", "docs/%ff.png"),
            [
                "path \"docs/%ff.png\" is not UTF-8 once its escapes are decoded: write the name \
              of the file between `<` and `>`, such as ![A figure](<docs/my figure.png>)"
            ]
        );
    }

    #[test]
    fn every_fault_of_a_spec_comes_at_once() {
        assert_eq!(
            messages(" ", "docs/flow.bmp"),
            [
                "the alt text is empty: say what the image shows, such as ![The states of a \
                 server](docs/states.svg). A screen reader reads it in place of the image",
                "docs/flow.bmp is not a PNG, JPEG, GIF, WebP or SVG file: the page shows only \
                 those",
            ]
        );
    }

    #[test]
    fn the_type_comes_from_the_bytes() {
        assert_eq!(sniff(PNG), Some(ImageType::Png));
        assert_eq!(sniff(SVG.as_bytes()), Some(ImageType::Svg));
        assert_eq!(sniff(&[0xff, 0xd8, 0xff, 0xe0]), Some(ImageType::Jpeg));
        assert_eq!(sniff(b"GIF89a\x01\x00\x01\x00"), Some(ImageType::Gif));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some(ImageType::Webp));
        for bytes in [
            &b""[..],
            b"int a;\n",
            b"\x89PNG",
            b"RIFF\0\0\0\0WAVE",
            b"<html><svg></svg></html>",
            b"<svgx/>",
            b"\xff\xfe<\0s\0v\0g\0",
        ] {
            assert_eq!(sniff(bytes), None, "{bytes:?}");
        }
    }

    #[test]
    fn an_svg_root_comes_after_a_declaration_comments_and_a_doctype() {
        let svg = "\u{feff}<?xml version=\"1.0\"?>\n<!-- a -->\n<!DOCTYPE svg PUBLIC \
                   \"-//W3C//DTD SVG 1.1//EN\" \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">\n\
                   <svg width=\"10\" height=\"20\"/>";

        assert_eq!(svg_root(svg.as_bytes()), Ok(" width=\"10\" height=\"20\"/"));
        assert_eq!(size(ImageType::Svg, svg.as_bytes()), Some((10, 20)));
    }

    #[test]
    fn an_svg_that_declares_entities_is_refused() {
        let svg = "<!DOCTYPE svg [<!ENTITY a \"aaaa\">]><svg>&a;</svg>";

        assert_eq!(sniff(svg.as_bytes()), None);
        assert_eq!(svg_root(svg.as_bytes()), Err(NotSvg::Entities));
        assert_eq!(svg_root(b"\xff\xfe<\0s\0v\0g\0"), Err(NotSvg::NotUtf8));
        assert_eq!(svg_root(b"<html><svg></svg></html>"), Err(NotSvg::NoRoot));
    }

    #[test]
    fn the_size_of_a_png_and_a_gif_is_in_their_header() {
        assert_eq!(size(ImageType::Png, PNG), Some((3, 2)));
        assert_eq!(
            size(ImageType::Gif, b"GIF89a\x40\x01\xc8\x00"),
            Some((320, 200))
        );
        assert_eq!(size(ImageType::Png, &PNG[..20]), None, "a cut header");
    }

    #[test]
    fn the_size_of_a_jpeg_is_in_its_frame_header() {
        let mut jpeg = vec![0xff, 0xd8];
        // An APP0 segment of 16 bytes, a fill byte, then the frame.
        jpeg.extend([0xff, 0xe0, 0x00, 0x10]);
        jpeg.extend([0u8; 14]);
        jpeg.extend([0xff, 0xff, 0xc2, 0x00, 0x11, 0x08, 0x01, 0xe0, 0x02, 0x80]);

        assert_eq!(size(ImageType::Jpeg, &jpeg), Some((640, 480)));
        // The tables of Huffman share the range of the frame markers.
        let huffman = [0xff, 0xd8, 0xff, 0xc4, 0x00, 0x04, 0x00, 0x00, 0xff, 0xda];
        assert_eq!(size(ImageType::Jpeg, &huffman), None);
    }

    #[test]
    fn the_size_of_a_webp_is_in_its_first_chunk() {
        let head = |chunk: &[u8]| {
            let mut bytes = b"RIFF\0\0\0\0WEBP".to_vec();
            bytes.extend_from_slice(chunk);
            bytes.extend_from_slice(&[0u8; 4]);
            bytes
        };
        let mut lossy = head(b"VP8 ");
        lossy.extend([0, 0, 0, 0x9d, 0x01, 0x2a, 0x80, 0x02, 0xe0, 0x01]);
        let mut lossless = head(b"VP8L");
        // 640 - 1 and 480 - 1, on 14 bits each.
        let bits: u32 = 639 | (479 << 14);
        lossless.push(0x2f);
        lossless.extend(bits.to_le_bytes());
        let mut extended = head(b"VP8X");
        extended.extend([0, 0, 0, 0, 0x7f, 0x02, 0x00, 0xdf, 0x01, 0x00]);

        assert_eq!(size(ImageType::Webp, &lossy), Some((640, 480)));
        assert_eq!(size(ImageType::Webp, &lossless), Some((640, 480)));
        assert_eq!(size(ImageType::Webp, &extended), Some((640, 480)));
    }

    #[test]
    fn the_size_of_an_svg_is_in_pixels_or_its_view_box() {
        for (root, found) in [
            ("<svg width=\"120\" height=\"40\">", Some((120, 40))),
            ("<svg width='120px' height='40.4px'>", Some((120, 40))),
            ("<svg viewBox=\"0 0 300 150\">", Some((300, 150))),
            (
                "<svg viewBox=\"0,0,300,150\" width=\"600\">",
                Some((600, 300)),
            ),
            (
                "<svg viewBox=\"0 0 300 150\" height=\"75\">",
                Some((150, 75)),
            ),
            (
                "<svg stroke-width=\"2\" viewBox=\"0 0 30 10\">",
                Some((30, 10)),
            ),
            ("<svg width=\"100%\" height=\"100%\">", None),
            ("<svg width=\"10em\" viewBox=\"0 0 3 1\">", Some((3, 1))),
            ("<svg viewBox=\"0 0 0 10\">", None),
            ("<svg>", None),
        ] {
            assert_eq!(size(ImageType::Svg, root.as_bytes()), found, "{root}");
        }
    }

    #[test]
    fn a_size_rounds_up_to_a_tenth_of_a_megabyte() {
        assert_eq!(megabytes(MAX_BYTES), "5.0 MB");
        assert_eq!(megabytes(MAX_BYTES + 1), "5.1 MB");
        assert_eq!(megabytes(6 * 1024 * 1024 + 200 * 1024), "6.2 MB");
    }
}
