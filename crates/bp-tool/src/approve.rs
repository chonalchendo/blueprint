//! `bp approve <issue>` and `bp lower <issue> <level>`: the human's signed acts on a pull request head (37, design
//! change 4). Each reads the head from the forge, as `land` does, signs the act with the human's key and writes one
//! record under the issue: `approvals/<head>` or `lowerings/<head>`. A gate reads a record only through [`approved`]
//! or [`lowered`], which rebuild the message from the head they test and verify the signature against the signer that
//! origin's `.bp/risk.yml` names. A hand-written record, a copied signature or a changed field does not verify. An
//! approval also prints the comment that carries it to the forge check `bp-gate`, and `--post` posts it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bp_change::risk::Level;
use bp_forge::{Gh, PullRequest};
use bp_read::snapshot::clone_of;
use bp_vault::{Code, Id, Index, Kind, Record, Refusal, Vault};
use clap::Args;
use serde_json::{Value, json};

use crate::signed::{Act, armour, blocks, can_verify, comment, in_agent, is_branch, is_sha, sign, signer, verify};
use crate::{Wrote, node_id, subject};

/// `bp approve <issue> [--key <path>] [--post]`.
#[derive(Args)]
pub struct Approve {
    /// The issue whose open pull request the human approves at its current head.
    pub issue: String,
    /// The private key that signs; you type its passphrase at the prompt. Default `~/.ssh/bp_human`.
    #[arg(long)]
    pub key: Option<PathBuf>,
    /// Post the signed comment on the pull request with `gh pr comment`, for the forge check `bp-gate`.
    #[arg(long)]
    pub post: bool,
}

/// `bp lower <issue> <level> --reason <why> [--key <path>]`.
#[derive(Args)]
pub struct Lower {
    /// The issue whose risk level the human lowers at its pull request's current head.
    pub issue: String,
    /// The level in force at that head.
    #[arg(value_parser = ["low", "medium"])]
    pub level: String,
    /// Why the level is lower, in one line.
    #[arg(long)]
    pub reason: String,
    /// The private key that signs; you type its passphrase at the prompt. Default `~/.ssh/bp_human`.
    #[arg(long)]
    pub key: Option<PathBuf>,
}

pub(crate) fn approve(cwd: &Path, a: &Approve) -> Result<Wrote, Refusal> {
    act(cwd, &a.issue, a.key.as_deref(), Kind::Approval, &[], a.post)
}

pub(crate) fn lower(cwd: &Path, l: &Lower) -> Result<Wrote, Refusal> {
    if l.reason.trim().is_empty() || l.reason.contains(['\n', '\r']) {
        return Err(Refusal::usage("--reason", "give the reason as one line of text"));
    }
    act(cwd, &l.issue, l.key.as_deref(), Kind::Lowering, &[("level", &l.level), ("reason", l.reason.trim())], false)
}

/// The verb of a signed record's kind.
fn verb(kind: Kind) -> &'static str {
    if kind == Kind::Approval { "approve" } else { "lower" }
}

