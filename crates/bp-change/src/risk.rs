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

/// The gate's own paths (36, section 1: CI workflows, bp hooks, risk rules, agent settings). They are high with or
/// without a risk file; the file's `high` list adds to them and cannot remove one.
pub const BUILT_IN_HIGH: [&str; 5] = [".bp/", ".github/workflows/", ".claude/", ".codex/", ".agents/"];

/// The files that change how git reads every other path: what a submodule is, and how a file is diffed, merged or
/// checked out. They are high in any directory, as the built-in high paths are.
pub const BUILT_IN_HIGH_NAMES: [&str; 2] = [".gitmodules", ".gitattributes"];

/// The level in force: the highest of the declared level, the computed level and the raises. Each raise is high: a
/// review's `risk` row, or a pending delta the issue caused. A computed level that nothing could read counts for
/// nothing.
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
    /// Reads one pattern. A wildcard outside the leading `*.`, white space at an end, and a path that git never
    /// prints (empty, absolute, or with a `./` or `..` segment) refuse, since such a pattern would never match and a
    /// high path would pass as low.
    pub fn parse(text: &str) -> Result<Self, String> {
        let bad =
            |why: &str| Err(format!("the pattern {text:?} {why}; write a prefix ending in `/`, `*.<ext>` or a path"));
        if text.trim() != text {
            return bad("has white space at an end");
        }
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

    /// [`Self::matches`] with ASCII case folded on both sides, and a `Dir` that also holds its own name and each
    /// parent's. A high pattern matches so: a case-folding volume, as macOS's default, checks `.Claude/x` out as
    /// `.claude/x`, and a link named `infra` moves all of `infra/`, so a trick can only raise the level.
    pub fn matches_any_case(&self, path: &str) -> bool {
        let path = path.to_ascii_lowercase();
        match self {
            Self::Dir(p) => touches(&p.to_ascii_lowercase(), &path),
            Self::Ext(p) => Self::Ext(p.to_ascii_lowercase()).matches(&path),
            Self::Exact(p) => p.to_ascii_lowercase() == path,
        }
    }
}

/// Whether `path` is under the directory `dir`, which ends in `/`, is its name, or is the name of a parent. A link or
/// a file at such a name points the whole directory elsewhere, so it is a change to the directory.
fn touches(dir: &str, path: &str) -> bool {
    let at = format!("{path}/");
    at.starts_with(dir) || dir.starts_with(&at)
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
    /// The size limits; `low_lines` is at most `high_lines`.
    pub limits: Limits,
    /// The human's OpenSSH public key line, which verifies a signed act (37, design change 4). Kept, not verified here.
    pub signer: Option<String>,
    /// The Blueprint commit that the forge check `bp-gate` builds `bp` from (37, design change 5). The pin sits in this
    /// file, so a change of it is high and needs the human's signature; `bp gate` refuses a base with none.
    pub gate: Option<String>,
}

