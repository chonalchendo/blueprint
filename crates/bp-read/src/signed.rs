//! The human's signature as a reader checks it (37, design change 4): the canonical message of a signed act, the
//! armoured signature a record holds and `ssh-keygen -Y verify`, run as a process. `bp-tool` signs and reads the
//! signer; the intents and the gates verify here, so each reads one message for one act.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use bp_vault::{Id, Refusal};

/// The signature namespace: a signature made for git commits or for another tool never verifies here.
pub const NAMESPACE: &str = "bp";

/// The one principal of the allowed-signers file that `verify` writes.
const PRINCIPAL: &str = "bp-human";

/// One signed act: the canonical message that `sign` signs and `verify` checks.
pub struct Act<'a> {
    /// The verb: `approve`, `lower` or `clear`.
    pub verb: &'a str,
    /// The issue.
    pub node: &'a Id,
    /// The pull request number on the forge.
    pub pull_request: u64,
    /// The branch the pull request merges into. A stacked pull request retargeted to the default branch then needs a
    /// new signature: the human saw the diff against the lower layer only.
    pub base: &'a str,
    /// The full head sha of the pull request.
    pub head: &'a str,
    /// The act's own fields, in the order the act fixes: a lowering's `level`, then `reason`; an approval's `accepts` lines.
    pub more: &'a [(&'a str, &'a str)],
}

impl Act<'_> {
    /// One `key: value` line per field, in a fixed order. A gate rebuilds this text from the values it expects, never
    /// from a record's own text, so a changed field fails the signature.
    pub fn message(&self) -> String {
        let project = self.node.project().unwrap_or_default();
        let mut text = format!(
            "act: {}\nproject: {project}\nnode: {}\npull_request: {}\nbase: {}\nhead: {}\n",
            self.verb, self.node, self.pull_request, self.base, self.head
        );
        text.extend(self.more.iter().map(|(k, v)| format!("{k}: {v}\n")));
        text
    }
}

/// Whether `signature` is a valid signature of `message` by `signer`, a public key line. Any failure of `ssh-keygen`
/// reads false: its stderr can echo a field of the signature, so its words name no tool fault (review of #99, row 52).
/// [`can_verify`] tells a tool that cannot verify at all. `Err` only when `ssh-keygen` or a scratch file cannot start.
pub fn verify(message: &str, signature: &str, signer: &str) -> Result<bool, Refusal> {
    let Some(signer) = public_key(signer) else { return Ok(false) };
    Ok(keygen_verify(message, signature, &signer)?.is_ok())
}

