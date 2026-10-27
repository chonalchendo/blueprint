//! The arms of an eval (29, design change 1). An arm is a `bp` binary that embeds another bundle: the manifest check
//! (17, check 7) refuses an installed file that differs from the embedded one, so the skill cannot be taken out of an
//! install. `without` has no skill directory and no preload, and its dispatch keeps the five lines of the shipped one
//! with `Skill: none`: the baseline tests the missing body, not a different protocol. The role body is the shipped
//! one. `filler` keeps the structure and replaces the skill's body with equal-length text that holds no instruction.
//! An arm from a git ref, such as `old` (30, design change 2), is the bundle at that commit, from `git archive`, with
//! today's hooks: the hooks are the harness's bound, not the measured text, and the tool refuses a bundle that lacks a
//! hook it registers (`bp-agent.sh`, 31 design change 6).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::eval::Skill;
use crate::eval_sandbox::capture;

/// The text of the `filler` arm's body: it holds no instruction and no fact.
pub const FILLER: &str = "This line holds no instruction.";

/// The version of how the arms are built. A trial stores it beside the fixture's hash (`eval_grade::stamp`), so a
/// store of trials that ran under another construction is refused. Bump it when `arm_bundle` or `ref_bundle` changes.
pub const PROTOCOL: &str = "arms-3";

/// What follows a skill file's frontmatter.
pub fn body_of(skill_md: &str) -> &str {
    skill_md.strip_prefix("---\n").and_then(|t| t.split_once("\n---\n")).map_or(skill_md, |(_, body)| body)
}

/// Lines of [`FILLER`] cut to `len` bytes: the body of the `filler` arm is as long as the real one.
fn filler(len: usize) -> String {
    let mut text = format!("{FILLER}\n").repeat(len / (FILLER.len() + 1) + 1);
    text.truncate(len);
    text
}

/// The role file without the skill in its `skills:` line; the line goes when no skill is left.
fn unpreload(role: &str, skill: &str) -> String {
    let mut out = Vec::new();
    for line in role.lines() {
        let Some(list) = line.strip_prefix("skills:") else {
            out.push(line.to_string());
            continue;
        };
        let left: Vec<&str> = list.split(',').map(str::trim).filter(|s| *s != skill).collect();
        if !left.is_empty() {
            out.push(format!("skills: {}", left.join(", ")));
        }
    }
    out.join("\n") + "\n"
}

/// A copy of the bundle at `dest` with the arm's edit: no skill and no preload, or the skill's body as filler.
fn arm_bundle(root: &Path, skill: &Skill, arm: &str, dest: &Path) -> Result<()> {
    capture(Command::new("cp").arg("-R").arg(root.join("bundle")).arg(dest))?;
    let dir = dest.join("skills").join(&skill.name);
    if arm == "without" {
        std::fs::remove_dir_all(&dir)?;
        let role = dest.join(format!("roles/{}.md", skill.role_file()));
        std::fs::write(&role, unpreload(&std::fs::read_to_string(&role)?, &skill.name))?;
        // Install refuses a template that names a skill its role does not preload, and accepts `none`.
        let template = dest.join(format!("roles/{}/{}.md", skill.role_file(), skill.action));
        let text = std::fs::read_to_string(&template)?;
        let lines: Vec<&str> = text.lines().map(|l| if l.starts_with("Skill:") { "Skill: none" } else { l }).collect();
        std::fs::write(&template, lines.join("\n") + "\n")?;
    } else {
        let file = dir.join("SKILL.md");
        let text = std::fs::read_to_string(&file)?;
        let head = text.len() - body_of(&text).len();
        std::fs::write(&file, format!("{}{}", &text[..head], filler(text.len() - head)))?;
    }
    Ok(())
}

/// The bundle of the commit `sha` at `dest`, from `git archive <sha> bundle/`, with the hooks of `root`'s bundle.
fn ref_bundle(root: &Path, sha: &str, dest: &Path) -> Result<()> {
    let unpacked = dest.with_extension("archive");
    std::fs::create_dir_all(&unpacked)?;
    let tar = unpacked.join("bundle.tar");
    capture(
        Command::new("git").args(["archive", "--format=tar", "-o"]).arg(&tar).args([sha, "bundle/"]).current_dir(root),
    )?;
    capture(Command::new("tar").arg("-xf").arg(&tar).arg("-C").arg(&unpacked))?;
    std::fs::rename(unpacked.join("bundle"), dest).with_context(|| format!("move the bundle of {sha}"))?;
    std::fs::remove_dir_all(dest.join("hooks"))?;
    capture(Command::new("cp").arg("-R").arg(root.join("bundle/hooks")).arg(dest.join("hooks")))?;
    Ok(())
}

