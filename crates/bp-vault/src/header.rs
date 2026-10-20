//! The YAML header: the universal fields (`kind`, `title`, `status`, `under`, `aliases`), the closed link set, and
//! every other field as a plain value. Labels and record fields are not special to the substrate; the domain
//! crates read them from `fields`.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;
use yaml_rust2::{ScanError, Yaml, YamlLoader};

use crate::{Broken, DesignLevel, Id, Kind};

/// The closed set of link fields on nodes (12, the kind schemas). Every value is a path id. `carries` is not one:
/// it is derived from sidecars and check blocks, never stored (13). A record's `→` fields are its kind's own
/// (`Kind::record_links`).
pub const LINKS: [&str; 6] = ["cites", "answers", "targets", "authorizes", "supersedes", "depends"];

/// Names a verb's result uses at the top level beside the header's fields, so no header may use them. The five
/// envelope names are among them: an agent tells a refusal from a result by `code`.
pub const RESERVED: [&str; 15] = [
    "id",
    "findings",
    "hash",
    "record",
    "lead",
    "body",
    "sections",
    "section",
    "contract_version",
    "unresolved",
    "code",
    "subject",
    "rule",
    "fix",
    "exit",
];

/// Fields the reader derives from a design's path (12, Layout). No header stores them: a stored copy could contradict
/// the path.
const DERIVED: [&str; 2] = ["design_kind", "scope"];

/// A link target that is not a path id: a legacy link text, a placeholder, a typo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unresolved {
    /// The header field.
    pub field: String,
    /// The text as written.
    pub text: String,
}

/// A parsed header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// The kind.
    pub kind: Kind,
    /// The title. Records have none.
    pub title: Option<String>,
    /// The status. Records have none.
    pub status: Option<String>,
    /// The parent.
    pub under: Option<Id>,
    /// Old slugs a rename left behind.
    pub aliases: Vec<String>,
    /// Links by field: a node's in the closed set, a record's in its kind's `→` fields.
    pub links: BTreeMap<String, Vec<Id>>,
    /// Link and `under` values that are not path ids.
    pub unresolved: Vec<Unresolved>,
    /// Every other field: labels and record fields, as JSON values.
    pub fields: BTreeMap<String, Value>,
}

impl Header {
    /// Parses the YAML between the `---` lines.
    pub fn parse(yaml: &str) -> Result<Self, Broken> {
        let docs = YamlLoader::load_from_str(yaml).map_err(|e| Broken::unread(yaml_error(&e)))?;
        let Some(Yaml::Hash(map)) = docs.first() else { return Err(Broken::unread("the header is not a map")) };
        let kind = map.get(&Yaml::String("kind".into())).ok_or_else(|| Broken::rule("`kind` is missing"))?;
        let kind = kind.as_str().ok_or_else(|| Broken::rule("`kind` is not text"))?;
        let mut header = Self {
            kind: kind.parse::<Kind>().map_err(|e| Broken::rule(e.to_string()))?,
            title: None,
            status: None,
            under: None,
            aliases: Vec::new(),
            links: BTreeMap::new(),
            unresolved: Vec::new(),
            fields: BTreeMap::new(),
        };
        for (key, value) in map {
            let Some(key) = key.as_str() else {
                return Err(Broken::rule(format!("the key `{}` is not text", undebug(&format!("{key:?}")))));
            };
            let text =
                || value.as_str().map(str::to_string).ok_or_else(|| Broken::rule(format!("`{key}` is not text")));
            match key {
                "kind" => {}
                "title" => header.title = Some(text()?),
                "status" => header.status = Some(text()?),
                "under" => match Id::parse(&text()?) {
                    Ok(id) => header.under = Some(id),
                    Err(_) => header.unresolved.push(Unresolved { field: key.into(), text: text()? }),
                },
                "aliases" => {
                    header.aliases = texts(value).ok_or_else(|| Broken::rule("`aliases` holds a non-text item"))?;
                }
                k if LINKS.contains(&k) || header.kind.record_links().contains(&k) => {
                    let mut ids = Vec::new();
                    for t in texts(value).ok_or_else(|| Broken::rule(format!("`{k}` holds a non-text item")))? {
                        match Id::parse(&t) {
                            Ok(id) => ids.push(id),
                            Err(_) => header.unresolved.push(Unresolved { field: k.into(), text: t }),
                        }
                    }
                    header.links.insert(k.into(), ids);
                }
                k if RESERVED.contains(&k) => return Err(Broken::rule(format!("`{k}` is a reserved name"))),
                k if DERIVED.contains(&k) => {
                    // A design's path names the value; no other kind has the field at all (13).
                    return Err(Broken::rule(match header.kind {
                        Kind::Design => format!("`{k}` comes from the path, not the header; delete the line"),
                        other => {
                            let a = if other.as_str().starts_with(['a', 'e', 'i', 'o', 'u']) { "an" } else { "a" };
                            format!("`{k}` is not a field of {a} {other}; delete the line")
                        }
                    }));
                }
                "state" if header.kind == Kind::Delta => {
                    // A stored copy could contradict the closing append the state derives from (13, Delta).
                    return Err(Broken::rule("a delta's `state` derives from its closing append; delete the line"));
                }
                k => {
                    header.fields.insert(k.into(), json(value));
                }
            }
        }
        // A node is named and has a state; a record is evidence about its parent and has neither (13, rule 4).
        match (header.kind.is_record(), &header.title, &header.status) {
            (false, None, _) => Err(Broken::rule(format!("`kind: {}` is a node: it needs a `title`", header.kind))),
            (true, Some(_), _) | (true, _, Some(_)) => {
                Err(Broken::rule(format!("`kind: {}` is a record: it has no `title` or `status`", header.kind)))
            }
            _ => Ok(header),
        }
    }

