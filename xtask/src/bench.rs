//! `cargo xtask bench <runs> <plain> <label=command>...`: the call budget (22, constraint 8). Each command runs
//! `runs` times after one warm run, in one batch with the plain binary. Prints each label's median and its ratio
//! to the plain median, then the largest ratio alone on the last line: the scenario's measure (15).

use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result, bail};

pub fn run(args: &[String]) -> Result<f64> {
    let [runs, plain, commands @ ..] = args else {
        bail!("usage: cargo xtask bench <runs> <plain> <label=command>...")
    };
    let runs: usize = runs.parse().context("runs is a count")?;
    let base = median(plain, runs)?;
    println!("plain median {base:.2} ms");
    let mut worst = 0.0_f64;
    for spec in commands {
        let (label, command) = spec.split_once('=').context("each command is label=command")?;
        let ms = median(command, runs)?;
        let ratio = ms / base;
        println!("{label} median {ms:.2} ms ratio {ratio:.2}");
        worst = worst.max(ratio);
    }
    println!("{worst:.2}");
    Ok(worst)
}

/// The median wall time of `runs` spawns, in milliseconds. A failed run fails the bench: a refusal is fast.
fn median(command: &str, runs: usize) -> Result<f64> {
    let words: Vec<&str> = command.split_whitespace().collect();
    let [program, args @ ..] = words.as_slice() else { bail!("an empty command") };
    let once = || -> Result<f64> {
        let t = Instant::now();
        let out = Command::new(program).args(args).output().with_context(|| format!("run {command}"))?;
        if !out.status.success() {
            bail!("{command} exited {}: {}", out.status, String::from_utf8_lossy(&out.stdout));
        }
        Ok(t.elapsed().as_secs_f64() * 1000.0)
    };
    once()?;
    let mut times = (0..runs).map(|_| once()).collect::<Result<Vec<_>>>()?;
    times.sort_by(f64::total_cmp);
    Ok(times[times.len() / 2])
}
