//! Blueprint's sections inside files it shares with the user: the always-on section in `AGENTS.md`, and the import
//! that makes Claude Code read it (27, design change 1). A section is the lines from `<!-- bp:begin -->` through
//! `<!-- bp:end -->`; the user's lines around it stay.

use std::ops::Range;
use std::path::Path;

use bp_vault::Refusal;

use crate::spec::Spec;

const BEGIN: &str = "<!-- bp:begin -->";
const END: &str = "<!-- bp:end -->";

/// A section around `body`, the markers included.
pub(crate) fn wrap(body: &str) -> String {
    format!("{BEGIN}\n{body}{END}\n")
}

/// The section that imports the always-on file into a harness file that would make the harness skip it.
pub(crate) fn import(spec: &Spec) -> String {
    wrap(&format!("@{}\n", spec.always_on))
}

/// The file of `spec.imports` that needs the import: the first that exists, unless one of them imports the always-on
/// file already, or one is the always-on file through a link. With none of them, the harness reads the always-on file
/// itself.
pub(crate) fn importer(top: &Path, spec: &Spec) -> Option<&'static str> {
    let held: Vec<&'static str> = spec.imports.iter().copied().filter(|r| top.join(r).is_file()).collect();
    let always_on = top.join(spec.always_on).canonicalize().ok();
    let linked = |r: &str| always_on.is_some() && top.join(r).canonicalize().ok() == always_on;
    let imports = |r: &str| imports(&std::fs::read_to_string(top.join(r)).unwrap_or_default(), spec.always_on);
    let first = *held.first()?;
    (!held.iter().any(|r| linked(r) || imports(r))).then_some(first)
}

/// Whether `text` imports `file` outside Blueprint's section: a line `@<file>` or `@./<file>`. Claude Code reads no
/// import inside a code block, so a fenced line does not count.
fn imports(text: &str, file: &str) -> bool {
    let outside = span(text).map_or_else(|| text.to_string(), |r| format!("{}{}", &text[..r.start], &text[r.end..]));
    let mut fenced = false;
    outside.lines().map(str::trim).any(|l| {
        if l.starts_with("```") || l.starts_with("~~~") {
            fenced = !fenced;
        }
        !fenced && l.strip_prefix('@').is_some_and(|l| l.strip_prefix("./").unwrap_or(l) == file)
    })
}

/// A file that holds or will hold a section, read: `None` when absent. Unpaired markers refuse, as no one section
/// could be replaced.
pub(crate) fn sectioned(top: &Path, rel: &str) -> Result<Option<String>, Refusal> {
    let held = crate::read(&top.join(rel))?;
    if let Some(text) = held.as_deref()
        && (text.matches(BEGIN).count(), text.matches(END).count())
            != if span(text).is_some() { (1, 1) } else { (0, 0) }
    {
        let fix = format!("{rel} holds an unpaired {BEGIN} or {END}; leave one pair or none, then rerun");
        return Err(Refusal::failed(rel, fix));
    }
    Ok(held)
}

/// The section's bytes in a file: the begin line through the end line and its line end.
pub(crate) fn span(text: &str) -> Option<Range<usize>> {
    let begin = format!("{BEGIN}\n");
    let start = if text.starts_with(&begin) { 0 } else { text.find(&format!("\n{begin}"))? + 1 };
    let end = start + text[start..].find(&format!("\n{END}"))? + 1 + END.len();
    Some(start..if text[end..].starts_with('\n') { end + 1 } else { end })
}

/// The file with `section` in place of the old one, or appended after a blank line.
pub(crate) fn with_section(held: Option<&str>, section: &str) -> String {
    match held {
        None => section.to_string(),
        Some(text) => match span(text) {
            Some(r) => format!("{}{section}{}", &text[..r.start], &text[r.end..]),
            None if text.is_empty() => section.to_string(),
            None => format!("{}\n{section}", if text.ends_with('\n') { text.to_string() } else { format!("{text}\n") }),
        },
    }
}

/// The file without its section, and without the blank line that `with_section` put before a section at the end.
pub(crate) fn without_section(text: &str) -> Option<String> {
    let r = span(text)?;
    let (before, after) = (&text[..r.start], &text[r.end..]);
    let before = if after.is_empty() {
        before.strip_suffix('\n').filter(|b| b.ends_with('\n')).unwrap_or(before)
    } else {
        before
    };
    Some(format!("{before}{after}"))
}
