//! The human's signature (37, design change 4). The role label is forgeable, so a human act that a gate trusts carries
//! an SSH signature by a key whose passphrase only the human types. OpenSSH's `ssh-keygen -Y` signs and verifies, run
//! as a process: no crate. The signed message is canonical text that names the act, the project, the node, the pull
//! request and the full head, so a signature of one act never verifies as another.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use bp_vault::{Code, Id, Refusal};
use yaml_rust2::{Yaml, YamlLoader};

use crate::claim::git;

/// The signature namespace: a signature made for git commits or for another tool never verifies here.
const NAMESPACE: &str = "bp";

/// The one principal of the allowed-signers file that `verify` writes.
const PRINCIPAL: &str = "bp-human";

/// One signed act: the canonical message that `sign` signs and `verify` checks.
pub(crate) struct Act<'a> {
    /// The verb: `approve` or `lower`.
    pub(crate) verb: &'a str,
    /// The issue.
    pub(crate) node: &'a Id,
    /// The pull request number on the forge.
    pub(crate) pull_request: u64,
    /// The full head sha of the pull request.
    pub(crate) head: &'a str,
    /// The act's own fields, in the order the act fixes: a lowering's `level`, then `reason`.
    pub(crate) more: &'a [(&'a str, &'a str)],
}

impl Act<'_> {
    /// One `key: value` line per field, in a fixed order. A gate rebuilds this text from the values it expects, never
    /// from a record's own text, so a changed field fails the signature.
    pub(crate) fn message(&self) -> String {
        let project = self.node.project().unwrap_or_default();
        let mut text = format!(
            "act: {}\nproject: {project}\nnode: {}\npull_request: {}\nhead: {}\n",
            self.verb, self.node, self.pull_request, self.head
        );
        text.extend(self.more.iter().map(|(k, v)| format!("{k}: {v}\n")));
        text
    }
}

/// The armoured signature of `message` by the private key at `key`. `SSH_AUTH_SOCK` is removed, so `ssh-keygen` reads
/// the key file and asks for its passphrase on the terminal; a key loaded in an ssh-agent is not used.
pub(crate) fn sign(message: &str, key: &Path) -> Result<String, Refusal> {
    let mut child = Command::new("ssh-keygen")
        .args(["-Y", "sign", "-n", NAMESPACE, "-f"])
        .arg(key)
        .env_remove("SSH_AUTH_SOCK")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Refusal::failed("ssh-keygen", format!("{e}; install OpenSSH 8.2 or later")))?;
    let fed = child.stdin.take().expect("stdin is piped").write_all(message.as_bytes());
    let out = child.wait_with_output().map_err(|e| Refusal::failed("ssh-keygen", e.to_string()))?;
    let signature = String::from_utf8_lossy(&out.stdout).to_string();
    if fed.is_err() || !out.status.success() || !signature.contains("-----BEGIN SSH SIGNATURE-----") {
        return Err(Refusal::failed(
            format!("ssh-keygen -Y sign -f {}", key.display()),
            format!(
                "{}; name the private key with --key <path> and type its passphrase, then rerun",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        ));
    }
    Ok(signature)
}

/// Whether `signature` is a valid signature of `message` by `signer`, a public key line. `Err` only when
/// `ssh-keygen` cannot run.
pub(crate) fn verify(message: &str, signature: &str, signer: &str) -> Result<bool, Refusal> {
    let Some(signer) = public_key(signer) else { return Ok(false) };
    let dir = Scratch::new()?;
    // Only the key's type and base64 go in the allowed-signers line, so no option or second line can ride in.
    let allowed = dir.0.join("allowed");
    let sig = dir.0.join("sig");
    let failed = |e: std::io::Error| Refusal::failed(dir.0.display().to_string(), e.to_string());
    std::fs::write(&allowed, format!("{PRINCIPAL} namespaces=\"{NAMESPACE}\" {signer}\n")).map_err(failed)?;
    std::fs::write(&sig, signature).map_err(failed)?;
    let mut child = Command::new("ssh-keygen")
        .args(["-Y", "verify", "-n", NAMESPACE, "-I", PRINCIPAL, "-f"])
        .arg(&allowed)
        .arg("-s")
        .arg(&sig)
        .env_remove("SSH_AUTH_SOCK")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| Refusal::failed("ssh-keygen", format!("{e}; install OpenSSH 8.2 or later")))?;
    let fed = child.stdin.take().expect("stdin is piped").write_all(message.as_bytes());
    let status = child.wait().map_err(|e| Refusal::failed("ssh-keygen", e.to_string()))?;
    Ok(fed.is_ok() && status.success())
}

