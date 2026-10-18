//! One markdown file of the vault: its id, content hash, header and body. A node or a record by path shape; a
//! file without a header is an asset.

use std::path::Path;

use crate::{Body, Header, Id, Refusal};

/// One file, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// The path id.
    pub id: Id,
    /// The blake3 hash of the bytes on disk, hex.
    pub hash: String,
    /// The header, when the file starts with `---`.
    pub header: Option<Header>,
    /// The body: everything after the header.
    pub body: Body,
}

impl File {
    /// Reads and parses the file this id names under `root`. Refuses `no_node` when it is not there.
    pub fn read(root: &Path, id: &Id) -> Result<Self, Refusal> {
        let path = id.path(root);
        let bytes = std::fs::read(&path).map_err(|_| {
            Refusal::new("no_node", id.as_str(), "an id names a markdown file under the vault root", "bp list <kind>")
        })?;
        let text = String::from_utf8_lossy(&bytes);
        let mut file = Self::parse(id.clone(), &text)?;
        file.hash = blake3::hash(&bytes).to_hex().to_string();
        Ok(file)
    }

    /// Parses the text of a file. The hash is of the text; `read` replaces it with the hash of the bytes.
    pub fn parse(id: Id, text: &str) -> Result<Self, Refusal> {
        let (header, body) = match text.strip_prefix("---\n") {
            Some(rest) => {
                let end = rest.find("\n---\n").or_else(|| rest.strip_suffix("\n---").map(str::len));
                let Some(end) = end else {
                    return Err(Refusal::new(
                        "bad_header",
                        id.as_str(),
                        "a header is a YAML map between --- lines with a kind",
                        "fix the header: the closing --- is missing",
                    ));
                };
                let yaml = &rest[..end];
                let body = rest.get(end + 5..).unwrap_or("");
                (Some(Header::parse(yaml, id.as_str())?), body)
            }
            None => (None, text),
        };
        Ok(Self { hash: blake3::hash(text.as_bytes()).to_hex().to_string(), id, header, body: Body::parse(body) })
    }

    /// A record: a headed file at a record path.
    pub fn is_record(&self) -> bool {
        self.header.is_some() && self.id.is_record()
    }

    /// A node: a headed file at a node path.
    pub fn is_node(&self) -> bool {
        self.header.is_some() && !self.id.is_record()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_header_and_body() {
        let f =
            File::parse(Id::new("projects/bp/issues/x"), "---\nkind: issue\ntitle: T\n---\n\nLead.\n\n## A\n\nText.\n")
                .unwrap();
        assert_eq!(f.header.as_ref().unwrap().kind, "issue");
        assert_eq!(f.body.lead, "Lead.");
        assert!(f.is_node());
        assert_eq!(f.hash.len(), 64);
    }

    #[test]
    fn a_file_without_a_header_is_an_asset() {
        let f = File::parse(Id::new("shared/assets/glossary"), "# Glossary\n").unwrap();
        assert!(f.header.is_none());
        assert!(!f.is_node() && !f.is_record());
    }

    #[test]
    fn a_header_only_record_parses() {
        let f = File::parse(
            Id::new("projects/bp/issues/x/claims/s"),
            "---\nkind: claim\nunder: projects/bp/issues/x\n---\n",
        )
        .unwrap();
        assert!(f.is_record());
        assert_eq!(f.body.text, "");
    }

    #[test]
    fn refuses_an_unclosed_header() {
        let e = File::parse(Id::new("projects/bp/issues/x"), "---\nkind: issue\n").unwrap_err();
        assert_eq!(e.code, "bad_header");
    }
}