    /// The header of the file at `id` as one flat JSON object: the universal fields, each link field, the unresolved
    /// links, every other field, then a design's kind and scope, which come from its path.
    pub fn to_json(&self, id: &Id) -> serde_json::Map<String, Value> {
        let mut m = serde_json::Map::new();
        m.insert("kind".into(), self.kind.as_str().into());
        if let Some(t) = &self.title {
            m.insert("title".into(), t.clone().into());
        }
        if let Some(s) = &self.status {
            m.insert("status".into(), s.clone().into());
        }
        if let Some(u) = &self.under {
            m.insert("under".into(), u.as_str().into());
        }
        if !self.aliases.is_empty() {
            m.insert("aliases".into(), self.aliases.clone().into());
        }
        for (k, ids) in &self.links {
            // A record's `→` field holds one id, so it reads as one, as it was written; a list field stays a list.
            let value = match ids.as_slice() {
                [] if self.kind.is_record() => continue,
                [one] if self.kind.is_record() && !self.kind.record_list(k) => one.as_str().into(),
                _ => ids.iter().map(|i| Value::from(i.as_str())).collect(),
            };
            m.insert(k.clone(), value);
        }
        if !self.unresolved.is_empty() {
            let rows =
                self.unresolved.iter().map(|u| serde_json::json!({ "field": u.field, "text": u.text })).collect();
            m.insert("unresolved".into(), Value::Array(rows));
        }
        for (k, v) in &self.fields {
            m.insert(k.clone(), v.clone());
        }
        if let Some(level) = DesignLevel::of(id) {
            m.insert("design_kind".into(), level.design_kind.into());
            m.insert("scope".into(), level.scope.into());
        }
        m
    }
}

/// A scalar or a sequence of text; `None` when an item is not text.
/// The text items of the field `key` in `yaml`, in order; empty when the field is missing or holds no text list.
pub(crate) fn items(yaml: &str, key: &str) -> Vec<String> {
    let docs = YamlLoader::load_from_str(yaml).unwrap_or_default();
    let Some(Yaml::Hash(map)) = docs.first() else { return Vec::new() };
    map.get(&Yaml::String(key.into())).and_then(texts).unwrap_or_default()
}

fn texts(value: &Yaml) -> Option<Vec<String>> {
    match value {
        Yaml::String(s) => Some(vec![s.clone()]),
        Yaml::Array(items) => items.iter().map(|i| i.as_str().map(str::to_string)).collect(),
        Yaml::Null => Some(Vec::new()),
        _ => None,
    }
}

fn json(value: &Yaml) -> Value {
    match value {
        Yaml::Real(r) => r.parse::<f64>().map_or_else(|_| Value::String(r.clone()), |f| serde_json::json!(f)),
        Yaml::Integer(i) => Value::from(*i),
        Yaml::String(s) => Value::String(s.clone()),
        Yaml::Boolean(b) => Value::Bool(*b),
        Yaml::Array(items) => Value::Array(items.iter().map(json).collect()),
        // Keys of a nested map are text in every schema; a non-text key keeps its debug form rather than vanishing.
        Yaml::Hash(map) => Value::Object(
            map.iter().map(|(k, v)| (k.as_str().map_or_else(|| format!("{k:?}"), str::to_string), json(v))).collect(),
        ),
        Yaml::Alias(_) | Yaml::Null | Yaml::BadValue => Value::Null,
    }
}

