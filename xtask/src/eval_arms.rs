//! The arms of an eval (29, design change 1). An arm is a `bp` binary that embeds another bundle: the manifest check
//! (17, check 7) refuses an installed file that differs from the embedded one, so the skill cannot be taken out of an
//! install. `without` has no skill directory, no preload, no `Skill:` line in the dispatch template and no sentence
//! about that line in the role body. `filler` keeps the structure and replaces the skill's body with equal-length text
//! that holds no instruction.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::eval::Skill;
use crate::eval_sandbox::capture;

/// The text of the `filler` arm's body: it holds no instruction and no fact.
pub const FILLER: &str = "This line holds no instruction.";

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

/// The text without each sentence that names the `Skill:` line. A sentence ends at a full stop and a space.
fn without_skill_sentences(text: &str) -> String {
    let lines: Vec<String> = text
        .lines()
        .map(|l| l.split_inclusive(". ").filter(|s| !s.contains("`Skill:`")).collect::<String>().trim_end().to_string())
        .collect();
    lines.join("\n") + "\n"
}

/// A copy of the bundle at `dest` with the arm's edit: no skill and no preload, or the skill's body as filler.
fn arm_bundle(root: &Path, skill: &Skill, arm: &str, dest: &Path) -> Result<()> {
    capture(Command::new("cp").arg("-R").arg(root.join("bundle")).arg(dest))?;
    let dir = dest.join("skills").join(&skill.name);
    if arm == "without" {
        std::fs::remove_dir_all(&dir)?;
        let role = dest.join(format!("roles/{}.md", skill.role_file()));
        std::fs::write(&role, unpreload(&std::fs::read_to_string(&role)?, &skill.name))?;
        // The role body tells the agent to run the skill on the `Skill:` line; the arm's dispatch has no such line.
        std::fs::write(&role, without_skill_sentences(&std::fs::read_to_string(&role)?))?;
        // Install refuses a template that names a skill its role does not preload, so the arm's dispatch names none.
        let template = dest.join(format!("roles/{}/{}.md", skill.role_file(), skill.action));
        let text = std::fs::read_to_string(&template)?;
        let kept: Vec<&str> = text.lines().filter(|l| !l.starts_with("Skill:")).collect();
        std::fs::write(&template, kept.join("\n") + "\n")?;
    } else {
        let file = dir.join("SKILL.md");
        let text = std::fs::read_to_string(&file)?;
        let head = text.len() - body_of(&text).len();
        std::fs::write(&file, format!("{}{}", &text[..head], filler(text.len() - head)))?;
    }
    Ok(())
}

/// One directory per arm, each holding a `bp`. `given` names binaries the caller built; the others build here, the
/// edited bundles first and the shipped bundle last, so `target/release/bp` ends as the shipped bundle's even when
/// only an edited arm runs.
pub fn bins(
    root: &Path,
    skill: &Skill,
    arms: &[String],
    work: &Path,
    given: &[(String, PathBuf)],
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
                arm_bundle(root, skill, &arm, &b)?;
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
/// the line the owner's replay typed to a session so that it dispatches the role and adds nothing (27, the replay).
pub fn prompt(bundle: &Path, skill: &Skill, id: &str) -> Result<String> {
    let file = bundle.join(format!("roles/{}/{}.md", skill.role_file(), skill.action));
    let template = std::fs::read_to_string(&file).with_context(|| format!("read {}", file.display()))?;
    Ok(format!(
        "Dispatch the {} agent with exactly this prompt and add nothing to it:\n\n{}",
        skill.role_file(),
        template.replace("<id>", id)
    ))
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
    fn the_worker_arms_drop_one_skill_and_keep_the_other() {
        let bundle = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bundle");
        let role = std::fs::read_to_string(bundle.join("roles/bp-worker.md")).unwrap();
        assert!(role.contains("skills: bp-complete, bp-respond") && role.contains("`Skill:`"));
        let without = without_skill_sentences(&unpreload(&role, "bp-complete"));
        assert!(without.contains("skills: bp-respond\n") && !without.contains("Skill:"), "{without}");
        assert!(without.contains("Do not run a verb on the `Forbidden:` line."), "{without}");
        for action in ["complete", "respond"] {
            let template = std::fs::read_to_string(bundle.join(format!("roles/bp-worker/{action}.md"))).unwrap();
            assert!(template.lines().any(|l| l == format!("Skill: bp-{action}")), "{action}");
        }
    }

    #[test]
    fn the_role_body_loses_the_sentences_about_the_skill_line() {
        let role = "You are X. The prompt has five lines: `Node:`, `Skill:` and `Stop:`. Run the skill on the `Skill:` line for the node. Do not run a verb on the `Forbidden:` line. Return it.\n\nStop: here.\n";
        assert_eq!(
            without_skill_sentences(role),
            "You are X. Do not run a verb on the `Forbidden:` line. Return it.\n\nStop: here.\n"
        );
        let real =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../bundle/roles/bp-reviewer.md"))
                .unwrap();
        assert!(real.contains("`Skill:`") && !without_skill_sentences(&real).contains("Skill:"));
    }
}
