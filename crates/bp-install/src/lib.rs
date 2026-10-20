//! Installer: harness dialects, the embedded bundle, version comparison, precondition tests. `install` validates the
//! whole bundle against one target's surface spec, then writes it at the top of the clone's main worktree with a
//! manifest of content hashes. `check` holds the installed files to that manifest before a walk reads the vault (17;
//! 27, design changes 1 and 2). The install is local: it calls no forge and writes nothing outside the clone.

mod spec;
mod validate;

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::Path;

use bp_vault::{Code, Kind, Refusal};
use serde_json::{Map, Value, json};

use crate::spec::Spec;

/// Every file under `bundle/`, by its path from there, in path order (23).
pub static BUNDLE: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/bundle.rs"));

/// The version a manifest records; a walk under another refuses (15, check 6).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A node kind's body template, from `bundle/templates/`; `None` for a record kind, which has none. The binary
/// embeds them, so `create --template` needs no install, and `install` does not copy them (27, design change 7).
pub fn template(kind: Kind) -> Option<&'static str> {
    let path = format!("templates/{kind}.md");
    BUNDLE.iter().find(|(p, _)| *p == path).map(|(_, b)| std::str::from_utf8(b).expect("a template is UTF-8"))
}

bp_vault::closed_enum! {
    /// The harnesses `install` writes for: Claude Code (verified, 17) and Codex (documented only, 27).
    pub enum Target {
        Claude = "claude",
        Codex = "codex",
    }
}

const BEGIN: &str = "<!-- bp:begin -->";
const END: &str = "<!-- bp:end -->";

/// Installs the bundle for `target` at `top`: `(true, result)` for a write, `(false, result)` when the clone already
/// holds this bundle, so the verb exits 3.
pub fn install(top: &Path, target: Target) -> Result<(bool, Value), Refusal> {
    let spec = Spec::of(target);
    validate::validate(BUNDLE, spec)?;
    let (files, skipped) = plan(spec);
    let mut entries: BTreeMap<String, String> = files.iter().map(|(p, b)| (p.clone(), hash(b))).collect();
    let section = format!("{BEGIN}\n{}{END}\n", text_of("always-on.md"));
    let always_on = top.join(spec.always_on);
    let held = read(&always_on)?;
    entries.insert(spec.section_entry(), hash(section.as_bytes()));
    let settings_path = top.join(spec.settings);
    let settings = match read(&settings_path)? {
        Some(t) => serde_json::from_str(&t).ok().filter(Value::is_object),
        None => Some(json!({})),
    };
    let settings = settings.and_then(|s| merged(s, spec)).ok_or_else(|| {
        Refusal::failed(spec.settings, format!("{} is not a JSON object with an object `hooks`; fix it", spec.settings))
    })?;
    entries.insert(spec.hooks_entry(), hash(&ours(&settings, spec)));
    let manifest_rel = spec.manifest();
    let manifest_path = top.join(&manifest_rel);
    let manifest = json!({ "tool_version": VERSION, "target": target, "files": entries });
    let manifest = pretty(&manifest);
    let result = json!({ "target": target, "manifest": manifest_rel, "files": entries, "skipped": skipped });
    let old = read(&manifest_path)?;
    if old.as_deref() == Some(manifest.as_str()) && check_target(top, spec).is_ok() {
        return Ok((false, result));
    }
    // A file an older bundle held and this one does not goes, so no stale `bp-` entry stays behind.
    let old: Value = old.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    for gone in old["files"].as_object().into_iter().flat_map(Map::keys).filter(|p| !entries.contains_key(*p)) {
        if !gone.contains('#') {
            let path = top.join(gone);
            let _ = std::fs::remove_file(&path);
            let _ = path.parent().map(std::fs::remove_dir);
        }
    }
    for (rel, bytes) in &files {
        put(&top.join(rel), bytes)?;
    }
    put(&always_on, with_section(held.as_deref(), &section).as_bytes())?;
    put(&settings_path, pretty(&settings).as_bytes())?;
    put(&manifest_path, manifest.as_bytes())?;
    let mut local: Vec<&str> = files.keys().map(String::as_str).collect();
    local.extend([spec.settings, &manifest_rel]);
    // The one tracked edit is the section; an always-on file the install created is the install's own.
    if held.is_none() {
        local.push(spec.always_on);
    }
    exclude(top, &local)?;
    Ok((true, result))
}

/// Refuses `bundle_mismatch` when an installed bundle at `top` drifted from its manifest: another tool version, an
/// entry absent or with another hash, or a `bp-` entry with no manifest. A clone with no `bp-` entry is not checked.
pub fn check(top: &Path) -> Result<(), Refusal> {
    check_target(top, &spec::CLAUDE)?;
    check_target(top, &spec::CODEX)
}