/// A YAML error as a reader of the fix wants it. yaml-rust2 names a duplicated key by its Rust debug form
/// (`String("title")`), which the reader should not have to decode.
fn yaml_error(e: &ScanError) -> String {
    let info = e.info();
    let info = info
        .strip_suffix(": duplicated key in mapping")
        .map_or_else(|| info.to_string(), |key| format!("the key `{}` appears twice", undebug(key)));
    // The header's YAML starts on the file's second line, after the opening `---`; the reader edits the file.
    format!("{info} at line {} of the file", e.marker().line() + 1)
}

/// A YAML value's debug form without the variant: `String("title")` is `title`, `Integer(1)` is `1`.
fn undebug(text: &str) -> &str {
    text.split_once('(').and_then(|(_, inner)| inner.strip_suffix(')')).map_or(text, |inner| inner.trim_matches('"'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_universal_fields_links_and_the_rest() {
        let h = Header::parse(
            "kind: issue\ntitle: T\nstatus: open\nunder: projects/bp/milestones/m\ncites: [\"projects/a/designs/b\", \"[[milestone.anvil.x]]\"]\ndepends: projects/a/issues/e\nseverity: high\ntags: [\"domain/x\"]\ncost_diff: 53\n",
        )
        .unwrap();
        assert_eq!(h.kind, Kind::Issue);
        assert_eq!(h.status.as_deref(), Some("open"));
        assert_eq!(h.under.as_ref().unwrap().as_str(), "projects/bp/milestones/m");
        assert_eq!(h.links["cites"].len(), 1);
        assert_eq!(h.unresolved, [Unresolved { field: "cites".into(), text: "[[milestone.anvil.x]]".into() }]);
        assert_eq!(h.links["depends"][0].as_str(), "projects/a/issues/e");
        assert_eq!(h.fields["cost_diff"], 53);
        let j = h.to_json(&Id::parse("projects/bp/issues/x").unwrap());
        assert_eq!(j["tags"][0], "domain/x");
        assert_eq!(j["unresolved"][0]["field"], "cites");
        assert!(!j.contains_key("design_kind"));
    }

    #[test]
    fn a_design_reads_its_kind_and_scope_from_its_path() {
        let h = Header::parse("kind: design\ntitle: T\nstatus: draft\n").unwrap();
        let j = h.to_json(&Id::parse("projects/bp/designs/data-model").unwrap());
        assert_eq!((j["design_kind"].as_str(), j["scope"].as_str()), (Some("data-model"), Some("system")));
    }

    #[test]
    fn a_legacy_under_is_unresolved_not_an_id() {
        let h = Header::parse("kind: issue\ntitle: T\nunder: '[[milestone.anvil.x]]'\n").unwrap();
        assert_eq!(h.under, None);
        assert_eq!(h.unresolved[0].field, "under");
    }

    #[test]
    fn refuses_what_breaks_the_schema() {
        for (yaml, why) in [
            ("title: T\n", "`kind` is missing"),
            ("- a\n", "not a map"),
            ("kind: issue\ntitle: A\ntitle: B\n", "the key `title` appears twice at line 4 of the file"),
            ("kind: session\n", "not a Kind value"),
            ("kind: issue\nstatus: 1\n", "`status` is not text"),
            ("kind: issue\ncites: [1]\n", "non-text item"),
            ("kind: issue\nbody: x\n", "reserved"),
            ("kind: design\ntitle: T\ndesign_kind: system\n", "comes from the path"),
            ("kind: design\ntitle: T\nscope: system\n", "comes from the path"),
            ("kind: issue\ntitle: T\nscope: system\n", "`scope` is not a field of an issue; delete the line"),
            ("kind: milestone\ntitle: T\ndesign_kind: system\n", "`design_kind` is not a field of a milestone"),
            ("kind: delta\nstate: accepted\n", "derives from its closing append"),
        ] {
            let e = Header::parse(yaml).unwrap_err();
            assert!(e.why.contains(why), "{yaml}: {}", e.why);
        }
    }
}
