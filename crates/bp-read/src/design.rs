//! The Design page object (18, Design; 32, design change 8): `show` of one design with its level, the diagrams beside
//! it, the deltas proposed against it, its version in words and the outline of its body. The client renders the body
//! and inlines each diagram's SVG from [`crate::diagram`]; every word it shows past the body is built here.

use std::collections::BTreeMap;

use bp_vault::{Body, Code, DeltaState, DesignLevel, Id, Index, Kind, Refusal, Vault};
use serde::Serialize;
use ts_rs::TS;

use crate::inputs::text;
use crate::milestone::{Concern, concerns, missing_kinds};
use crate::pages::{accepted, versions};
use crate::{Show, show};

/// One design's page.
#[derive(Serialize, TS)]
#[ts(optional_fields)]
pub struct Design {
    /// The `show` object: the header, the lead, the section headings and the body.
    #[serde(flatten)]
    pub show: Show,
    /// The design's kind from its path: `product`, `system`, `data-model`, `glossary`, `container` or `component`.
    pub level: String,
    /// The text of its `## TL;DR` section; absent when it has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tldr: Option<String>,
    /// The rendered diagrams in its `diagrams/` directory, by name.
    pub diagrams: Vec<Diagram>,
    /// The delta records under it, in their order.
    pub deltas: Vec<DesignDelta>,
    /// "accepted at <tag>" when the newest version holding its current hash exists, else "changed since <tag>" with
    /// the project's newest tag, else "no version".
    pub version: String,
    /// The `## ` and `### ` headings of the body outside code, each with the anchor the client gives it.
    pub outline: Vec<Heading>,
    /// Coverage per concern (18, Design): the product design's only, one row per concern's latest answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<Vec<Concern>>,
    /// The record kinds a row of the page reads that the vault holds none of: those rows read "not in vault".
    pub rows_not_in_vault: Vec<String>,
}

/// A rendered diagram of the design (28, design change 12).
#[derive(Serialize, TS)]
#[ts(optional_fields)]
pub struct Diagram {
    /// The name: the page is `<design>/diagrams/<name>.html`.
    pub name: String,
    /// The page's `<title>`; the name when it has none.
    pub title: String,
    /// The spec's `type`: from `<name>.yaml`, or a converted `<name>.nodes.yaml`; absent with no spec.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// The page's path in the vault.
    pub html_path: String,
}

/// A delta record against the design.
#[derive(Serialize, TS)]
#[ts(optional_fields)]
pub struct DesignDelta {
    /// The record's id.
    pub id: String,
    /// `proposed`, `accepted` or `rejected`.
    pub state: String,
    /// The issue that caused it; an editorial delta has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
}

/// One heading of the outline.
#[derive(Serialize, TS)]
pub struct Heading {
    /// 2 for `## `, 3 for `### `.
    pub depth: u8,
    /// The heading as written, without its marks.
    pub heading: String,
    /// The element id: the heading in lower case, each run of other characters one `-`, then `-2`, `-3` on a repeat.
    pub anchor: String,
}

/// The Design page of `id` in `project`. `at` names a version tag, which this build does not read yet: the page is
/// the current body and its `version` says so. An id that names no design of the project refuses `no_node`.
pub fn design(vault: &Vault, index: &Index, project: &str, id: &str, at: Option<&str>) -> Result<Design, Refusal> {
    let (id, level) = design_id(index, project, id)?;
    let level = level.design_kind;
    let show = show(vault, Some(index), &id, None, None, false)?;
    let body = Body::parse(show.body.as_deref().unwrap_or_default());
    let versions = versions(index, project)?;
    let mut version = match (accepted(vault, id.as_str(), &versions)?, versions.first()) {
        (Some(tag), _) => format!("accepted at {tag}"),
        (None, Some((_, tag, _))) => format!("changed since {tag}"),
        (None, None) => "no version".to_string(),
    };
    if let Some(tag) = at {
        version = format!("{version}; the body at {tag} is not read yet, so this is the current body");
    }
    let deltas = index
        .records(id.as_str(), Kind::Delta)?
        .into_iter()
        .map(|d| DesignDelta {
            state: DeltaState::of(&Body::parse(&d.body)).as_str().to_string(),
            cause: text(&d.header, "cause"),
            id: d.id,
        })
        .collect();
    let product = level == "product";
    let mut kinds = vec![Kind::Delta];
    if product {
        kinds.push(Kind::ConcernAnswer);
    }
    Ok(Design {
        level: level.to_string(),
        tldr: body.sections_named("TL;DR").first().map(|s| s.text.clone()),
        diagrams: pages(vault, id.as_str()).into_iter().map(|name| diagram(vault, &id, name)).collect(),
        deltas,
        version,
        outline: outline(&body),
        coverage: product.then(|| concerns(index, project)).transpose()?,
        rows_not_in_vault: missing_kinds(index, &kinds)?,
        show,
    })
}

