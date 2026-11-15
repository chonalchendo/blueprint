//! `bp clear <project>`: the human's signed clear of the project's red master (37, design change 11). It signs the act
//! `clear`, the project, the red commit, the pull request whose merge made it, its base branch, the checks that failed
//! there and the base branch's head now, in the shape of `bp approve` (37, design change 4), and writes one record
//! `clears/<red>` under the project. A gate reads a clear only through [`cleared`], which rebuilds the message from the
//! master-red record it tests and the clear's `tip`, and verifies the signature against the signer that origin's
//! `.bp/risk.yml` names. An unsigned or wrongly signed clear, a clear of another red or another project, and a copied
//! record change nothing. The human sees the failed checks the record names; `bp clear` asks the forge only which red
//! `land` holds, by the test `land` uses, and judges each red at the head `land` reads: origin's default branch.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bp_change::intents::Red;
use bp_forge::Gh;
use bp_read::inputs::{Reds, clear_message};
use bp_read::snapshot::clone_of;
use bp_vault::{Code, Id, Index, Kind, Record, Refusal, Vault};
use clap::Args;
use serde_json::{Value, json};

use crate::Wrote;
use crate::signed::{armour, can_verify, in_agent, sign, signer, verify};

/// `bp clear <project> [--key <path>]`.
#[derive(Args)]
pub struct Clear {
    /// The project whose red master the human accepts: no pull request of it lands but the revert until then.
    pub project: String,
    /// The private key that signs; you type its passphrase at the prompt. Default `~/.ssh/bp_human`.
    #[arg(long)]
    pub key: Option<PathBuf>,
}

/// The project's own node, where its records sit.
fn node(project: &str) -> Result<Id, Refusal> {
    Id::parse(&format!("projects/{project}/project"))
        .map_err(|_| Refusal::usage(project, "name a project by its directory under projects/: bp list project"))
}

pub(crate) fn clear(cwd: &Path, c: &Clear) -> Result<Wrote, Refusal> {
    let role = crate::set::human(format!("clear {}", c.project))?;
    let node = node(&c.project)?;
    let vault = Vault::discover(cwd)?;
    let index = Index::open(&vault)?;
    if index.header(node.as_str())?.is_none() {
        return Err(Refusal::usage(node.as_str(), "name a project the vault holds: bp list project"));
    }
    let top = clone_of(cwd, &index, &c.project)?;
    crate::claim::git(&top, &["fetch", "-q", "origin"])?;
    let (signer, tip) = signer(&top)?;
    if in_agent(&signer) {
        return Err(Refusal::new(
            Code::KeyInAgent,
            signer,
            "a key loaded in an ssh-agent signs with no passphrase, so an agent session could use it (37, design \
             change 4)",
            format!("ssh-add -d <the key's .pub file>, then rerun bp clear {}", c.project),
        ));
    }
    can_verify()?;
    // The red to clear is the earliest that `land` holds: the forge confirms it, and git shows no exact revert of it at
    // the head of the default branch, the tip `land` reads (review of #102, L1). An end record, which a hand can write,
    // never stops it (V1). With none, it is the earliest that the intents hold, which no end record names: a red always
    // has a legal exit (W1).
    let Reds { reds, ended } = bp_read::inputs::reds(&vault, &index, &c.project)?;
    let gh = Gh::new(&top);
    let mut open = None;
    for r in &reds {
        if cleared(&index, &node, r, &signer)? {
            continue;
        }
        if reverted(&top, r, &tip).is_none() && crate::land::confirmed(&top, &gh, r)? {
            open = Some(r.clone());
            break;
        }
        if open.is_none() && !ended.contains(&r.sha) {
            open = Some(r.clone());
        }
    }
    let Some(red) = open else {
        if let Some(r) = reds.last() {
            return Ok(Wrote::Already(json!({ "project": c.project, "red": r.sha, "id": r.id })));
        }
        return Err(Refusal::new(
            Code::NoRed,
            node.as_str(),
            "bp clear ends a master-red record of the project, which only land writes (37, design change 11)",
            format!("no master-red record of {} exists, so nothing waits on a clear; bp list issue", c.project),
        ));
    };
    drop(index);
    let key = match &c.key {
        Some(k) => k.clone(),
        None => Path::new(&std::env::var_os("HOME").unwrap_or_default()).join(".ssh/bp_human"),
    };
    let message = clear_message(&node, &red, &tip);
    eprint!("bp clear signs:\n{message}");
    let signature = sign(&message, &key)?;
    if !verify(&message, &signature, &signer)? {
        return Err(Refusal::new(
            Code::SignatureInvalid,
            key.display().to_string(),
            "bp trusts a human act only with a signature by the key that .bp/risk.yml names (37, design change 4)",
            format!("the key at {} is not the signer; name the signer's private key with --key <path>", key.display()),
        ));
    }
    let writer = vault.writer()?;
    let record = crate::record::free(&vault.root, &format!("projects/{}/{}/{}", c.project, Kind::Clear.dir(), red.sha));
    let at = crate::clock::instant(crate::clock::now()?);
    let fields: Vec<(&str, Value)> = vec![
        ("red", red.sha.clone().into()),
        ("pull_request", red.pull_request.into()),
        ("base", red.base.clone().into()),
        ("checks", red.checks.clone().into()),
        ("tip", tip.into()),
        ("at", at.into()),
    ];
    let text = crate::approve::record_text(Kind::Clear, &node, fields, &signature);
    let changes = BTreeMap::from([(record.clone(), text)]);
    let written = writer.commit("clear", role.as_str(), &format!("clear {}", c.project), &changes)?;
    Ok(Wrote::Changed(json!({
        "id": record,
        "project": c.project,
        "red": red.sha,
        "commit": written.commit,
        "findings": written.findings,
    })))
}