/// One directory per arm, each holding a `bp`. `given` names binaries the caller built, and `refs` the commits whose
/// bundle an arm runs; the others build here, the edited bundles first and the shipped bundle last, so
/// `target/release/bp` ends as the shipped bundle's even when only an edited arm runs.
pub fn bins(
    root: &Path,
    skill: &Skill,
    arms: &[String],
    work: &Path,
    (given, refs): (&[(String, PathBuf)], &BTreeMap<String, String>),
) -> Result<BTreeMap<String, PathBuf>> {
    let mut out = BTreeMap::new();
    let mut order: Vec<String> = arms.to_vec();
    let edited = |arm: &String| arm != "with" && !given.iter().any(|(a, _)| a == arm);
    if order.iter().any(edited) && !order.contains(&"with".to_string()) {
        order.push("with".into());
    }
    order.sort_by_key(|a| a == "with");
    for arm in order {
        let dir = work.join(format!("bin-{arm}"));
        std::fs::create_dir_all(&dir)?;
        if let Some((_, path)) = given.iter().find(|(a, _)| *a == arm) {
            std::fs::copy(path, dir.join("bp")).with_context(|| format!("copy {}", path.display()))?;
        } else {
            let bundle = if arm == "with" {
                None
            } else {
                let b = work.join(format!("bundle-{arm}"));
                match refs.get(&arm) {
                    Some(sha) => ref_bundle(root, sha, &b)?,
                    None => arm_bundle(root, skill, &arm, &b)?,
                }
                Some(b.canonicalize()?)
            };
            let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
            let mut build = Command::new(cargo);
            build.args(["build", "--release", "-p", "bp-tool"]).current_dir(root).stdin(Stdio::null());
            match &bundle {
                Some(b) => build.env("BP_BUNDLE_DIR", b),
                None => build.env_remove("BP_BUNDLE_DIR"),
            };
            if !build.status().context("run cargo build")?.success() {
                bail!("cargo build for the {arm} arm failed");
            }
            let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
            std::fs::copy(target.join("release/bp"), dir.join("bp")).context("copy the built bp")?;
        }
        out.insert(arm, dir);
    }
    Ok(out)
}

