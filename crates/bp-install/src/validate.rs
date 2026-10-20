//! The validator (17, An install; check 1): the whole bundle against one target's surface spec, refused on the first
//! file that fails, so `install` writes nothing. No harness refuses a bad file: three skip it silently (research).
//! Seven failures: a skill with no trigger, a name that differs from its directory, a body over the limit, a script
//! that does not ship, a role that preloads a skill the bundle lacks, a listing over budget (17), and a template
//! whose role does not preload its skill (27, design change 1).

use std::collections::{BTreeMap, BTreeSet};

use bp_vault::{Code, Refusal};
use yaml_rust2::{Yaml, YamlLoader};

use crate::spec::Spec;

/// A markdown file's frontmatter, as a harness reads it, and the body after it.
pub(crate) struct Front<'t> {
    map: yaml_rust2::yaml::Hash,
    pub(crate) body: &'t str,
}

impl Front<'_> {
    pub(crate) fn text(&self, key: &str) -> Option<&str> {
        self.map.get(&Yaml::String(key.into()))?.as_str()
    }

    /// A list field, as a YAML list or Claude Code's comma-separated string.
    fn list(&self, key: &str) -> Vec<String> {
        match self.map.get(&Yaml::String(key.into())) {
            Some(Yaml::Array(items)) => items.iter().filter_map(Yaml::as_str).map(str::to_string).collect(),
            Some(Yaml::String(s)) => s.split(',').map(|i| i.trim().to_string()).filter(|i| !i.is_empty()).collect(),
            _ => Vec::new(),
        }
    }
}

pub(crate) fn front(text: &str) -> Option<Front<'_>> {
    let rest = text.strip_prefix("---\n")?;
    let (yaml, body) = rest.split_once("\n---\n").or_else(|| Some((rest.strip_suffix("\n---")?, "")))?;
    let Some(Yaml::Hash(map)) = YamlLoader::load_from_str(yaml).ok()?.into_iter().next() else { return None };
    Some(Front { map, body })
}

fn invalid(path: &str, field: &str, why: &str) -> Refusal {
    Refusal::new(
        Code::BundleInvalid,
        format!("bundle/{path}: {field}"),
        "each bundle file holds its target's surface spec, and install writes nothing on the first that fails (17)",
        format!("{why}; fix bundle/{path}, then rebuild bp"),
    )
}

