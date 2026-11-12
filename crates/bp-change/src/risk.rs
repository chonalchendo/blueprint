//! The risk level of an issue (37, design changes 1 and 2; 36, section 1). One pure function gives the level in
//! force: the highest of the declared level, the level the diff computes against `.bp/risk.yml`, and each raise.
//! Every reader calls it, so no two readers disagree (21, rule 1).

use std::collections::BTreeSet;
use std::fmt;

use yaml_rust2::{Yaml, YamlLoader};

bp_vault::closed_enum! {
    /// A risk level. The order is the declaration order, so the highest of several is their `max`.
    pub enum Level {
        Low = "low",
        Medium = "medium",
        High = "high",
    }
}

/// The severity of a review row that raises the issue's level to high (37, design change 2). The row needs no
/// disposition and blocks nothing by itself.
pub const RISK_ROW: &str = "risk";

/// The risk file, at the top of the code repository. A diff that touches it is high: it holds the rules of the gate.
pub const RISK_FILE: &str = ".bp/risk.yml";

/// The level in force: the highest of the declared level, the computed level and the raises. Each raise is high. A
/// computed level that nothing could read counts for nothing.
pub fn level(declared: Level, computed: Option<Level>, raises: usize) -> Level {
    let floor = if raises > 0 { Level::High } else { Level::Low };
    declared.max(computed.unwrap_or(Level::Low)).max(floor)
}

/// One path pattern of the risk file. No glob library: a prefix that ends in `/`, a `*.<ext>` suffix, or an exact
/// path (37, design change 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pattern {
    /// Each path under this directory; `infra/` holds `infra/a.tf`, not `infrastructure.md`.
    Dir(String),
    /// Each file whose name ends in this suffix, dot included; `.yml` holds `a/b.yml`, not `a.yml.bak`.
    Ext(String),
    /// This path alone.
    Exact(String),
}

impl Pattern {
    /// Reads one pattern. A wildcard outside the leading `*.` and a path that git never prints (empty, absolute, or
    /// with a `./` or `..` segment) refuse, since such a pattern would never match and a high path would pass as low.
    pub fn parse(text: &str) -> Result<Self, String> {
        let bad =
            |why: &str| Err(format!("the pattern {text:?} {why}; write a prefix ending in `/`, `*.<ext>` or a path"));
        let (rest, ext) = match text.strip_prefix("*.") {
            Some(ext) => (ext, true),
            None => (text, false),
        };
        if rest.is_empty() {
            return bad("is empty");
        }
        if rest.contains(['*', '?', '[', ']', '{', '}', '\\']) {
            return bad("holds a wildcard, and the risk file reads no glob");
        }
        if ext {
            return if rest.contains('/') {
                bad("names a directory after `*.`")
            } else {
                Ok(Self::Ext(format!(".{rest}")))
            };
        }
        if rest.starts_with('/') || rest.split('/').any(|s| s == "." || s == "..") || rest.contains("//") {
            return bad("is not a path relative to the repository's top");
        }
        Ok(if rest.ends_with('/') { Self::Dir(rest.into()) } else { Self::Exact(rest.into()) })
    }

    /// Whether the path, as git prints it from the repository's top, matches.
    pub fn matches(&self, path: &str) -> bool {
        match self {
            Self::Dir(dir) => path.starts_with(dir.as_str()),
            Self::Ext(ext) => path.rsplit('/').next().is_some_and(|name| name.ends_with(ext.as_str())),
            Self::Exact(exact) => path == exact,
        }
    }
}

impl fmt::Display for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dir(p) | Self::Exact(p) => f.write_str(p),
            Self::Ext(e) => write!(f, "*{e}"),
        }
    }
}

/// The size limits of the risk file (36, section 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// A low diff changes fewer lines than this.
    pub low_lines: u64,
    /// A diff whose lines outside the low patterns exceed this is high.
    pub high_lines: u64,
    /// A diff that changes more top-level directories than this is high.
    pub packages: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self { low_lines: 150, high_lines: 800, packages: 3 }
    }
}