/// A directory of this process's own under the system temp directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, Refusal> {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        loop {
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("bp-verify-{}-{n}", std::process::id()));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok(Self(dir)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(Refusal::failed(dir.display().to_string(), e.to_string())),
            }
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The signer: the public key of the `signer` key of `.bp/risk.yml` on the forge's default branch, as `<type>
/// <base64>`. The forge protects that branch, so an agent's edit of the file in a worktree, in the clone or on its own
/// branch names no signer. Else `signer_missing`: `bp` cannot trust a human act without a key.
///
/// The branch and its sha come from the server, by `git ls-remote --symref origin HEAD`, never from a local ref. The
/// shared `default_branch` reads `refs/remotes/origin/HEAD`, and `git remote set-head` or `git update-ref` moves that
/// ref and `refs/remotes/origin/*` with no push: an agent would point them at a branch that names its own key. The
/// other verbs keep `default_branch`, because a wrong base there costs a bad claim, not a forged human act.
pub(crate) fn signer(top: &Path) -> Result<String, Refusal> {
    let listed = git(top, &["ls-remote", "--symref", "origin", "HEAD"])?;
    let branch = listed.lines().find_map(|l| l.strip_prefix("ref: refs/heads/")?.strip_suffix("\tHEAD"));
    let sha = listed.lines().find_map(|l| l.strip_suffix("\tHEAD").filter(|s| is_sha(s)));
    let (Some(branch), Some(sha)) = (branch, sha) else {
        return Err(Refusal::new(
            Code::SignerMissing,
            "git ls-remote --symref origin HEAD",
            "bp reads the signer from the default branch that the forge names (37, design change 4)",
            "origin names no default branch; set it on the forge, then rerun",
        ));
    };
    git(top, &["fetch", "-q", "origin", sha])?;
    let text = git(top, &["show", &format!("{sha}:.bp/risk.yml")]).unwrap_or_default();
    // Issue 0 of milestone 12 (`risk-level`) reads the same file with yaml-rust2. At rebase, read its `signer` here.
    signer_of(&text).ok_or_else(|| {
        Refusal::new(
            Code::SignerMissing,
            format!("origin/{branch}:.bp/risk.yml at {sha}"),
            "bp trusts a human act only with a signature by the key that .bp/risk.yml names (37, design change 4)",
            format!(
                "ssh-keygen -t ed25519 -f ~/.ssh/bp_human with a passphrase; add the one line `signer: <the .pub \
                 file's type and key>` to .bp/risk.yml; commit it and push it to origin/{branch} from a shell; then \
                 rerun"
            ),
        )
    })
}

/// The `signer` key of a risk file as yaml-rust2 reads it: one document, a mapping, and a one-line string that holds a
/// public key. A duplicate key fails the parse, so no second `signer` line can shadow the first.
fn signer_of(text: &str) -> Option<String> {
    let docs = YamlLoader::load_from_str(text).ok()?;
    let [doc] = docs.as_slice() else { return None };
    let value = doc.as_hash()?.get(&Yaml::String("signer".into()))?.as_str()?;
    if value.contains(['\n', '\r']) {
        return None;
    }
    public_key(value)
}