/// `Ok` when `ssh-keygen -Y verify` here verifies a fixed good signature, made once by a key of its own; else
/// `could_not_run`. A gate runs it once before it reads the comments, so a usage error or an old OpenSSH is a tool
/// fault, and no comment's text can raise one (review of #99, rows 32 and 52).
pub fn can_verify() -> Result<(), Refusal> {
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

/// Whether `signature` is a valid signature of `message` in the `bp` namespace by any key: `ssh-keygen -Y
/// check-novalidate`. A reader with no signer to hand, the intents, reads a clear with it: a record with no signature, a
/// garbled one or one made for another message reads false. Who signed is `land`'s test, with the signer ([`verify`]).
/// Any failure of `ssh-keygen` reads false.
pub fn signs(message: &str, signature: &str) -> bool {
    keygen(message, signature, &["-Y", "check-novalidate", "-n", NAMESPACE], None).is_ok_and(|r| r.is_ok())
}

/// One `ssh-keygen -Y verify` of `message` against `signer`, `<type> <base64>`: `Ok(Err(stderr))` when it does not
/// verify. OpenSSH exits 255 with "Could not verify signature." for a false one.
fn keygen_verify(message: &str, signature: &str, signer: &str) -> Result<Result<(), String>, Refusal> {
    // Only the key's type and base64 go in the allowed-signers line, so no option or second line can ride in.
    let allowed = format!("{PRINCIPAL} namespaces=\"{NAMESPACE}\" {signer}\n");
    keygen(message, signature, &["-Y", "verify", "-n", NAMESPACE, "-I", PRINCIPAL], Some(&allowed))
}

/// `ssh-keygen <args> [-f <allowed>] -s <signature>`, the message on stdin, in a scratch directory of its own.
fn keygen(message: &str, signature: &str, args: &[&str], allowed: Option<&str>) -> Result<Result<(), String>, Refusal> {
    let dir = Scratch::new()?;
    let sig = dir.0.join("sig");
    let failed = |e: std::io::Error| Refusal::failed(dir.0.display().to_string(), e.to_string());
    std::fs::write(&sig, signature).map_err(failed)?;
    let mut cmd = Command::new("ssh-keygen");
    cmd.args(args);
    if let Some(text) = allowed {
        let path = dir.0.join("allowed");
        std::fs::write(&path, text).map_err(failed)?;
        cmd.arg("-f").arg(path);
    }
    let mut child = cmd
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
pub fn cap(text: &str) -> String {
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

/// `<type> <base64>` of a public key line, or `None` when the line is not one.
pub fn public_key(line: &str) -> Option<String> {
    let mut words = line.split_whitespace();
    let (kind, key) = (words.next()?, words.next()?);
    let kind_ok = kind.bytes().all(|b| b.is_ascii_alphanumeric() || b"@.-".contains(&b));
    let key_ok = key.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b));
    (kind_ok && key_ok).then(|| format!("{kind} {key}"))
}

/// The armoured signature a record's body holds, from its first line to its last.
pub fn armour(body: &str) -> Option<String> {
    let start = body.find("-----BEGIN SSH SIGNATURE-----")?;
    let end = "-----END SSH SIGNATURE-----";
    let stop = start + body[start..].find(end)? + end.len();
    Some(format!("{}\n", &body[start..stop]))
}

/// A full commit sha: 40 or 64 lower-case hex digits. A head that is not one never enters a message or a path.
pub fn is_sha(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A name git takes for a branch, as `git check-ref-format --branch` judges it, so it is one line of a signed message
/// too. `a..b` and the other names git refuses fail, so no record names a base that no push can make (review of #102,
/// W1). The check runs outside any repository, and its output must be the name itself, so no `@{-1}` expands. A git
/// that does not run is a tool fault, `could_not_run`, not a bad name.
pub fn is_branch(name: &str) -> Result<bool, Refusal> {
    if name.is_empty() || name.starts_with('-') || name.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Ok(false);
    }
    let out = Command::new("git")
        .args(["check-ref-format", "--branch", name])
        .current_dir(std::env::temp_dir())
        .env("GIT_CEILING_DIRECTORIES", std::env::temp_dir())
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Refusal::failed("git check-ref-format", format!("{e}; install git and put it on PATH")))?;
    Ok(out.status.success() && out.stdout.strip_suffix(b"\n") == Some(name.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{cap, is_branch};

    #[test]
    fn cap_cuts_at_200_bytes_on_a_character_boundary() {
        assert_eq!(cap("short"), "short");
        assert_eq!(cap(&"a".repeat(200)), "a".repeat(200));
        assert_eq!(cap(&"a".repeat(201)), format!("{}…", "a".repeat(200)));
        // A two-byte character that ends at byte 200 stays; one across the cut goes whole.
        assert_eq!(cap(&format!("{}é", "a".repeat(198))), format!("{}é", "a".repeat(198)));
        assert_eq!(cap(&format!("{}é", "a".repeat(199))), format!("{}…", "a".repeat(199)));
    }

    #[test]
    fn a_base_is_a_name_git_takes_for_a_branch() {
        let ok = |name: &str| is_branch(name).unwrap();
        assert!(ok("master") && ok("bp/a-b_c.1") && ok("nope"));
        for bad in ["", "a b", "master\nact: lower", "a\tb", "a\u{7f}", "a..b", "-x", "@{-1}", "a.lock", "HEAD", "a/"] {
            assert!(!ok(bad), "{bad:?}");
        }
    }
}