fn check_target(top: &Path, spec: &Spec) -> Result<(), Refusal> {
    let refuse = |subject: String, why: String| {
        Refusal::new(
            Code::BundleMismatch,
            subject,
            "the installed bundle is the tool's version and hashes to its manifest (17, check 7)",
            format!("{why}; bp install --target {}", spec.target),
        )
    };
    let Ok(text) = std::fs::read_to_string(top.join(spec.manifest())) else {
        let bare = [spec.skills, spec.agents, spec.hooks].into_iter().find(|d| {
            std::fs::read_dir(top.join(d))
                .is_ok_and(|mut e| e.any(|e| e.is_ok_and(|e| e.file_name().to_string_lossy().starts_with("bp-"))))
        });
        return bare.map_or(Ok(()), |d| Err(refuse(d.into(), format!("{d} holds a bp- entry and no bundle.json"))));
    };
    let manifest: Value = serde_json::from_str(&text).unwrap_or_default();
    if manifest["tool_version"] != VERSION {
        return Err(refuse(spec.manifest(), format!("the bundle is {}, the tool {VERSION}", manifest["tool_version"])));
    }
    for (entry, want) in manifest["files"].as_object().into_iter().flatten() {
        if current(top, spec, entry).as_deref() != want.as_str() {
            return Err(refuse(entry.clone(), format!("{entry} is absent or does not hash to the manifest")));
        }
    }
    Ok(())
}

/// The hash of one manifest entry as the clone holds it now.
fn current(top: &Path, spec: &Spec, entry: &str) -> Option<String> {
    if entry == spec.section_entry() {
        let text = std::fs::read_to_string(top.join(spec.always_on)).ok()?;
        return Some(hash(text[span(&text)?].as_bytes()));
    }
    if entry == spec.hooks_entry() {
        let settings = serde_json::from_slice(&std::fs::read(top.join(spec.settings)).ok()?).ok()?;
        return Some(hash(&ours(&settings, spec)));
    }
    std::fs::read(top.join(entry)).ok().map(|b| hash(&b))
}

/// The installed files of `spec`'s target, path to bytes, and the bundle entries the target does not install: the
/// body templates, which `create --template` reads (27, design change 7).
fn plan(spec: &Spec) -> (BTreeMap<String, Vec<u8>>, Vec<String>) {
    let (mut files, mut skipped) = (BTreeMap::new(), Vec::new());
    for (path, bytes) in BUNDLE {
        let (dir, rest) = path.split_once('/').unwrap_or(("", path));
        let placed = match dir {
            "skills" => Some((format!("{}/{rest}", spec.skills), bytes.to_vec())),
            // A role's templates sit in its directory; the harness reads none, so they install apart, flat.
            "roles" if rest.contains('/') => {
                Some((format!("{}/{}", spec.dispatch, rest.replace('/', "-")), bytes.to_vec()))
            }
            "roles" if spec.target == Target::Codex => {
                let stem = rest.trim_end_matches(".md");
                Some((format!("{}/{stem}.toml", spec.agents), codex_role(stem, &String::from_utf8_lossy(bytes))))
            }
            "roles" => Some((format!("{}/{rest}", spec.agents), bytes.to_vec())),
            "hooks" if spec.events.iter().any(|h| h.script == rest) => {
                Some((format!("{}/{rest}", spec.hooks), bytes.to_vec()))
            }
            _ => None,
        };
        match placed {
            Some((to, b)) => drop(files.insert(to, b)),
            None if *path != "always-on.md" => skipped.push((*path).to_string()),
            None => {}
        }
    }
    (files, skipped)
}

/// A Codex role from the role's markdown: Codex reads TOML with no tool list and no preload. Its
/// `shell_environment_policy` sets `BP_ROLE`, which Codex does not document for a subagent (27, design change 3).
fn codex_role(stem: &str, text: &str) -> Vec<u8> {
    let f = validate::front(text).expect("the validator passed the role");
    // A JSON string is a TOML basic string: the same quotes and escapes.
    let q = |s: &str| serde_json::to_string(s.trim()).expect("a string serialises");
    let role = stem.strip_prefix("bp-").unwrap_or(stem);
    format!(
        "name = {}\ndescription = {}\ndeveloper_instructions = {}\n\n[shell_environment_policy]\nset = {{ BP_ROLE = {} }}\n",
        q(stem),
        q(f.text("description").unwrap_or_default()),
        q(f.body),
        q(role)
    )
    .into_bytes()
}

fn text_of(path: &str) -> String {
    let bytes = BUNDLE.iter().find(|(p, _)| *p == path).map_or(&b""[..], |(_, b)| b);
    let text = String::from_utf8_lossy(bytes).into_owned();
    if text.ends_with('\n') { text } else { text + "\n" }
}