/// Whether a clear record under the project `node` carries a valid signature by `signer` of the clear of `red` (37,
/// design change 11). The message comes from `red` and the clear's `tip`, not from the record's other fields, so only
/// the signature the human made for this red commit of this project, with these failed checks, passes. `can_verify`
/// runs first when a record names the red.
pub(crate) fn cleared(index: &Index, node: &Id, red: &Red, signer: &str) -> Result<bool, Refusal> {
    let names = |r: &Record| r.header.get("red").and_then(Value::as_str) == Some(red.sha.as_str());
    let records: Vec<Record> = index.records(node.as_str(), Kind::Clear)?.into_iter().filter(names).collect();
    if records.is_empty() {
        return Ok(false);
    }
    can_verify()?;
    for r in &records {
        let tip = r.header.get("tip").and_then(Value::as_str).unwrap_or_default();
        if armour(&r.body).map_or(Ok(false), |sig| verify(&clear_message(node, red, tip), &sig, signer))? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The commit that reverts `red` exactly on the base branch at `tip`, the sha `git ls-remote` names (never a local
/// ref, which `git update-ref` moves with no push): the commit after `red` on the branch's first-parent line, when its
/// tree is the tree of `red`'s first parent. Git alone judges it, so no record ends a red for `land` by naming a
/// revert. `None` when `red` is not on that line or the next commit is no exact revert.
pub(crate) fn reverted(top: &Path, red: &Red, tip: &str) -> Option<String> {
    let git = |args: &[&str]| crate::claim::git(top, args).ok();
    let line = git(&["rev-list", "--first-parent", &format!("{}..{tip}", red.sha)])?;
    let next = line.lines().last()?.to_string();
    let parent = git(&["rev-parse", "--verify", "-q", &format!("{next}^1")])?;
    let tree = |r: &str| git(&["rev-parse", "--verify", "-q", &format!("{r}^{{tree}}")]);
    (parent == red.sha && tree(&next)? == tree(&format!("{}^1", red.sha))?).then_some(next)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use bp_change::intents::Red;
    use bp_vault::{Id, Index, Kind, Vault};
    use serde_json::Value;

    use bp_read::inputs::clear_message;

    use super::cleared;
    use crate::approve::record_text;
    use crate::signed::sign;

    const NODE: &str = "projects/bp/project";
    const R1: &str = "1111111111111111111111111111111111111111";
    const R2: &str = "2222222222222222222222222222222222222222";
    const TIP: &str = "3333333333333333333333333333333333333333";

    fn message(node: &Id, red: &Red) -> String {
        clear_message(node, red, TIP)
    }

    fn run(dir: &Path, program: &str, args: &[&str]) {
        let isolated = [("GIT_CONFIG_GLOBAL", "/dev/null"), ("GIT_CONFIG_NOSYSTEM", "1")];
        let out = Command::new(program).current_dir(dir).envs(isolated).env_remove("SSH_AUTH_SOCK").args(args).output();
        let out = out.unwrap();
        assert!(out.status.success(), "{program} {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    fn key(dir: &Path, name: &str) -> (PathBuf, String) {
        run(dir, "ssh-keygen", &["-q", "-t", "ed25519", "-N", "", "-C", "test", "-f", name]);
        let public = std::fs::read_to_string(dir.join(format!("{name}.pub"))).unwrap();
        (dir.join(name), public.split_whitespace().take(2).collect::<Vec<_>>().join(" "))
    }

    fn put(root: &Path, rel: &str, text: &str) {
        std::fs::create_dir_all(root.join(rel).parent().unwrap()).unwrap();
        std::fs::write(root.join(rel), text).unwrap();
    }

    fn red(sha: &str) -> Red {
        Red {
            id: format!("projects/bp/reds/{sha}"),
            sha: sha.into(),
            pull_request: 7,
            base: "master".into(),
            checks: vec!["ci".into()],
            ..Red::default()
        }
    }

    /// A clear record written by hand under the project at `key`, as an agent could write it, then committed.
    fn forge(v: &Vault, key: &str, sha: &str, n: u64, signature: &str) {
        let node = Id::parse(NODE).unwrap();
        let fields: Vec<(&str, Value)> = vec![
            ("red", sha.into()),
            ("pull_request", n.into()),
            ("base", "master".into()),
            ("checks", vec!["ci"].into()),
            ("tip", TIP.into()),
            ("at", format!("2026-11-14T10:00:{:02}Z", key.len()).into()),
        ];
        put(&v.root, &format!("projects/bp/clears/{key}.md"), &record_text(Kind::Clear, &node, fields, signature));
        run(&v.root, "git", &["add", "-A"]);
        run(&v.root, "git", &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", key]);
    }

    #[test]
    fn clear_needs_signature() {
        let keys = tempfile::tempdir().unwrap();
        let (human, signer) = key(keys.path(), "human");
        let (agent, _) = key(keys.path(), "agent");
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "projects/bp/project.md", "---\nkind: project\ntitle: B\nstatus: active\n---\n");
        put(dir.path(), "projects/mentat/project.md", "---\nkind: project\ntitle: M\nstatus: active\n---\n");
        put(dir.path(), "shared/.keep", "");
        put(dir.path(), ".gitignore", ".bp/\n");
        run(dir.path(), "git", &["init", "-q"]);
        let v = Vault { root: dir.path().to_path_buf() };
        let node = Id::parse(NODE).unwrap();
        let ok = |r: &Red| cleared(&Index::open(&v).unwrap(), &node, r, &signer).unwrap();
        let (r1, r2) = (red(R1), red(R2));
        assert!(!ok(&r1), "no clear");

        let garbage = "-----BEGIN SSH SIGNATURE-----\nU1NIU0lHAAAAAQ==\n-----END SSH SIGNATURE-----";
        let of_r2 = sign(&message(&node, &r2), &human).unwrap();
        let mentat = Id::parse("projects/mentat/project").unwrap();
        let of_mentat = sign(&message(&mentat, &r1), &human).unwrap();
        let of_pr8 = sign(&message(&node, &Red { pull_request: 8, ..r1.clone() }), &human).unwrap();
        // A clear names the failed checks it accepts: one signed before a new check failed does not clear it.
        let of_fewer = sign(&message(&node, &Red { checks: vec![], ..r1.clone() }), &human).unwrap();
        let other_tip = sign(&clear_message(&node, &r1, R2), &human).unwrap();
        let of_base = sign(&message(&node, &Red { base: "release".into(), ..r1.clone() }), &human).unwrap();
        let namespace = {
            put(keys.path(), "m", &message(&node, &r1));
            run(keys.path(), "ssh-keygen", &["-Y", "sign", "-n", "git", "-f", "human", "m"]);
            std::fs::read_to_string(keys.path().join("m.sig")).unwrap()
        };
        for (n, (why, signature)) in [
            ("a hand-written clear", garbage),
            ("a clear with no signature", ""),
            ("a clear by the agent's own key", sign(&message(&node, &r1), &agent).unwrap().as_str()),
            ("the human's clear of another red, its sha edited", of_r2.as_str()),
            ("the human's clear of the same red in another project, copied", of_mentat.as_str()),
            ("the human's clear of another pull request's red", of_pr8.as_str()),
            ("the human's clear of fewer failed checks", of_fewer.as_str()),
            ("the human's clear signed at another tip than the record names", other_tip.as_str()),
            ("the human's clear of the red on another base branch", of_base.as_str()),
            ("a signature in git's namespace", namespace.as_str()),
        ]
        .into_iter()
        .enumerate()
        {
            forge(&v, &format!("{R1}-{}", n + 1), R1, 7, signature);
            assert!(!ok(&r1), "{why}");
        }
        // The human's valid clear of R2, copied to a record that names R1, clears neither R1 nor anything else.
        forge(&v, &format!("{R1}-copy"), R1, 7, &of_r2);
        assert!(!ok(&r1), "a copied record");
        forge(&v, R2, R2, 7, &of_r2);
        assert!(ok(&r2) && !ok(&r1), "the human's clear clears the red it signed, and no other");

        let good = sign(&message(&node, &r1), &human).unwrap();
        forge(&v, &format!("{R1}-good"), R1, 7, &good);
        assert!(ok(&r1), "the human's clear of this red of this project");
        assert!(!cleared(&Index::open(&v).unwrap(), &mentat, &r1, &signer).unwrap(), "it clears no other project");
    }
}
