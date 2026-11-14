//! The human's signature (37, design change 4). The role label is forgeable, so a human act that a gate trusts carries
//! an SSH signature by a key whose passphrase only the human types. OpenSSH's `ssh-keygen -Y` signs and verifies, run
//! as a process: no crate. The signed message is canonical text that names the act, the project, the node, the pull
//! request, its base branch, the full head and the act's own fields, so a signature of one act never verifies as
//! another.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use bp_change::risk::{Config, RISK_FILE};
use bp_vault::{Code, Id, Refusal};

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
    /// The branch the pull request merges into. A stacked pull request retargeted to the default branch then needs a
    /// new signature: the human saw the diff against the lower layer only.
    pub(crate) base: &'a str,
    /// The full head sha of the pull request.
    pub(crate) head: &'a str,
    /// The act's own fields, in the order the act fixes: a lowering's `level`, then `reason`; an approval's [`accepts`].
    pub(crate) more: &'a [(&'a str, &'a str)],
}

impl Act<'_> {
    /// One `key: value` line per field, in a fixed order. A gate rebuilds this text from the values it expects, never
    /// from a record's own text, so a changed field fails the signature.
    pub(crate) fn message(&self) -> String {
        let project = self.node.project().unwrap_or_default();
        let mut text = format!(
            "act: {}\nproject: {project}\nnode: {}\npull_request: {}\nbase: {}\nhead: {}\n",
            self.verb, self.node, self.pull_request, self.base, self.head
        );
        text.extend(self.more.iter().map(|(k, v)| format!("{k}: {v}\n")));
        text
    }
}

/// The lines an approval signs for the rows it accepts at high's round cap (37, design change 6): one
/// `accepts: <review id>#<row>` line each, sorted, after the head. The human signs the list with the head, so a list
/// edited in a record, or copied to another record with more names, does not verify (review of #100, W1). An empty
/// list signs as no line.
pub(crate) fn accepts(names: &[String]) -> Vec<(&'static str, &str)> {
    let mut lines: Vec<(&str, &str)> = names.iter().map(|n| ("accepts", n.as_str())).collect();
    lines.sort_unstable();
    lines
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
        return Err(Refusal::failed(format!("ssh-keygen -Y sign -f {}", key.display()), {
            let said = String::from_utf8_lossy(&out.stderr);
            let said = if said.trim().is_empty() { String::new() } else { format!("{}; ", said.trim()) };
            format!("{said}name the private key with --key <path> and type its passphrase, then rerun")
        }));
    }
    Ok(signature)
}

/// Whether `signature` is a valid signature of `message` by `signer`, a public key line. Any failure of `ssh-keygen`
/// reads false: its stderr can echo a field of the signature, so its words name no tool fault (review of #99, row 52).
/// [`can_verify`] tells a tool that cannot verify at all. `Err` only when `ssh-keygen` or a scratch file cannot start.
pub(crate) fn verify(message: &str, signature: &str, signer: &str) -> Result<bool, Refusal> {
    let Some(signer) = public_key(signer) else { return Ok(false) };
    Ok(keygen_verify(message, signature, &signer)?.is_ok())
}

/// `Ok` when `ssh-keygen -Y verify` here verifies a fixed good signature, made once by a key of its own; else
/// `could_not_run`. A gate runs it once before it reads the comments, so a usage error or an old OpenSSH is a tool
/// fault, and no comment's text can raise one (review of #99, rows 32 and 52).
pub(crate) fn can_verify() -> Result<(), Refusal> {
    const KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINjbOecrh5eedysW/3Z2kX1jWgwtcIkLbT1lYY+0Q4Im";
    const MESSAGE: &str = "bp checks that ssh-keygen verifies\n";
    const SIGNATURE: &str = "-----BEGIN SSH SIGNATURE-----
U1NIU0lHAAAAAQAAADMAAAALc3NoLWVkMjU1MTkAAAAg2Ns55yuHl553Kxb/dnaRfWNaDC
1wiQttPWVhj7RDgiYAAAACYnAAAAAAAAAABnNoYTUxMgAAAFMAAAALc3NoLWVkMjU1MTkA
AABAr8NIAhaojSZ3/7EC9lleKMNj0m9NneVCJX09NdGXID8aR6oDkSgCsaNMmUtmmuLTti
rmky+tna0L01P4XFXTDQ==
-----END SSH SIGNATURE-----
";
    keygen_verify(MESSAGE, SIGNATURE, KEY)?.map_err(|stderr| {
        let said = cap(stderr.trim());
        let said = if said.is_empty() { said } else { format!("{said}; ") };
        Refusal::failed("ssh-keygen -Y verify", format!("{said}install OpenSSH 8.2 or later, then rerun"))
    })
}