/// The section's bytes in an always-on file: the begin line through the end line and its line end.
fn span(text: &str) -> Option<Range<usize>> {
    let begin = format!("{BEGIN}\n");
    let start = if text.starts_with(&begin) { 0 } else { text.find(&format!("\n{begin}"))? + 1 };
    let end = start + text[start..].find(&format!("\n{END}"))? + 1 + END.len();
    Some(start..if text[end..].starts_with('\n') { end + 1 } else { end })
}

/// The always-on file with `section` in place of the old one, or appended after a blank line.
fn with_section(held: Option<&str>, section: &str) -> String {
    match held {
        None => section.to_string(),
        Some(text) => match span(text) {
            Some(r) => format!("{}{section}{}", &text[..r.start], &text[r.end..]),
            None if text.is_empty() => section.to_string(),
            None => format!("{}\n{section}", if text.ends_with('\n') { text.to_string() } else { format!("{text}\n") }),
        },
    }
}

/// A matcher group is Blueprint's when one of its commands runs a `bp-` script from the target's hook directory.
fn is_ours(group: &Value, spec: &Spec) -> bool {
    let mark = format!("{}/bp-", spec.hooks);
    group["hooks"]
        .as_array()
        .is_some_and(|h| h.iter().any(|h| h["command"].as_str().is_some_and(|c| c.contains(&mark))))
}

/// Blueprint's hook entries alone, by event: what the `#hooks` entry hashes, so the user's own entries may change.
fn ours(settings: &Value, spec: &Spec) -> Vec<u8> {
    let mut out = Map::new();
    for (event, groups) in settings["hooks"].as_object().into_iter().flatten() {
        let mine: Vec<&Value> = groups.as_array().into_iter().flatten().filter(|g| is_ours(g, spec)).collect();
        if !mine.is_empty() {
            out.insert(event.clone(), json!(mine));
        }
    }
    serde_json::to_vec(&out).expect("JSON serialises")
}

/// The settings with Blueprint's entries replaced by the spec's, the user's kept; `None` when `hooks` is not an
/// object.
fn merged(mut settings: Value, spec: &Spec) -> Option<Value> {
    let hooks = settings.as_object_mut()?.entry("hooks").or_insert_with(|| json!({})).as_object_mut()?;
    for groups in hooks.values_mut() {
        if let Some(g) = groups.as_array_mut() {
            g.retain(|g| !is_ours(g, spec));
        }
    }
    hooks.retain(|_, g| g.as_array().is_none_or(|g| !g.is_empty()));
    for h in spec.events {
        let mut group = json!({ "hooks": [{ "type": "command", "command": spec.command(h.script) }] });
        if let Some(m) = h.matcher {
            group["matcher"] = m.into();
        }
        let groups = hooks.entry(h.event).or_insert_with(|| json!([]));
        groups.as_array_mut()?.push(group);
    }
    Some(settings)
}

/// Adds each installed path to `.git/info/exclude`, so the install stays local and `git status` stays clean.
fn exclude(top: &Path, paths: &[&str]) -> Result<(), Refusal> {
    let path = top.join(".git/info/exclude");
    let mut text = read(&path)?.unwrap_or_default();
    let held: Vec<String> = text.lines().map(str::to_string).collect();
    let missing: Vec<String> = paths.iter().map(|p| format!("/{p}")).filter(|l| !held.contains(l)).collect();
    if missing.is_empty() {
        return Ok(());
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&(missing.join("\n") + "\n"));
    put(&path, text.as_bytes())
}

fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).expect("JSON serialises") + "\n"
}

fn read(path: &Path) -> Result<Option<String>, Refusal> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Refusal::failed(path.display().to_string(), e.to_string())),
    }
}

/// Writes one file, its directories first; a script is executable, as the harness runs it by path.
fn put(path: &Path, bytes: &[u8]) -> Result<(), Refusal> {
    let failed = |e: std::io::Error| Refusal::failed(path.display().to_string(), e.to_string());
    std::fs::create_dir_all(path.parent().expect("an installed path has a parent")).map_err(failed)?;
    std::fs::write(path, bytes).map_err(failed)?;
    if path.extension().is_some_and(|e| e == "sh") {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).map_err(failed)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_codex_role_is_toml_that_sets_its_role() {
        let md = "---\nname: bp-worker\ndescription: Says \"x\".\nskills: bp-a\n---\n\nLine one.\nLine two.\n";
        let toml = String::from_utf8(super::codex_role("bp-worker", md)).unwrap();
        let want = "name = \"bp-worker\"\ndescription = \"Says \\\"x\\\".\"\ndeveloper_instructions = \"Line one.\\nLine two.\"\n\n\
                    [shell_environment_policy]\nset = { BP_ROLE = \"worker\" }\n";
        assert_eq!(toml, want);
    }
}