/// A full commit sha: 40 or 64 lower-case hex digits. A head that is not one never enters a message or a path.
pub(crate) fn is_sha(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// `<type> <base64>` of a public key line, or `None` when the line is not one.
fn public_key(line: &str) -> Option<String> {
    let mut words = line.split_whitespace();
    let (kind, key) = (words.next()?, words.next()?);
    let kind_ok = kind.bytes().all(|b| b.is_ascii_alphanumeric() || b"@.-".contains(&b));
    let key_ok = key.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b));
    (kind_ok && key_ok).then(|| format!("{kind} {key}"))
}

/// Whether a running ssh-agent holds `signer`, as `ssh-add -L` lists it. With no agent, or no `ssh-add`, it holds
/// nothing.
///
/// This guards a human slip only: a loaded key signs with no passphrase. It defends nothing against an agent on the
/// same machine, which controls `PATH` and `SSH_AUTH_SOCK`; the forge check (37, issue 4) is the gate an agent cannot
/// pass. `/usr/bin/ssh-add` runs when it exists, so a `PATH` without `ssh-add` does not drop the check. The `ssh-add`
/// on `PATH` runs too, and either listing refuses, so a program on `PATH` can only add a refusal.
pub(crate) fn in_agent(signer: &str) -> bool {
    let system = Path::new("/usr/bin/ssh-add");
    let programs = if system.exists() { vec![system, Path::new("ssh-add")] } else { vec![Path::new("ssh-add")] };
    programs.into_iter().any(|program| {
        let Ok(out) = Command::new(program).arg("-L").stdin(Stdio::null()).stderr(Stdio::null()).output() else {
            return false;
        };
        out.status.success()
            && String::from_utf8_lossy(&out.stdout).lines().any(|l| public_key(l).as_deref() == Some(signer))
    })
}

/// The armoured signature a record's body holds, from its first line to its last.
pub(crate) fn armour(body: &str) -> Option<String> {
    let start = body.find("-----BEGIN SSH SIGNATURE-----")?;
    let end = "-----END SSH SIGNATURE-----";
    let stop = start + body[start..].find(end)? + end.len();
    Some(format!("{}\n", &body[start..stop]))
}

#[cfg(test)]
mod tests {
    use super::{is_sha, signer_of};

    const KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKey";
    const OTHER: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOther";

    #[test]
    fn signer_reads_as_yaml() {
        let one = signer_of;
        assert_eq!(one(&format!("high:\n  - .bp/\nsigner: {KEY} human\n")).as_deref(), Some(KEY));
        assert_eq!(one(&format!("signer: \"{KEY}\" # the human\n")).as_deref(), Some(KEY));
        // A second `signer` fails the parse: neither line names the signer.
        assert_eq!(one(&format!("signer: {OTHER}\nsigner: {KEY}\n")), None, "a duplicate key");
        assert_eq!(one(&format!("signer: |\n  {KEY}\n")), None, "a block scalar ends in a newline");
        assert_eq!(one(&format!("signer: >\n  {KEY}\n")), None, "a folded scalar ends in a newline");
        assert_eq!(one(&format!("signer:\n  - {KEY}\n")), None, "a list is not a string");
        assert_eq!(one("signer: 12\n"), None, "a number is not a string");
        assert_eq!(one(&format!("x:\n  signer: {KEY}\n")), None, "only the top-level key names the signer");
        assert_eq!(one(&format!("signer: {KEY}\n---\nsigner: {OTHER}\n")), None, "two documents");
        assert_eq!(one(""), None, "an empty file");
    }

    #[test]
    fn a_head_is_a_full_lower_case_sha() {
        assert!(is_sha(&"a".repeat(40)) && is_sha(&"0".repeat(64)));
        for bad in ["a".repeat(39), "A".repeat(40), "a".repeat(41), format!("{}/..", "a".repeat(38)), "a".repeat(7)] {
            assert!(!is_sha(&bad), "{bad}");
        }
        assert!(!is_sha(&format!("{}\nact", "a".repeat(36))));
    }
}
