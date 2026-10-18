//! The markdown body: the lead (text before the first `## ` heading), the `## ` sections, every wikilink outside
//! code, and every fenced `check` block with its attributes. Block structure comes from pulldown-cmark, so fences,
//! block quotes and HTML blocks follow CommonMark (23); wikilinks and check attributes are our own pass over it.

use std::collections::BTreeMap;
use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Parser, Tag, TagEnd};
use serde::Serialize;

use crate::Id;

/// A parsed body.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Body {
    /// The whole body, trimmed.
    pub text: String,
    /// The text before the first `## ` heading, trimmed.
    pub lead: String,
    /// The `## ` sections, in order.
    pub sections: Vec<Section>,
    /// Every wikilink outside code that is a path id, first occurrence only, in order.
    pub links: Vec<Id>,
    /// Every wikilink target outside code that is not a path id.
    pub unresolved: Vec<String>,
    /// Every fenced `check` block, in order.
    pub checks: Vec<Check>,
    /// What the body does that CommonMark allows but the vault should not: kept, not refused.
    pub findings: Vec<Finding>,
}

/// One `## ` section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Section {
    /// The heading text, without the `#` marks.
    pub heading: String,
    /// The text under the heading up to the next `## `, trimmed.
    pub text: String,
}

/// One fenced check block: ```` ```check <runner> {k=v ...} ````.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    /// The runner: `shell`, `scenario`, `deps`, and whatever the check crate adds.
    pub runner: String,
    /// The attributes inside the braces.
    pub attrs: BTreeMap<String, String>,
    /// The block's content, as written.
    pub text: String,
}

/// A body finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// `unclosed_fence`, `duplicate_heading` or `check_unnamed`.
    pub kind: &'static str,
    /// The heading, the fence's first line, or the check's runner.
    pub detail: String,
}

impl Body {
    /// Parses the body.
    pub fn parse(text: &str) -> Self {
        let mut body = Self { text: text.trim().to_string(), ..Self::default() };
        let mut headings: Vec<(String, Range<usize>)> = Vec::new();
        let mut code: Vec<Range<usize>> = Vec::new();
        let mut heading: Option<(String, Range<usize>)> = None;
        let mut check: Option<Check> = None;
        for (event, range) in Parser::new(text).into_offset_iter() {
            match event {
                // Only ATX headings: a `---` line under a paragraph would otherwise make a setext heading.
                Event::Start(Tag::Heading { level: HeadingLevel::H2, .. }) if text[range.clone()].starts_with('#') => {
                    heading = Some((String::new(), range));
                }
                Event::End(TagEnd::Heading(HeadingLevel::H2)) => {
                    if let Some(h) = heading.take() {
                        headings.push((h.0.trim().to_string(), h.1));
                    }
                }
                Event::Text(t) | Event::Code(t) if heading.is_some() => {
                    if let Some((h, _)) = &mut heading {
                        h.push_str(&t);
                    }
                    if matches!(text.get(range.clone()), Some(s) if s.starts_with('`')) {
                        code.push(range);
                    }
                }
                Event::Code(_) | Event::Html(_) | Event::InlineHtml(_) => code.push(range),
                Event::Start(Tag::CodeBlock(kind)) => {
                    if let CodeBlockKind::Fenced(info) = &kind {
                        if is_check(info) {
                            check = Some(check_header(info[5..].trim()));
                        }
                        if !closed(&text[range.clone()]) {
                            let first = text[range.clone()].lines().next().unwrap_or_default().trim().to_string();
                            body.findings.push(Finding { kind: "unclosed_fence", detail: first });
                        }
                    }
                    code.push(range);
                }
                Event::Text(t) if check.is_some() => {
                    if let Some(c) = &mut check {
                        c.text.push_str(&t);
                    }
                }
                Event::End(TagEnd::CodeBlock) => {
                    if let Some(mut c) = check.take() {
                        c.text = c.text.trim_end_matches('\n').to_string();
                        if !c.attrs.contains_key("name") {
                            body.findings.push(Finding { kind: "check_unnamed", detail: c.runner.clone() });
                        }
                        body.checks.push(c);
                    }
                }
                _ => {}
            }
        }
        body.lead = text[..headings.first().map_or(text.len(), |h| h.1.start)].trim().to_string();
        for (i, (h, range)) in headings.iter().enumerate() {
            let end = headings.get(i + 1).map_or(text.len(), |next| next.1.start);
            if headings[..i].iter().any(|(seen, _)| seen == h) {
                body.findings.push(Finding { kind: "duplicate_heading", detail: h.clone() });
            }
            body.sections.push(Section { heading: h.clone(), text: text[range.end..end].trim().to_string() });
        }
        for (target, at) in wikilinks(text) {
            if code.iter().any(|c| c.contains(&at)) {
                continue;
            }
            match Id::parse(target) {
                Ok(id) if !body.links.contains(&id) => body.links.push(id),
                Ok(_) => {}
                Err(_) => body.unresolved.push(target.to_string()),
            }
        }
        body
    }

    /// The sections with this heading, matched exactly. More than one is a duplicate heading.
    pub fn sections_named(&self, heading: &str) -> Vec<&Section> {
        self.sections.iter().filter(|s| s.heading == heading).collect()
    }
}

