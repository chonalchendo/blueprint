//! What a block's attributes ask, and the verdict its process earns (15, the check grammar; 25, design change 4).
//! Pure: the date and the way the process ended come in, so every verdict is a unit test.

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use bp_change::intents::timestamp;
use bp_vault::Check;

use crate::Verdict;
use crate::process::End;

/// The timeout of a block without `timeout` (25, design change 4).
const TIMEOUT: Duration = Duration::from_secs(600);

/// The runners not built yet: each records `skipped`, so the census still shows the block (25, design change 4).
const LATER: [&str; 4] = ["deps", "contract", "pin", "nodes"];

/// What the attributes ask of one block.
#[derive(Debug, PartialEq)]
pub(crate) enum Plan {
    /// Run the block.
    Run(Spec),
    /// Record `skipped`, for this reason.
    Skip(String),
    /// Record `could-not-run`: an attribute does not parse, so the block cannot start.
    Broken(String),
    /// Run the project's integrity rules in process, over the index (25, issue 11).
    Integrity,
}

/// How to run one block and what passes.
#[derive(Debug, PartialEq)]
pub(crate) struct Spec {
    pub(crate) test: Test,
    /// The directory under the repository root that `cwd` names; none for the root.
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) timeout: Duration,
}

/// What passes.
#[derive(Debug, PartialEq)]
pub(crate) enum Test {
    /// A `shell` block exits with this code.
    Exit(i32),
    /// A `shell` block exits non-zero: `expect=fail`, quarantined while its `until` is ahead.
    Fails,
    /// A `scenario` block exits 0 and its measure is at most `max`: the wall time for `measure=seconds`, else the
    /// number on its last stdout line.
    Measure { max: f64, seconds: bool },
}

/// An `error` block fails the verb; a `warn` block prints and never fails (15). Any other severity is broken, so it
/// counts as `error`.
pub(crate) fn is_error(check: &Check) -> bool {
    check.attrs.get("severity").is_none_or(|s| s != "warn")
}

/// The attributes every kind accepts (15, the check grammar), with `timeout` (25, design change 4).
const COMMON: [&str; 5] = ["name", "severity", "cwd", "when", "timeout"];

/// Reads one block's attributes. `today` is the clock's UTC date, `YYYY-MM-DD`.
pub(crate) fn plan(check: &Check, today: &str) -> Plan {
    let attr = |k: &str| check.attrs.get(k).map(String::as_str);
    match attr("severity") {
        None | Some("error" | "warn") => {}
        Some(s) => return Plan::Broken(format!("severity={s}: error or warn")),
    }
    let own: &[&str] = match check.runner.as_str() {
        "shell" => &["expect", "until"],
        "scenario" => &["measure", "max"],
        // It runs in process over the vault, so `cwd`, `timeout` and `when` would ask nothing of it; a `when` that
        // skipped it would read as no run at all (15, the integrity row).
        "integrity" => &[],
        r if LATER.contains(&r) => return Plan::Skip(format!("check {r}: this runner is not built yet")),
        r => {
            return Plan::Broken(format!(
                "check {r}: the runners are shell, scenario, integrity, {}",
                LATER.join(", ")
            ));
        }
    };
    let common: &[&str] = if check.runner == "integrity" { &COMMON[..2] } else { &COMMON };
    // A typo must not run: `expct=1` would otherwise pass on exit 0.
    if let Some((k, v)) = check.attrs.iter().find(|(k, _)| !common.contains(&k.as_str()) && !own.contains(&k.as_str()))
    {
        let known: Vec<&str> = common.iter().chain(own).copied().collect();
        return Plan::Broken(format!(
            "{k}={v}: not an attribute of check {}; it takes {}",
            check.runner,
            known.join(", ")
        ));
    }
    match attr("when") {
        None | Some("pr") => {}
        Some("post-land") => return Plan::Skip("when=post-land: it holds only after the merge".into()),
        Some(w) => return Plan::Broken(format!("when={w}: pr or post-land")),
    }
    if check.runner == "integrity" {
        return Plan::Integrity;
    }
    let test = if check.runner == "shell" {
        shell(attr("expect"), attr("until"), today)
    } else {
        measure(attr("measure"), attr("max"))
    };
    let test = match test {
        Ok(t) => t,
        Err(why) => return Plan::Broken(why),
    };
    let timeout = match attr("timeout") {
        None => TIMEOUT,
        Some(t) => match t.parse::<u64>() {
            Ok(n) if n > 0 => Duration::from_secs(n),
            _ => return Plan::Broken(format!("timeout={t}: whole seconds from 1")),
        },
    };
    let cwd = match attr("cwd") {
        None | Some("repo") => None,
        Some(c) if Path::new(c).components().all(|p| matches!(p, Component::Normal(_) | Component::CurDir)) => {
            Some(PathBuf::from(c))
        }
        Some(c) => return Plan::Broken(format!("cwd={c}: repo, or a relative path under it")),
    };
    Plan::Run(Spec { test, cwd, timeout })
}

