//! One markdown file of the vault: its id, content hash, header and body. A file without a header is an asset.

use serde_json::{Map, Value};
use yaml_rust2::{Yaml, YamlLoader};

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

/// Why a file does not parse: the error, and whether the text reads but breaks a rule of its kind's schema rather
/// than failing to read at all (bytes, the header's lines, YAML). The census stores `why`; a verb refuses with
/// `Refusal::schema`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Broken {
    /// The error.
    pub why: String,
    /// The text reads, and a schema rule refuses it.
    pub rule: bool,
}

impl Broken {
    pub(crate) fn unread(why: impl Into<String>) -> Self {
        Self { why: why.into(), rule: false }
    }

    pub(crate) fn rule(why: impl Into<String>) -> Self {
        Self { why: why.into(), rule: true }
    }
}

impl File {
    /// Parses a file's bytes, refusing `schema_invalid` with the by-hand fix when it does not parse (`File::parse`).
    pub fn from_bytes(id: Id, bytes: &[u8]) -> Result<Self, Refusal> {
        let subject = id.as_str().to_owned();
        Self::parse(id, bytes).map_err(|b| Refusal::schema(&subject, &b))
    }

    /// Parses a file's bytes. Fails on non-UTF-8, an unclosed header, a header outside its kind's schema, a path that
    /// does not fit the kind, and a record whose `under` is not the parent its path names.
    pub fn parse(id: Id, bytes: &[u8]) -> Result<Self, Broken> {
        let hash = blake3::hash(bytes).to_hex().to_string();
        let text = std::str::from_utf8(bytes).map_err(|e| Broken::unread(format!("not UTF-8: {e}")))?;
        let text = normalise(text);
        let Some((yaml, body)) = split(&text) else {
            if text.starts_with("---\n") {
                return Err(Broken::unread("the closing --- is missing"));
            }
            if !is_asset(&id) {
                return Err(Broken::unread(
                    "no header: the first line is not `---`, and a file without a header belongs in shared/assets/ \
                     or a node's own directory",
                ));
            }
            return Ok(Self { id, hash, header: None, body: Body::parse(&text) });
        };
        let header = Header::parse(yaml)?;
        if !header.kind.fits(&id) {
            return Err(Broken::rule(format!("the path does not fit the kind `{}`", header.kind)));
        }
        let parent = header.kind.parent(&id);
        if header.kind.is_record() && header.under != parent {
            let name = |id: Option<&Id>| id.map_or("nothing", Id::as_str).to_string();
            return Err(Broken::rule(format!(
                "a {} record must name `under: {}`; it names `{}`",
                header.kind,
                name(parent.as_ref()),
                name(header.under.as_ref())
            )));
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

crate::closed_enum! {
    /// A delta's state. The header stores none: it derives from the record's one closing append (13, Delta).
    pub enum DeltaState {
        Proposed = "proposed",
        Accepted = "accepted",
        Rejected = "rejected",
    }
}

impl DeltaState {
    /// The state a delta's body holds. The closing append is the last `## ` section, and it holds its instant
    /// (`accepted_at:` or `rejected_at:`), so a proposed design body that ends with a heading of the same name
    /// closes nothing.
    pub fn of(body: &Body) -> Self {
        let Some(last) = body.sections.last() else { return Self::Proposed };
        let holds = |key: &str| last.text.lines().any(|l| l.starts_with(key));
        match last.heading.as_str() {
            "Accepted" if holds("accepted_at: ") => Self::Accepted,
            "Rejected" if holds("rejected_at: ") => Self::Rejected,
            _ => Self::Proposed,
        }
    }
}

impl File {
    /// The header as `show` prints it and the index stores it: [`Header::to_json`], and a delta's derived state, so
    /// the two cannot disagree. `None` for an asset.
    pub fn header_json(&self) -> Option<Map<String, Value>> {
        let header = self.header.as_ref()?;
        let mut m = header.to_json(&self.id);
        if header.kind == Kind::Delta {
            m.insert("state".into(), DeltaState::of(&self.body).as_str().into());
        }
        Some(m)
    }
}

/// The items of the list field `key` as the header writes them, in order: ids and link texts alike. Empty when the
/// header lacks the field. A link edit keeps this order and appends (15, link).
pub fn written(text: &str, key: &str) -> Vec<String> {
    let text = normalise(text);
    split(&text).map_or_else(Vec::new, |(yaml, _)| crate::header::items(yaml, key))
}

/// The text without a BOM and with LF line endings: a CRLF or a lone CR ends a line, as `lines` reads it.
pub fn normalise(text: &str) -> String {
    text.strip_prefix('\u{feff}').unwrap_or(text).replace("\r\n", "\n").replace('\r', "\n")
}

/// The byte offset in `text` as written where its header ends, the closing `---` line's line end included; `None`
/// when the text opens with no closed header. A write that keeps the header byte for byte cuts there, so it reads
/// a BOM, CRLF and a lone CR as the parser does (25, design change 2).
pub fn header_end(text: &str) -> Option<usize> {
    bounds(text).map(|(_, _, end)| end)
}

/// Whether `text` opens with a header: a `---` line, a closing `---` line, and YAML between them that holds `kind`.
/// A body that only opens with a thematic break holds none.
pub fn opens_with_header(text: &str) -> bool {
    let text = normalise(text);
    split(&text).is_some_and(|(yaml, _)| {
        matches!(YamlLoader::load_from_str(yaml).ok().as_deref(), Some([Yaml::Hash(map), ..])
            if map.contains_key(&Yaml::String("kind".into())))
    })
}

/// The header's YAML and the body; `None` when the first line is not `---` or no later line is exactly `---`.
fn split(text: &str) -> Option<(&str, &str)> {
    let (yaml, close, body) = bounds(text)?;
    Some((&text[yaml..close], &text[body..]))
}

/// The byte offsets of the YAML's start and end and of the body's start, in `text` as written, after any BOM.
fn bounds(text: &str) -> Option<(usize, usize, usize)> {
    let bom = text.strip_prefix('\u{feff}').map_or(0, |_| '\u{feff}'.len_utf8());
    let mut lines = lines(&text[bom..]).map(|(start, line, end)| (bom + start, line, bom + end));
    // The opening line needs its line end: `---` alone is not a header.
    let (_, _, yaml) = lines.next().filter(|(_, line, end)| *line == "---" && *end > bom + 3)?;
    lines.find(|(_, line, _)| *line == "---").map(|(close, _, body)| (yaml, close, body))
}

/// Each line's start, text without its line end, and the start of the next line. A line ends at LF, CRLF or a lone
/// CR.
fn lines(text: &str) -> impl Iterator<Item = (usize, &str, usize)> {
    let mut at = 0;
    std::iter::from_fn(move || {
        let rest = text.get(at..).filter(|r| !r.is_empty())?;
        let (len, eol) = match rest.find(['\n', '\r']) {
            Some(i) if rest[i..].starts_with("\r\n") => (i, 2),
            Some(i) => (i, 1),
            None => (rest.len(), 0),
        };
        let start = at;
        at += len + eol;
        Some((start, &rest[..len], at))
    })
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
    fn the_header_end_reads_line_ends_as_the_parser_does() {
        for (text, end) in [
            ("---\nkind: issue\n---\nBody.\n", 20),
            ("\u{feff}---\r\nkind: issue\r\n---\r\n\r\nBody.\r\n", 26),
            ("---\rkind: issue\r---\rBody.\r", 20),
            ("---\nkind: issue\n---", 19),
        ] {
            assert_eq!(header_end(text), Some(end), "{text:?}");
            assert_eq!(split(&normalise(text)).map(|(yaml, _)| yaml), Some("kind: issue\n"), "{text:?}");
        }
        assert_eq!(header_end("---"), None);
        assert_eq!(header_end("---\nkind: issue\n"), None);
        assert_eq!(header_end("Body.\n---\n"), None);
    }

    #[test]
    fn a_header_holds_kind_and_a_thematic_break_does_not() {
        assert!(opens_with_header("\u{feff}---\r\nkind: issue\r\ntitle: T\r\n---\r\nBody.\r\n"));
        assert!(!opens_with_header("---\nA paragraph between rules.\n---\n"));
        assert!(!opens_with_header("---\ntitle: T\n---\n"));
        assert!(!opens_with_header("---\n\nBody.\n"));
        assert!(!opens_with_header("Body.\n\n---\nkind: issue\n---\n"));
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

    #[test]
    fn a_delta_is_closed_only_by_an_append_that_holds_its_instant() {
        let state = |body: &str| DeltaState::of(&Body::parse(body));
        assert_eq!(state("## One\n\nNew.\n"), DeltaState::Proposed);
        assert_eq!(state("## One\n\nNew.\n\n## Accepted\n\nThe options we accepted.\n"), DeltaState::Proposed);
        let close = "## Accepted\n\naccepted_at: 2026-10-20T00:00:00Z\nversion: [[projects/bp/versions/1]]\n";
        assert_eq!(state(&format!("## One\n\nNew.\n\n{close}")), DeltaState::Accepted);
        assert_eq!(state("## Rejected\n\nrejected_at: 2026-10-20T00:00:00Z\n"), DeltaState::Rejected);
    }
}