/// `check` alone or followed by whitespace: `checklist` and `checkout` are other languages.
fn is_check(info: &str) -> bool {
    info == "check" || info.strip_prefix("check").is_some_and(|r| r.starts_with(char::is_whitespace))
}

/// The block's last line closes the fence its first line opened: the same character, at least as many.
fn closed(block: &str) -> bool {
    let mut lines = block.trim_end_matches('\n').lines();
    let open = lines.next().unwrap_or_default().trim_start();
    let mark = open.chars().next().unwrap_or('`');
    let len = open.chars().take_while(|c| *c == mark).count();
    lines.next_back().is_some_and(|last| {
        let last = last.trim();
        last.chars().all(|c| c == mark) && last.chars().count() >= len
    })
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

/// Every `[[target]]`, `[[target|label]]` or `[[target#section]]`, as written, with its byte offset.
fn wikilinks(text: &str) -> impl Iterator<Item = (&str, usize)> + '_ {
    text.match_indices("[[").filter_map(move |(start, _)| {
        let inner = &text[start + 2..];
        let end = inner.find("]]").filter(|e| !inner[..*e].contains('\n'))?;
        let target = inner[..end].split(['|', '#']).next()?.trim();
        (!target.is_empty()).then_some((target, start))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "Lead with [[shared/a/b|label]] and [[shared/a/c#part]] and [[old.style]].\n\n## One\n\nText one [[shared/a/b]] and `[[shared/in/code]]`.\n\n```check shell {name=x expect=0}\n## not a heading [[shared/not/link]]\nbp where\n```\n\n## Two ##\n\n### Sub\n\nText two.\n";

    #[test]
    fn scans_lead_sections_links_and_checks() {
        let b = Body::parse(TEXT);
        assert_eq!(b.lead, "Lead with [[shared/a/b|label]] and [[shared/a/c#part]] and [[old.style]].");
        let headings: Vec<&str> = b.sections.iter().map(|s| s.heading.as_str()).collect();
        assert_eq!(headings, ["One", "Two"], "a closing ## run is not part of the heading");
        assert!(b.sections[0].text.starts_with("Text one") && b.sections[0].text.ends_with("```"));
        assert_eq!(b.sections[1].text, "### Sub\n\nText two.");
        let links: Vec<&str> = b.links.iter().map(Id::as_str).collect();
        assert_eq!(links, ["shared/a/b", "shared/a/c"], "links in code are not links");
        assert_eq!(b.unresolved, ["old.style"]);
        assert_eq!(b.checks.len(), 1);
        assert_eq!((b.checks[0].runner.as_str(), b.checks[0].attrs["name"].as_str()), ("shell", "x"));
        assert_eq!(b.checks[0].text, "## not a heading [[shared/not/link]]\nbp where");
        assert!(b.findings.is_empty(), "{:?}", b.findings);
    }

    #[test]
    fn fences_follow_commonmark() {
        // A four-space indent is an indented code block, not a fence.
        let b = Body::parse("## A\n\n    ```check shell {name=x}\n    y\n\n## B\n");
        assert!(b.checks.is_empty());
        assert_eq!(b.sections.len(), 2);
        // A longer fence holds a shorter one; tildes close only tildes.
        let b = Body::parse("````check shell {name=outer}\n```\n## inside\n```\n````\n\n~~~\n```\n~~~\n## After\n");
        assert_eq!(b.checks[0].text, "```\n## inside\n```");
        assert_eq!(b.sections.iter().map(|s| s.heading.as_str()).collect::<Vec<_>>(), ["After"]);
        // A fence inside a block quote is a fence.
        let b = Body::parse("> ```\n> [[shared/a/b]]\n> ```\n");
        assert!(b.links.is_empty());
        // An HTML comment hides what it holds.
        let b = Body::parse("<!--\n## Hidden\n-->\n\n## Shown\n");
        assert_eq!(b.sections.len(), 1);
    }

    #[test]
    fn a_setext_underline_is_not_a_section() {
        let b = Body::parse("Lead.\n\nA paragraph\n---\n\n## Real\n\nText.\n");
        assert_eq!(b.sections.iter().map(|s| s.heading.as_str()).collect::<Vec<_>>(), ["Real"]);
    }

    #[test]
    fn findings_name_what_the_vault_should_not_hold() {
        let b = Body::parse("## A\n\n```bash\nnever closed\n## B\n");
        assert_eq!(b.findings, [Finding { kind: "unclosed_fence", detail: "```bash".into() }]);
        assert_eq!(b.sections.len(), 1, "CommonMark runs an unclosed fence to the end");
        let b = Body::parse("## A\n\nx\n\n## A\n\ny\n");
        assert_eq!(b.findings[0].kind, "duplicate_heading");
        assert_eq!(b.sections_named("A").len(), 2);
        let b = Body::parse("```check shell {expect=0}\nx\n```\n\n```checklist\n- a\n```\n");
        assert_eq!(b.checks.len(), 1, "checklist is not a check block");
        assert_eq!(b.findings[0].kind, "check_unnamed");
    }

    #[test]
    fn a_body_without_headings_is_all_lead() {
        let b = Body::parse("\nJust text.\n");
        assert_eq!(b.lead, "Just text.");
        assert!(b.sections.is_empty());
    }
}
