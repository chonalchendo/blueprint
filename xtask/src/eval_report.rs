//! The report the owner reads at the walkthrough (29, issue 1): per arm, then per trial with a pointer to the raw
//! trace. The columns follow the skill: a review is scored on its findings, a worker on its stop, its checks and
//! its file scope (complete) or its dispositions (respond).

use std::path::Path;

use bp_operation::Action;

use crate::eval_grade::{Grade, Stats, short};
use crate::eval_kills::label;
use crate::eval_result::f2;
use crate::eval_score::{ASPECTS, Graded, RECALL};

/// The arms table's last column: how many runs had the skill where the arm puts it.
fn state(arm: &str, loaded: usize, runs: usize) -> String {
    match (arm, loaded == runs) {
        ("without", true) => "absent, as designed".into(),
        ("without", false) => format!("present in {} of {runs}", runs - loaded),
        _ => loaded.to_string(),
    }
}

fn yes(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

impl Grade {
    pub fn report(&self, store: &Path) -> String {
        if self.skill.action == Action::Research {
            return self.research_report(store);
        }
        let mut lines =
            vec![format!("# Eval report: {}\n", self.skill.name), format!("Traces: `{}`\n", store.display())];
        lines.push("## Ground truth\n".into());
        lines.extend(self.truth_lines());
        lines.push("## Arms\n".into());
        lines.extend(self.arm_lines());
        lines.push(self.notes());
        lines.push("## Trials".into());
        for t in &self.trials {
            lines.extend(self.trial_lines(t, store));
        }
        lines.join("\n") + "\n"
    }

    fn aspects(&self) -> [&'static str; 2] {
        ASPECTS[usize::from(self.skill.action == Action::Respond)]
    }

    fn truth_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for f in &self.fixtures {
            lines.push(format!("Fixture {} (base `{}`, head `{}`):\n", f.id, short(&f.base), short(&f.head)));
            let respond = self.skill.action == Action::Respond;
            lines.extend(f.truth.iter().enumerate().map(|(i, t)| {
                let expect = if !respond {
                    ""
                } else if t.fixed {
                    " (expect: fixed)"
                } else {
                    " (expect: a reasoned decline)"
                };
                format!("{}. `{}`{expect}: {}", i + 1, t.location(), t.what)
            }));
            lines.extend(f.probes.iter().map(|(n, cmd)| format!("Probe for item {n}: `{cmd}`")));
            if !f.scope.is_empty() {
                lines.push(format!("The landed issue changed these crates: `{}`.", f.scope.join("`, `")));
            }
            if let Some((sha, title)) = &f.stack {
                lines.push(format!(
                    "Stacked: the dependency \"{title}\" holds an open pull request at `{}`.",
                    short(sha)
                ));
            }
            lines.push(String::new());
        }
        lines
    }

    /// The arms table: the review columns, or the worker's stop and its two aspects.
    fn arm_lines(&self) -> Vec<String> {
        let review = self.skill.action == Action::Review;
        let names = self.aspects();
        let mut lines = vec![if review {
            "| arm | runs | outcome | prose recall (upper bound) | reached the tool | adherence there | out of row | findings | false | runs with no finding | skill loaded |".to_string()
        } else {
            format!(
                "| arm | runs | outcome | stop reached | escalated | {} | {} | reached the tool | adherence there | out of row | skill loaded |",
                names[0], names[1]
            )
        }];
        lines.push("|---|---|---|---|---|---|---|---|---|---|---|".into());
        for arm in self.arms() {
            let runs = self.arm(arm);
            let s = Stats::of(&runs);
            if s.runs == 0 {
                continue;
            }
            let adherence = s.adherence.map_or("n/a".into(), f2);
            let count = |f: fn(&Graded) -> bool| runs.iter().filter(|r| f(r)).count();
            lines.push(if review {
                format!(
                    "| {arm} | {} | {} of {} | {} of {} | {} | {adherence} | {} | {} | {} | {} | {} |",
                    s.runs,
                    s.passes,
                    s.runs,
                    s.prose,
                    s.truth,
                    s.reached,
                    s.out_of_row,
                    s.found,
                    s.wrong,
                    s.silent,
                    state(arm, s.loaded, s.runs)
                )
            } else {
                format!(
                    "| {arm} | {} | {} of {} | {} | {} | {} | {} | {} | {adherence} | {} | {} |",
                    s.runs,
                    s.passes,
                    s.runs,
                    count(|r| r.end.done && !r.end.escalated()),
                    count(|r| r.end.escalated()),
                    count(|r| r.aspects[0]),
                    count(|r| r.aspects[1]),
                    s.reached,
                    s.out_of_row,
                    state(arm, s.loaded, s.runs)
                )
            });
        }
        if self.kill_set() {
            lines.extend([
                "\n| arm | planted items killed | false rows per control trial | duplicate rows |".into(),
                "|---|---|---|---|".into(),
            ]);
            for arm in self.arms() {
                let (k, n) = self.kills(arm);
                let control = self.control_false(arm).map_or("n/a".into(), f2);
                lines.push(format!("| {arm} | {k} of {n} | {control} | {} |", Stats::of(&self.arm(arm)).duplicates));
            }
        }
        lines
    }

    /// What the columns mean, for the skill. The last two bullets hold for every skill.
    fn notes(&self) -> String {
        let own = match self.skill.action {
            Action::Review if self.kill_set() => "- **Outcome:** on a mutant or an escape, a run passes when its review is recorded and kills every planted item: a row points into the item's span (the same file, a range that overlaps the span, and fewer than 20 cited lines in all) and carries evidence: a command in backticks that opens a command the run executed (a read such as `git diff` or `cat` does not count), a design or convention id, or `design NN` or `(NN, <section>)` naming an existing design and a section of it. When two items share a span, the row must also hold one of the item's `match:` words. A row with prose, a place or an advised command only kills nothing. On a control a run passes with no false row; a row within 3 lines of a `known:` entry, or in its file with its `match:` words, is `known`.\n- **Duplicate:** a row on an item that another row matched is neither a match nor a false row.\n- **Kills:** an arm kills an item when more than half of its trials on the item's fixture killed it. `holdout_gain` in the result file is `with`'s kills less `old`'s over the planted items, with its Newcombe 95% interval.\n".to_string(),
            Action::Review => format!(
                "- **Outcome:** a run passes when `bp walk --for review --done` holds after the review is recorded and the findings table matches at least {RECALL} of the ground truth. A finding matches the item whose slots (the `match:` words of the fixture) its text holds, the most slots first, then the nearest line; where it points earns nothing, and a finding with an `unless:` phrase matches nothing. Keywords cannot read a negation or a verdict, so read each match and each FALSE row. That needs the shape only `bp-review` defines (a `head:` line, one table).\n- **Prose recall:** an upper bound, never an outcome. Items of the ground truth that the final text substantiates in any form: a citation of the item's file, with a line number or none, on a line that holds the item's words. A citation earns one item, and a line that states the item is no defect can still earn it when it holds no `unless:` phrase. It does not depend on the shape, so a gap in outcome with none in prose recall means the arm cannot write the shape.\n"
            ),
            Action::Complete => "- **Outcome:** a run passes when `bp walk --for complete --done` holds with the claim not released `escalated`, the latest run of each error block of the issue passed, and every changed file sits in a crate that the landed issue changed (the first two path parts, as `replay-evidence.sh` compares them). The file set is not compared: the model is not deterministic.\n- **Adherence:** the share of walk, cut, check and pr that the transcript shows as calls, over the runs that reached the tool.\n".to_string(),
            _ => "- **Outcome:** a run passes when `bp walk --for respond --done` holds with the claim not released `escalated` and each finding is closed as the past review closed it. A `fixed` finding needs a fixed disposition, its file in the worker's diff against the review's head, and its probe when the fixture gives one. A `declined` finding needs a reasoned decline, and a fix of it fails. The probe and the file are evidence of a fix, not proof of a good one: the owner reads the trace.\n- **Adherence:** the share of walk, check, pr, one closed disposition per finding and a commit that the transcript and the vault show, over the runs that reached the tool.\n".to_string(),
        };
        let counted = "- **Counted by line:** the adherence and `out of row` columns read `bp` calls from the transcript's Bash commands, one per line, `;`, `&`, `|`, parenthesis or backtick outside quotes, after `if`, `while` and the other keywords that open a command. A heredoc body, a redirect and a comment are skipped, and a stray `<<` hides a body only when a later line equals the word after it. A `bp` built under `target/` is not counted.\n";
        let scope = if self.skill.action == Action::Complete {
            "- **Scope:** the unit is the first two path parts, and `xtask` as a whole, so a test file in `xtask/tests` is in scope.\n"
        } else {
            ""
        };
        format!(
            "\n{own}{scope}{counted}- **Failed trial:** a trial the runner killed at its limit, or that left no transcript, fails whatever the sandbox holds. A rerun of the same command replaces it.\n- **Uplift:** the `with` rate less the `without` rate; the result file gives its Newcombe 95% interval.\n- **Role body:** the `without` arm's role body is the shipped one and its dispatch reads `Skill: none`, so it differs from `with` in the missing body and preload only.\n"
        )
    }

    fn trial_lines(&self, t: &Graded, store: &Path) -> Vec<String> {
        let names = self.aspects();
        let outcome = if t.pass { "pass" } else { "fail" };
        let head = match self.skill.action {
            Action::Review => format!(
                "outcome: {outcome}; recorded: {}; ground truth matched by a table row: {} of {}; named in the text: {}",
                yes(t.end.done),
                t.hits,
                t.truth,
                t.prose_hits
            ),
            _ => format!(
                "outcome: {outcome}; stop reached: {}; escalated: {}; {}: {}; {}: {}",
                yes(t.end.done),
                yes(t.end.escalated()),
                names[0],
                yes(t.aspects[0]),
                names[1],
                yes(t.aspects[1])
            ),
        };
        let steps: Vec<String> = t.steps.iter().map(|(n, ok)| format!("{n} {}", yes(*ok))).collect();
        let (dir, trial) = (store.display(), &t.trial);
        let mut lines = vec![format!(
            "\n### {trial}\n\n- {head}\n- reached the tool: {}; steps: {}\n- out of row: {}; skill: {}\n- trace: `{dir}/{trial}.jsonl`, vault log: `{dir}/{trial}.vault.log`",
            yes(t.reached),
            steps.join(", "),
            t.out_of_row,
            match (t.arm.as_str(), t.loaded) {
                ("without", true) => "absent, as designed",
                ("without", false) => "PRESENT, not as designed",
                (_, true) => "loaded as designed",
                (_, false) => "NOT loaded as designed",
            },
        )];
        if let Some(why) = &t.failure {
            lines.push(format!("- FAILED: {why}; a failed trial counts as a fail"));
        }
        if self.skill.action != Action::Review && !t.end.kept(self.fixture(&t.fixture)) {
            lines.push("- the issue's body differs from the fixture's: the worker edited the issue".into());
        }
        if self.skill.action != Action::Review {
            let files = if t.end.files.is_empty() { "none".into() } else { t.end.files.join(", ") };
            lines.push(format!("- files changed: {files}"));
        }
        if !t.end.probes.is_empty() {
            let probes: Vec<String> =
                t.end.probes.iter().map(|(n, ok)| format!("{n} {}", if *ok { "pass" } else { "fail" })).collect();
            lines.push(format!("- probes: {}", probes.join(", ")));
        }
        let answers: Vec<String> = t
            .end
            .answers
            .iter()
            .map(|a| {
                let how = match (a.fixed, a.reasoned) {
                    (true, _) => "fixed",
                    (false, true) => "declined with a reason",
                    (false, false) => "declined, no reason",
                };
                format!("{} {how}", a.finding)
            })
            .collect();
        if !answers.is_empty() {
            lines.push(format!("- dispositions: {}", answers.join(", ")));
        }
        for (at, what, item) in &t.findings {
            let what: String = what.chars().take(160).collect();
            lines.push(format!("  - {} `{at}`: {what}", label(*item)));
        }
        lines
    }
}
