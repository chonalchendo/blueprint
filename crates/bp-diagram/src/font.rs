//! The three embedded faces. Their advance tables measure text and their bytes draw it, so a measured box is the box
//! the browser fills (28, design change 3). Integer arithmetic keeps the widths identical on every platform.

use std::fmt::Write as _;

use crate::metrics::{INTER_400, INTER_600, MONO_400};

/// One face's metrics, in font units.
pub(crate) struct Face {
    pub(crate) upm: u16,
    pub(crate) cap: u16,
    /// `(code point, advance)`, sorted by code point.
    pub(crate) advances: &'static [(u32, u16)],
}

/// A face by its use: running text, names, and the mono of ids, fields and kind labels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Font {
    Body,
    Strong,
    Mono,
}

impl Font {
    fn face(self) -> &'static Face {
        match self {
            Self::Body => &INTER_400,
            Self::Strong => &INTER_600,
            Self::Mono => &MONO_400,
        }
    }

    /// The CSS class that draws text in this face.
    pub(crate) const fn class(self) -> &'static str {
        match self {
            Self::Body => "t-body",
            Self::Strong => "t-strong",
            Self::Mono => "t-mono",
        }
    }

    /// The width of `text` at `size` px with `tracking` px after each character, rounded up to a whole px. A
    /// character outside the subset draws in a fallback face, so it counts as one em to keep its box large enough.
    pub(crate) fn width(self, text: &str, size: i32, tracking: i32) -> i32 {
        let face = self.face();
        let mut units = 0_i64;
        let mut n = 0_i64;
        for c in text.chars() {
            n += 1;
            units += face
                .advances
                .binary_search_by_key(&u32::from(c), |&(code, _)| code)
                .map_or(i64::from(face.upm), |i| i64::from(face.advances[i].1));
        }
        let upm = i64::from(face.upm);
        i32::try_from((units * i64::from(size) + upm - 1) / upm + i64::from(tracking) * n).unwrap_or(i32::MAX)
    }

    /// The cap height at `size` px, rounded: the offset from a line's centre to its baseline is half of it.
    pub(crate) fn cap(self, size: i32) -> i32 {
        let face = self.face();
        (i32::from(face.cap) * size + i32::from(face.upm) / 2) / i32::from(face.upm)
    }

    /// `text` broken at spaces into lines no wider than `max`. A word wider than `max` keeps a line of its own, and
    /// the overflow rule names it.
    pub(crate) fn wrap(self, text: &str, size: i32, max: i32) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        for word in text.split_whitespace() {
            match lines.last_mut() {
                Some(line) if self.width(&format!("{line} {word}"), size, 0) <= max => {
                    line.push(' ');
                    line.push_str(word);
                }
                _ => lines.push(word.to_string()),
            }
        }
        lines
    }
}

/// The `@font-face` rules: each face as base64 WOFF2, so the file needs no network and no installed font.
pub(crate) fn faces_css() -> String {
    let faces: [(&str, u16, &[u8]); 3] = [
        ("bp-inter", 400, include_bytes!("../fonts/inter-400.woff2")),
        ("bp-inter", 600, include_bytes!("../fonts/inter-600.woff2")),
        ("bp-mono", 400, include_bytes!("../fonts/geist-mono-400.woff2")),
    ];
    let mut css = String::new();
    for (family, weight, bytes) in faces {
        let _ = writeln!(
            css,
            "@font-face{{font-family:{family};font-weight:{weight};src:url(data:font/woff2;base64,{}) format(\"woff2\")}}",
            base64(bytes)
        );
    }
    css
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ABC[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_follow_the_advance_table() {
        // Geist Mono advances 600 of 1000 units: 6 px at 10 px.
        assert_eq!(Font::Mono.width("abcd", 10, 0), 24);
        assert_eq!(Font::Mono.width("abcd", 10, 1), 28);
        assert!(Font::Strong.width("Tool", 14, 0) > Font::Body.width("Tool", 14, 0) - 2);
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
    }

    #[test]
    fn wrap_breaks_at_spaces() {
        assert_eq!(Font::Mono.wrap("aa bb cc", 10, 30), ["aa bb", "cc"]);
    }
}
