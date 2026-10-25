kind: mutant
base: 1c7b3e8e494c67208d7c8e8ff225ddc917690666
head: ecc5da57d83bda4535608f24f6eb0cc1589a7b26
title: Confine the worker; refuse a verb outside the role's row
genre: FnValue
span: xtask/src/sandbox.rs:27-84
seed: 7
mutant_diff: |
    diff --git a/xtask/src/sandbox.rs b/xtask/src/sandbox.rs
    index 1ef9667..60d532a 100644
    --- a/xtask/src/sandbox.rs
    +++ b/xtask/src/sandbox.rs
    @@ -24,64 +24,7 @@ const KEEP: usize = 16;
     const AGENT: &str = "a-q";
     
     pub fn run(claim: bool) -> Result<String> {
    -    let release = std::env::current_dir()?.join("target/release");
    -    if !release.join("bp").exists() {
    -        bail!("{} holds no bp; run cargo build --release -p bp-tool first", release.display());
    -    }
    -    let vault = scratch::run()?;
    -    let forge = Command::new(std::env::current_exe()?)
    -        .args(["forge", "bp"])
    -        .env("BP_VAULT", &vault)
    -        .output()
    -        .context("run xtask forge")?;
    -    if !forge.status.success() {
    -        bail!("xtask forge exited {}: {}", forge.status, String::from_utf8_lossy(&forge.stderr));
    -    }
    -    let t = PathBuf::from(String::from_utf8(forge.stdout)?.trim());
    -    let h = scratch::fresh(&std::env::temp_dir().join("bp-sandbox"), KEEP)?;
    -    let code = t.join("code");
    -    let path = format!("{}:{}", t.join("bin").display(), release.display());
    -    let env = [
    -        ("BP_VAULT", vault.display().to_string()),
    -        ("GIT_ALLOW_PROTOCOL", "file".into()),
    -        ("HOME", h.display().to_string()),
    -        ("XDG_CONFIG_HOME", h.display().to_string()),
    -        ("CODEX_HOME", h.join(".codex").display().to_string()),
    -        ("CLAUDE_CONFIG_DIR", h.join(".claude").display().to_string()),
    -        ("CLAUDE_PROJECT_DIR", code.display().to_string()),
    -    ];
    -    let mut out = String::from("unset BP_ROLE BP_SESSION BP_HARNESS\n");
    -    writeln!(out, "export D='{}' T='{}' H='{}'", vault.display(), t.display(), h.display())?;
    -    writeln!(out, "export PATH='{path}':\"$PATH\"")?;
    -    for (name, value) in &env {
    -        writeln!(out, "export {name}='{value}'")?;
    -    }
    -    if claim {
    -        let bp = |args: &[&str], role: bool| -> Result<Value> {
    -            let mut cmd = Command::new(release.join("bp"));
    -            cmd.args(args).arg("--json").current_dir(&code).envs(env.iter().map(|(k, v)| (k, v)));
    -            cmd.env("PATH", format!("{path}:{}", std::env::var("PATH").unwrap_or_default()));
    -            if role {
    -                cmd.env("BP_ROLE", "worker").env("BP_SESSION", AGENT);
    -            } else {
    -                cmd.env_remove("BP_ROLE").env_remove("BP_SESSION");
    -            }
    -            let run = cmd.stdin(std::process::Stdio::null()).output().with_context(|| format!("run bp {args:?}"))?;
    -            serde_json::from_slice(&run.stdout).with_context(|| format!("bp {args:?} printed no JSON"))
    -        };
    -        bp(&["install", "--target", "claude"], false)?;
    -        let ready = bp(&["list", "issue", "--ready", "--limit", "1"], false)?;
    -        let issue = ready["rows"][0]["id"].as_str().context("the scratch vault holds no ready issue")?;
    -        let cut = bp(&["cut", issue], true)?;
    -        let worktree = cut["worktree"].as_str().with_context(|| format!("bp cut {issue} made no worktree: {cut}"))?;
    -        writeln!(out, "export I='{issue}' W='{worktree}'")?;
    -        let settings = std::fs::read_to_string(code.join(".claude/settings.local.json"))?;
    -        let settings: Value = serde_json::from_str(&settings)?;
    -        for (name, script) in [("G", "bp-guard"), ("K", "bp-role")] {
    -            writeln!(out, "export {name}='{}'", command(&settings, script, &code)?)?;
    -        }
    -    }
    -    Ok(out)
    +    Ok(String::new())
     }
     
     /// The command the install registered for `script`.
truth:
- xtask/src/sandbox.rs:27-84 | replace run -> Result<String> with Ok(String::new())
---
## Problem

Needs the verb table of issue 5, because the widened check reads it.

## Shape

5. **Confinement** (17 Hooks, check 11).
   - `bp-guard.sh` denies an Edit or Write by `bp-worker` inside the clone's main checkout. It confines nothing else: the claim's worktree and the system temp directory (where `bp-complete` step 6 writes its delta body) pass, and so do a sibling worktree, another repository and `$HOME`. It makes no claim call, because a claim row holds no worktree. Paths compare with `-ef` up the ancestors, so case and links cannot slip past (review of PR 58). Issue 3 bounds the hook at 200 ms (choice; 42 ms measured).
   - `bp-role.sh` denies the `run_in_background` flag for `bp-worker`. It does not catch `cmd &`, `nohup` or `sleep` polling.
   - `bp-end.sh` gets a `timeout`.
   - The tool refuses, with a new closed code `verb_outside_role`, every verb outside the row of `BP_ROLE`. A code never changes meaning [`refusal.rs`], so `role_forbidden` keeps its text. The check reads the role's verbs, the union of its rows in design change 7's table, because `BP_ROLE` names a role, not an action. A `transition` to `approved` or `active` still refuses `role_forbidden` first. The label is forgeable, so this stops a slip, not an attack [`set.rs:136`].
   - Scope: the guard covers the Edit and Write tools on Claude Code. Bash redirects, `git` and `sed -i` are not covered. On Codex the tool check works only if `BP_ROLE` reaches the subagent, which Codex does not document [27 design change 3]. That is unverified.

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-install -p bp-tool -p xtask
~~~
