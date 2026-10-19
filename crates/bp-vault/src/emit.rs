//! The header writer. `set` rewrites one field's lines and leaves every other byte alone, so a label change is a
//! one-line diff; `Header::emit` writes a whole header for a new file. Both write a field one way: plain when YAML
//! reads the text back unchanged, else as JSON, which YAML reads as the same value.

use serde_json::Value;
use yaml_rust2::{Yaml, YamlLoader};

use crate::{Header, LINKS};

impl Header {
    /// The header's YAML, one field a line, without the `---` lines. Parsing it gives this header back. Link fields
    /// that hold unresolved text come in the order their texts appear, so the unresolved list keeps its order.
    pub fn emit(&self) -> String {
        let mut out = format!("kind: {}\n", self.kind);
        if let Some(t) = &self.title {
            out.push_str(&line("title", &Value::from(t.as_str())));
        }
        if let Some(s) = &self.status {
            out.push_str(&line("status", &Value::from(s.as_str())));
        }
        let mut links: Vec<&str> = std::iter::once("under").chain(LINKS).collect();
        links.sort_by_key(|k| self.unresolved.iter().position(|u| u.field == *k).unwrap_or(usize::MAX));
        for key in links {
            let mut loose = self.unresolved.iter().filter(|u| u.field == key).map(|u| Value::from(u.text.as_str()));
            if key == "under" {
                let under = self.under.as_ref().map(|u| Value::from(u.as_str())).or_else(|| loose.next());
                if let Some(u) = under {
                    out.push_str(&line(key, &u));
                }
            } else if let Some(ids) = self.links.get(key) {
                let all = ids.iter().map(|i| Value::from(i.as_str())).chain(loose).collect();
                out.push_str(&line(key, &Value::Array(all)));
            }
        }
        if !self.aliases.is_empty() {
            out.push_str(&line("aliases", &self.aliases.clone().into()));
        }
        for (k, v) in &self.fields {
            out.push_str(&line(k, v));
        }
        out
    }
}

/// `text`, a whole file whose header holds `key`, with that field's lines replaced by `key: value`; a header without
/// the field gains the line before its close. Every other byte, line endings included, stays as it was.
///
/// # Panics
///
/// Never for a file that parses with a header: its first line is `---` and a later line closes it.
pub fn set_field(text: &str, key: &str, value: &Value) -> String {
    let (bom, rest) = text.strip_prefix('\u{feff}').map_or(("", text), |r| ("\u{feff}", r));
    let lines: Vec<&str> = rest.split_inclusive('\n').collect();
    let bare = |l: &str| l.trim_end_matches(['\n', '\r']).to_string();
    let close = (1..lines.len()).find(|&i| bare(lines[i]) == "---").expect("a parsed header closes");
    let start = (1..close).find(|&i| lines[i].strip_prefix(key).is_some_and(|r| r.starts_with(':')));
    // A block value continues on the indented or `- ` lines under its key.
    let end = start.map(|s| (s + 1..close).find(|&i| !lines[i].starts_with([' ', '\t', '-'])).unwrap_or(close));
    let at = start.unwrap_or(close);
    // The new line ends as the line it replaces, or as the last field line when it is added.
    let eol = if lines[start.unwrap_or(close - 1)].ends_with("\r\n") { "\r\n" } else { "\n" };
    let mut new = line(key, value);
    new.truncate(new.len() - 1);
    new.push_str(eol);
    let mut out = String::from(bom);
    out.extend(lines[..at].iter().copied());
    out.push_str(&new);
    out.extend(lines[end.unwrap_or(at)..].iter().copied());
    out
}

/// `key: value\n`.
fn line(key: &str, value: &Value) -> String {
    format!("{key}: {}\n", yaml(key, value))
}

fn yaml(key: &str, value: &Value) -> String {
    match value {
        Value::String(s) if plain(key, s) => s.clone(),
        Value::Array(items) => {
            format!(
                "[{}]",
                items.iter().map(|i| serde_json::to_string(i).expect("JSON serialises")).collect::<Vec<_>>().join(", ")
            )
        }
        v => serde_json::to_string(v).expect("JSON serialises"),
    }
}

/// Whether `key: s` reads back as the text `s`: not empty, not a number, a boolean or null, no comment, no flow.
fn plain(key: &str, s: &str) -> bool {
    let Ok(docs) = YamlLoader::load_from_str(&format!("{key}: {s}")) else { return false };
    let Some(Yaml::Hash(map)) = docs.first() else { return false };
    map.get(&Yaml::String(key.into())).and_then(Yaml::as_str) == Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> Header {
        Header::parse(yaml, "f").unwrap()
    }

    #[test]
    fn emit_reads_back_as_the_same_header() {
        for yaml in [
            "kind: issue\ntitle: 'a: b # c'\nstatus: open\nunder: '[[milestone.anvil.x]]'\ncites: [\"projects/a/designs/b\", \"[[x]]\"]\ndepends: projects/a/issues/e\nseverity: high\ntags: [\"domain/x\"]\ncost: 53\nratio: 1.5\nflag: true\nnone: ~\nwhen: 2026-10-11\nnum: '42'\n",
            "kind: issue\ntitle: T\ncites: ['[[a]]']\nunder: '[[b]]'\naliases: [old.slug]\nmap: {a: 1}\n",
            "kind: claim\nunder: projects/bp/issues/x\nfrom: 2026-05-31T10-00-00Z\nnote: \"line one\\nline two\"\n",
        ] {
            let h = parse(yaml);
            assert_eq!(parse(&h.emit()), h, "{}", h.emit());
        }
    }

    #[test]
    fn set_field_rewrites_one_line() {
        let text = "---\nkind: issue\ntitle: T\nseverity: low\ntags:\n  - a\n  - b\n---\n\nBody: x\n";
        assert_eq!(
            set_field(text, "severity", &"high".into()),
            "---\nkind: issue\ntitle: T\nseverity: high\ntags:\n  - a\n  - b\n---\n\nBody: x\n"
        );
        assert_eq!(
            set_field(text, "tags", &serde_json::json!(["c"])),
            "---\nkind: issue\ntitle: T\nseverity: low\ntags: [\"c\"]\n---\n\nBody: x\n"
        );
        let crlf = "\u{feff}---\r\nkind: issue\r\ntitle: T\r\n---\r\nBody\r\n";
        assert_eq!(
            set_field(crlf, "walkthrough", &true.into()),
            "\u{feff}---\r\nkind: issue\r\ntitle: T\r\nwalkthrough: true\r\n---\r\nBody\r\n"
        );
        assert_eq!(
            set_field("---\nkind: issue\nseverity_x: 1\n---\n", "severity", &"true".into()),
            "---\nkind: issue\nseverity_x: 1\nseverity: \"true\"\n---\n"
        );
    }
}