fn shell(expect: Option<&str>, until: Option<&str>, today: &str) -> Result<Test, String> {
    if let Some(u) = until
        && timestamp(&format!("{u}T00:00:00Z")).is_none()
    {
        return Err(format!("until={u}: a date, YYYY-MM-DD"));
    }
    match expect.unwrap_or("0") {
        // A quarantine that has lapsed: the block must pass again. Dates of one form compare as text.
        "fail" if until.is_some_and(|u| u <= today) => Ok(Test::Exit(0)),
        "fail" if until.is_some() => Ok(Test::Fails),
        // 15 pairs the two: a quarantine with no end would never ask for the fix.
        "fail" => Err("expect=fail: a quarantine needs until=<date>, the day it ends".into()),
        e => {
            e.parse::<u8>().map(|n| Test::Exit(i32::from(n))).map_err(|_| format!("expect={e}: an exit code, or fail"))
        }
    }
}

fn measure(measure: Option<&str>, max: Option<&str>) -> Result<Test, String> {
    let max = max.ok_or("max: a scenario needs its bound")?;
    let bound = max.parse::<f64>().ok().filter(|m| m.is_finite()).ok_or(format!("max={max}: a number"))?;
    Ok(Test::Measure { max: bound, seconds: measure == Some("seconds") })
}

/// The verdict and the one line that says why, for the record's body.
pub(crate) fn judge(test: &Test, end: &End) -> (Verdict, String) {
    let (code, seconds, last) = match end {
        End::TimedOut(after) => {
            return (
                Verdict::TimedOut,
                format!("still running after {} s; its process group was killed", after.as_secs()),
            );
        }
        End::NotStarted(why) => return (Verdict::CouldNotRun, why.clone()),
        End::Exited { code, seconds, last } => (*code, *seconds, last.as_deref()),
    };
    let exit = code.map_or_else(|| "killed by a signal".to_string(), |c| format!("exit {c}"));
    match test {
        Test::Exit(want) if code == Some(*want) => (Verdict::Pass, format!("{exit}, as expected")),
        Test::Exit(want) => (Verdict::Fail, format!("{exit}; expected exit {want}")),
        Test::Fails if code != Some(0) => (Verdict::Pass, format!("{exit}: the expected failure")),
        Test::Fails => (Verdict::Fail, "exit 0: an expect=fail block passed; drop the quarantine".into()),
        Test::Measure { .. } if code != Some(0) => (Verdict::Fail, format!("{exit} before the measure")),
        Test::Measure { max, seconds: true } => compare(seconds, *max),
        Test::Measure { max, .. } => match last.and_then(|l| l.trim().parse::<f64>().ok()).filter(|v| v.is_finite()) {
            Some(value) => compare(value, *max),
            None => {
                (Verdict::CouldNotRun, format!("the last stdout line is not a finite number: {:?}", last.unwrap_or("")))
            }
        },
    }
}

