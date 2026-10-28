//! A project's version and a design's version in words (12, Blueprint version; 32, design change 9). Home, Projects
//! and Design all read this one function, so a project never shows two versions: the newest version record, else the
//! newest `blueprint/<project>/<n>` tag, else none.

use std::cell::OnceCell;

use bp_vault::{Index, Kind, Refusal, Vault};

/// A blueprint version record, reduced: its `n`, its tag, and its manifest of design id to hash.
type Record = (u64, String, serde_json::Map<String, serde_json::Value>);

/// The vault's `blueprint/` tags, read with one `git for-each-ref` on first use. A request that finds a record for
/// every project never asks, so it runs no git.
pub(crate) struct Tags<'a> {
    vault: &'a Vault,
    read: OnceCell<Vec<String>>,
}

impl<'a> Tags<'a> {
    pub(crate) fn new(vault: &'a Vault) -> Self {
        Self { vault, read: OnceCell::new() }
    }

    fn all(&self) -> &[String] {
        self.read.get_or_init(|| {
            let args = ["for-each-ref", "--format=%(refname:strip=2)", "refs/tags/blueprint/"];
            // A vault outside git has no tags.
            crate::git::git(&self.vault.root, &args)
                .map(|out| out.lines().map(str::to_string).collect())
                .unwrap_or_default()
        })
    }

    /// The `n` of `tag` when it is a version tag of `project`, `blueprint/<project>/<n>` with `n` all digits.
    pub(crate) fn n(tag: &str, project: &str) -> Option<u64> {
        let n = tag.strip_prefix("blueprint/")?.strip_prefix(project)?.strip_prefix('/')?;
        n.bytes().all(|b| b.is_ascii_digit()).then(|| n.parse().ok())?
    }

    fn newest(&self, project: &str) -> Option<String> {
        self.all()
            .iter()
            .filter_map(|t| Some((Self::n(t, project)?, t)))
            .max_by_key(|(n, _)| *n)
            .map(|(_, t)| t.clone())
    }
}

/// A project's version: the tag that names it, and the records behind it.
pub(crate) struct ProjectVersion {
    pub(crate) tag: String,
    /// The version records, newest first; empty when only a tag names the version, and then no manifest is read.
    records: Vec<Record>,
}

impl ProjectVersion {
    /// The newest version record of `project`, else its newest `blueprint/<project>/<n>` tag; `None` before either
    /// exists. The tags are read only when the project has no record.
    pub(crate) fn of(index: &Index, project: &str, tags: &Tags) -> Result<Option<Self>, Refusal> {
        let mut records: Vec<Record> = index
            .records(&format!("projects/{project}/project"), Kind::BlueprintVersion)?
            .into_iter()
            .filter_map(|r| {
                let n = r.header.get("n")?.as_u64()?;
                let tag = r.header.get("tag")?.as_str()?.to_string();
                Some((n, tag, r.header.get("manifest")?.as_object().cloned().unwrap_or_default()))
            })
            .collect();
        records.sort_by(|a, b| b.0.cmp(&a.0));
        Ok(match records.first() {
            Some((_, tag, _)) => Some(Self { tag: tag.clone(), records }),
            None => tags.newest(project).map(|tag| Self { tag, records }),
        })
    }

    /// Whether the version's tag is in the vault's git. A record's tag may not be: a stamp can stop before its tag,
    /// and the session's push leaves tags out (12).
    pub(crate) fn exists(&self, tags: &Tags) -> bool {
        self.records.is_empty() || tags.all().contains(&self.tag)
    }
}

/// A design's version in words, and the tag the words end with.
pub(crate) struct Word {
    pub(crate) words: String,
    pub(crate) tag: Option<String>,
    /// The design changed since the project's version: the words read "changed since <tag>".
    pub(crate) changed: bool,
}

/// "accepted at <tag>" names the newest version whose manifest holds the design file's current hash (blake3 over the
/// whole file, as `stamp` writes it); else "changed since <tag>" names the project's version, with no manifest read
/// when only a tag names it; else "no version". The file is read only when a record exists.
pub(crate) fn word(vault: &Vault, id: &str, version: Option<&ProjectVersion>) -> Result<Word, Refusal> {
    let Some(version) = version else {
        return Ok(Word { words: "no version".to_string(), tag: None, changed: false });
    };
    if !version.records.is_empty() {
        let hash = vault.read(&crate::node_id(id)?)?.hash;
        let held = |(_, _, m): &&Record| m.get(id).and_then(|h| h.as_str()) == Some(hash.as_str());
        if let Some((_, tag, _)) = version.records.iter().find(held) {
            return Ok(Word { words: format!("accepted at {tag}"), tag: Some(tag.clone()), changed: false });
        }
    }
    let tag = version.tag.clone();
    Ok(Word { words: format!("changed since {tag}"), tag: Some(tag), changed: true })
}