/// One signed act of `kind` on the issue's open pull request at its head; a valid record of the same act at that head
/// is a repeat. An approval, new or repeated, prints its forge comment, and posts it when `post`.
fn act(
    cwd: &Path,
    issue: &str,
    key: Option<&Path>,
    kind: Kind,
    more: &[(&str, &str)],
    post: bool,
) -> Result<Wrote, Refusal> {
    let verb = verb(kind);
    let role = crate::set::human(format!("{verb} {issue}"))?;
    node_id(issue)?;
    let vault = Vault::discover(cwd)?;
    let index = Index::open(&vault)?;
    let id = subject(&index, issue)?;
    if !Kind::Issue.fits(&id) {
        return Err(Refusal::usage(id.as_str(), format!("bp {verb} takes an issue id: bp list issue")));
    }
    let top = clone_of(cwd, &index, id.project().unwrap_or_default())?;
    crate::claim::git(&top, &["fetch", "-q", "origin"])?;
    let branch = crate::claim::branch(&id);
    let gh = Gh::new(&top);
    let Some(pr) = gh.prs_for_head(&branch)?.into_iter().filter(PullRequest::is_open).max_by_key(|p| p.number) else {
        return Err(Refusal::new(
            Code::ApprovalMissing,
            id.as_str(),
            "a signed act names a pull request and its head (37, design change 4)",
            format!("{branch} has no open pull request; the worker runs bp pr {id}, then rerun"),
        ));
    };
    let (head, base) = forge_shas(&branch, &pr)?;
    let (signer, _) = signer(&top)?;
    if in_agent(&signer) {
        return Err(Refusal::new(
            Code::KeyInAgent,
            signer,
            "a key loaded in an ssh-agent signs with no passphrase, so an agent session could use it (37, design \
             change 4)",
            format!("ssh-add -d <the key's .pub file>, then rerun bp {verb} {id}"),
        ));
    }
    // A tool that cannot verify refuses here, not as a record or a key that does not verify (review of #100, F3).
    can_verify()?;
    let act = Act { verb, node: &id, pull_request: pr.number, base, head, more };
    let message = act.message();
    // At high's round cap the approval names the open rows it accepts (37, design change 6). Rows an earlier record
    // of this head accepted are not named again, so a repeat names none and finds its record.
    let accepts = match kind {
        Kind::Approval => {
            let issue = bp_read::inputs::reviewed(&vault, &index, id.as_str())?;
            bp_change::intents::accepting(&issue, head)
        }
        _ => Vec::new(),
    };
    for r in index.records(id.as_str(), kind)?.into_iter().filter(|_| accepts.is_empty()) {
        if holds(&r, &message, &signer)? {
            let mut v = json!({ "id": r.id, "issue": id, "head": head, "pull_request": pr.number });
            if kind == Kind::Approval {
                let signature = armour(&r.body).expect("a record that verifies holds a signature");
                v["comment"] = again(&gh, &id, pr.number, &message, &signature, post)?.into();
            }
            return Ok(Wrote::Already(v));
        }
    }
    drop(index);
    let key = match key {
        Some(k) => k.to_path_buf(),
        None => Path::new(&std::env::var_os("HOME").unwrap_or_default()).join(".ssh/bp_human"),
    };
    let named: Vec<String> = accepts.iter().map(|(review, n)| format!("{review}#{n}")).collect();
    // The passphrase prompt names no act, so the human reads what the key signs first.
    eprint!("bp {verb} signs:\n{message}");
    if !named.is_empty() {
        eprintln!("bp approve accepts these open rows as they stand: {}", named.join(", "));
    }
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
    writer.clean([&id])?;
    // A record already at this head that does not verify stays; the valid one takes the next key.
    let record = crate::record::free(&vault.root, &format!("{id}/{}/{head}", kind.dir()));
    let at = crate::clock::instant(crate::clock::now()?);
    let mut fields: Vec<(&str, Value)> =
        vec![("head", head.into()), ("pull_request", pr.number.into()), ("base", base.into())];
    fields.extend(more.iter().map(|(k, v)| (*k, Value::from(*v))));
    if !accepts.is_empty() {
        fields.push(("accepts", named.clone().into()));
    }
    fields.push(("at", at.into()));
    let text = record_text(kind, &id, fields, &signature);
    let changes = BTreeMap::from([(record.clone(), text)]);
    let written = writer.commit(verb, role.as_str(), &format!("{verb} {id}"), &changes)?;
    let mut v = json!({
        "id": record,
        "issue": id,
        "head": head,
        "pull_request": pr.number,
        "commit": written.commit,
        "findings": written.findings,
    });
    if !accepts.is_empty() {
        v["accepts"] = named.into();
    }
    if kind == Kind::Approval {
        v["comment"] = share(&gh, &id, pr.number, &message, &signature, post)?.into();
    }
    Ok(Wrote::Changed(v))
}

/// The head and the base branch the forge names, each a shape a record and a signed line can hold. The head names the
/// record's file and a line of the signed message, so a slash or a newline from the forge would move the one and add a
/// field to the other.
fn forge_shas<'a>(branch: &str, pr: &'a PullRequest) -> Result<(&'a str, &'a str), Refusal> {
    let head = pr.head_ref_oid.as_str();
    if !is_sha(head) {
        return Err(Refusal::failed(
            format!("gh pr list --head {branch}"),
            format!("the forge gave head {head:?}, not a full commit sha; fix gh or the pull request, then rerun"),
        ));
    }
    let base = pr.base_ref_name.as_deref().unwrap_or_default();
    if !is_branch(base) {
        return Err(Refusal::failed(
            format!("gh pr list --head {branch}"),
            format!("the forge gave base branch {base:?}, not a branch name; fix gh or the pull request, then rerun"),
        ));
    }
    Ok((head, base))
}

