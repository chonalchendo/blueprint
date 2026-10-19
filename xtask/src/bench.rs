//! `cargo xtask bench <runs> <plain> <label=command>...`: the call budget (22, constraint 8). Each round spawns the
//! plain binary and then every command once, so host drift falls on all of them alike. After one warm round, `runs`
//! rounds. Prints each label's median and its ratio to the plain median, then the largest ratio alone on the last
//! line: the scenario's measure (15). A command splits on spaces outside double or single quotes.

use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result, bail};

pub fn run(args: &[String]) -> Result<f64> {
    let [runs, plain, commands @ ..] = args else {
        bail!("usage: cargo xtask bench <runs> <plain> <label=command>...")
    };
    let runs: usize = runs.parse().context("runs is a count")?;
    let mut labelled = vec![("plain", words(plain))];
    for spec in commands {
        let (label, command) = spec.split_once('=').context("each command is label=command")?;
        labelled.push((label, words(command)));
    }
    let mut times = vec![Vec::with_capacity(runs); labelled.len()];
    for round in 0..=runs {
        for (i, (_, argv)) in labelled.iter().enumerate() {
            let ms = once(argv)?;
            if round > 0 {
                times[i].push(ms);
            }
        }
    }
    let medians: Vec<f64> = times.iter_mut().map(|t| median(t)).collect();
    println!("plain median {:.2} ms", medians[0]);
    let mut worst = 0.0_f64;
    for ((label, _), ms) in labelled.iter().zip(&medians).skip(1) {
        let ratio = ms / medians[0];
        println!("{label} median {ms:.2} ms ratio {ratio:.2}");
        worst = worst.max(ratio);
    }
    println!("{worst:.2}");
    Ok(worst)
}

/// One spawn's wall time in milliseconds. A failed run fails the bench: a refusal is fast.
fn once(command: &[String]) -> Result<f64> {
    let [program, args @ ..] = command else { bail!("an empty command") };
    let t = Instant::now();
    let out = Command::new(program).args(args).output().with_context(|| format!("run {}", command.join(" ")))?;
    if !out.status.success() {
        bail!("{} exited {}: {}", command.join(" "), out.status, String::from_utf8_lossy(&out.stdout));
    }
    Ok(t.elapsed().as_secs_f64() * 1000.0)
}

fn median(times: &mut [f64]) -> f64 {
    times.sort_by(f64::total_cmp);
    times[times.len() / 2]
}

fn words(command: &str) -> Vec<String> {
    let (mut out, mut word, mut quote, mut any) = (Vec::new(), String::new(), None, false);
    for c in command.chars() {
        match (quote, c) {
            (None, '"' | '\'') => (quote, any) = (Some(c), true),
            (Some(q), c) if c == q => quote = None,
            (None, c) if c.is_whitespace() => {
                if any {
                    out.push(std::mem::take(&mut word));
                    any = false;
                }
            }
            (_, c) => {
                word.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(word);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn words_keep_quoted_spaces() {
        assert_eq!(super::words(r#"bp search "walk budget" --json ''"#), ["bp", "search", "walk budget", "--json", ""]);
    }
}