/// The id `id` names when it is a design of `project`, else the refusal of its page.
pub(crate) fn design_id(index: &Index, project: &str, id: &str) -> Result<(Id, DesignLevel), Refusal> {
    let id = crate::subject(index, id)?;
    if let Some(level) = DesignLevel::of(&id).filter(|_| id.project() == Some(project))
        && index.header(id.as_str())?.is_some()
    {
        return Ok((id, level));
    }
    Err(Refusal::new(
        Code::NoNode,
        id.as_str(),
        "a design page shows one design of its project: /<project>/design/projects/<project>/designs/<name>",
        format!("bp list design --project {project}"),
    ))
}

/// The names of the rendered pages in the design's `diagrams/` directory, sorted: each `<name>.html`. The home's
/// diagram count is their number.
pub(crate) fn pages(vault: &Vault, id: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(vault.root.join(id).join("diagrams")) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".html").map(str::to_string))
        .collect();
    names.sort();
    names
}

/// The diagram `name` of the design `id`: its page's title and its spec's type.
fn diagram(vault: &Vault, id: &Id, name: String) -> Diagram {
    let dir = vault.root.join(id.as_str()).join("diagrams");
    let page = std::fs::read_to_string(dir.join(format!("{name}.html"))).unwrap_or_default();
    let title = page
        .split_once("<title>")
        .and_then(|(_, t)| t.split_once("</title>"))
        .map(|(t, _)| unescape(t.trim()))
        .filter(|t| !t.is_empty());
    // A converted diagram has no spec yet, only the node list the conversion kept beside its page.
    let spec = [format!("{name}.yaml"), format!("{name}.nodes.yaml")]
        .iter()
        .find_map(|f| std::fs::read_to_string(dir.join(f)).ok());
    let kind = spec.and_then(|s| {
        let line = s.lines().find_map(|l| l.strip_prefix("type:"))?;
        Some(line.trim().trim_matches(['"', '\'']).to_string()).filter(|t| !t.is_empty() && t != "null")
    });
    Diagram {
        html_path: format!("{id}/diagrams/{name}.html"),
        title: title.unwrap_or_else(|| name.clone()),
        kind,
        name,
    }
}

/// The text of an HTML title: the five entities `bp-diagram` and the converted pages escape.
fn unescape(t: &str) -> String {
    t.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&")
}

/// The body's `## ` and `### ` ATX headings, as CommonMark reads them: none inside code.
fn outline(body: &Body) -> Vec<Heading> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut out = Vec::new();
    for h in &body.headings {
        let depth = h.chars().take_while(|c| *c == '#').count();
        if !(2..=3).contains(&depth) || h.contains('\n') {
            continue;
        }
        let t = h[depth..].trim();
        // A closing `#` run, after a space, is not part of the heading.
        let closed = t.trim_end_matches('#');
        let heading = if closed.ends_with([' ', '\t']) { closed.trim() } else { t }.to_string();
        let anchor = anchor(&heading, &mut seen);
        out.push(Heading { depth: u8::try_from(depth).expect("2 or 3"), heading, anchor });
    }
    out
}

