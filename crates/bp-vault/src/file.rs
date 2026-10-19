//! One markdown file of the vault: its id, content hash, header and body. A file without a header is an asset.

use crate::body::FindingKind;
use crate::{Body, Header, Id, Kind, Refusal};

/// One file, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// The path id.
    pub id: Id,
    /// The blake3 hash of the bytes on disk, hex.
    pub hash: String,
    /// The header, when the file starts with a `---` line.
    pub header: Option<Header>,
    /// The body: everything after the header.
    pub body: Body,
}

impl File {
    /// Parses a file's bytes. Refuses non-UTF-8, an unclosed header, a header outside its kind's schema, a path that
    /// does not fit the kind, and a record whose `under` is not the parent its path names.
    pub fn from_bytes(id: Id, bytes: &[u8]) -> Result<Self, Refusal> {
        let hash = blake3::hash(bytes).to_hex().to_string();
        let text = std::str::from_utf8(bytes).map_err(|e| Refusal::schema(id.as_str(), format!("not UTF-8: {e}")))?;
        let text = normalise(text);
        let Some((yaml, body)) = split(&text) else {
            if text.starts_with("---\n") {
                return Err(Refusal::schema(id.as_str(), "the closing --- is missing"));
            }
            if !is_asset(&id) {
                return Err(Refusal::schema(
                    id.as_str(),
                    "no header: the first line is not `---`, and a file without a header belongs in shared/assets/ \
                     or a node's own directory",
                ));
            }
            return Ok(Self { id, hash, header: None, body: Body::parse(&text) });
        };
        let header = Header::parse(yaml, id.as_str())?;
        if !header.kind.fits(&id) {
            return Err(Refusal::schema(id.as_str(), format!("the path does not fit the kind `{}`", header.kind)));
        }
        let parent = header.kind.parent(&id);
        if header.kind.is_record() && header.under != parent {
            let want = parent.as_ref().map_or("no parent", Id::as_str);
            return Err(Refusal::schema(id.as_str(), format!("a {} record names `under: {want}`", header.kind)));
        }
        let mut body = Body::parse(body);
        if !Kind::CHECK_OWNERS.contains(&header.kind) {
            // Only a check owner's blocks are checks; in any other body the block is prose (13, rule 1).
            body.checks.clear();
            body.empty_checks.clear();
            body.findings.retain(|f| !matches!(f.kind, FindingKind::CheckUnnamed | FindingKind::CheckMalformed));
        }
        Ok(Self { id, hash, header: Some(header), body })
    }
}

/// The items of the list field `key` as the header writes them, in order: ids and link texts alike. Empty when the
/// header lacks the field. A link edit keeps this order and appends (15, link).
pub fn written(text: &str, key: &str) -> Vec<String> {
    let text = normalise(text);
    split(&text).map_or_else(Vec::new, |(yaml, _)| crate::header::items(yaml, key))
}

/// The text without a BOM and with LF line endings.
fn normalise(text: &str) -> String {
    text.strip_prefix('\u{feff}').unwrap_or(text).replace("\r\n", "\n").replace('\r', "\n")
}