/// The risk file, read. A missing file is the default: no pattern and the default limits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    /// A changed path that matches one of these is high.
    pub high: Vec<Pattern>,
    /// The test and documentation paths: a diff of only these, and small, is low.
    pub low: Vec<Pattern>,
    /// The size limits.
    pub limits: Limits,
    /// The human's OpenSSH public key line, which verifies a signed act (37, design change 4). Kept, not verified here.
    pub signer: Option<String>,
}

impl Config {
    /// Reads the risk file's text. A key outside `high`, `low`, `limits` and `signer`, or a value outside its shape,
    /// refuses: a misspelt `high` would drop each high path without a word.
    pub fn parse(text: &str) -> Result<Self, String> {
        let docs = YamlLoader::load_from_str(text).map_err(|e| format!("it is not YAML: {e}"))?;
        let map = match docs.as_slice() {
            [] | [Yaml::Null] => return Ok(Self::default()),
            [Yaml::Hash(map)] => map,
            [_] => return Err("its top is not a mapping of high, low, limits and signer".into()),
            _ => return Err("it holds more than one YAML document".into()),
        };
        let mut config = Self::default();
        for (key, value) in map {
            match key.as_str() {
                Some("high") => config.high = patterns("high", value)?,
                Some("low") => config.low = patterns("low", value)?,
                Some("limits") => config.limits = limits(value)?,
                Some("signer") => config.signer = signer(value)?,
                _ => {
                    return Err(format!(
                        "{} is no key of the risk file: the keys are high, low, limits and signer",
                        name(key)
                    ));
                }
            }
        }
        Ok(config)
    }
}

fn patterns(key: &str, value: &Yaml) -> Result<Vec<Pattern>, String> {
    match value {
        Yaml::Null => Ok(Vec::new()),
        Yaml::Array(items) => items
            .iter()
            .map(|i| i.as_str().ok_or_else(|| format!("{key} holds {}, which is not a path pattern", name(i))))
            .map(|t| t.and_then(Pattern::parse))
            .collect(),
        _ => Err(format!("{key} is not a list of path patterns")),
    }
}

fn limits(value: &Yaml) -> Result<Limits, String> {
    let mut limits = Limits::default();
    let map = match value {
        Yaml::Null => return Ok(limits),
        Yaml::Hash(map) => map,
        _ => return Err("limits is not a mapping of low_lines, high_lines and packages".into()),
    };
    for (key, value) in map {
        let slot = match key.as_str() {
            Some("low_lines") => &mut limits.low_lines,
            Some("high_lines") => &mut limits.high_lines,
            Some("packages") => &mut limits.packages,
            _ => return Err(format!("{} is no limit: the limits are low_lines, high_lines and packages", name(key))),
        };
        *slot = value
            .as_i64()
            .and_then(|n| u64::try_from(n).ok())
            .ok_or_else(|| format!("the limit {} is {}, not a whole number of 0 or more", name(key), name(value)))?;
    }
    Ok(limits)
}

/// A YAML key or value as the file wrote it, for a refusal.
fn name(value: &Yaml) -> String {
    match value {
        Yaml::String(s) | Yaml::Real(s) => format!("{s:?}"),
        Yaml::Integer(n) => n.to_string(),
        Yaml::Boolean(b) => b.to_string(),
        Yaml::Null => "null".into(),
        _ => "a list or a mapping".into(),
    }
}

fn signer(value: &Yaml) -> Result<Option<String>, String> {
    match value {
        Yaml::Null => Ok(None),
        Yaml::String(line) if !line.trim().is_empty() && !line.trim().contains('\n') => Ok(Some(line.trim().into())),
        _ => Err("signer is not one OpenSSH public key line".into()),
    }
}

/// One changed file of a diff, as `git diff --numstat -z` prints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    /// The path after the change.
    pub path: String,
    /// The path before a rename or a copy.
    pub from: Option<String>,
    /// The lines added; 0 for a binary file.
    pub added: u64,
    /// The lines removed; 0 for a binary file.
    pub removed: u64,
}

