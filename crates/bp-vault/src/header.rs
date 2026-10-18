//! The YAML header: the universal fields (`kind`, `title`, `status`, `under`, `aliases`), the closed link set, and
//! every other field as a plain value. Labels and record fields are not special to the substrate; the domain
//! crates read them from `fields`.

use std::collections::BTreeMap;

use serde_json::Value;
use yaml_rust2::{Yaml, YamlLoader};

use crate::{Id, Refusal};

/// The closed set of link fields (13, cross-cutting rule 1). Every value is a path id.
pub const LINKS: [&str; 7] = ["cites", "answers", "targets", "authorizes", "supersedes", "carries", "depends"];

/// A parsed header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// The kind, for example `issue` or `claim`.
    pub kind: String,
    /// The title. Records have none.
    pub title: Option<String>,
    /// The status. Records have none.
    pub status: Option<String>,
    /// The parent node.
    pub under: Option<Id>,
    /// Old slugs a rename left behind.
    pub aliases: Vec<String>,
    /// Links by field, in the closed set only; each field present in the header.
    pub links: BTreeMap<String, Vec<Id>>,
    /// Every other field: labels and record fields, as JSON values.
    pub fields: BTreeMap<String, Value>,
}

impl Header {
    /// Parses the YAML between the `---` lines. `subject` names the file in a refusal.
    pub fn parse(yaml: &str, subject: &str) -> Result<Self, Refusal> {
        let bad = |why: &str| {
            Refusal::new(
                "bad_header",
                subject,
                "a header is a YAML map between --- lines with a kind",
                format!("fix the header: {why}"),
            )
        };
        let docs = YamlLoader::load_from_str(yaml).map_err(|e| bad(&e.to_string()))?;
        let map = match docs.first() {
            Some(Yaml::Hash(map)) => map,
            _ => return Err(bad("not a map")),
        };
        let mut header = Self {
            kind: String::new(),
            title: None,
            status: None,
            under: None,
            aliases: Vec::new(),
            links: BTreeMap::new(),
            fields: BTreeMap::new(),
        };
        for (key, value) in map {
            let Some(key) = key.as_str() else { return Err(bad("a key is not a string")) };
            match key {
                "kind" => header.kind = value.as_str().ok_or_else(|| bad("kind is not a string"))?.to_string(),
                "title" => header.title = value.as_str().map(str::to_string),
                "status" => header.status = value.as_str().map(str::to_string),
                "under" => header.under = value.as_str().map(Id::new),
                "aliases" => header.aliases = strings(value),
                k if LINKS.contains(&k) => {
                    header.links.insert(k.to_string(), strings(value).iter().map(|s| Id::new(s)).collect());
                }
                k => {
                    header.fields.insert(k.to_string(), json(value));
                }
            }
        }
        if header.kind.is_empty() {
            return Err(bad("kind is missing"));
        }
        Ok(header)
    }

    /// The header as one flat JSON object: the universal fields, then each link field, then every other field.
    pub fn to_json(&self) -> serde_json::Map<String, Value> {
        let mut m = serde_json::Map::new();
        m.insert("kind".into(), self.kind.clone().into());
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
            m.insert(k.clone(), ids.iter().map(|i| Value::from(i.as_str())).collect());
        }
        for (k, v) in &self.fields {
            m.insert(k.clone(), v.clone());
        }
        m
    }
}

/// A scalar or a sequence, as strings. Anything else is empty.
fn strings(value: &Yaml) -> Vec<String> {
    match value {
        Yaml::String(s) => vec![s.clone()],
        Yaml::Array(items) => items.iter().filter_map(Yaml::as_str).map(str::to_string).collect(),
        _ => Vec::new(),
    }
}

fn json(value: &Yaml) -> Value {
    match value {
        Yaml::Real(r) => r.parse::<f64>().map_or_else(|_| Value::String(r.clone()), |f| serde_json::json!(f)),
        Yaml::Integer(i) => Value::from(*i),
        Yaml::String(s) => Value::String(s.clone()),
        Yaml::Boolean(b) => Value::Bool(*b),
        Yaml::Array(items) => Value::Array(items.iter().map(json).collect()),
        Yaml::Hash(map) => {
            Value::Object(map.iter().map(|(k, v)| (k.as_str().unwrap_or("").to_string(), json(v))).collect())
        }
        Yaml::Alias(_) | Yaml::Null | Yaml::BadValue => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_universal_fields_links_and_the_rest() {
        let h = Header::parse(
            "kind: issue\ntitle: T\nstatus: open\nunder: '[[milestone.anvil.x]]'\ncites: [\"a/b\", \"c/d\"]\ndepends: a/e\nseverity: high\ntags: [\"domain/x\"]\ncost_diff: 53\n",
            "f",
        )
        .unwrap();
        assert_eq!(h.kind, "issue");
        assert_eq!(h.status.as_deref(), Some("open"));
        assert_eq!(h.under.as_ref().unwrap().as_str(), "milestone.anvil.x");
        assert_eq!(h.links["cites"].len(), 2);
        assert_eq!(h.links["depends"][0].as_str(), "a/e");
        assert_eq!(h.fields["severity"], "high");
        assert_eq!(h.fields["cost_diff"], 53);
        let j = h.to_json();
        assert_eq!(j["cites"][1], "c/d");
        assert_eq!(j["tags"][0], "domain/x");
        assert!(j.get("aliases").is_none());
    }

    #[test]
    fn refuses_a_header_without_a_kind() {
        let e = Header::parse("title: T\n", "f").unwrap_err();
        assert_eq!(e.code, "bad_header");
        assert!(e.fix.contains("kind is missing"));
        assert_eq!(Header::parse("- a\n", "f").unwrap_err().fix, "fix the header: not a map");
    }
}