impl Config {
    /// Reads the risk file's text. A key outside `high`, `low`, `limits`, `signer` and `gate`, or a value outside its
    /// shape, refuses: a misspelt `high` would drop each high path without a word.
    pub fn parse(text: &str) -> Result<Self, String> {
        let docs = YamlLoader::load_from_str(text).map_err(|e| format!("it is not YAML: {e}"))?;
        let map = match docs.as_slice() {
            [] | [Yaml::Null] => return Ok(Self::default()),
            [Yaml::Hash(map)] => map,
            [_] => return Err("its top is not a mapping of high, low, limits, signer and gate".into()),
            _ => return Err("it holds more than one YAML document".into()),
        };
        let mut config = Self::default();
        for (key, value) in map {
            match key.as_str() {
                Some("high") => config.high = patterns("high", value)?,
                Some("low") => config.low = patterns("low", value)?,
                Some("limits") => config.limits = limits(value)?,
                Some("signer") => config.signer = signer(value)?,
                Some("gate") => config.gate = gate(value)?,
                _ => {
                    return Err(format!(
                        "{} is no key of the risk file: the keys are high, low, limits, signer and gate",
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
    if limits.low_lines > limits.high_lines {
        let (low, high) = (limits.low_lines, limits.high_lines);
        return Err(format!(
            "low_lines {low} is over high_lines {high}, so no diff could be low and under the high limit"
        ));
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

/// One OpenSSH public key line: `<type> <base64 key> [comment]`, the type `ssh-…`, `ecdsa-…` or `sk-…`. A block
/// scalar ends in a newline, so it is no line: the signer is a plain or a quoted string.
fn signer(value: &Yaml) -> Result<Option<String>, String> {
    let line = match value {
        Yaml::Null => return Ok(None),
        Yaml::String(line) if !line.contains(['\n', '\r']) => line.trim(),
        _ => "",
    };
    let mut words = line.split(' ');
    let kind = words.next().unwrap_or_default();
    let key = words.next().unwrap_or_default();
    let typed = ["ssh-", "ecdsa-", "sk-"].iter().any(|p| kind.starts_with(p));
    let base64 = !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b));
    if typed && base64 {
        return Ok(Some(line.into()));
    }
    Err("signer is not one OpenSSH public key line: `<type> <base64 key> [comment]`, as a `.pub` file holds it".into())
}

/// The gate pin: a full commit sha, as `bp install` writes it. The workflow reads the line `gate: <sha>` with no YAML
/// parser, so a quoted value refuses there; here it reads as the same string.
fn gate(value: &Yaml) -> Result<Option<String>, String> {
    match value {
        Yaml::Null => Ok(None),
        Yaml::String(sha) if is_commit(sha) => Ok(Some(sha.clone())),
        _ => Err(format!("gate is {}, not the 40 lower-case hex digits of a Blueprint commit", name(value))),
    }
}

/// A full SHA-1 commit name: 40 lower-case hex digits, as a gate pin is.
pub fn is_commit(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// One changed file of a diff, as `git diff --raw --numstat -z` prints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    /// The path after the change.
    pub path: String,
    /// The path before a rename or a copy.
    pub from: Option<String>,
    /// The lines added and removed; `None` when git prints `-`, for a binary file or a path an attribute marks
    /// `-diff`. The attributes come from files the diff does not show, so an unknown size is never read as 0.
    pub lines: Option<u64>,
    /// A submodule's commit (mode 160000) on either side: code that the diff names by sha and does not show.
    pub gitlink: bool,
}

impl Changed {
    /// Both paths of a rename, else the one path: a file moved out of a high directory is a change to it.
    fn paths(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.path.as_str()).chain(self.from.as_deref())
    }
}

/// Reads the output of `git diff --raw --numstat -z`. Git prints each raw entry first, as
/// `:<mode> <mode> <sha> <sha> <status>\0<path>\0`, with a second path for a rename or a copy. Then each numstat
/// entry, as `<added>\t<removed>\t<path>\0`, or for a rename `<added>\t<removed>\t\0<from>\0<to>\0`. Git prints `-`
/// for the counts of a file whose lines it does not count. The raw entries give the modes: a gitlink on either side.
pub fn changes(text: &str) -> Vec<Changed> {
    let mut fields = text.split('\0');
    let mut gitlinks = BTreeSet::new();
    let mut out = Vec::new();
    while let Some(entry) = fields.next() {
        if let Some(raw) = entry.strip_prefix(':') {
            let cells: Vec<&str> = raw.split(' ').collect();
            let paths = if cells.last().is_some_and(|s| s.starts_with(['R', 'C'])) { 2 } else { 1 };
            let named: Vec<&str> = fields.by_ref().take(paths).collect();
            if cells.iter().take(2).any(|m| *m == "160000") {
                gitlinks.extend(named);
            }
            continue;
        }
        let mut cells = entry.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (cells.next(), cells.next(), cells.next()) else { continue };
        let lines = added.parse::<u64>().ok().zip(removed.parse::<u64>().ok()).map(|(a, r)| a + r);
        let (from, path) = if path.is_empty() {
            (fields.next().map(String::from), fields.next().unwrap_or_default().to_string())
        } else {
            (None, path.to_string())
        };
        let gitlink = gitlinks.contains(path.as_str()) || from.as_deref().is_some_and(|f| gitlinks.contains(f));
        out.push(Changed { path, from, lines, gitlink });
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
    /// A changed path is high by its path and matches no low pattern, so round one adds the security lens (37, design
    /// change 8). A test or a doc on a high path does not set it.
    pub security: bool,
}

/// The level of a diff (36, section 1). High when a path holds a non-ASCII character, is under, at or above a
/// built-in high path, has a built-in high name or matches a high pattern (each in any ASCII case), when a path is a
/// gitlink or has a size git does not count (under a low pattern too: an attribute line can hide any size), when the lines outside the low patterns exceed `high_lines`, or when more packages than
/// `packages` change. Low only when each path matches a low pattern and the diff changes fewer than `low_lines`
/// lines. Medium otherwise. A package is a top-level directory that holds a changed path outside the low patterns; a
/// file at the top is in no package.
pub fn computed(diff: &[Changed], config: &Config) -> Computed {
    let limits = config.limits;
    let low_path = |path: &str| config.low.iter().any(|p| p.matches(path));
    let low = |c: &&Changed| c.paths().all(low_path);
    let (mut high, mut security) = (Vec::new(), false);
    for path in diff.iter().flat_map(Changed::paths) {
        let folded = path.to_ascii_lowercase();
        let before = high.len();
        if !path.is_ascii() {
            // A volume that folds case folds more than ASCII (U+017F opens as `s`), and bp holds no folding table.
            high.push(format!("non-ASCII path: {path}"));
        } else if folded == RISK_FILE {
            high.push(format!("{path} is the risk file, and an edit to the gate is high"));
        } else if let Some(p) = BUILT_IN_HIGH.iter().find(|p| touches(p, &folded)) {
            let how = if folded.starts_with(*p) { "is under" } else { "is at or above" };
            high.push(format!("{path} {how} the built-in high path {p}"));
        } else if let Some(n) = BUILT_IN_HIGH_NAMES.iter().find(|n| folded.rsplit('/').next() == Some(**n)) {
            high.push(format!("{path} is a {n} file, which changes how git reads other paths"));
        } else if let Some(p) = config.high.iter().find(|p| p.matches_any_case(path)) {
            high.push(format!("{path} matches the high pattern {p}"));
        }
        security |= high.len() > before && !low_path(path);
    }
    let outside: Vec<&Changed> = diff.iter().filter(|c| !low(c)).collect();
    high.extend(diff.iter().filter(|c| c.gitlink).map(|c| format!("{} is a submodule commit", c.path)));
    high.extend(diff.iter().filter(|c| c.lines.is_none()).map(|c| format!("size unknown: {}", c.path)));
    let lines: u64 = outside.iter().filter_map(|c| c.lines).sum();
    if lines > limits.high_lines {
        high.push(format!("{lines} changed lines outside the low patterns exceed high_lines {}", limits.high_lines));
    }
    let packages: BTreeSet<&str> = diff
        .iter()
        .flat_map(Changed::paths)
        .filter(|p| !low_path(p))
        .filter_map(|p| p.split_once('/').map(|(dir, _)| dir))
        .collect();
    if packages.len() as u64 > limits.packages {
        let (n, names) = (packages.len(), packages.into_iter().collect::<Vec<_>>().join(", "));
        high.push(format!("{n} top-level directories change ({names}), more than packages {}", limits.packages));
    }
    if !high.is_empty() {
        return Computed { level: Level::High, reasons: high, security };
    }
    let total: u64 = diff.iter().filter_map(|c| c.lines).sum();
    let mut reasons = Vec::new();
    match outside.as_slice() {
        [] => {}
        [one] => reasons.push(format!("{} matches no low pattern", one.path)),
        [first, rest @ ..] => {
            reasons.push(format!("{} and {} more changed paths match no low pattern", first.path, rest.len()));
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
        return Computed { level: Level::Low, reasons: vec![why], security };
    }
    Computed { level: Level::Medium, reasons, security }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(path: &str, added: u64, removed: u64) -> Changed {
        Changed { path: path.into(), from: None, lines: Some(added + removed), gitlink: false }
    }

    fn renamed(from: &str, to: &str) -> Changed {
        Changed { path: to.into(), from: Some(from.into()), lines: Some(0), gitlink: false }
    }

    /// A change whose size git prints as `-`.
    fn no_size(path: &str) -> Changed {
        Changed { path: path.into(), from: None, lines: None, gitlink: false }
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
        let at = |diff: &[Changed]| computed(diff, &rules).level;
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
        assert_eq!(level(Low, None, 1), High, "a raise holds with no diff in view");
    }

    #[test]
    fn risk_file_edit_is_high() {
        let none = Config::default();
        let lenient = config(
            "low: [.bp/, .github/, .claude/, .codex/, .agents/, '*.yml', '*.json']\n\
             limits: {low_lines: 1000, high_lines: 100000, packages: 100}\n",
        );
        for rules in [&none, &lenient] {
            for diff in [
                vec![change(RISK_FILE, 1, 0)],
                vec![change(RISK_FILE, 0, 0)],
                vec![change(".BP/Risk.yml", 1, 0)],
                vec![renamed(RISK_FILE, ".bp/old.yml")],
                vec![renamed(".bp/new.yml", RISK_FILE)],
                vec![change("docs/a.md", 1, 0), change(RISK_FILE, 0, 1)],
            ] {
                let c = computed(&diff, rules);
                assert_eq!(c.level, Level::High, "{diff:?}");
                assert!(c.reasons.iter().any(|r| r.contains(" is the risk file")), "{c:?}");
            }
            // The gate's own paths are high with no risk file, and a low pattern cannot remove one.
            for (path, under) in [
                (".bp/hooks/x.sh", ".bp/"),
                (".github/workflows/bp-gate.yml", ".github/workflows/"),
                (".GitHub/Workflows/ci.yml", ".github/workflows/"),
                (".claude/settings.json", ".claude/"),
                (".Claude/settings.json", ".claude/"),
                (".codex/config.toml", ".codex/"),
                (".agents/a.md", ".agents/"),
                (".bp/risk.yml.bak", ".bp/"),
            ] {
                let c = computed(&[change(path, 1, 0)], rules);
                assert_eq!(c.reasons, [format!("{path} is under the built-in high path {under}")], "{rules:?}");
            }
            // A link or a file at a gate directory's name, or at a parent's, moves the directory: a symlink `.codex`.
            for (path, at) in [
                (".codex", ".codex/"),
                (".Agents", ".agents/"),
                (".claude", ".claude/"),
                (".bp", ".bp/"),
                (".github/workflows", ".github/workflows/"),
                (".GitHub", ".github/workflows/"),
            ] {
                let c = computed(&[change(path, 1, 0)], rules);
                assert_eq!(c.reasons, [format!("{path} is at or above the built-in high path {at}")], "{rules:?}");
            }
        }
        // A file that changes how git reads other paths, and a submodule's commit, are high anywhere.
        for (path, name) in [
            (".gitmodules", ".gitmodules"),
            ("src/.gitattributes", ".gitattributes"),
            ("docs/.GitAttributes", ".gitattributes"),
        ] {
            let c = computed(&[change(path, 1, 0)], &lenient);
            assert_eq!(c.reasons, [format!("{path} is a {name} file, which changes how git reads other paths")]);
        }
        let link = Changed { gitlink: true, ..change("docs/vendor", 1, 1) };
        assert_eq!(computed(&[link], &lenient).reasons, ["docs/vendor is a submodule commit"]);
        for near in [
            ".gitmodules.yml",
            "bp/risk.yml",
            "x/.bp/risk.yml",
            ".github/dependabot.yml",
            "x/.claude/a.json",
            ".claude.json",
            ".codex.json",
        ] {
            assert_eq!(computed(&[change(near, 1, 0)], &lenient).level, Level::Low, "{near} is not the gate's");
        }
        assert_eq!(computed(&[], &none).level, Level::Low, "an empty diff edits no file");
    }

    #[test]
    fn a_case_variant_matches_a_high_pattern_and_never_a_low_one() {
        let rules = config("high: [Infra/, deploy/prod/, '*.TF', Cargo.toml]\nlow: [docs/, '*.md', infra.md]\n");
        let at = |path: &str| computed(&[change(path, 1, 0)], &rules);
        for (path, pattern) in [
            ("infra/a", "Infra/"),
            ("INFRA/a", "Infra/"),
            ("x/a.tf", "*.TF"),
            ("CARGO.TOML", "Cargo.toml"),
            // A link at the directory's name, or at a parent's, points the directory elsewhere.
            ("infra", "Infra/"),
            ("INFRA", "Infra/"),
            ("deploy/prod", "deploy/prod/"),
            ("deploy", "deploy/prod/"),
        ] {
            assert_eq!(at(path).reasons, [format!("{path} matches the high pattern {pattern}")]);
        }
        assert_eq!(at("infra.md").level, Level::Low, "a name that only starts as the directory's is not it");
        assert_eq!(at("Docs/a.txt").level, Level::Medium, "a low pattern is exact");
        assert_eq!(at("Docs/a.MD").level, Level::Medium, "a low suffix is exact");
        assert_eq!(at("docs/a.txt").level, Level::Low);
    }

    #[test]
    fn a_non_ascii_path_is_high() {
        // APFS opens U+017F (long s) as `s` and U+212A (Kelvin sign) as `k`: `\u{17f}ecrets/` is `secrets/` on macOS.
        let rules = config("high: [secrets/]\nlow: [docs/, '*.md']\n");
        for path in
            ["\u{17f}ecrets/prod.env", ".agent\u{17f}/a.md", ".github/wor\u{212a}flows/ci.yml", "docs/\u{e9}.md"]
        {
            let c = computed(&[change(path, 1, 0)], &rules);
            assert_eq!((c.level, c.reasons), (Level::High, vec![format!("non-ASCII path: {path}")]));
        }
        let c = computed(&[renamed("\u{17f}ecrets/a", "docs/a.md")], &rules);
        assert_eq!(c.reasons, ["non-ASCII path: \u{17f}ecrets/a"], "the old path of a rename counts");
    }

    #[test]
    fn the_size_and_package_limits_set_high_and_low() {
        let rules = config("low: [tests/, docs/]\nlimits: {low_lines: 10, high_lines: 20, packages: 2}\n");
        let at = |diff: &[Changed]| computed(diff, &rules);
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
        let in_low = at(&[change("a/x", 1, 0), change("tests/x", 1, 0), change("docs/x", 1, 0)]);
        assert_eq!(in_low.level, Level::Medium, "a path under a low pattern is in no package: {in_low:?}");
        let moved = at(&[renamed("a/x", "tests/x"), change("b/x", 1, 0), change("docs/x", 1, 0)]);
        assert_eq!(moved.level, Level::Medium, "{moved:?}");
        let moved = at(&[renamed("a/x", "c/x"), change("b/x", 1, 0)]);
        assert_eq!(
            moved.reasons,
            ["3 top-level directories change (a, b, c), more than packages 2"],
            "both paths count"
        );
        // Git prints `-` for a binary file or a `-diff` attribute that the diff does not show: never read as 0.
        assert_eq!(at(&[no_size("src/logo.png")]).reasons, ["size unknown: src/logo.png"]);
        // An attribute line on the base can mark any path `-diff`, under a low pattern too: an unknown size is high.
        let low = at(&[no_size("docs/logo.png"), change("tests/a.rs", 2, 0)]);
        assert_eq!(low.reasons, ["size unknown: docs/logo.png"], "{low:?}");
        let medium = at(&[change("src/a.rs", 1, 0), change("src/b.rs", 1, 0)]);
        assert_eq!(medium.reasons, ["src/a.rs and 1 more changed paths match no low pattern"]);
        assert_eq!(
            at(&[change("src/a.rs", 10, 0)]).reasons,
            ["src/a.rs matches no low pattern", "10 changed lines reach low_lines 10"]
        );
    }

    #[test]
    fn the_risk_file_reads_its_five_keys_and_refuses_the_rest() {
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
        let pin = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(config(&format!("gate: {pin}\n")).gate.as_deref(), Some(pin));
        assert_eq!(config(&format!("gate: '{pin}'\n")).gate.as_deref(), Some(pin), "a quoted pin reads the same");
        assert_eq!(config("gate:\n").gate, None);
        let key = "ecdsa-sha2-nistp256 AAAAE2VjZHNh+/= a b";
        assert_eq!(config(&format!("signer: {key}\n")).signer.as_deref(), Some(key));
        assert_eq!(
            config("signer: sk-ssh-ed25519@openssh.com AAAA\n").signer.as_deref(),
            Some("sk-ssh-ed25519@openssh.com AAAA")
        );
        let nulls = config("high:\nlow:\nlimits:\nsigner:\n");
        assert_eq!(nulls, Config::default(), "an empty key is the default, and an absent signer is null");
        assert_eq!(config("low: [docs/]\n").signer, None);
        assert_eq!(config("limits: {low_lines: 20, high_lines: 20}\n").limits.high_lines, 20, "equal limits parse");
        for (text, says) in [
            ("high: [a/\n", "not YAML"),
            ("- a/\n", "not a mapping"),
            ("hihg: [infra/]\n", "\"hihg\" is no key"),
            ("3: [infra/]\n", "3 is no key"),
            ("true: [infra/]\n", "true is no key"),
            ("~: [infra/]\n", "null is no key"),
            ("[a]: [infra/]\n", "a list or a mapping is no key"),
            ("high: infra/\n", "not a list"),
            ("high: [1]\n", "high holds 1, which is not a path pattern"),
            ("high: ['src/*.rs']\n", "wildcard"),
            ("high: ['*']\n", "wildcard"),
            ("high: ['*.']\n", "is empty"),
            ("high: ['*./x']\n", "directory after"),
            ("high: [./infra/]\n", "relative"),
            ("high: [/infra/]\n", "relative"),
            ("low: ['']\n", "is empty"),
            ("high: ['infra/ ']\n", "white space at an end"),
            ("high: [' infra/']\n", "white space at an end"),
            ("high: [\"infra\\t\"]\n", "white space at an end"),
            ("limits: {low_lines: 900, high_lines: 20}\n", "low_lines 900 is over high_lines 20"),
            ("limits: {low_lines: 900}\n", "low_lines 900 is over high_lines 800"),
            ("limits: 3\n", "not a mapping"),
            ("limits: {lines: 3}\n", "\"lines\" is no limit"),
            ("limits: {packages: -1}\n", "the limit \"packages\" is -1, not a whole number"),
            ("limits: {low_lines: 1.5}\n", "the limit \"low_lines\" is \"1.5\", not a whole number"),
            ("signer: [a]\n", "public key"),
            ("signer: ''\n", "public key"),
            ("signer: 12\n", "public key"),
            ("signer: \"ssh-ed25519 AAAA me\\nssh-ed25519 BBBB you\"\n", "public key"),
            ("signer: |\n  ssh-ed25519 AAAA me\n  ssh-rsa BBBB you\n", "public key"),
            ("signer: |\n  ssh-ed25519 AAAA me\n", "public key"),
            ("signer: >\n  ssh-ed25519 AAAA me\n", "public key"),
            ("signer: ed25519 AAAA me\n", "public key"),
            ("signer: ssh-ed25519\n", "public key"),
            ("signer: ssh-ed25519 AA*A me\n", "public key"),
            ("gate: abc\n", "gate is \"abc\", not the 40"),
            ("gate: 0123456789ABCDEF0123456789ABCDEF01234567\n", "not the 40 lower-case hex"),
            ("gate: 0123456789abcdef0123456789abcdef012345678\n", "not the 40 lower-case hex"),
            ("gate: [0123456789abcdef0123456789abcdef01234567]\n", "gate is a list or a mapping"),
            ("gate: 12\n", "gate is 12"),
            ("high: [a/]\nhigh: [b/]\n", "not YAML"),
            ("a: 1\n---\nb: 2\n", "more than one"),
        ] {
            let e = Config::parse(text).unwrap_err();
            assert!(e.contains(says), "{text:?}: {e}");
        }
    }

    #[test]
    fn changes_reads_paths_renames_binary_files_and_gitlinks() {
        let entries = [
            ":100644 100644 aaaaaaa bbbbbbb M",
            "src/a b.rs",
            ":100644 100644 aaaaaaa aaaaaaa R100",
            "docs/old.md",
            "docs/new.md",
            ":160000 160000 aaaaaaa bbbbbbb M",
            "vendor/auth",
            ":000000 160000 0000000 bbbbbbb A",
            ".claude/skills",
            "3\t1\tsrc/a b.rs",
            "-\t-\tlogo.png",
            "0\t0\t",
            "docs/old.md",
            "docs/new.md",
            "2\t0\tREADME",
            "1\t1\tvendor/auth",
            "1\t0\t.claude/skills",
            "",
        ];
        let link = |path: &str, added, removed| Changed { gitlink: true, ..change(path, added, removed) };
        assert_eq!(
            changes(&entries.join("\0")),
            [
                change("src/a b.rs", 3, 1),
                no_size("logo.png"),
                renamed("docs/old.md", "docs/new.md"),
                change("README", 2, 0),
                link("vendor/auth", 1, 1),
                link(".claude/skills", 1, 0),
            ]
        );
        assert_eq!(changes(""), []);
    }

    #[test]
    fn security_reads_a_high_path_outside_the_low_patterns() {
        let rules = config("high: [infra/]\nlow: [docs/, infra/tests/]\nlimits: {low_lines: 10, high_lines: 20}\n");
        let at = |diff: &[Changed]| {
            let c = computed(diff, &rules);
            (c.level, c.security)
        };
        assert_eq!(at(&[change("infra/main.tf", 1, 0)]), (Level::High, true));
        assert_eq!(at(&[change(".github/workflows/ci.yml", 1, 0)]), (Level::High, true), "a built-in high path");
        assert_eq!(at(&[change("infra/tests/a.rs", 1, 0)]), (Level::High, false), "a test on a high path");
        assert_eq!(at(&[change("src/a.rs", 30, 0)]), (Level::High, false), "high by size, not by a path");
        assert_eq!(at(&[change("src/a.rs", 1, 0)]), (Level::Medium, false));
        assert_eq!(at(&[change("docs/a.md", 30, 0), change("infra/x", 1, 0)]), (Level::High, true));
    }
}
