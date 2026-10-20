//! The header writer. `set` rewrites one field's lines and leaves every other byte alone, so a label change is a
//! one-line diff; `Header::emit` writes a whole header for a new file. Both write a field one way: plain when YAML
//! 1.2 reads the text back unchanged and YAML 1.1 reads it as text, else as JSON, which YAML reads as the same value.

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
        let mut links: Vec<&str> =
            std::iter::once("under").chain(LINKS).chain(self.kind.record_links().iter().copied()).collect();
        links.sort_by_key(|k| self.unresolved.iter().position(|u| u.field == *k).unwrap_or(usize::MAX));
        for key in links {
            let mut loose = self.unresolved.iter().filter(|u| u.field == key).map(|u| Value::from(u.text.as_str()));
            if key == "under" {
                let under = self.under.as_ref().map(|u| Value::from(u.as_str())).or_else(|| loose.next());
                if let Some(u) = under {
                    out.push_str(&line(key, &u));
                }
            } else if let Some(ids) = self.links.get(key) {
                let mut all: Vec<Value> = ids.iter().map(|i| Value::from(i.as_str())).chain(loose).collect();
                let one = self.kind.is_record() && !self.kind.record_list(key) && all.len() == 1;
                let value = if one { all.remove(0) } else { Value::Array(all) };
                out.push_str(&line(key, &value));
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
/// the field gains the line before its close. A null value deletes the field's lines: no converted header holds an
/// empty `field: []`. Every other byte, line endings included, stays as it was.
///
/// # Panics
///
/// Never for a file that parses with a header: its first line is `---` and a later line closes it.
pub fn set_field(text: &str, key: &str, value: &Value) -> String {
    let (bom, rest) = text.strip_prefix('\u{feff}').map_or(("", text), |r| ("\u{feff}", r));
    let lines: Vec<&str> = rest.split_inclusive('\n').collect();
    let bare = |l: &str| l.trim_end_matches(['\n', '\r']).to_string();
    let close = (1..lines.len()).find(|&i| bare(lines[i]) == "---").expect("a parsed header closes");
    // YAML reads `severity:`, `"severity":` and `'severity' :` as one key.
    let spellings = [key.to_string(), format!("\"{key}\""), format!("'{key}'")];
    let named =
        |l: &str| spellings.iter().any(|n| l.strip_prefix(n.as_str()).is_some_and(|r| r.trim_start().starts_with(':')));
    let start = (1..close).find(|&i| named(lines[i]));
    // A block value continues on the indented or `- ` lines under its key.
    let end = start.map(|s| (s + 1..close).find(|&i| !lines[i].starts_with([' ', '\t', '-'])).unwrap_or(close));
    let at = start.unwrap_or(close);
    // The new line ends as the line it replaces, or as the last field line when it is added.
    let eol = if lines[start.unwrap_or(close - 1)].ends_with("\r\n") { "\r\n" } else { "\n" };
    let mut new = String::new();
    if !value.is_null() {
        new = line(key, value);
        new.truncate(new.len() - 1);
        new.push_str(eol);
    }
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
    if yaml11_typed(s) {
        return false;
    }
    let Ok(docs) = YamlLoader::load_from_str(&format!("{key}: {s}")) else { return false };
    let Some(Yaml::Hash(map)) = docs.first() else { return false };
    map.get(&Yaml::String(key.into())).and_then(Yaml::as_str) == Some(s)
}

/// Whether a YAML 1.1 reader, such as `PyYAML`, takes the plain scalar `s` for a bool, a null, a number or a
/// timestamp. It errs towards yes: a needless quote still reads back as the same text.
fn yaml11_typed(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    if matches!(lower.as_str(), "" | "~" | "null" | "y" | "n" | "yes" | "no" | "on" | "off" | "true" | "false") {
        return true;
    }
    let t = lower.strip_prefix(['+', '-']).unwrap_or(&lower);
    let b = t.as_bytes();
    // A timestamp starts `YYYY-`; a number in any base, sexagesimal included, starts with a digit or a dot and holds
    // no space.
    let date = b.len() >= 8 && b[..4].iter().all(u8::is_ascii_digit) && b[4] == b'-';
    let number = t.starts_with(|c: char| c.is_ascii_digit() || c == '.')
        && t.chars().all(|c| c.is_ascii_alphanumeric() || "_:.+-".contains(c));
    date || number
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> Header {
        Header::parse(yaml).unwrap()
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
        assert_eq!(
            set_field(text, "tags", &Value::Null),
            "---\nkind: issue\ntitle: T\nseverity: low\n---\n\nBody: x\n",
            "null deletes a block value"
        );
        assert_eq!(set_field("---\nkind: issue\n---\n", "tags", &Value::Null), "---\nkind: issue\n---\n");
        for quoted in ["\"severity\": low", "'severity': low", "severity : low"] {
            let text = format!("---\nkind: issue\n{quoted}\n---\n");
            assert_eq!(set_field(&text, "severity", &"high".into()), "---\nkind: issue\nseverity: high\n---\n");
        }
    }

    #[test]
    fn quotes_what_yaml_1_1_reads_as_another_type() {
        for typed in [
            "yes",
            "No",
            "ON",
            "off",
            "y",
            "N",
            "True",
            "~",
            "Null",
            "",
            "2026-10-19",
            "2001-12-14 21:59:43.10 -5",
            "2001-12-14t21:59:43.10-05:00",
            "123",
            "-17",
            "0o17",
            "017",
            "0x1F",
            "0b101",
            "1_000",
            "190:20:30",
            "1.5",
            "6.8523015e+5",
            ".inf",
            "-.Inf",
            ".NaN",
            "+1",
        ] {
            let line = set_field("---\nkind: issue\n---\n", "title", &typed.into());
            assert_eq!(line, format!("---\nkind: issue\ntitle: {}\n---\n", serde_json::json!(typed)), "{typed}");
        }
        for text in ["yesterday", "Night on the town", "2 pass design", "v1.5", "no-op", "a: b"] {
            let want = if text == "a: b" { "\"a: b\"".to_string() } else { text.to_string() };
            assert_eq!(
                set_field("---\nkind: issue\n---\n", "title", &text.into()),
                format!("---\nkind: issue\ntitle: {want}\n---\n")
            );
        }
    }
}
