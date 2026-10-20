//! Installer: harness dialects, the embedded bundle, version comparison, precondition tests. `install` validates the
//! whole bundle against one target's surface spec, then writes it at the top of the clone's main worktree with a
//! manifest of content hashes. `check` holds the installed files to that manifest before a walk reads the vault (17;
//! 27, design changes 1 and 2). The install is local: it calls no forge and writes nothing outside the clone.

mod spec;
mod validate;

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::{Component, Path};

use bp_operation::Role;
use bp_vault::{Code, Kind, Refusal};
use serde_json::{Map, Value, json};

use crate::spec::Spec;

/// Every file under `bundle/`, by its path from there, in path order (23).
pub static BUNDLE: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/bundle.rs"));

/// The version a manifest records; a walk under another refuses (15, check 6).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A node kind's body template, from `bundle/templates/`; `None` for a record kind, which has none. The binary
/// embeds them, so `create --template` needs no install, and `install` does not copy them (27, design change 7).
///
/// # Panics
///
/// When a template is not UTF-8; the test `each_node_kind_has_a_text_template` holds the shipped bundle to it.
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
    let entries = entries(spec, &files);
    let always_on = top.join(spec.always_on);
    let held = read(&always_on)?;
    if let Some(text) = held.as_deref()
        && (text.matches(BEGIN).count(), text.matches(END).count())
            != if span(text).is_some() { (1, 1) } else { (0, 0) }
    {
        let fix = format!("{} holds an unpaired {BEGIN} or {END}; leave one pair or none, then rerun", spec.always_on);
        return Err(Refusal::failed(spec.always_on, fix));
    }
    let settings_path = top.join(spec.settings);
    let settings = match read(&settings_path)? {
        Some(t) => serde_json::from_str(&t).ok().filter(Value::is_object),
        None => Some(json!({})),
    };
    let settings = settings.and_then(|s| merged(s, spec)).ok_or_else(|| {
        Refusal::failed(spec.settings, format!("{} is not a JSON object with an object `hooks`; fix it", spec.settings))
    })?;
    let manifest_rel = spec.manifest();
    let manifest_path = top.join(&manifest_rel);
    let manifest = json!({ "tool_version": VERSION, "target": target, "files": entries });
    let manifest = pretty(&manifest);
    let result = json!({ "target": target, "manifest": manifest_rel, "files": entries, "skipped": skipped });
    let old = read(&manifest_path)?;
    if old.as_deref() == Some(manifest.as_str()) && check_target(top, spec).is_ok() {
        return Ok((false, result));
    }
    // A file an older bundle held and this one does not goes, so no stale `bp-` entry stays behind. The old manifest
    // is not trusted: a tracked one can name any path, so each must lie in one of the target's directories, inside
    // the clone, before anything goes.
    let old: Value = old.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let gone: Vec<&String> = old["files"]
        .as_object()
        .into_iter()
        .flat_map(Map::keys)
        .filter(|p| !p.contains('#') && !entries.contains_key(*p))
        .collect();
    let mut writes = files.keys().map(String::as_str).chain([spec.always_on, spec.settings, manifest_rel.as_str()]);
    let dirs = [spec.skills, spec.agents, spec.dispatch, spec.hooks];
    let removable = |p: &str| dirs.iter().any(|d| p.starts_with(&format!("{d}/"))) && inside(top, p);
    if let Some(out) =
        gone.iter().map(|p| p.as_str()).find(|p| !removable(p)).or_else(|| writes.find(|p| !inside(top, p)))
    {
        return Err(Refusal::new(
            Code::BundleInvalid,
            out.to_string(),
            "install removes only in its target's directories and writes only inside the clone's top: no absolute path, \
             no `..`, no symlink out (17)",
            format!(
                "{out} is not the install's to touch at {}; remove the manifest entry or the link, then rerun",
                top.display()
            ),
        ));
    }
    for gone in gone {
        let path = top.join(gone);
        let _ = std::fs::remove_file(&path);
        // The parent is inside the top, as the entry is; the top itself is never a parent here.
        if let Some(dir) = Path::new(gone).parent().filter(|d| !d.as_os_str().is_empty()) {
            let _ = std::fs::remove_dir(top.join(dir));
        }
    }
    for (rel, bytes) in &files {
        put(&top.join(rel), bytes)?;
    }
    put(&always_on, with_section(held.as_deref(), &section()).as_bytes())?;
    // `serde_json` holds no key order, so the rewrite sorts the user's keys; their values stay.
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