fn compare(value: f64, max: f64) -> (Verdict, String) {
    if value <= max {
        (Verdict::Pass, format!("measure {value} within max {max}"))
    } else {
        (Verdict::Fail, format!("measure {value} over max {max}"))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    const TODAY: &str = "2026-10-19";

    fn block(runner: &str, attrs: &str) -> Check {
        let attrs: BTreeMap<String, String> = attrs
            .split_whitespace()
            .map(|w| w.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())).unwrap())
            .collect();
        Check { runner: runner.into(), attrs, text: String::new() }
    }

    fn test_of(runner: &str, attrs: &str) -> Test {
        match plan(&block(runner, attrs), TODAY) {
            Plan::Run(spec) => spec.test,
            other => panic!("{other:?}"),
        }
    }

    fn exited(code: Option<i32>, last: Option<&str>) -> End {
        End::Exited { code, seconds: 2.0, last: last.map(str::to_string) }
    }

    fn verdict(runner: &str, attrs: &str, end: &End) -> Verdict {
        judge(&test_of(runner, attrs), end).0
    }

    #[test]
    fn a_shell_block_passes_on_its_expected_exit() {
        assert_eq!(verdict("shell", "name=a expect=0", &exited(Some(0), None)), Verdict::Pass);
        assert_eq!(verdict("shell", "name=a expect=0", &exited(Some(1), None)), Verdict::Fail);
        assert_eq!(verdict("shell", "name=a expect=2", &exited(Some(2), None)), Verdict::Pass);
        assert_eq!(verdict("shell", "name=a", &exited(Some(0), None)), Verdict::Pass, "expect defaults to 0");
        assert_eq!(verdict("shell", "name=a expect=0", &exited(None, None)), Verdict::Fail, "a signal is no exit 0");
    }

    #[test]
    fn expect_fail_quarantines_until_its_date() {
        let ahead = "name=q expect=fail until=2099-01-01";
        assert_eq!(verdict("shell", ahead, &exited(Some(1), None)), Verdict::Pass);
        assert_eq!(verdict("shell", ahead, &exited(Some(0), None)), Verdict::Fail, "an unexpected pass fails");
        assert!(
            matches!(plan(&block("shell", "name=q expect=fail"), TODAY), Plan::Broken(_)),
            "no until, no quarantine"
        );
        for lapsed in ["until=2026-10-19", "until=2020-01-01"] {
            assert_eq!(test_of("shell", &format!("name=q expect=fail {lapsed}")), Test::Exit(0), "{lapsed}");
        }
    }

    #[test]
    fn a_scenario_compares_its_last_line_with_max() {
        let s = "name=s measure=count max=3";
        assert_eq!(verdict("scenario", s, &exited(Some(0), Some("2"))), Verdict::Pass);
        assert_eq!(verdict("scenario", s, &exited(Some(0), Some(" 3 "))), Verdict::Pass, "at most max passes");
        assert_eq!(verdict("scenario", s, &exited(Some(0), Some("5"))), Verdict::Fail);
        assert_eq!(verdict("scenario", s, &exited(Some(1), Some("2"))), Verdict::Fail, "the command must finish");
        assert_eq!(verdict("scenario", s, &exited(Some(0), Some("two"))), Verdict::CouldNotRun);
        assert_eq!(verdict("scenario", s, &exited(Some(0), None)), Verdict::CouldNotRun);
        for odd in ["NaN", "inf", "-inf", ""] {
            assert_eq!(verdict("scenario", s, &exited(Some(0), Some(odd))), Verdict::CouldNotRun, "{odd:?}");
        }
        let wall = "name=w measure=seconds max=1.5";
        assert_eq!(verdict("scenario", wall, &exited(Some(0), Some("0"))), Verdict::Fail, "the wall time is 2 s");
    }

    #[test]
    fn a_timeout_and_a_failed_start_have_their_own_verdicts() {
        assert_eq!(verdict("shell", "name=t", &End::TimedOut(Duration::from_secs(1))), Verdict::TimedOut);
        assert_eq!(verdict("shell", "name=t", &End::NotStarted("no bash".into())), Verdict::CouldNotRun);
    }

    #[test]
    fn some_blocks_are_skipped_and_bad_attributes_break_the_block() {
        for (runner, attrs) in [("shell", "name=a when=post-land"), ("deps", "name=d"), ("pin", "name=p")] {
            assert!(matches!(plan(&block(runner, attrs), TODAY), Plan::Skip(_)), "{runner} {attrs}");
        }
        for (runner, attrs) in [
            ("shell", "name=a expect=maybe"),
            ("shell", "name=a expect=256"),
            ("shell", "name=a until=soon"),
            ("shell", "name=a timeout=0"),
            ("shell", "name=a when=nightly"),
            ("shell", "name=a severity=info"),
            ("shell", "name=a cwd=../elsewhere"),
            ("shell", "name=a cwd=/abs"),
            ("scenario", "name=s"),
            ("scenario", "name=s max=lots"),
            ("python", "name=a"),
            ("shell", "name=a colour=red"),
            ("shell", "name=a expct=1"),
            ("shell", "name=a max=3"),
            ("scenario", "name=s max=3 expect=0"),
            ("shell", "name=a when=post-land colour=red"),
        ] {
            assert!(matches!(plan(&block(runner, attrs), TODAY), Plan::Broken(_)), "{runner} {attrs}");
        }
        assert_eq!(plan(&block("integrity", "name=i severity=warn"), TODAY), Plan::Integrity);
        for attr in ["when=post-land", "when=pr", "cwd=x", "timeout=5", "expect=0"] {
            let broken = plan(&block("integrity", &format!("name=i {attr}")), TODAY);
            assert!(matches!(&broken, Plan::Broken(w) if w.ends_with("it takes name, severity")), "{attr}: {broken:?}");
        }
        let Plan::Run(spec) = plan(&block("shell", "name=a timeout=5 cwd=crates/bp-vault"), TODAY) else { panic!() };
        assert_eq!((spec.timeout, spec.cwd), (Duration::from_secs(5), Some(PathBuf::from("crates/bp-vault"))));
        let Plan::Run(spec) = plan(&block("shell", "name=a cwd=."), TODAY) else { panic!() };
        assert_eq!(spec.cwd, Some(PathBuf::from(".")), "the repository root");
        assert_eq!(
            plan(&block("shell", "name=a"), TODAY),
            Plan::Run(Spec { test: Test::Exit(0), cwd: None, timeout: TIMEOUT })
        );
    }

    #[test]
    fn only_warn_spares_the_verb() {
        assert!(is_error(&block("shell", "name=a")));
        assert!(!is_error(&block("shell", "name=a severity=warn")));
        assert!(is_error(&block("shell", "name=a severity=info")));
    }
}