/// A skill or role name: `bp-` first, as entries share one namespace with the user's, and 1–64 lower-case letters,
/// digits or hyphens.
fn entry_name(name: &str) -> bool {
    name.starts_with("bp-")
        && name.len() <= 64
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Each `scripts/<file>` a body names.
fn scripts(body: &str) -> BTreeSet<String> {
    body.match_indices("scripts/")
        .map(|(at, _)| {
            let tail = &body[at..];
            let end = tail.find(|c: char| !(c.is_ascii_alphanumeric() || "._/-".contains(c))).unwrap_or(tail.len());
            tail[..end].trim_end_matches('.').to_string()
        })
        .collect()
}

pub(crate) fn validate(bundle: &[(&str, &[u8])], spec: &Spec) -> Result<(), Refusal> {
    let files: BTreeMap<&str, String> =
        bundle.iter().map(|(p, b)| (*p, String::from_utf8_lossy(b).into_owned())).collect();
    let skills: BTreeSet<&str> =
        files.keys().filter_map(|p| p.strip_prefix("skills/")?.split_once('/').map(|(d, _)| d)).collect();
    let mut listed = 0;
    for dir in &skills {
        let path = format!("skills/{dir}/SKILL.md");
        let text =
            files.get(path.as_str()).ok_or_else(|| invalid(&path, "SKILL.md", "a skill directory holds SKILL.md"))?;
        let f = front(text).ok_or_else(|| invalid(&path, "name", "the file opens with a frontmatter"))?;
        if f.text("name") != Some(dir) || !entry_name(dir) {
            return Err(invalid(
                &path,
                "name",
                &format!("the name equals its directory, {dir}, and is bp- and 1-64 of a-z, 0-9, -"),
            ));
        }
        let description = f.text("description").unwrap_or_default().trim().chars().count();
        if description == 0 || description > spec.description_max {
            let why =
                format!("the description is the trigger: 1 to {} characters, not {description}", spec.description_max);
            return Err(invalid(&path, "description", &why));
        }
        let lines = f.body.lines().count();
        if lines > spec.body_lines {
            return Err(invalid(&path, "body", &format!("the body is at most {} lines, not {lines}", spec.body_lines)));
        }
        if let Some(s) = scripts(f.body).into_iter().find(|s| !files.contains_key(format!("skills/{dir}/{s}").as_str()))
        {
            return Err(invalid(&path, &s, &format!("a script the body names ships in skills/{dir}/")));
        }
        listed += description;
        if listed > spec.listing {
            let why = format!(
                "the descriptions together fit the listing budget of {} characters, not {listed}",
                spec.listing
            );
            return Err(invalid(&path, "description", &why));
        }
    }
    let mut preloads: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (path, stem, text) in files
        .iter()
        .filter_map(|(p, t)| Some((p, p.strip_prefix("roles/")?.strip_suffix(".md")?, t)))
        .filter(|r| !r.1.contains('/'))
    {
        let f = front(text).filter(|f| f.text("name") == Some(stem) && entry_name(stem));
        let f =
            f.ok_or_else(|| invalid(path, "name", &format!("the name equals its file name, {stem}, and is bp-")))?;
        if f.text("description").is_none_or(|d| d.trim().is_empty()) {
            return Err(invalid(path, "description", "a role says when to dispatch it"));
        }
        let loaded = f.list("skills");
        if let Some(s) = loaded.iter().find(|s| !skills.contains(s.as_str())) {
            return Err(invalid(path, "skills", &format!("a preloaded skill is in the bundle, and {s} is not")));
        }
        preloads.insert(stem, loaded);
    }
    // A template is a file in its role's directory (17, Interfaces 3): `roles/bp-worker/complete.md`.
    for (path, role, text) in
        files.iter().filter_map(|(p, t)| Some((p, p.strip_prefix("roles/")?.split_once('/')?.0, t)))
    {
        let skill = text.lines().find_map(|l| l.strip_prefix("Skill:")).map(str::trim).unwrap_or_default();
        if skill.starts_with("bp-") && !preloads.get(role).is_some_and(|l| l.iter().any(|s| s == skill)) {
            return Err(invalid(path, "Skill", &format!("the role {role} preloads the template's skill, {skill}")));
        }
    }
    if let Some(h) = spec.events.iter().find(|h| !files.contains_key(format!("hooks/{}", h.script).as_str())) {
        return Err(invalid(&format!("hooks/{}", h.script), "script", &format!("a {} hook ships its script", h.event)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{CLAUDE, CODEX};

    const SKILL: &str = "---\nname: bp-a\ndescription: Use when a test runs.\n---\n\nRun `scripts/go.sh`.\n";

    /// A bundle that passes: one skill with its script, one role that preloads it, its template, and the hooks.
    fn good() -> BTreeMap<String, String> {
        let mut b: BTreeMap<String, String> = [
            ("skills/bp-a/SKILL.md", SKILL),
            ("skills/bp-a/scripts/go.sh", "true\n"),
            ("roles/bp-worker.md", "---\nname: bp-worker\ndescription: Builds.\nskills:\n  - bp-a\n---\n\nStop.\n"),
            ("roles/bp-worker/a.md", "Node: x\nSkill: bp-a\n"),
            ("always-on.md", "## Blueprint\n"),
        ]
        .into_iter()
        .map(|(p, t)| (p.to_string(), t.to_string()))
        .collect();
        for h in CLAUDE.events.iter().chain(CODEX.events) {
            b.insert(format!("hooks/{}", h.script), "#!/bin/sh\n".into());
        }
        b
    }

    /// The field a refusal names, or `ok`.
    fn verdict(b: &BTreeMap<String, String>) -> String {
        let bundle: Vec<(&str, &[u8])> = b.iter().map(|(p, t)| (p.as_str(), t.as_bytes())).collect();
        match validate(&bundle, &CLAUDE) {
            Ok(()) => "ok".into(),
            Err(r) => {
                assert_eq!((r.code, r.exit), (Code::BundleInvalid, 2), "{r:?}");
                r.subject
            }
        }
    }

    fn broken(path: &str, text: &str) -> String {
        let mut b = good();
        b.insert(path.into(), text.into());
        verdict(&b)
    }

    #[test]
    fn the_fixture_and_the_shipped_bundle_pass() {
        assert_eq!(verdict(&good()), "ok");
        validate(crate::BUNDLE, &CLAUDE).unwrap();
        validate(crate::BUNDLE, &CODEX).unwrap();
    }

    #[test]
    fn refuses_no_trigger() {
        assert_eq!(
            broken("skills/bp-a/SKILL.md", &SKILL.replace("Use when a test runs.", "\"\"")),
            "bundle/skills/bp-a/SKILL.md: description"
        );
    }

    #[test]
    fn refuses_name_not_directory() {
        assert_eq!(
            broken("skills/bp-a/SKILL.md", &SKILL.replace("name: bp-a", "name: bp-b")),
            "bundle/skills/bp-a/SKILL.md: name"
        );
    }

    #[test]
    fn refuses_body_over_limit() {
        let long = format!("{SKILL}{}", "line\n".repeat(500));
        assert_eq!(broken("skills/bp-a/SKILL.md", &long), "bundle/skills/bp-a/SKILL.md: body");
    }

    #[test]
    fn refuses_script_not_shipped() {
        let mut b = good();
        b.remove("skills/bp-a/scripts/go.sh");
        assert_eq!(verdict(&b), "bundle/skills/bp-a/SKILL.md: scripts/go.sh");
    }

    #[test]
    fn refuses_role_preloading_an_absent_skill() {
        let role = "---\nname: bp-worker\ndescription: Builds.\nskills: bp-a, bp-gone\n---\n";
        assert_eq!(broken("roles/bp-worker.md", role), "bundle/roles/bp-worker.md: skills");
    }

    #[test]
    fn refuses_listing_over_budget() {
        let mut b = good();
        let description = "d".repeat(1000);
        for n in 0..9 {
            b.insert(
                format!("skills/bp-{n}/SKILL.md"),
                format!("---\nname: bp-{n}\ndescription: {description}\n---\n"),
            );
        }
        assert_eq!(verdict(&b), "bundle/skills/bp-8/SKILL.md: description");
    }

    #[test]
    fn refuses_template_whose_role_lacks_its_skill() {
        let mut b = good();
        b.insert("skills/bp-c/SKILL.md".into(), SKILL.replace("bp-a", "bp-c").replace("`scripts/go.sh`", "it"));
        b.insert("roles/bp-worker/a.md".into(), "Node: x\nSkill: bp-c\n".into());
        assert_eq!(verdict(&b), "bundle/roles/bp-worker/a.md: Skill");
    }

    #[test]
    fn a_hook_the_target_registers_ships() {
        let mut b = good();
        b.remove("hooks/bp-guard.sh");
        assert_eq!(verdict(&b), "bundle/hooks/bp-guard.sh: script");
    }
}