/// The forge comment of a signed approval, printed to stderr for the human to post, or posted with `gh pr comment`
/// when `post`. The forge check reruns only on a new run: the fix names the rerun. A failed post leaves the record,
/// so its fix names the rerun with `--post`, which posts what the pull request lacks.
fn share(gh: &Gh, issue: &Id, number: u64, message: &str, signature: &str, post: bool) -> Result<String, Refusal> {
    let text = comment(message, signature);
    if post {
        gh.comment(number, &text).map_err(|e| {
            let fix = format!("{}; the approval is recorded: rerun bp approve {issue} --post to post it", e.fix);
            Refusal::failed(e.subject, fix)
        })?;
        eprintln!("bp approve posted the signed comment on pull request {number}; rerun its bp-gate check");
    } else {
        eprint!("bp approve: post this comment on pull request {number}, then rerun its bp-gate check:\n\n{text}");
    }
    Ok(text)
}

/// A repeat's comment. With `post`, it posts only when no comment holds this whole block as `bp gate` reads it: a
/// failed post is then not stranded, and a rerun adds no second copy (review of #99, row 30). The signature under
/// another field is a block the gate does not count, so it posts (row 53).
fn again(gh: &Gh, issue: &Id, number: u64, message: &str, signature: &str, post: bool) -> Result<String, Refusal> {
    let text = comment(message, signature);
    if post {
        let want = blocks(&text);
        if gh.comments(number)?.iter().flat_map(|b| blocks(b)).any(|b| want.contains(&b)) {
            eprintln!("bp approve: pull request {number} holds the signed comment; rerun its bp-gate check");
            return Ok(text);
        }
    }
    share(gh, issue, number, message, signature, post)
}

/// A signed record's file: its header fields, then the armoured signature as its one section.
pub(crate) fn record_text(kind: Kind, issue: &Id, fields: Vec<(&str, Value)>, signature: &str) -> String {
    let header = crate::stamp::record(kind, issue, &[], fields);
    format!("{header}\n## Signature\n\n```\n{}\n```\n", signature.trim_end())
}

/// The record's signature verifies `message` by `signer`. The caller ran [`can_verify`] first, so a `false` is the
/// record's, never a tool that cannot verify.
fn holds(r: &Record, message: &str, signer: &str) -> Result<bool, Refusal> {
    armour(&r.body).map_or(Ok(false), |sig| verify(message, &sig, signer))
}

/// The record's pull request number and base branch, as its header holds them.
fn number(r: &Record) -> Option<(u64, &str)> {
    Some((r.header.get("pull_request")?.as_u64()?, r.header.get("base")?.as_str()?))
}

/// The forge's pull request as a signed act names it: its number and its base branch. A record of another pull
/// request or base never counts, even with a valid signature (37, design changes 3 and 4).
pub(crate) type Forge<'a> = (u64, &'a str);

/// The rows an approval accepts, each as a review's id and the row's number.
pub(crate) type Accepts = Vec<(String, u64)>;