/// The dispatch prompt: the role's template for the action, from the arm's `bundle`, with the node filled in, under
/// the line the owner's replay typed to a session so that it dispatches the role and adds nothing (27, the replay). A
/// research fixture's question fills the template's `<question>`, below the five lines (31, design change 3).
pub fn prompt(bundle: &Path, skill: &Skill, id: &str, question: Option<&str>) -> Result<String> {
    let file = bundle.join(format!("roles/{}/{}.md", skill.role_file(), skill.action));
    let template = std::fs::read_to_string(&file).with_context(|| format!("read {}", file.display()))?;
    let mut text = template.replace("<id>", id);
    if let Some(q) = question {
        text = text.replace("<question>", q);
    }
    Ok(format!("Dispatch the {} agent with exactly this prompt and add nothing to it:\n\n{text}", skill.role_file()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filler_is_as_long_as_the_body_and_holds_no_instruction() {
        for len in [0, 5, 32, 33, 1000] {
            let f = filler(len);
            assert_eq!(f.len(), len);
            assert!(f.lines().all(|l| FILLER.starts_with(l)));
        }
        assert_eq!(body_of("---\nname: x\n---\nTask: a\n"), "Task: a\n");
        assert_eq!(body_of("no frontmatter"), "no frontmatter");
    }

    #[test]
    fn the_preload_goes_and_the_other_skills_stay() {
        let role = "---\nname: r\nskills: bp-complete, bp-respond\n---\nbody\n";
        assert_eq!(unpreload(role, "bp-complete"), "---\nname: r\nskills: bp-respond\n---\nbody\n");
        assert_eq!(unpreload("---\nskills: bp-review\n---\nx\n", "bp-review"), "---\n---\nx\n");
    }

    #[test]
    fn the_without_arm_keeps_the_five_line_dispatch_and_the_role_body() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let dest = tempfile::tempdir().unwrap();
        let skill = Skill::parse("bp-review").unwrap();
        arm_bundle(&root, &skill, "without", &dest.path().join("b")).unwrap();
        let (shipped, arm) = (root.join("bundle"), dest.path().join("b"));
        let read = |dir: &Path, f: &str| std::fs::read_to_string(dir.join(f)).unwrap();
        let (want, have) = (read(&shipped, "roles/bp-reviewer/review.md"), read(&arm, "roles/bp-reviewer/review.md"));
        assert_eq!(have.lines().count(), 5);
        assert_eq!(have, want.replace("Skill: bp-review", "Skill: none"), "only the Skill line differs");
        let role = read(&shipped, "roles/bp-reviewer.md");
        assert_eq!(read(&arm, "roles/bp-reviewer.md"), role.replace("skills: bp-review\n", ""), "only the preload");
        assert!(!arm.join("skills/bp-review").exists() && arm.join("skills/bp-capture").exists());
    }

    #[test]
    fn an_arm_from_a_git_ref_is_the_bundle_at_that_commit() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let git = |args: &[&str]| capture(Command::new("git").args(args).current_dir(&root)).ok();
        let Some(sha) = git(&["rev-parse", "--verify", "7b71a68^{commit}"]) else {
            eprintln!("skipped: history is shallow");
            return;
        };
        let dest = tempfile::tempdir().unwrap();
        ref_bundle(&root, &sha, &dest.path().join("bundle-old")).unwrap();
        let file = "skills/bp-review/SKILL.md";
        let have = std::fs::read_to_string(dest.path().join("bundle-old").join(file)).unwrap();
        assert_eq!(Some(have.trim().to_string()), git(&["show", &format!("{sha}:bundle/{file}")]));
        assert!(dest.path().join("bundle-old/roles/bp-reviewer/review.md").is_file(), "the arm's own template");
        let hook = |b: &Path| std::fs::read_to_string(b.join("hooks/bp-role.sh")).unwrap();
        assert_eq!(hook(&dest.path().join("bundle-old")), hook(&root.join("bundle")), "today's hooks");
    }

    #[test]
    fn fills_the_question_below_the_five_lines() {
        let bundle = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bundle");
        let skill = Skill::parse("bp-research").unwrap();
        let q = "How many lines does the check module in bp-tool's src have? Report under `## Findings` as a table.";
        let text = prompt(&bundle, &skill, "projects/bp/project", Some(q)).unwrap();
        let (head, dispatch) = text.split_once("\n\n").unwrap();
        assert_eq!(head, "Dispatch the bp-researcher agent with exactly this prompt and add nothing to it:");
        let lines: Vec<&str> = dispatch.lines().collect();
        let keys: Vec<&str> = lines[..5].iter().map(|l| l.split_once(':').unwrap().0).collect();
        assert_eq!(keys, ["Node", "Skill", "Budget", "Forbidden", "Stop"]);
        assert_eq!((lines[0], lines[5], lines[6], lines.len()), ("Node: projects/bp/project", "", q, 7));
        assert!(!text.contains("<question>") && !text.contains("<id>"), "{text}");
    }

    #[test]
    fn an_old_ref_with_no_skill_hashes_the_role_alone() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let skill = Skill::parse("bp-research").unwrap();
        let role = "bundle/roles/bp-researcher.md";
        // a27fc93 is master before the researcher's skill: issue 1 of milestone 8.
        let Ok(old_role) = crate::eval_grade::show_at(&root, "a27fc93", role) else {
            eprintln!("skipped: history is shallow");
            return;
        };
        let dest = tempfile::tempdir().unwrap();
        ref_bundle(&root, "a27fc93", &dest.path().join("bundle-old")).unwrap();
        assert!(!dest.path().join("bundle-old/skills/bp-research").exists(), "the ref has no skill");
        let (hash, marker) = crate::eval_grade::ref_hash(&root, &skill, "a27fc93").unwrap();
        assert_eq!(hash, crate::eval_grade::hash_object(&old_role).unwrap(), "the role file alone");
        assert_eq!(marker, crate::eval_grade::Marker::Role("You are the Blueprint researcher.".into()));

        // A bundle that holds the skill hashes the role, the body and each reference, in name order.
        let t = tempfile::tempdir().unwrap();
        let file = |rel: &str, text: &str| {
            let p = t.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        file(role, "role\n");
        let alone = crate::eval_grade::bundle_hash(t.path(), &skill).unwrap();
        assert_eq!(alone, crate::eval_grade::hash_object("role\n").unwrap(), "no skill directory: the role alone");
        file("bundle/skills/bp-research/SKILL.md", "body\n");
        file("bundle/skills/bp-research/references/fan-out.md", "fan\n");
        file("bundle/skills/bp-research/references/b.md", "b\n");
        let all = crate::eval_grade::bundle_hash(t.path(), &skill).unwrap();
        assert_eq!(all, crate::eval_grade::hash_object("role\nbody\nb\nfan\n").unwrap());
    }

    #[test]
    fn the_worker_arms_drop_one_skill_and_keep_the_other() {
        let bundle = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bundle");
        let role = std::fs::read_to_string(bundle.join("roles/bp-worker.md")).unwrap();
        assert!(role.contains("skills: bp-complete, bp-respond") && role.contains("`Skill:`"));
        let without = unpreload(&role, "bp-complete");
        assert!(without.contains("skills: bp-respond\n"), "{without}");
        assert_eq!(without.replace("skills: bp-respond", "skills: bp-complete, bp-respond"), role, "only the preload");
        for action in ["complete", "respond"] {
            let template = std::fs::read_to_string(bundle.join(format!("roles/bp-worker/{action}.md"))).unwrap();
            assert!(template.lines().any(|l| l == format!("Skill: bp-{action}")), "{action}");
        }
    }
}
