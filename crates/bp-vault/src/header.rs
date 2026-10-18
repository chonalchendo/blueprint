//! The YAML header: the universal fields (`kind`, `title`, `status`, `under`, `aliases`), the closed link set, and
//! every other field as a plain value. Labels and record fields are not special to the substrate; the domain
//! crates read them from `fields`.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;
use yaml_rust2::{Yaml, YamlLoader};

use crate::{Id, Kind, Refusal};

/// The closed set of link fields on nodes (12, the kind schemas). Every value is a path id.
pub const LINKS: [&str; 7] = ["cites", "answers", "targets", "authorizes", "supersedes", "carries", "depends"];

/// Names a verb's result uses at the top level beside the header's fields, so no header may use them. The five
/// envelope names are among them: an agent tells a refusal from a result by `code`.
pub const RESERVED: [&str; 14] = [
    "id",
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
    /// Links by field, in the closed set only.
    pub links: BTreeMap<String, Vec<Id>>,
    /// Link and `under` values that are not path ids.
    pub unresolved: Vec<Unresolved>,
    /// Every other field: labels and record fields, as JSON values.
    pub fields: BTreeMap<String, Value>,
}

impl Header {
    /// Parses the YAML between the `---` lines. `subject` names the file in a refusal.
    pub fn parse(yaml: &str, subject: &str) -> Result<Self, Refusal> {
        let bad = |why: String| Refusal::schema(subject, why);
        let docs = YamlLoader::load_from_str(yaml).map_err(|e| bad(e.to_string()))?;
        let Some(Yaml::Hash(map)) = docs.first() else { return Err(bad("the header is not a map".into())) };
        let kind = map.get(&Yaml::String("kind".into())).ok_or_else(|| bad("`kind` is missing".into()))?;
        let kind = kind.as_str().ok_or_else(|| bad("`kind` is not text".into()))?;
        let mut header = Self {
            kind: kind.parse::<Kind>().map_err(|e| bad(e.to_string()))?,
            title: None,
            status: None,
            under: None,
            aliases: Vec::new(),
            links: BTreeMap::new(),
            unresolved: Vec::new(),
            fields: BTreeMap::new(),
        };
        for (key, value) in map {
            let Some(key) = key.as_str() else { return Err(bad(format!("the key {key:?} is not text"))) };
            let text = || value.as_str().map(str::to_string).ok_or_else(|| bad(format!("`{key}` is not text")));
            match key {
                "kind" => {}
                "title" => header.title = Some(text()?),
                "status" => header.status = Some(text()?),
                "under" => match Id::parse(&text()?) {
                    Ok(id) => header.under = Some(id),
                    Err(_) => header.unresolved.push(Unresolved { field: key.into(), text: text()? }),
                },
                "aliases" => {
                    header.aliases = texts(value).ok_or_else(|| bad("`aliases` holds a non-text item".into()))?;
                }
                k if LINKS.contains(&k) => {
                    let mut ids = Vec::new();
                    for t in texts(value).ok_or_else(|| bad(format!("`{k}` holds a non-text item")))? {
                        match Id::parse(&t) {
                            Ok(id) => ids.push(id),
                            Err(_) => header.unresolved.push(Unresolved { field: k.into(), text: t }),
                        }
                    }
                    header.links.insert(k.into(), ids);
                }
                k if RESERVED.contains(&k) => return Err(bad(format!("`{k}` is a reserved name"))),
                k => {
                    header.fields.insert(k.into(), json(value));
                }
            }
        }
        // A node is named and has a state; a record is evidence about its parent and has neither (13, rule 4).
        match (header.kind.is_record(), &header.title, &header.status) {
            (false, None, _) => Err(bad(format!("`kind: {}` is a node: it needs a `title`", header.kind))),
            (true, Some(_), _) | (true, _, Some(_)) => {
                Err(bad(format!("`kind: {}` is a record: it has no `title` or `status`", header.kind)))
            }
            _ => Ok(header),
        }
    }

    /// The header as one flat JSON object: the universal fields, each link field, the unresolved links, then every
    /// other field.
    pub fn to_json(&self) -> serde_json::Map<String, Value> {
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
            m.insert(k.clone(), ids.iter().map(|i| Value::from(i.as_str())).collect());
        }
        if !self.unresolved.is_empty() {
            let rows =
                self.unresolved.iter().map(|u| serde_json::json!({ "field": u.field, "text": u.text })).collect();
            m.insert("unresolved".into(), Value::Array(rows));
        }
        for (k, v) in &self.fields {
            m.insert(k.clone(), v.clone());
        }
        m
    }
}

/// A scalar or a sequence of text; `None` when an item is not text.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_universal_fields_links_and_the_rest() {
        let h = Header::parse(
            "kind: issue\ntitle: T\nstatus: open\nunder: projects/bp/milestones/m\ncites: [\"projects/a/designs/b\", \"[[milestone.anvil.x]]\"]\ndepends: projects/a/issues/e\nseverity: high\ntags: [\"domain/x\"]\ncost_diff: 53\n",
            "f",
        )
        .unwrap();
        assert_eq!(h.kind, Kind::Issue);
        assert_eq!(h.status.as_deref(), Some("open"));
        assert_eq!(h.under.as_ref().unwrap().as_str(), "projects/bp/milestones/m");
        assert_eq!(h.links["cites"].len(), 1);
        assert_eq!(h.unresolved, [Unresolved { field: "cites".into(), text: "[[milestone.anvil.x]]".into() }]);
        assert_eq!(h.links["depends"][0].as_str(), "projects/a/issues/e");
        assert_eq!(h.fields["cost_diff"], 53);
        let j = h.to_json();
        assert_eq!(j["tags"][0], "domain/x");
        assert_eq!(j["unresolved"][0]["field"], "cites");
    }

    #[test]
    fn a_legacy_under_is_unresolved_not_an_id() {
        let h = Header::parse("kind: issue\ntitle: T\nunder: '[[milestone.anvil.x]]'\n", "f").unwrap();
        assert_eq!(h.under, None);
        assert_eq!(h.unresolved[0].field, "under");
    }

    #[test]
    fn refuses_what_breaks_the_schema() {
        for (yaml, why) in [
            ("title: T\n", "`kind` is missing"),
            ("- a\n", "not a map"),
            ("kind: session\n", "not a Kind value"),
            ("kind: issue\nstatus: 1\n", "`status` is not text"),
            ("kind: issue\ncites: [1]\n", "non-text item"),
            ("kind: issue\nbody: x\n", "reserved"),
        ] {
            let e = Header::parse(yaml, "f").unwrap_err();
            assert_eq!(e.code, crate::Code::SchemaInvalid, "{yaml}");
            assert!(e.fix.contains(why), "{yaml}: {}", e.fix);
        }
    }
}