/// `Ok` with the rows it accepts when an approval under `issue` carries a valid signature of the approval of `head` by `signer`, and names the
/// forge's pull request number and base branch (37, design changes 3 and 4). The message comes from `issue`, `head`
/// and the forge, not from the record, so only a signature the human made for this head of this pull request on this
/// base passes. Else `signature_invalid` for a record at this head that does not verify, or `approval_missing`, which
/// names the heads a stale approval signed.
pub(crate) fn approved(index: &Index, issue: &Id, head: &str, signer: &str, forge: Forge) -> Result<Accepts, Refusal> {
    let records = index.records(issue.as_str(), Kind::Approval)?;
    if !records.is_empty() {
        can_verify()?;
    }
    let (n, base) = forge;
    let message = Act { verb: "approve", node: issue, pull_request: n, base, head, more: &[] }.message();
    let mut accepts = None;
    for r in records.iter().filter(|r| number(r) == Some(forge)) {
        if holds(r, &message, signer)? {
            accepts.get_or_insert_with(Vec::new).extend(bp_read::inputs::accepted(r));
        }
    }
    if let Some(accepts) = accepts {
        return Ok(accepts);
    }
    let fix = format!("the human runs bp approve {issue} from a shell, at head {head} of pull request {n} into {base}");
    let at = |r: &&Record| r.header.get("head").and_then(Value::as_str) == Some(head);
    if let Some(r) = records.iter().find(at) {
        return Err(Refusal::new(
            Code::SignatureInvalid,
            r.id.as_str(),
            "an approval counts only with a signature, by the signer that .bp/risk.yml names, of this act at this \
             head of this pull request and base (37, design change 4)",
            fix,
        ));
    }
    let heads: Vec<&str> = records.iter().filter_map(|r| r.header.get("head")?.as_str()).collect();
    let stale = if heads.is_empty() { String::new() } else { format!("; the approvals sign {}", heads.join(", ")) };
    Err(Refusal::new(
        Code::ApprovalMissing,
        issue.as_str(),
        "a high pull request lands only with the human's signed approval of its head (37, design change 3)",
        format!("{fix}{stale}"),
    ))
}