/// Whether `rel` names a path inside `top`: relative, with no `.` or `..` part, and no existing part of it a symlink
/// that resolves outside the top. A dangling symlink is outside, as a write through it lands where it points.
fn inside(top: &Path, rel: &str) -> bool {
    let path = Path::new(rel);
    if rel.is_empty() || !path.components().all(|c| matches!(c, Component::Normal(_))) {
        return false;
    }
    let Ok(root) = top.canonicalize() else { return false };
    path.ancestors()
        .filter(|a| !a.as_os_str().is_empty())
        .map(|a| top.join(a))
        .filter(|a| a.symlink_metadata().is_ok())
        .all(|a| a.canonicalize().is_ok_and(|a| a.starts_with(&root)))
}

/// The manifest entries for `spec`: each installed file's hash, then the section's and Blueprint's hook entries'.
/// `install` writes them, and `check` holds a manifest to them, so a bundle the binary no longer ships refuses even
/// under the same tool version (17, check 7).
fn entries(spec: &Spec, files: &BTreeMap<String, Vec<u8>>) -> BTreeMap<String, String> {
    let mut entries: BTreeMap<String, String> = files.iter().map(|(p, b)| (p.clone(), hash(b))).collect();
    entries.insert(spec.section_entry(), hash(section().as_bytes()));
    // `ours` keeps Blueprint's groups alone, so the user's settings do not change the entry.
    let hooks = merged(json!({}), spec).expect("an empty object merges");
    entries.insert(spec.hooks_entry(), hash(&ours(&hooks, spec)));
    entries
}

/// Blueprint's section of the always-on file, the markers included.
fn section() -> String {
    format!("{BEGIN}\n{}{END}\n", text_of("always-on.md"))
}

/// Refuses `bundle_mismatch` when an installed bundle at `top` drifted: another tool version, a manifest that is not
/// the bundle this binary ships, an entry absent or with another hash, or a `bp-` entry with no manifest. A clone with
/// no `bp-` entry is not checked.
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
    // The version alone cannot see a bundle that changed under one version, so the manifest is held to the bundle the
    // binary embeds, entry by entry.
    let held = manifest["files"].as_object().cloned().unwrap_or_default();
    let ships = entries(spec, &plan(spec).0);
    let names: BTreeSet<&String> = held.keys().chain(ships.keys()).collect();
    if let Some(entry) =
        names.into_iter().find(|e| held.get(*e).and_then(Value::as_str) != ships.get(*e).map(String::as_str))
    {
        return Err(refuse(entry.clone(), format!("{entry} is not the tool's bundle")));
    }
    for (entry, want) in &ships {
        if current(top, spec, entry).as_ref() != Some(want) {
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
                Some((format!("{}/{rest}", spec.hooks), with_role_verbs(bytes)))
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

/// A hook script with each agent role's verbs from `bp-operation` in place of its `# bp:role-verbs` line, so the role
/// hook cannot drift from 21's rows. The human has no line: no hook runs in the human's shell.
fn with_role_verbs(bytes: &[u8]) -> Vec<u8> {
    let arms: Vec<String> =
        Role::ALL.iter().filter_map(|r| Some(format!("{r}) allowed=\"{}\" ;;", r.verbs()?.join(" ")))).collect();
    String::from_utf8_lossy(bytes).replace("# bp:role-verbs", &arms.join("\n")).into_bytes()
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

/// Adds each installed path to `info/exclude`, so the install stays local and `git status` stays clean. The main
/// worktree's git directory is the common one, which git reads it from, also when `.git` is a file.
fn exclude(top: &Path, paths: &[&str]) -> Result<(), Refusal> {
    let repo = bp_vault::git::Repo::find(top).expect("install runs at a clone's top");
    let path = repo.git.join("info/exclude");
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
    fn each_node_kind_has_a_text_template() {
        for kind in bp_vault::Kind::ALL {
            assert_eq!(super::template(*kind).is_some(), !kind.is_record(), "{kind}");
        }
    }

    #[test]
    fn a_codex_role_is_toml_that_sets_its_role() {
        let md = "---\nname: bp-worker\ndescription: Says \"x\".\nskills: bp-a\n---\n\nLine one.\nLine two.\n";
        let toml = String::from_utf8(super::codex_role("bp-worker", md)).unwrap();
        let want = "name = \"bp-worker\"\ndescription = \"Says \\\"x\\\".\"\ndeveloper_instructions = \"Line one.\\nLine two.\"\n\n\
                    [shell_environment_policy]\nset = { BP_ROLE = \"worker\" }\n";
        assert_eq!(toml, want);
    }
}
