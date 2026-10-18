//! The markdown body: the lead (text before the first `## ` heading), the `## ` sections, every wikilink outside a
//! fence, and every fenced `check` block kept as text with its attributes.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::Id;

/// A parsed body. The text is kept whole; sections and checks are views into its lines, copied out.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Body {
    /// The whole body, trimmed.
    pub text: String,
    /// The text before the first `## ` heading, trimmed.
    pub lead: String,
    /// The `## ` sections, in order.
    pub sections: Vec<Section>,
    /// Every wikilink outside a fence that is a path id, first occurrence only, in order.
    pub links: Vec<Id>,
    /// Every wikilink target outside a fence that is not a path id.
    pub unresolved: Vec<String>,
    /// Every ```` ```check ```` block, in order.
    pub checks: Vec<Check>,
}

/// One `## ` section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Section {
    /// The heading text after `## `.
    pub heading: String,
    /// The lines under the heading up to the next `## `, trimmed.
    pub text: String,
}

/// One fenced check block: ```` ```check <runner> {k=v ...} ````.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    /// The runner: `shell`, `scenario`, `deps`, and whatever the check crate adds.
    pub runner: String,
    /// The attributes inside the braces.
    pub attrs: BTreeMap<String, String>,
    /// The lines between the fences, as written.
    pub text: String,
}

impl Body {
    /// Scans the body. Headings and wikilinks inside a fence are text, not structure.
    pub fn parse(text: &str) -> Self {
        let mut body = Self { text: text.trim().to_string(), ..Self::default() };
        let mut fence: Option<String> = None;
        let mut check: Option<(Check, Vec<&str>)> = None;
        let mut current: Option<(String, Vec<&str>)> = None;
        let mut lead: Vec<&str> = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim_start();
            if let Some(open) = &fence {
                if trimmed.starts_with(open.as_str()) && trimmed.trim_end().chars().all(|c| c == '`' || c == '~') {
                    fence = None;
                    if let Some((mut c, lines)) = check.take() {
                        c.text = lines.join("\n");
                        body.checks.push(c);
                    }
                } else if let Some((_, lines)) = &mut check {
                    lines.push(line);
                }
            } else if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                let marker: String = trimmed.chars().take_while(|c| *c == '`' || *c == '~').collect();
                let info = trimmed[marker.len()..].trim();
                fence = Some(marker);
                if let Some(rest) = info.strip_prefix("check") {
                    check = Some((check_header(rest), Vec::new()));
                }
            } else if let Some(heading) = line.strip_prefix("## ") {
                if let Some((h, lines)) = current.take() {
                    body.sections.push(Section { heading: h, text: lines.join("\n").trim().to_string() });
                }
                current = Some((heading.trim().to_string(), Vec::new()));
                continue;
            }
            if fence.is_none() {
                for target in wikilinks(line) {
                    match Id::parse(target) {
                        Ok(id) if !body.links.contains(&id) => body.links.push(id),
                        Ok(_) => {}
                        Err(_) => body.unresolved.push(target.to_string()),
                    }
                }
            }
            match &mut current {
                Some((_, lines)) => lines.push(line),
                None => lead.push(line),
            }
        }
        if let Some((h, lines)) = current.take() {
            body.sections.push(Section { heading: h, text: lines.join("\n").trim().to_string() });
        }
        body.lead = lead.join("\n").trim().to_string();
        body
    }

    /// The section with this heading, matched exactly.
    pub fn section(&self, heading: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.heading == heading)
    }
}

/// `<runner> {k=v k=v}` after the word `check`.
fn check_header(rest: &str) -> Check {
    let (runner, attrs) = rest.split_once('{').unwrap_or((rest, ""));
    let attrs = attrs
        .trim_end_matches('}')
        .split_whitespace()
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    Check { runner: runner.trim().to_string(), attrs, text: String::new() }
}

/// Every `[[target]]`, `[[target|label]]` or `[[target#section]]` in one line, as written.
fn wikilinks(line: &str) -> impl Iterator<Item = &str> + '_ {
    line.match_indices("[[").filter_map(move |(start, _)| {
        let inner = &line[start + 2..];
        let end = inner.find("]]")?;
        let target = inner[..end].split(['|', '#']).next()?.trim();
        (!target.is_empty()).then_some(target)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "Lead with [[shared/a/b|label]] and [[shared/a/c#part]] and [[old.style]].\n\n## One\n\nText one [[shared/a/b]].\n\n```check shell {name=x expect=0}\n## not a heading [[not/a/link]]\nbp where\n```\n\n## Two\n\n### Sub\n\nText two.\n";

    #[test]
    fn scans_lead_sections_links_and_checks() {
        let b = Body::parse(TEXT);
        assert_eq!(b.lead, "Lead with [[shared/a/b|label]] and [[shared/a/c#part]] and [[old.style]].");
        let headings: Vec<&str> = b.sections.iter().map(|s| s.heading.as_str()).collect();
        assert_eq!(headings, ["One", "Two"]);
        assert!(b.section("One").unwrap().text.starts_with("Text one"));
        assert!(b.section("One").unwrap().text.ends_with("```"));
        assert_eq!(b.section("Two").unwrap().text, "### Sub\n\nText two.");
        let links: Vec<&str> = b.links.iter().map(Id::as_str).collect();
        assert_eq!(links, ["shared/a/b", "shared/a/c"]);
        assert_eq!(b.unresolved, ["old.style"]);
        assert_eq!(b.checks.len(), 1);
        assert_eq!(b.checks[0].runner, "shell");
        assert_eq!(b.checks[0].attrs["name"], "x");
        assert_eq!(b.checks[0].attrs["expect"], "0");
        assert_eq!(b.checks[0].text, "## not a heading [[not/a/link]]\nbp where");
    }

    #[test]
    fn a_body_without_headings_is_all_lead() {
        let b = Body::parse("\nJust text.\n");
        assert_eq!(b.lead, "Just text.");
        assert!(b.sections.is_empty());
        assert_eq!(b.text, "Just text.");
    }
}