/// The level of the latest lowering under `issue` whose signature by `signer` verifies at `head`, or `None`. With
/// `forge`, a lowering counts only when it names the forge's pull request and base; `land` passes it. `bp risk` calls
/// no `gh`, so it passes `None` and reads the record's own. A lowering whose signature does not verify, or that signed
/// another head, is ignored: only a signed human record lowers a level (37, design change 2).
pub(crate) fn lowered(
    index: &Index,
    issue: &Id,
    head: &str,
    signer: &str,
    forge: Option<Forge>,
) -> Result<Option<Level>, Refusal> {
    let records = index.records(issue.as_str(), Kind::Lowering)?;
    if !records.is_empty() {
        can_verify()?;
    }
    let text = |r: &Record, f: &str| r.header.get(f).and_then(Value::as_str).map(str::to_string);
    for r in records.iter().rev() {
        let (Some((n, base)), Some(level), Some(reason)) = (number(r), text(r, "level"), text(r, "reason")) else {
            continue;
        };
        if forge.is_some_and(|f| f != (n, base)) {
            continue;
        }
        let more = [("level", level.as_str()), ("reason", reason.as_str())];
        let message = Act { verb: "lower", node: issue, pull_request: n, base, head, more: &more }.message();
        if let Ok(level) = level.parse()
            && holds(r, &message, signer)?
        {
            return Ok(Some(level));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use bp_change::risk::Level;
    use bp_vault::{Code, Id, Index, Kind, Vault};
    use serde_json::Value;

    use super::{approved, lowered, record_text};
    use crate::signed::{Act, sign, signer};

    const ISSUE: &str = "projects/bp/issues/a";
    const H1: &str = "1111111111111111111111111111111111111111";
    const H2: &str = "2222222222222222222222222222222222222222";
    const B: &str = "master";

    fn run(dir: &Path, program: &str, args: &[&str]) -> String {
        // The developer's git configuration stays out: a global `commit.gpgsign` would run their signing program.
        let isolated = [("GIT_CONFIG_GLOBAL", "/dev/null"), ("GIT_CONFIG_NOSYSTEM", "1")];
        let out = Command::new(program).current_dir(dir).envs(isolated).env_remove("SSH_AUTH_SOCK").args(args).output();
        let out = out.unwrap();
        assert!(out.status.success(), "{program} {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    }

    /// A key pair made in `dir` with no passphrase: the private key's path and the public key's `<type> <base64>`.
    fn key(dir: &Path, name: &str) -> (PathBuf, String) {
        run(dir, "ssh-keygen", &["-q", "-t", "ed25519", "-N", "", "-C", "test", "-f", name]);
        let public = std::fs::read_to_string(dir.join(format!("{name}.pub"))).unwrap();
        (dir.join(name), public.split_whitespace().take(2).collect::<Vec<_>>().join(" "))
    }

    /// A vault under git with one project and the issue [`ISSUE`].
    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        for (rel, text) in [
            ("projects/bp/project.md", "---\nkind: project\ntitle: B\nstatus: active\n---\n"),
            ("projects/bp/issues/a.md", "---\nkind: issue\ntitle: A\nstatus: open\n---\n"),
            ("shared/.keep", ""),
            (".gitignore", ".bp/\n"),
        ] {
            put(dir.path(), rel, text);
        }
        run(dir.path(), "git", &["init", "-q"]);
        let vault = Vault { root: dir.path().to_path_buf() };
        (dir, vault)
    }

    fn put(root: &Path, rel: &str, text: &str) {
        std::fs::create_dir_all(root.join(rel).parent().unwrap()).unwrap();
        std::fs::write(root.join(rel), text).unwrap();
    }

    /// A signed record written by hand at `key` under the issue, as an agent could write it, with `fields` and the
    /// armoured `signature`, then committed.
    fn forge(v: &Vault, kind: Kind, key: &str, fields: Vec<(&str, Value)>, signature: &str) {
        let issue = Id::parse(ISSUE).unwrap();
        put(&v.root, &format!("{ISSUE}/{}/{key}.md", kind.dir()), &record_text(kind, &issue, fields, signature));
        run(&v.root, "git", &["add", "-A"]);
        run(&v.root, "git", &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", key]);
    }

    fn fields(head: &str, n: u64, at: &str, more: &[(&'static str, &str)]) -> Vec<(&'static str, Value)> {
        fields_on(head, n, B, at, more)
    }

    /// [`fields`] of a pull request into `base`.
    fn fields_on(
        head: &str,
        n: u64,
        base: &str,
        at: &str,
        more: &[(&'static str, &str)],
    ) -> Vec<(&'static str, Value)> {
        let mut f: Vec<(&str, Value)> = vec![("head", head.into()), ("pull_request", n.into()), ("base", base.into())];
        f.extend(more.iter().map(|(k, v)| (*k, Value::from(*v))));
        f.push(("at", format!("2026-11-12T10:00:{at}Z").into()));
        f
    }

    #[test]
    fn approve_refuses_bad_or_stale_signature() {
        let keys = tempfile::tempdir().unwrap();
        let (human, signer) = key(keys.path(), "human");
        let (agent, agent_public) = key(keys.path(), "agent");
        let (_dir, v) = vault();
        let issue = Id::parse(ISSUE).unwrap();
        let on = |verb: &str, head: &str, n: u64, base: &str| {
            Act { verb, node: &issue, pull_request: n, base, head, more: &[] }.message()
        };
        let act = |verb: &str, head: &str, n: u64| on(verb, head, n, B);
        let message = |head: &str, n: u64| act("approve", head, n);
        let gate = |head: &str, signer: &str| {
            approved(&Index::open(&v).unwrap(), &issue, head, signer, (1, B)).map_err(|r| (r.code, r.fix))
        };
        assert_eq!(gate(H1, &signer).unwrap_err().0, Code::ApprovalMissing, "no approval");

        // A valid signature of another head is stale: it approves that head only.
        let of_h2 = sign(&message(H2, 1), &human).unwrap();
        forge(&v, Kind::Approval, H2, fields(H2, 1, "01", &[]), &of_h2);
        let (code, fix) = gate(H1, &signer).unwrap_err();
        assert!(code == Code::ApprovalMissing && fix.contains(&format!("the approvals sign {H2}")), "{fix}");
        assert_eq!(gate(H2, &signer), Ok(Vec::new()), "it does approve the head it signed");
        assert_eq!(gate(H2, &agent_public).unwrap_err().0, Code::SignatureInvalid, "only the signer's key counts");

        // Each record at this head that an agent can make without the human's key refuses.
        let garbage = "-----BEGIN SSH SIGNATURE-----\nU1NIU0lHAAAAAQ==\n-----END SSH SIGNATURE-----";
        let by_agent = sign(&message(H1, 1), &agent).unwrap();
        let other_act = sign(&act("lower", H1, 1), &human).unwrap();
        let other_pr = sign(&message(H1, 2), &human).unwrap();
        let other_base = sign(&on("approve", H1, 1, "bp/b"), &human).unwrap();
        let namespace = {
            put(keys.path(), "m", &message(H1, 1));
            run(keys.path(), "ssh-keygen", &["-Y", "sign", "-n", "git", "-f", "human", "m"]);
            std::fs::read_to_string(keys.path().join("m.sig")).unwrap()
        };
        for (n, (why, signature, pr)) in [
            ("a hand-written record", garbage, 1),
            ("a record with no signature", "", 1),
            ("a signature by the agent's own key", by_agent.as_str(), 1),
            ("a signature of another head, copied", of_h2.as_str(), 1),
            ("a signature of another act", other_act.as_str(), 1),
            ("a changed field: the pull request", other_pr.as_str(), 1),
            ("a changed field: the base branch", other_base.as_str(), 1),
            ("a signature in git's namespace", namespace.as_str(), 1),
            ("a valid approval of another pull request", other_pr.as_str(), 2),
        ]
        .into_iter()
        .enumerate()
        {
            let key = if n == 0 { H1.to_string() } else { format!("{H1}-{}", n + 1) };
            forge(&v, Kind::Approval, &key, fields(H1, pr, &format!("{:02}", n + 2), &[]), signature);
            assert_eq!(gate(H1, &signer).unwrap_err().0, Code::SignatureInvalid, "{why}");
        }
        // A valid approval of this head into another base: a stacked pull request, retargeted since.
        forge(&v, Kind::Approval, &format!("{H1}-10"), fields_on(H1, 1, "bp/b", "10", &[]), &other_base);
        assert_eq!(gate(H1, &signer).unwrap_err().0, Code::SignatureInvalid, "an approval into another base");
        let into = |base| approved(&Index::open(&v).unwrap(), &issue, H1, &signer, (1, base)).map_err(|r| r.code);
        assert_eq!(into("bp/b"), Ok(Vec::new()), "it approves the base it signed");
        let of_pr2 = |n| approved(&Index::open(&v).unwrap(), &issue, H1, &signer, (n, B)).map_err(|r| r.code);
        assert_eq!(of_pr2(2), Ok(Vec::new()), "and the record of pull request 2 approves that one");
        assert!(gate(H1, &signer).unwrap_err().1.contains("bp approve projects/bp/issues/a"));

        // The human's valid signature in a record with no `base` refuses: the gate rebuilds the message from the record's
        // base branch, so a record without one approves nothing.
        let mut no_base = fields(H1, 1, "19", &[]);
        no_base.retain(|(k, _)| *k != "base");
        forge(&v, Kind::Approval, &format!("{H1}-10"), no_base, &sign(&message(H1, 1), &human).unwrap());
        assert_eq!(gate(H1, &signer).unwrap_err().0, Code::SignatureInvalid, "a record with no base");

        // The human's signature of this act at this head passes, whatever forged records sit beside it.
        let good = sign(&message(H1, 1), &human).unwrap();
        forge(&v, Kind::Approval, &format!("{H1}-9"), fields(H1, 1, "20", &[]), &good);
        assert_eq!(gate(H1, &signer), Ok(Vec::new()));
        // The gate trusts the signer it is given, so `signer` reads it from origin's protected branch only.
        assert_eq!(gate(H1, &agent_public), Ok(Vec::new()), "the agent's record verifies against the agent's own key");
    }

    #[test]
    fn lowering_needs_signature() {
        let keys = tempfile::tempdir().unwrap();
        let (human, signer) = key(keys.path(), "human");
        let (agent, _) = key(keys.path(), "agent");
        let (_dir, v) = vault();
        let issue = Id::parse(ISSUE).unwrap();
        let level = || lowered(&Index::open(&v).unwrap(), &issue, H1, &signer, None).unwrap();
        let lowering = |key: &Path, head: &str, level: &str| {
            let more = [("level", level), ("reason", "docs only")];
            let act = Act { verb: "lower", node: &issue, pull_request: 1, base: B, head, more: &more };
            sign(&act.message(), key).unwrap()
        };
        let low = [("level", "low"), ("reason", "docs only")];
        assert_eq!(level(), None, "no lowering");
        let forged = "-----BEGIN SSH SIGNATURE-----\nU1NIU0lHAAAAAQ==\n-----END SSH SIGNATURE-----";
        forge(&v, Kind::Lowering, H1, fields(H1, 1, "01", &low), forged);
        assert_eq!(level(), None, "a hand-written lowering is ignored");
        forge(&v, Kind::Lowering, &format!("{H1}-2"), fields(H1, 1, "02", &low), "");
        assert_eq!(level(), None, "an unsigned lowering is ignored");
        forge(&v, Kind::Lowering, &format!("{H1}-3"), fields(H1, 1, "03", &low), &lowering(&agent, H1, "low"));
        assert_eq!(level(), None, "a lowering by another key is ignored");
        forge(&v, Kind::Lowering, &format!("{H1}-4"), fields(H1, 1, "04", &low), &lowering(&human, H1, "medium"));
        assert_eq!(level(), None, "a changed level does not verify");
        forge(&v, Kind::Lowering, H2, fields(H2, 1, "05", &low), &lowering(&human, H2, "low"));
        assert_eq!(level(), None, "a lowering of another head does not count at this one");
        forge(&v, Kind::Lowering, &format!("{H1}-5"), fields(H1, 1, "06", &low), &lowering(&human, H1, "low"));
        assert_eq!(level(), Some(Level::Low), "the human's signed lowering counts");
        let at = |forge| lowered(&Index::open(&v).unwrap(), &issue, H1, &signer, Some(forge)).unwrap();
        assert_eq!(at((1, B)), Some(Level::Low), "it names the forge's pull request and base");
        assert_eq!(at((2, B)), None, "another pull request at the same head");
        assert_eq!(at((1, "bp/b")), None, "another base at the same head");
        forge(
            &v,
            Kind::Lowering,
            &format!("{H1}-6"),
            fields(H1, 1, "07", &[("level", "medium"), ("reason", "x")]),
            forged,
        );
        assert_eq!(level(), Some(Level::Low), "a later forged lowering changes nothing");
    }

    #[test]
    fn approved_reads_the_signer_from_the_forge_not_a_local_ref() {
        let keys = tempfile::tempdir().unwrap();
        let (_, human) = key(keys.path(), "human");
        let (agent, agent_public) = key(keys.path(), "agent");
        let t = tempfile::tempdir().unwrap();
        let code = t.path().join("code");
        let commit = |text: &str| {
            put(&code, ".bp/risk.yml", text);
            run(&code, "git", &["add", "-A"]);
            run(&code, "git", &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "risk"]);
        };
        run(t.path(), "git", &["init", "-q", "--bare", "-b", "master", "origin.git"]);
        run(t.path(), "git", &["init", "-q", "-b", "master", "code"]);
        run(&code, "git", &["remote", "add", "origin", &t.path().join("origin.git").display().to_string()]);
        commit(&format!("signer: {human}\n"));
        run(&code, "git", &["push", "-q", "-u", "origin", "master"]);
        // The agent pushes its own branch, which names its key, as normal work.
        run(&code, "git", &["checkout", "-q", "-b", "bp/x"]);
        commit(&format!("signer: {agent_public}\n"));
        run(&code, "git", &["push", "-q", "origin", "bp/x"]);
        run(&code, "git", &["checkout", "-q", "master"]);

        let (_dir, v) = vault();
        let issue = Id::parse(ISSUE).unwrap();
        let message = Act { verb: "approve", node: &issue, pull_request: 1, base: B, head: H1, more: &[] }.message();
        forge(&v, Kind::Approval, H1, fields(H1, 1, "01", &[]), &sign(&message, &agent).unwrap());
        let signer = |code: &Path| signer(code).unwrap().0;
        let gate = || approved(&Index::open(&v).unwrap(), &issue, H1, &signer(&code), (1, B)).unwrap_err().code;
        let local = |r: &str| run(&code, "git", &["show", &format!("{r}:.bp/risk.yml")]);

        // Poison one: origin/HEAD names the agent's pushed branch, with no push.
        run(&code, "git", &["remote", "set-head", "origin", "bp/x"]);
        assert!(local("origin/HEAD").contains(&agent_public), "the local ref names the agent's key");
        assert_eq!(signer(&code), human, "the signer is the server's default branch's");
        assert_eq!(gate(), Code::SignatureInvalid, "the agent's approval does not verify");

        // Poison two: origin/master is a local commit that names the agent's key.
        run(&code, "git", &["remote", "set-head", "origin", "master"]);
        commit(&format!("signer: {agent_public}\n"));
        run(&code, "git", &["update-ref", "refs/remotes/origin/master", "HEAD"]);
        assert!(local("origin/master").contains(&agent_public), "the local ref names the agent's key");
        assert_eq!(signer(&code), human, "the signer is the server's default branch's");
        assert_eq!(gate(), Code::SignatureInvalid, "the agent's approval does not verify");
    }
}