/// The heading's anchor, unique among those `seen`.
fn anchor(heading: &str, seen: &mut BTreeMap<String, usize>) -> String {
    let mut slug = String::new();
    for c in heading.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let base = match slug.trim_end_matches('-') {
        "" => "section".to_string(),
        s => s.to_string(),
    };
    let n = seen.entry(base.clone()).or_default();
    *n += 1;
    if *n == 1 { base } else { format!("{base}-{n}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_outline_takes_two_levels_outside_code_with_unique_anchors() {
        let body = Body::parse(
            "Lead.\n\n## TL;DR\n\nx\n\n### Deep `one` ##\n\n```\n## not a heading\n```\n\n#### Too deep\n\n## TL;DR\n",
        );
        let outline = outline(&body);
        let got: Vec<(u8, &str, &str)> =
            outline.iter().map(|h| (h.depth, h.heading.as_str(), h.anchor.as_str())).collect();
        assert_eq!(got, [(2, "TL;DR", "tl-dr"), (3, "Deep `one`", "deep-one"), (2, "TL;DR", "tl-dr-2")]);
    }
}

#[cfg(test)]
mod page_tests {
    use super::*;

    const BODY: &str = "Lead.\n\n## TL;DR\n\nShort.\n\n## Context\n\n*Diagram [`context`](/projects/bp/designs/system/\
                        diagrams/context.html).*\n\n### Inside\n\nText.\n";

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let page = "<html><head><title>Context &amp; scope</title><style>:root{--paper:#fff}body{margin:0}</style>\
                    </head><body><svg class=\"bp\" viewBox=\"0 0 10 10\"><g data-id=\"a\"></g></svg></body></html>";
        let files = [
            ("projects/bp/project.md", "---\nkind: project\ntitle: B\nstatus: active\n---\n".to_string()),
            ("shared/.keep", String::new()),
            ("projects/bp/designs/system.md", format!("---\nkind: design\ntitle: S\nstatus: active\n---\n\n{BODY}")),
            ("projects/bp/designs/system/diagrams/context.html", page.into()),
            ("projects/bp/designs/system/diagrams/context.yaml", "type: architecture\nnodes: []\n".into()),
            ("projects/bp/designs/system/diagrams/old.html", "<svg></svg>".into()),
            ("projects/bp/designs/system/diagrams/old.nodes.yaml", "type: \"sequence\"\nnodes:\n".into()),
            ("projects/bp/designs/product.md", "---\nkind: design\ntitle: P\nstatus: active\n---\n".into()),
        ];
        for (rel, text) in &files {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let vault = Vault { root: dir.path().to_path_buf() };
        (dir, vault)
    }

    #[test]
    fn the_design_page_lists_its_diagrams_and_outline_and_has_no_version() {
        let (_d, v) = vault();
        let index = Index::open(&v).unwrap();
        let d = design(&v, &index, "bp", "projects/bp/designs/system", None).unwrap();
        let diagrams: Vec<(&str, &str, Option<&str>)> =
            d.diagrams.iter().map(|g| (g.name.as_str(), g.title.as_str(), g.kind.as_deref())).collect();
        assert_eq!(diagrams, [("context", "Context & scope", Some("architecture")), ("old", "old", Some("sequence"))]);
        assert_eq!(d.diagrams[0].html_path, "projects/bp/designs/system/diagrams/context.html");
        let outline: Vec<(u8, &str)> = d.outline.iter().map(|h| (h.depth, h.anchor.as_str())).collect();
        assert_eq!(outline, [(2, "tl-dr"), (2, "context"), (3, "inside")]);
        assert_eq!((d.level.as_str(), d.version.as_str(), d.tldr.as_deref()), ("system", "no version", Some("Short.")));
        assert_eq!((d.coverage.is_none(), d.rows_not_in_vault.as_slice()), (true, ["delta".to_string()].as_slice()));
        let at = design(&v, &index, "bp", "projects/bp/designs/system", Some("blueprint/bp/1")).unwrap();
        assert!(at.version.starts_with("no version; the body at blueprint/bp/1 is not read yet"), "{}", at.version);
        let product = design(&v, &index, "bp", "projects/bp/designs/product", None).unwrap();
        assert_eq!(product.coverage.map(|c| c.len()), Some(0));
        assert_eq!(product.rows_not_in_vault, ["delta", "concern-answer"], "coverage reads not in the vault");
        for (project, id) in [("bp", "projects/bp/project"), ("two", "projects/bp/designs/system")] {
            assert_eq!(design(&v, &index, project, id, None).err().map(|r| r.code), Some(Code::NoNode), "{id}");
        }
    }

    #[test]
    fn the_diagram_route_gives_the_svg_of_a_listed_page_only() {
        let (_d, v) = vault();
        let index = Index::open(&v).unwrap();
        let svg = crate::diagram(&v, &index, "bp", "projects/bp/designs/system", "context").unwrap();
        assert_eq!(
            svg,
            "<svg class=\"bp\" viewBox=\"0 0 10 10\"><style>svg.bp{--paper:#fff}</style><g data-id=\"a\"></g></svg>"
        );
        for name in ["nope", "context.yaml", "../system"] {
            let refusal = crate::diagram(&v, &index, "bp", "projects/bp/designs/system", name).unwrap_err();
            assert_eq!(refusal.code, Code::NoNode, "{name}");
        }
    }
}