impl Changed {
    /// Both paths of a rename, else the one path: a file moved out of a high directory is a change to it.
    fn paths(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.path.as_str()).chain(self.from.as_deref())
    }

    fn lines(&self) -> u64 {
        self.added + self.removed
    }
}

/// Reads the output of `git diff --numstat -z`: each entry is `<added>\t<removed>\t<path>\0`, or for a rename
/// `<added>\t<removed>\t\0<from>\0<to>\0`. Git prints `-` for the counts of a binary file.
pub fn numstat(text: &str) -> Vec<Changed> {
    let mut fields = text.split('\0');
    let mut out = Vec::new();
    while let Some(entry) = fields.next() {
        let mut cells = entry.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (cells.next(), cells.next(), cells.next()) else { continue };
        let count = |n: &str| n.trim().parse().unwrap_or(0);
        let (from, path) = if path.is_empty() {
            (fields.next().map(String::from), fields.next().unwrap_or_default().to_string())
        } else {
            (None, path.to_string())
        };
        out.push(Changed { path, from, added: count(added), removed: count(removed) });
    }
    out
}

/// The computed level and each rule that set it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Computed {
    /// The level the diff computes.
    pub level: Level,
    /// Each rule that set the level, in words: a path and its pattern, a line count, a package count.
    pub reasons: Vec<String>,
}