/// One `ssh-keygen -Y verify` of `message` against `signer`, `<type> <base64>`: `Ok(Err(stderr))` when it does not
/// verify. OpenSSH exits 255 with "Could not verify signature." for a false one.
fn keygen_verify(message: &str, signature: &str, signer: &str) -> Result<Result<(), String>, Refusal> {
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
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Refusal::failed("ssh-keygen", format!("{e}; install OpenSSH 8.2 or later")))?;
    let fed = child.stdin.take().expect("stdin is piped").write_all(message.as_bytes());
    let out = child.wait_with_output().map_err(|e| Refusal::failed("ssh-keygen", e.to_string()))?;
    if fed.is_ok() && out.status.success() {
        return Ok(Ok(()));
    }
    Ok(Err(String::from_utf8_lossy(&out.stderr).trim().to_string()))
}

/// `text` cut to its first 200 bytes, at a character boundary, with a mark when cut: a value echoed in an envelope
/// never carries a whole page of input.
pub(crate) fn cap(text: &str) -> String {
    const MAX: usize = 200;
    if text.len() <= MAX {
        return text.to_string();
    }
    let mut end = MAX;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
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
/// <base64>`, and the sha it was read at. The forge protects that branch, so an agent's edit of the file in a worktree,
/// in the clone or on its own branch names no signer. Else `signer_missing`: `bp` cannot trust a human act without a
/// key; or `risk_file_invalid` when the file does not parse, as `bp risk` refuses it.
///
/// The branch and its sha come from the server, by `git ls-remote --symref origin HEAD`, never from a local ref. The
/// shared `default_branch` reads `refs/remotes/origin/HEAD`, and `git remote set-head` or `git update-ref` moves that
/// ref and `refs/remotes/origin/*` with no push: an agent would point them at a branch that names its own key. The
/// other verbs keep `default_branch`, because a wrong base there costs a bad claim, not a forged human act. A
/// `url.<x>.insteadOf` rule still redirects `ls-remote`, so a rewritten `origin` refuses here, and `land` compares the
/// sha with the forge's own default-branch head (review of #95, row 24).
pub(crate) fn signer(top: &Path) -> Result<(String, String), Refusal> {
    let (url, read) =
        (git(top, &["config", "--get", "remote.origin.url"])?, git(top, &["ls-remote", "--get-url", "origin"])?);
    if url != read {
        return Err(Refusal::new(
            Code::SignerMissing,
            "git ls-remote --get-url origin",
            "bp reads the signer from the repository that origin names, never through a rewrite of its url (37, design \
             change 4)",
            format!(
                "origin is {url}, but git reads {read}: remove the url.<x>.insteadOf rule (git config --get-regexp \
                 '^url\\.'), then rerun"
            ),
        ));
    }
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
    let text = git(top, &["show", &format!("{sha}:{RISK_FILE}")]).unwrap_or_default();
    let key = signer_of(&text).map_err(|why| {
        Refusal::new(
            Code::RiskFileInvalid,
            format!("origin/{branch}:{RISK_FILE}"),
            "the risk file holds `high` and `low` lists of path patterns, `limits` and `signer` (37, design change 1)",
            format!("{why}; edit {RISK_FILE} on {branch} by hand, then git commit it"),
        )
    })?;
    let key = key.ok_or_else(|| {
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
    })?;
    Ok((key, sha.to_string()))
}

/// The `signer` key of a risk file, as [`Config::parse`] reads it for the level: one reader of `.bp/risk.yml`. A file
/// that does not parse is the parse error, not "no signer" (review of #95, row 25); a duplicate key fails the parse, so
/// no second `signer` line can shadow the first.
fn signer_of(text: &str) -> Result<Option<String>, String> {
    Ok(Config::parse(text)?.signer.as_deref().and_then(public_key))
}

/// A full commit sha: 40 or 64 lower-case hex digits. A head that is not one never enters a message or a path.
pub(crate) fn is_sha(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A branch name that can be one line of a signed message: not empty, with no space or control character. Git refuses
/// both in a ref name, so only a broken forge or a forged argument gives one.
pub(crate) fn is_branch(name: &str) -> bool {
    !name.is_empty() && !name.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// `<type> <base64>` of a public key line, or `None` when the line is not one.
pub(crate) fn public_key(line: &str) -> Option<String> {
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

/// The info string of the fenced block that carries a signed approval on the forge.
const BLOCK: &str = "bp-approval";

/// The pull request comment that carries a signed act to the forge check (37, design change 5): the message's
/// `key: value` lines and the armoured signature, in one fenced block. `bp gate` reads it with [`blocks`].
pub(crate) fn comment(message: &str, signature: &str) -> String {
    format!(
        "The human's signed approval of this head. The check `bp-gate` verifies it.\n\n```{BLOCK}\n{message}{}\n```\n",
        signature.trim_end()
    )
}

/// One signed block of a comment: its `key: value` lines in order, and its armoured signature, if any.
#[derive(PartialEq)]
pub(crate) struct Block {
    pub(crate) fields: Vec<(String, String)>,
    pub(crate) signature: Option<String>,
}

impl Block {
    /// The value of the first line with `key`.
    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

/// Each signed block of a comment's body. A forge that stores CRLF line ends gives the same block.
pub(crate) fn blocks(body: &str) -> Vec<Block> {
    let body = body.replace("\r\n", "\n");
    let mut out = Vec::new();
    let mut lines = body.lines();
    while let Some(line) = lines.next() {
        if line.trim_end() != format!("```{BLOCK}") {
            continue;
        }
        let inner: Vec<&str> = lines.by_ref().take_while(|l| l.trim_end() != "```").collect();
        let text = inner.join("\n");
        let fields = inner
            .iter()
            .take_while(|l| !l.starts_with("-----BEGIN"))
            .filter_map(|l| l.split_once(": ").map(|(k, v)| (k.to_string(), v.to_string())))
            .collect();
        out.push(Block { fields, signature: armour(&text) });
    }
    out
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
    use super::{cap, is_branch, is_sha, signer_of};

    const KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKey";
    const OTHER: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOther";

    #[test]
    fn signer_reads_as_yaml() {
        let one = |text: &str| signer_of(text).ok().flatten();
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
        assert_eq!(signer_of(""), Ok(None), "no signer is no parse error");
        let bad = signer_of(&format!("high: [\"infra/*\"]\nsigner: {KEY}\n")).unwrap_err();
        assert!(bad.contains("infra/*"), "a bad pattern names itself, not a missing signer: {bad}");
    }

    #[test]
    fn a_head_is_a_full_lower_case_sha() {
        assert!(is_sha(&"a".repeat(40)) && is_sha(&"0".repeat(64)));
        for bad in ["a".repeat(39), "A".repeat(40), "a".repeat(41), format!("{}/..", "a".repeat(38)), "a".repeat(7)] {
            assert!(!is_sha(&bad), "{bad}");
        }
        assert!(!is_sha(&format!("{}\nact", "a".repeat(36))));
    }

    #[test]
    fn a_base_is_one_line_of_a_message() {
        assert!(is_branch("master") && is_branch("bp/a-b_c.1"));
        for bad in ["", "a b", "master\nact: lower", "a\tb", "a\u{7f}"] {
            assert!(!is_branch(bad), "{bad:?}");
        }
    }

    #[test]
    fn cap_cuts_at_200_bytes_on_a_character_boundary() {
        assert_eq!(cap("short"), "short");
        assert_eq!(cap(&"a".repeat(200)), "a".repeat(200));
        assert_eq!(cap(&"a".repeat(201)), format!("{}…", "a".repeat(200)));
        // A two-byte character that ends at byte 200 stays; one across the cut goes whole.
        assert_eq!(cap(&format!("{}é", "a".repeat(198))), format!("{}é", "a".repeat(198)));
        assert_eq!(cap(&format!("{}é", "a".repeat(199))), format!("{}…", "a".repeat(199)));
    }
}