/// The header's YAML and the body; `None` when the first line is not `---` or no later line is exactly `---`.
fn split(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---\n")?;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches('\n') == "---" {
            return Some((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

/// A file without a header may sit in `shared/assets/` or in a node's own directory, beside its diagrams (12).
fn is_asset(id: &Id) -> bool {
    let p: Vec<&str> = id.as_str().split('/').collect();
    p[..2] == ["shared", "assets"]
        || (3..p.len()).any(|n| {
            let prefix = Id::from_trusted(p[..n].join("/"));
            Kind::ALL.iter().any(|k| !k.is_record() && k.fits(&prefix))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Code;

    fn parse(id: &str, text: &str) -> Result<File, Refusal> {
        File::from_bytes(Id::parse(id).unwrap(), text.as_bytes())
    }

    #[test]
    fn splits_header_and_body() {
        let f = parse("projects/bp/issues/x", "---\nkind: issue\ntitle: T\n---\n\nLead.\n\n## A\n\nText.\n").unwrap();
        assert_eq!(f.header.as_ref().unwrap().kind, Kind::Issue);
        assert_eq!(f.body.lead, "Lead.");
        assert_eq!(f.hash.len(), 64);
    }

    #[test]
    fn crlf_and_bom_read_like_lf() {
        let f =
            parse("projects/bp/issues/x", "\u{feff}---\r\nkind: issue\r\ntitle: T\r\n---\r\n\r\nLead.\r\n").unwrap();
        assert_eq!(f.body.lead, "Lead.");
    }

    #[test]
    fn a_list_field_reads_in_written_order() {
        let text = "\u{feff}---\r\nkind: issue\r\ncites: [\"b/x\", \"[[legacy]]\", \"projects/a/y\"]\r\n---\r\n";
        assert_eq!(written(text, "cites"), ["b/x", "[[legacy]]", "projects/a/y"]);
        assert!(written(text, "depends").is_empty());
        assert!(written("no header\n", "cites").is_empty());
    }

    #[test]
    fn a_file_without_a_header_is_an_asset() {
        assert!(parse("shared/assets/glossary", "# Glossary\n").unwrap().header.is_none());
    }

    #[test]
    fn a_record_names_its_parent() {
        let f = parse("projects/bp/issues/x/claims/s", "---\nkind: claim\nunder: projects/bp/issues/x\n---\n").unwrap();
        assert_eq!(f.body.text, "");
        let e =
            parse("projects/bp/issues/x/claims/s", "---\nkind: claim\nunder: projects/bp/issues/y\n---\n").unwrap_err();
        assert!(e.fix.contains("projects/bp/issues/x"), "{}", e.fix);
        assert!(parse("projects/bp/handoffs/h", "---\nkind: handoff\nunder: projects/bp/project\n---\n").is_ok());
    }

    #[test]
    fn refuses_what_breaks_the_schema() {
        for (id, text) in [
            ("projects/bp/issues/x", "---\nkind: issue\n"),
            ("projects/bp/issues/x", "---\n---\n"),
            ("projects/bp/milestones/x", "---\nkind: issue\n---\n"),
            ("projects/bp/issues/x/runs/r", "---\nkind: claim\nunder: projects/bp/issues/x\n---\n"),
        ] {
            assert_eq!(parse(id, text).unwrap_err().code, Code::SchemaInvalid, "{text}");
        }
        let e = File::from_bytes(Id::parse("projects/bp/issues/x").unwrap(), b"---\nkind: \xff\n---\n").unwrap_err();
        assert!(e.fix.contains("UTF-8"));
    }

    #[test]
    fn a_body_line_of_dashes_is_not_the_header_close() {
        let f =
            parse("projects/bp/issues/x", "---\nkind: issue\ntitle: a --- b\n---\nLead.\n\n---\n\nMore.\n").unwrap();
        assert_eq!(f.header.unwrap().title.as_deref(), Some("a --- b"));
        assert!(f.body.text.contains("More."));
    }

    #[test]
    fn a_file_without_a_header_is_an_asset_only_where_assets_live() {
        assert!(parse("shared/assets/glossary", "# Glossary\n").unwrap().header.is_none());
        assert!(parse("projects/bp/designs/system/diagrams/notes", "notes\n").unwrap().header.is_none());
        for id in ["projects/bp/issues/c", "shared/notes/n"] {
            let e = parse(id, "\n---\nkind: issue\ntitle: T\n---\n").err().unwrap();
            assert_eq!(e.code, Code::SchemaInvalid, "{id}");
        }
    }

    #[test]
    fn a_node_has_a_title_and_a_record_has_none() {
        assert!(parse("projects/bp/issues/x", "---\nkind: issue\n---\n").is_err());
        let claim = "---\nkind: claim\nunder: projects/bp/issues/x\nstatus: open\n---\n";
        assert!(parse("projects/bp/issues/x/claims/c", claim).is_err());
    }
}
