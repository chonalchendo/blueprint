//! One block as a process: `bash -e` in a process group of its own, with `BP_VAULT` set and both output streams read
//! as they come. At the timeout the whole group dies, so a grandchild such as `sleep` or a server goes with the shell
//! (15; 23, Subprocess). The group is also killed once the shell exits, so nothing a block left running outlives the
//! run or holds its output open. The kill goes through `kill(1)`: the workspace denies `unsafe`, and `killpg` is
//! unsafe.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The lines of output a run record keeps (25, design change 5).
pub(crate) const TAIL: usize = 100;

/// How long a group has between `TERM` and `KILL`.
const GRACE: Duration = Duration::from_secs(2);

/// How a block's process ended.
#[derive(Debug)]
pub(crate) enum End {
    /// The shell exited: its code, none when a signal ended it; the wall time; the last non-blank stdout line.
    Exited { code: Option<i32>, seconds: f64, last: Option<String> },
    /// It ran past this timeout and its group was killed.
    TimedOut(Duration),
    /// The shell did not start.
    NotStarted(String),
}

/// One run of a block.
pub(crate) struct Ran {
    pub(crate) end: End,
    /// The last [`TAIL`] lines of stdout and stderr, in the order they arrived.
    pub(crate) tail: Vec<String>,
    /// The lines before the tail.
    pub(crate) dropped: usize,
    /// The blake3 of every byte of output, in the order it arrived.
    pub(crate) evidence: String,
}

#[derive(Default)]
struct Output {
    hasher: blake3::Hasher,
    tail: VecDeque<String>,
    lines: usize,
    last: Option<String>,
}

impl Output {
    fn push(&mut self, bytes: &[u8], stdout: bool) {
        self.hasher.update(bytes);
        let line = String::from_utf8_lossy(bytes).trim_end_matches(['\n', '\r']).to_string();
        if stdout && !line.trim().is_empty() {
            self.last = Some(line.clone());
        }
        self.lines += 1;
        self.tail.push_back(line);
        if self.tail.len() > TAIL {
            self.tail.pop_front();
        }
    }
}

/// Runs `script` under `bash -e` in `dir`, with `BP_VAULT` set to `vault`, for at most `timeout`.
pub(crate) fn run(script: &str, dir: &Path, vault: &Path, timeout: Duration) -> Ran {
    let out = Arc::new(Mutex::new(Output::default()));
    let start = Instant::now();
    let spawned = Command::new("bash")
        .arg("-e")
        .arg("-c")
        .arg(script)
        .current_dir(dir)
        .env("BP_VAULT", vault)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => return finish(End::NotStarted(format!("bash did not start in {}: {e}", dir.display())), &out),
    };
    let readers = [read(child.stdout.take(), true, &out), read(child.stderr.take(), false, &out)];
    let group = child.id();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait());
    });
    let end = if let Ok(status) = rx.recv_timeout(timeout) {
        kill(group, "TERM");
        let seconds = start.elapsed().as_secs_f64();
        End::Exited { code: status.ok().and_then(|s| s.code()), seconds, last: None }
    } else {
        kill(group, "TERM");
        if rx.recv_timeout(GRACE).is_err() {
            kill(group, "KILL");
            let _ = rx.recv();
        }
        End::TimedOut(timeout)
    };
    for r in readers {
        let _ = r.join();
    }
    finish(end, &out)
}

fn finish(mut end: End, out: &Mutex<Output>) -> Ran {
    let out = std::mem::take(&mut *out.lock().unwrap_or_else(PoisonError::into_inner));
    if let End::Exited { last, .. } = &mut end {
        last.clone_from(&out.last);
    }
    let dropped = out.lines - out.tail.len();
    Ran { end, tail: out.tail.into(), dropped, evidence: out.hasher.finalize().to_hex().to_string() }
}

/// Reads one stream line by line until every holder of its write end has gone.
fn read(pipe: Option<impl Read + Send + 'static>, stdout: bool, out: &Arc<Mutex<Output>>) -> JoinHandle<()> {
    let out = Arc::clone(out);
    std::thread::spawn(move || {
        let Some(pipe) = pipe else { return };
        let mut pipe = BufReader::new(pipe);
        let mut line = Vec::new();
        while pipe.read_until(b'\n', &mut line).is_ok_and(|n| n > 0) {
            out.lock().unwrap_or_else(PoisonError::into_inner).push(&line, stdout);
            line.clear();
        }
    })
}

/// Sends `signal` to the process group. A group already empty is the common case after an exit, so the answer is
/// not read.
fn kill(group: u32, signal: &str) {
    let _ = Command::new("kill")
        .args([&format!("-{signal}"), "--", &format!("-{group}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_here(script: &str, timeout: Duration) -> Ran {
        let dir = std::env::temp_dir();
        run(script, &dir, &dir, timeout)
    }

    #[test]
    fn output_is_kept_hashed_and_its_last_line_read() {
        let ran = run_here("echo one; echo two >&2; echo 3; echo", Duration::from_secs(10));
        let End::Exited { code, last, .. } = &ran.end else { panic!("{:?}", ran.end) };
        assert_eq!((*code, last.as_deref()), (Some(0), Some("3")), "the last non-blank stdout line");
        assert_eq!(ran.tail.len(), 4);
        assert_eq!(ran.evidence.len(), 64);
        let many = run_here("seq 150", Duration::from_secs(10));
        assert_eq!((many.tail.len(), many.dropped, many.tail[0].as_str()), (TAIL, 50, "51"));
    }

    #[test]
    fn bash_e_stops_at_the_first_failure_and_bp_vault_is_set() {
        let ran = run_here("false\necho never", Duration::from_secs(10));
        assert!(matches!(ran.end, End::Exited { code: Some(1), .. }), "{:?}", ran.end);
        assert!(ran.tail.is_empty());
        let ran = run_here("test -n \"$BP_VAULT\"", Duration::from_secs(10));
        assert!(matches!(ran.end, End::Exited { code: Some(0), .. }), "{:?}", ran.end);
    }

    #[test]
    fn the_timeout_kills_the_group_grandchildren_included() {
        let start = Instant::now();
        // The grandchild holds stdout open; only a group kill lets the readers finish.
        let ran = run_here("sleep 30 & sleep 30", Duration::from_secs(1));
        assert!(matches!(ran.end, End::TimedOut(_)), "{:?}", ran.end);
        assert!(start.elapsed() < Duration::from_secs(10), "{:?}", start.elapsed());
    }

    #[test]
    fn a_backgrounded_process_does_not_outlive_the_block() {
        let start = Instant::now();
        let ran = run_here("sleep 30 &\necho started", Duration::from_secs(20));
        assert!(matches!(ran.end, End::Exited { code: Some(0), .. }), "{:?}", ran.end);
        assert!(start.elapsed() < Duration::from_secs(10), "{:?}", start.elapsed());
    }

    #[test]
    fn a_missing_directory_cannot_start() {
        let ran = run("true", Path::new("/no/such/dir"), Path::new("/"), Duration::from_secs(1));
        assert!(matches!(ran.end, End::NotStarted(_)), "{:?}", ran.end);
    }
}
