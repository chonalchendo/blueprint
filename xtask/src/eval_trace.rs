//! A trial's transcript for the graders (29, design change 2): a Claude Code `.jsonl` file, one JSON value per line.
//! The graders read tool calls, the final reply, the model and whether a text sits anywhere in the record.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde_json::Value;

pub struct Trace(Vec<Value>);

/// One `bp` call found in a Bash command: its verb and the words after it.
pub struct BpCall {
    pub verb: String,
    pub args: String,
}

impl Trace {
    /// A line that is no JSON value is skipped: the harness may write a progress line the grader does not read.
    pub fn parse(text: &str) -> Self {
        Self(text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
    }

    fn assistant(&self) -> impl Iterator<Item = &Value> {
        self.0.iter().filter(|v| v["type"] == "assistant")
    }

    /// Every Bash command the agent ran.
    pub fn commands(&self) -> Vec<&str> {
        let blocks = self.assistant().flat_map(|m| m["message"]["content"].as_array().into_iter().flatten());
        blocks
            .filter(|b| b["type"] == "tool_use" && b["name"] == "Bash")
            .filter_map(|b| b["input"]["command"].as_str())
            .collect()
    }

    pub fn bp_calls(&self) -> Vec<BpCall> {
        self.commands().into_iter().flat_map(calls_in).collect()
    }

    /// The text blocks of the last assistant message that holds any: what the role returned.
    pub fn final_text(&self) -> String {
        let text_of = |m: &Value| {
            let blocks = m["message"]["content"].as_array().into_iter().flatten();
            blocks.filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n")
        };
        self.assistant().map(text_of).filter(|t| !t.is_empty()).last().unwrap_or_default()
    }

    pub fn models(&self) -> BTreeSet<String> {
        let models = self.assistant().filter_map(|m| m["message"]["model"].as_str());
        models.filter(|m| !m.starts_with('<')).map(str::to_string).collect()
    }

    /// Whether `needle` sits in any string of the record, decoded.
    pub fn mentions(&self, needle: &str) -> bool {
        self.0.iter().any(|v| in_value(v, needle))
    }

    /// Whether `needle` sits in a skill preload: a `user` record the harness marks `isMeta` that holds
    /// `<skill-format>`, as in the owner's replay transcript. A skill the agent read from disk does not count, so a
    /// preload that failed cannot pass as one that worked (RA 5.5, 6).
    pub fn preloaded(&self, needle: &str) -> bool {
        let preload = |v: &&Value| v["type"] == "user" && v["isMeta"] == true && in_value(v, "<skill-format>");
        self.0.iter().filter(preload).any(|v| in_value(v, needle))
    }

    /// Why the runner stored this trial as failed: the agent ran past its limit or left no transcript.
    pub fn failure(&self) -> Option<&str> {
        self.0.iter().find(|v| v["subtype"] == "eval_failed").and_then(|v| v["reason"].as_str())
    }
}

fn in_value(v: &Value, needle: &str) -> bool {
    match v {
        Value::String(s) => s.contains(needle),
        Value::Array(a) => a.iter().any(|v| in_value(v, needle)),
        Value::Object(o) => o.values().any(|v| in_value(v, needle)),
        _ => false,
    }
}

/// The `bp` calls of one shell command. The command splits at `;`, `&`, `|`, a newline, a parenthesis and a
/// backtick outside quotes ([`segments`]), and a segment counts when its first word, after `export`, `env`,
/// `timeout N`, a keyword that opens a command (`if`, `elif`, `while`, `until`, `do`, `then`, `else`, `{`, `!`) and
/// assignments, is `bp` (as the role's hook resolves it) or a path to a `bp` in a `bin*` directory, the sandbox's. A
/// `bp` that the worker builds under `target/` is the code under test, not a call of the role, and `$B set` is no call
/// either.
/// A redirect is removed first, as `2>&1` would else split at its `&` and leave `2>` as the verb. A heredoc body is
/// skipped, from the line after `<<WORD` to the line that is `WORD`: it is text the shell never runs, and source code
/// in it that quotes `bp set <id>` would invent a call. A body counts only once its terminator line is seen, so a
/// stray `<<` hides nothing, unless a later line happens to equal the word after it.
fn calls_in(command: &str) -> Vec<BpCall> {
    let mut out = Vec::new();
    for segment in segments(&without_redirects(&without_heredocs(command))) {
        let mut words = segment.split_whitespace().peekable();
        while let Some(w) = words.peek() {
            if matches!(
                *w,
                "export"
                    | "env"
                    | "command"
                    | "exec"
                    | "if"
                    | "elif"
                    | "while"
                    | "until"
                    | "do"
                    | "then"
                    | "else"
                    | "{"
                    | "!"
            ) || (w.contains('=') && !w.starts_with('-'))
            {
                words.next();
            } else if *w == "timeout" {
                words.nth(1);
            } else {
                break;
            }
        }
        if words.next().is_none_or(|w| !is_bp(w)) {
            continue;
        }
        let rest: Vec<&str> = words.collect();
        if let Some(i) = rest.iter().position(|w| !w.starts_with('-')) {
            out.push(BpCall { verb: rest[i].to_string(), args: rest[i + 1..].join(" ") });
        }
    }
    out
}

/// Where a character of a command sits: in quotes, or in a substitution that opened inside double quotes.
#[derive(Clone, Copy, PartialEq)]
enum Quote {
    Single,
    /// ANSI-C quoting, `$'…'`: a backslash escapes, so `\'` does not close it.
    Ansi,
    Double,
    /// `$(` inside double quotes, with the depth of the parentheses opened in it.
    Sub(u32),
    /// A backtick inside double quotes.
    Tick,
}

/// The command split at `;`, `&`, `|`, a newline, a parenthesis and a backtick, none of them quoted or escaped: a quoted
/// `;` is text, not a second command. A `$(` or a backtick inside double quotes opens a substitution, which splits as
/// the command does, so `"$(bp show x)"` is still a call. A comment runs to the end of its line, so its apostrophe
/// opens no quote.
fn segments(command: &str) -> Vec<String> {
    let mut stack: Vec<Quote> = Vec::new();
    let mut out = vec![String::new()];
    let mut chars = command.chars().peekable();
    let mut word_start = true;
    while let Some(c) = chars.next() {
        // An escaped character is text, outside single quotes. In `$'…'` it is too, so `$'it\'s'` stays one word.
        if c == '\\' && stack.last() != Some(&Quote::Single) {
            let segment = out.last_mut().expect("one segment");
            segment.push(c);
            segment.extend(chars.next());
            word_start = false;
            continue;
        }
        let split = match stack.last().copied() {
            Some(Quote::Single | Quote::Ansi) => {
                if c == '\'' {
                    stack.pop();
                }
                false
            }
            Some(Quote::Double) => match c {
                '"' => {
                    stack.pop();
                    false
                }
                '$' if chars.peek() == Some(&'(') => {
                    chars.next();
                    stack.push(Quote::Sub(0));
                    true
                }
                '`' => {
                    stack.push(Quote::Tick);
                    true
                }
                _ => false,
            },
            top => match c {
                '$' if chars.peek() == Some(&'\'') => {
                    out.last_mut().expect("one segment").push(c);
                    out.last_mut().expect("one segment").extend(chars.next());
                    stack.push(Quote::Ansi);
                    word_start = false;
                    continue;
                }
                '#' if word_start => {
                    while chars.next_if(|n| *n != '\n').is_some() {}
                    continue;
                }
                '\'' => {
                    stack.push(Quote::Single);
                    false
                }
                '"' => {
                    stack.push(Quote::Double);
                    false
                }
                '(' => {
                    if let Some(Quote::Sub(depth)) = stack.last_mut() {
                        *depth += 1;
                    }
                    true
                }
                ')' => {
                    match stack.last_mut() {
                        Some(Quote::Sub(0)) => drop(stack.pop()),
                        Some(Quote::Sub(depth)) => *depth -= 1,
                        _ => {}
                    }
                    true
                }
                '`' => {
                    if top == Some(Quote::Tick) {
                        stack.pop();
                    }
                    true
                }
                ';' | '&' | '|' | '\n' => true,
                _ => false,
            },
        };
        word_start = split || c.is_whitespace();
        if split {
            out.push(String::new());
        } else {
            out.last_mut().expect("one segment").push(c);
        }
    }
    out
}

fn without_heredocs(command: &str) -> String {
    let mut kept = Vec::new();
    let mut held = Vec::new();
    let mut open: Vec<&str> = Vec::new();
    for line in command.lines() {
        if let Some(&end) = open.first() {
            held.push(line);
            if line.trim() == end {
                open.remove(0);
                if open.is_empty() {
                    held.clear();
                }
            }
            continue;
        }
        open.extend(heredoc_words(line));
        kept.push(line);
    }
    kept.extend(held);
    kept.join("\n")
}

/// The text without its redirects (`N>&M`, `>&N`, `&>f`, `N>f`, `>f`, `>>f`, `<f`), which sit at the start of a word. A
/// `<<` opener and a `<<<` here-string stay: the first is the heredoc scan's, and the second reads as a word.
fn without_redirects(text: &str) -> String {
    let b = text.as_bytes();
    // A quote ends a redirect's target too: the redirect goes, the quote stays, and the quotes stay paired.
    let stops = |c: u8| c.is_ascii_whitespace() || b";&|()`'\"".contains(&c);
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < b.len() {
        if i == 0 || stops(b[i - 1]) {
            let mut j = i;
            while b.get(j).is_some_and(u8::is_ascii_digit) {
                j += 1;
            }
            let op = if b[j..].starts_with(b"&>>") {
                3
            } else if b[j..].starts_with(b"&>") || b[j..].starts_with(b">>") || b[j..].starts_with(b">&") {
                2
            } else {
                usize::from(b.get(j) == Some(&b'>') || (b.get(j) == Some(&b'<') && b.get(j + 1) != Some(&b'<')))
            };
            if op > 0 {
                j += op;
                while j < b.len() && !stops(b[j]) {
                    j += 1;
                }
                out.push(' ');
                i = j;
                continue;
            }
        }
        let c = text[i..].chars().next().unwrap_or_default();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// The terminators of the `<<WORD`, `<<-WORD`, `<<\WORD` and quoted forms on one line; `<<<` is a here-string, no
/// heredoc. A quoted word runs to its closing quote.
fn heredoc_words(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(i) = rest.find("<<") {
        rest = &rest[i + 2..];
        if rest.starts_with('<') {
            rest = rest.trim_start_matches('<');
            continue;
        }
        let word = rest.strip_prefix('-').unwrap_or(rest).trim_start();
        let word = if let Some(q @ ('\'' | '"')) = word.chars().next() {
            word[1..].split_once(q).map_or("", |(w, _)| w)
        } else {
            let word = word.strip_prefix('\\').unwrap_or(word);
            &word[..word.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(word.len())]
        };
        if !word.is_empty() {
            out.push(word);
        }
    }
    out
}

fn is_bp(word: &str) -> bool {
    let mut parts = word.rsplit('/');
    parts.next() == Some("bp") && parts.next().is_none_or(|dir| dir.starts_with("bin"))
}

/// The sha256 of a file as `shasum` prints it: a hash the result file holds, so a swapped transcript shows.
pub fn sha256(file: &Path) -> Result<String> {
    let out = Command::new("shasum").args(["-a", "256"]).arg(file).output().context("run shasum")?;
    if !out.status.success() {
        bail!("shasum {} exited {}", file.display(), out.status);
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text.split_whitespace().next().unwrap_or_default().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(blocks: &str) -> String {
        format!(r#"{{"type":"assistant","message":{{"model":"m1","content":[{blocks}]}}}}"#)
    }

    #[test]
    fn finds_bp_calls_behind_prefixes_and_separators() {
        let c = calls_in(
            "cd /x && BP_ROLE=worker timeout 5 bp walk a --for review --json | jq .; export A=b; /t/bin/bp list claim",
        );
        let seen: Vec<_> = c.iter().map(|c| (c.verb.as_str(), c.args.as_str())).collect();
        assert_eq!(seen, [("walk", "a --for review --json"), ("list", "claim")]);
        assert!(calls_in("echo bp land; git diff").is_empty(), "a word later in a segment is no call");
        assert!(
            calls_in("target/release/bp set x; ./target/debug/bp create y; $B set z").is_empty(),
            "code under test"
        );
        assert!(calls_in("bp --help").is_empty(), "no verb");
    }

    #[test]
    fn skips_heredoc_bodies() {
        let rust =
            "cat <<'EOF' > f.rs\n/// `bp set <id> x`\nlet s = format!(\"bp record delta ...\");\nbp set <id> y\nEOF";
        assert!(calls_in(rust).is_empty(), "a Rust body is no call");
        let python = "python3 - <<'PY'\nimport os\nos.system(\"bp show x\")\nbp land x\nPY\n";
        assert!(calls_in(python).is_empty(), "a Python body is no call");
        let after = "cat <<-EOF > f\n\tbp set a b\n\tEOF\nbp cut a1 --json\n";
        let seen: Vec<_> = calls_in(after).into_iter().map(|c| (c.verb, c.args)).collect();
        assert_eq!(seen, [("cut".to_string(), "a1 --json".to_string())], "a call after the terminator counts");
        let two = "cat <<A <<\"B\"\nbp set 1\nA\nbp set 2\nB\nbp show z";
        assert_eq!(calls_in(two).len(), 1, "two heredocs on one line");
        assert_eq!(calls_in("cat <<< bp\nbp show y").len(), 1, "a here-string opens no body");
    }

    #[test]
    fn skips_redirects() {
        assert!(calls_in("bp --help 2>&1 | head -40").is_empty(), "no verb behind a 2>&1");
        let verbs = |c: &str| calls_in(c).into_iter().map(|c| c.verb).collect::<Vec<_>>();
        assert_eq!(verbs("cmd &> log; bp cut x --json"), ["cut"]);
        assert_eq!(verbs("bp show x >out.txt 2>>err.txt </dev/null; bp walk y >&2"), ["show", "walk"]);
        assert_eq!(calls_in("bp show x 2>&1 | head")[0].args, "x", "the redirect leaves no argument");
        assert_eq!(verbs("cat <<< bp\nbp show y"), ["show"], "a here-string stays");
    }

    #[test]
    fn a_stray_double_angle_opens_no_body() {
        let verbs = |c: &str| calls_in(c).into_iter().map(|c| c.verb).collect::<Vec<_>>();
        assert_eq!(verbs("echo \"a << b\"; bp cut x --json"), ["cut"]);
        assert_eq!(verbs("echo $((1<<3)); bp walk y"), ["walk"]);
        assert_eq!(verbs("python3 -c \"print('<<')\"\nbp show z"), ["show"]);
        assert_eq!(verbs("python3 -c \"print(1 << 4)\"\nbp check i --json"), ["check"], "an unclosed body is kept");
        assert!(calls_in("cat <<'PR-BODY'\nbp set fake\nPR-BODY").is_empty(), "a quoted word with a dash");
        assert_eq!(verbs("cat <<'PR-BODY'\nbp set fake\nPR-BODY\nbp show real"), ["show"]);
        assert_eq!(verbs("cat <<\\EOF\nbp set fake\nEOF\nbp show real"), ["show"], "a backslash word");
    }

    #[test]
    fn counts_calls_after_a_command_keyword() {
        let verbs = |c: &str| calls_in(c).into_iter().map(|c| c.verb).collect::<Vec<_>>();
        assert_eq!(verbs("for n in 1 2; do bp record x"), ["record"]);
        assert_eq!(verbs("d(){ bp record x; }"), ["record"]);
        assert_eq!(verbs("if true; then bp pr i; else bp walk i; fi"), ["pr", "walk"]);
        assert_eq!(verbs("! bp check i"), ["check"]);
    }

    #[test]
    fn counts_calls_after_if_and_while() {
        let verbs = |c: &str| calls_in(c).into_iter().map(|c| c.verb).collect::<Vec<_>>();
        assert_eq!(verbs("if bp check i --json; then echo ok; fi"), ["check"]);
        assert_eq!(verbs("if false; then :; elif bp list claim; then :; fi"), ["list"]);
        assert_eq!(verbs("while ! bp walk x --for review --done; do sleep 1; done"), ["walk"]);
        assert_eq!(verbs("until bp show x; do :; done"), ["show"]);
    }

    #[test]
    fn a_quoted_semicolon_does_not_split() {
        let verbs = |c: &str| calls_in(c).into_iter().map(|c| c.verb).collect::<Vec<_>>();
        assert_eq!(verbs("echo 'a; bp set x'; bp show y"), ["show"], "single quotes");
        assert!(verbs("git commit -qm \"fix; bp land z\"").is_empty(), "double quotes");
        assert_eq!(verbs("echo \"a | bp cut x\" && bp walk y"), ["walk"], "a quoted pipe");
        assert_eq!(verbs("echo \"it's; bp set x\"; bp show y"), ["show"], "an apostrophe in double quotes");
        assert_eq!(verbs("x=\"$(bp show x --json)\"; echo \"`bp list claim`\""), ["show", "list"], "substitutions");
        assert_eq!(verbs("# don't; bp set x\nbp show z"), ["show"], "a comment opens no quote");
        assert_eq!(verbs("find . -exec cat {} \\; ; bp walk q"), ["walk"], "an escaped semicolon");
        assert_eq!(verbs("echo hi >\"out file\"; bp show y"), ["show"], "a quoted redirect target");
        assert_eq!(verbs("echo $'it\\'s; x'; bp set n status x"), ["set"], "an escaped quote in ANSI-C quoting");
    }

    #[test]
    fn counts_substitutions() {
        let c = calls_in("x=$(bp show x --json); echo `bp list claim`");
        let seen: Vec<_> = c.iter().map(|c| c.verb.as_str()).collect();
        assert_eq!(seen, ["show", "list"]);
    }

    #[test]
    fn reads_commands_the_final_text_and_the_model() {
        let text = [
            line(
                r#"{"type":"tool_use","name":"Bash","input":{"command":"bp show x"}},{"type":"text","text":"working"}"#,
            ),
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"read body here"}]}}"#.to_string(),
            r#"{"type":"user","isMeta":true,"message":{"content":"<skill-format>true</skill-format>\npreload body here"}}"#
                .to_string(),
            r#"{"type":"system","subtype":"eval_failed","reason":"ran past"}"#.to_string(),
            "not json".to_string(),
            line(r#"{"type":"text","text":"head: abc"},{"type":"tool_use","name":"Read","input":{}}"#),
            line(r#"{"type":"tool_use","name":"Read","input":{}}"#),
        ]
        .join("\n");
        let t = Trace::parse(&text);
        assert_eq!(t.commands(), ["bp show x"]);
        assert_eq!(t.final_text(), "head: abc");
        assert_eq!(t.models().into_iter().collect::<Vec<_>>(), ["m1"]);
        assert!(t.mentions("read body") && t.mentions("preload body") && !t.mentions("absent"));
        assert!(t.preloaded("preload body") && !t.preloaded("read body"), "a read is no preload");
        assert_eq!(t.failure(), Some("ran past"));
    }
}