/// The level of a diff (36, section 1). High when the issue caused a pending delta, when a path is the risk file or
/// matches a high pattern, when the lines outside the low patterns exceed `high_lines`, or when more top-level
/// directories than `packages` change. Low only when each path matches a low pattern and the diff changes fewer than
/// `low_lines` lines. Medium otherwise. A file at the top is in no directory, so it counts as no package.
pub fn computed(diff: &[Changed], config: &Config, delta: bool) -> Computed {
    let limits = config.limits;
    let mut high = Vec::new();
    if delta {
        high.push("the issue caused a pending delta, and a design change is high".to_string());
    }
    for path in diff.iter().flat_map(Changed::paths) {
        if path == RISK_FILE {
            high.push(format!("{path} is the risk file, and an edit to the gate is high"));
        } else if let Some(p) = config.high.iter().find(|p| p.matches(path)) {
            high.push(format!("{path} matches the high pattern {p}"));
        }
    }
    let low = |c: &&Changed| c.paths().all(|path| config.low.iter().any(|p| p.matches(path)));
    let outside: u64 = diff.iter().filter(|c| !low(c)).map(Changed::lines).sum();
    if outside > limits.high_lines {
        high.push(format!("{outside} changed lines outside the low patterns exceed high_lines {}", limits.high_lines));
    }
    let packages: BTreeSet<&str> =
        diff.iter().flat_map(Changed::paths).filter_map(|p| p.split_once('/').map(|(dir, _)| dir)).collect();
    if packages.len() as u64 > limits.packages {
        let (n, names) = (packages.len(), packages.into_iter().collect::<Vec<_>>().join(", "));
        high.push(format!("{n} top-level directories change ({names}), more than packages {}", limits.packages));
    }
    if !high.is_empty() {
        return Computed { level: Level::High, reasons: high };
    }
    let total: u64 = diff.iter().map(Changed::lines).sum();
    let not_low: Vec<&str> = diff.iter().filter(|c| !low(c)).map(|c| c.path.as_str()).collect();
    let mut reasons = Vec::new();
    match not_low.as_slice() {
        [] => {}
        [one] => reasons.push(format!("{one} matches no low pattern")),
        [first, rest @ ..] => {
            reasons.push(format!("{first} and {} more changed paths match no low pattern", rest.len()));
        }
    }
    if total >= limits.low_lines {
        reasons.push(format!("{total} changed lines reach low_lines {}", limits.low_lines));
    }
    if reasons.is_empty() {
        let why = format!(
            "each changed path matches a low pattern, and {total} changed lines are under low_lines {}",
            limits.low_lines
        );
        return Computed { level: Level::Low, reasons: vec![why] };
    }
    Computed { level: Level::Medium, reasons }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(path: &str, added: u64, removed: u64) -> Changed {
        Changed { path: path.into(), from: None, added, removed }
    }

    fn renamed(from: &str, to: &str) -> Changed {
        Changed { path: to.into(), from: Some(from.into()), added: 0, removed: 0 }
    }

    fn config(text: &str) -> Config {
        Config::parse(text).unwrap()
    }

    #[test]
    fn risk_is_highest_of_declared_computed_raised() {
        use Level::{High, Low, Medium};
        for declared in [Low, Medium, High] {
            for computed in [None, Some(Low), Some(Medium), Some(High)] {
                for raises in [0, 1, 2] {
                    let want =
                        *[Some(declared), computed, (raises > 0).then_some(High)].iter().flatten().max().unwrap();
                    assert_eq!(level(declared, computed, raises), want, "{declared} {computed:?} {raises}");
                }
            }
        }
        // The computed level from real diffs, through the same function.
        let rules = config("high: [infra/, '*.yml', Cargo.toml]\nlow: [docs/, '*.md']\n");
        let at = |diff: &[Changed]| computed(diff, &rules, false).level;
        assert_eq!(at(&[change("infrastructure.md", 3, 0)]), Low, "infra/ stops at its slash");
        assert_eq!(at(&[change("infra/main.tf", 1, 0)]), High);
        assert_eq!(at(&[change("a.yml.bak", 1, 0)]), Medium, "*.yml is a suffix of the name");
        assert_eq!(at(&[change("ci/a.yml", 1, 0)]), High);
        assert_eq!(at(&[change("Cargo.toml.orig", 1, 0)]), Medium, "an exact path is exact");
        assert_eq!(at(&[]), Low, "an empty diff changes no path outside the low patterns");
        assert_eq!(at(&[renamed("src/a.rs", "src/b.rs")]), Medium, "a rename with no changed line still moves code");
        assert_eq!(at(&[renamed("docs/a.md", "docs/b.md")]), Low);
        assert_eq!(at(&[renamed("infra/a.tf", "src/a.tf")]), High, "the old path of a rename counts");
        assert_eq!(level(Medium, Some(at(&[change("docs/a.md", 2, 1)])), 0), Medium, "a low diff lowers nothing");
        assert_eq!(level(Low, Some(at(&[change("src/a.rs", 2, 1)])), 0), Medium);
        assert_eq!(level(Low, Some(at(&[change("docs/a.md", 2, 1)])), 1), High, "a raise holds over a low diff");
        assert_eq!(computed(&[change("docs/a.md", 1, 0)], &rules, true).level, High, "a pending delta is high");
    }

    #[test]
    fn risk_file_edit_is_high() {
        let none = Config::default();
        let lenient = config("low: [.bp/, '*.yml']\nlimits: {low_lines: 1000, high_lines: 100000, packages: 100}\n");
        for rules in [&none, &lenient] {
            for diff in [
                vec![change(RISK_FILE, 1, 0)],
                vec![change(RISK_FILE, 0, 0)],
                vec![renamed(RISK_FILE, ".bp/old.yml")],
                vec![renamed(".bp/new.yml", RISK_FILE)],
                vec![change("docs/a.md", 1, 0), change(RISK_FILE, 0, 1)],
            ] {
                let c = computed(&diff, rules, false);
                assert_eq!(c.level, Level::High, "{diff:?}");
                assert!(c.reasons.iter().any(|r| r.starts_with(".bp/risk.yml is the risk file")), "{c:?}");
            }
        }
        for near in [".bp/risk.yml.bak", "bp/risk.yml", "x/.bp/risk.yml", ".bp/risk.yaml"] {
            assert_eq!(computed(&[change(near, 1, 0)], &lenient, false).level, Level::Low, "{near} is not the file");
        }
        assert_eq!(computed(&[], &none, false).level, Level::Low, "an empty diff edits no file");
    }

    #[test]
    fn the_size_and_package_limits_set_high_and_low() {
        let rules = config("low: [tests/, docs/]\nlimits: {low_lines: 10, high_lines: 20, packages: 2}\n");
        let at = |diff: &[Changed]| computed(diff, &rules, false);
        assert_eq!(at(&[change("tests/a.rs", 5, 4)]).level, Level::Low);
        assert_eq!(at(&[change("tests/a.rs", 5, 5)]).level, Level::Medium, "low needs fewer lines than low_lines");
        assert_eq!(at(&[change("tests/a.rs", 500, 0)]).level, Level::Medium, "test lines never make a diff high");
        assert_eq!(at(&[change("src/a.rs", 20, 0)]).level, Level::Medium, "high needs more lines than high_lines");
        let big = at(&[change("src/a.rs", 20, 1), change("tests/a.rs", 900, 0)]);
        assert_eq!(big.level, Level::High);
        assert_eq!(big.reasons, ["21 changed lines outside the low patterns exceed high_lines 20"]);
        assert_eq!(at(&[change("a/x", 1, 0), change("b/x", 1, 0), change("README", 1, 0)]).level, Level::Medium);
        let wide = at(&[change("a/x", 1, 0), change("b/x", 1, 0), change("c/x", 1, 0)]);
        assert_eq!(wide.reasons, ["3 top-level directories change (a, b, c), more than packages 2"]);
        let medium = at(&[change("src/a.rs", 1, 0), change("src/b.rs", 1, 0)]);
        assert_eq!(medium.reasons, ["src/a.rs and 1 more changed paths match no low pattern"]);
        assert_eq!(
            at(&[change("src/a.rs", 10, 0)]).reasons,
            ["src/a.rs matches no low pattern", "10 changed lines reach low_lines 10"]
        );
    }

    #[test]
    fn the_risk_file_reads_its_four_keys_and_refuses_the_rest() {
        assert_eq!(config(""), Config::default());
        assert_eq!(config("# nothing yet\n"), Config::default());
        let full = config(
            "high: [infra/, '*.tf', Cargo.lock]\nlow:\n  - tests/\nlimits:\n  low_lines: 50\nsigner: ssh-ed25519 AAAAC3 me\n",
        );
        assert_eq!(
            full.high,
            [Pattern::Dir("infra/".into()), Pattern::Ext(".tf".into()), Pattern::Exact("Cargo.lock".into())]
        );
        assert_eq!(full.low, [Pattern::Dir("tests/".into())]);
        assert_eq!(full.limits, Limits { low_lines: 50, ..Limits::default() });
        assert_eq!(full.signer.as_deref(), Some("ssh-ed25519 AAAAC3 me"));
        for (text, says) in [
            ("high: [a/\n", "not YAML"),
            ("- a/\n", "not a mapping"),
            ("hihg: [infra/]\n", "no key"),
            ("high: infra/\n", "not a list"),
            ("high: [1]\n", "not a path pattern"),
            ("high: ['src/*.rs']\n", "wildcard"),
            ("high: ['*']\n", "wildcard"),
            ("high: ['*.']\n", "is empty"),
            ("high: ['*./x']\n", "directory after"),
            ("high: [./infra/]\n", "relative"),
            ("high: [/infra/]\n", "relative"),
            ("low: ['']\n", "is empty"),
            ("limits: 3\n", "not a mapping"),
            ("limits: {lines: 3}\n", "no limit"),
            ("limits: {packages: -1}\n", "whole number"),
            ("limits: {low_lines: 1.5}\n", "whole number"),
            ("signer: [a]\n", "public key"),
            ("high: [a/]\nhigh: [b/]\n", "not YAML"),
            ("a: 1\n---\nb: 2\n", "more than one"),
        ] {
            let e = Config::parse(text).unwrap_err();
            assert!(e.contains(says), "{text:?}: {e}");
        }
    }

    #[test]
    fn numstat_reads_paths_renames_and_binary_files() {
        let entries =
            ["3\t1\tsrc/a b.rs", "-\t-\tlogo.png", "0\t0\t", "docs/old.md", "docs/new.md", "2\t0\tREADME", ""];
        let text = entries.join("\0");
        assert_eq!(
            numstat(&text),
            [
                change("src/a b.rs", 3, 1),
                change("logo.png", 0, 0),
                renamed("docs/old.md", "docs/new.md"),
                change("README", 2, 0),
            ]
        );
        assert_eq!(numstat(""), []);
    }
}
