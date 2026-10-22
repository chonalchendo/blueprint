//! One block as a process: `bash -e` in a process group of its own, with `BP_VAULT` set and both output streams read
//! as they come (15; 23, Subprocess). At the timeout the group gets `TERM`, then `KILL` after a grace; after a normal
//! exit it gets `KILL` at once, so a grandchild such as `sleep` or a server dies with the run. The readers then get a
//! short drain and are left behind, so the run is bounded even when a process escaped the group with `setsid` and
//! still holds the output open: such an escapee is the one thing that outlives the run. The kill goes through
//! `kill(1)`: the workspace denies `unsafe`, and `killpg` is unsafe.
//!
//! Memory stays bounded however much a block prints: a line keeps its first [`LINE`] bytes, and the tail keeps at most
//! [`TAIL`] lines and [`TAIL_BYTES`] bytes. The evidence still hashes every byte: stdout streams into the hasher, and
//! stderr into an unlinked spool file that is hashed after it (13, Run).

use std::collections::VecDeque;
use std::fs::File;
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, mpsc};
use std::time::{Duration, Instant};

/// The lines of output a run record keeps (25, design change 5).
pub(crate) const TAIL: usize = 100;

/// The bytes of output a run record keeps (25, design change 5).
const TAIL_BYTES: usize = 64 * 1024;

/// The bytes one kept line holds before it is cut and marked `…`.
const LINE: usize = 4 * 1024;

/// How long a group has between `TERM` and `KILL`.
const GRACE: Duration = Duration::from_secs(2);

/// How long the readers have to finish after the group's `KILL`; only an escapee holds a pipe past it.
const DRAIN: Duration = Duration::from_secs(2);

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
    /// The last lines of stdout and stderr, in the order they arrived, as text without control characters.
    pub(crate) tail: Vec<String>,
    /// The lines before the tail.
    pub(crate) dropped: usize,
    /// blake3 of stdout, a NUL byte, then stderr; none when the stderr spool failed, so the digest would not cover it.
    pub(crate) evidence: Option<String>,
}

/// The last lines of output, within [`TAIL`] lines and [`TAIL_BYTES`] bytes, and the count of every line.
#[derive(Default)]
pub(crate) struct Tail {
    kept: VecDeque<String>,
    bytes: usize,
    lines: usize,
}

impl Tail {
    pub(crate) fn push(&mut self, line: String) {
        self.lines += 1;
        self.bytes += line.len() + 1;
        self.kept.push_back(line);
        while self.kept.len() > TAIL || self.bytes > TAIL_BYTES {
            let gone = self.kept.pop_front().expect("a tail over its bound holds a line");
            self.bytes -= gone.len() + 1;
        }
    }

    /// The kept lines and the count of the lines before them.
    pub(crate) fn drain(&mut self) -> (Vec<String>, usize) {
        let kept: Vec<String> = std::mem::take(&mut self.kept).into();
        let dropped = self.lines - kept.len();
        (kept, dropped)
    }
}

struct Output {
    stdout: blake3::Hasher,
    /// stderr's bytes, already unlinked, so a run that dies leaves no file behind.
    spool: File,
    spooled: u64,
    spool_failed: bool,
    tail: Tail,
    last: Option<String>,
    /// The run took what was read; a reader still blocked on an escapee's pipe adds nothing more.
    closed: bool,
}

impl Output {
    fn take(&mut self, bytes: &[u8], stdout: bool) {
        if stdout {
            self.stdout.update(bytes);
        } else if self.spool.write_all(bytes).is_ok() {
            self.spooled += bytes.len() as u64;
        } else {
            self.spool_failed = true;
        }
    }

    fn push(&mut self, line: &[u8], cut: bool, stdout: bool) {
        let mut line: String =
            String::from_utf8_lossy(line).chars().filter(|c| !c.is_control() || *c == '\t').collect();
        if cut {
            line.push('…');
        }
        if stdout && !line.trim().is_empty() {
            self.last = Some(line.clone());
        }
        self.tail.push(line);
    }
}

fn lock(out: &Mutex<Output>) -> MutexGuard<'_, Output> {
    out.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs `script` under `bash -e` in `dir`, with `BP_VAULT` set to `vault`, for at most `timeout`.
pub(crate) fn run(script: &str, dir: &Path, vault: &Path, timeout: Duration) -> Ran {
    let not_started = |why: String| Ran { end: End::NotStarted(why), tail: Vec::new(), dropped: 0, evidence: None };
    let spool = match spool() {
        Ok(f) => f,
        Err(e) => return not_started(format!("no spool file for stderr in {}: {e}", std::env::temp_dir().display())),
    };
    let start = Instant::now();
    // A block is reproducible without an agent (15): it sees neither the frozen clock nor the caller's role, session
    // or harness, so a `cut` inside it claims as a fresh session would, and a walk checks both bundles whoever runs it.
    let spawned = Command::new("bash")
        .arg("-e")
        .arg("-c")
        .arg(script)
        .current_dir(dir)
        .env("BP_VAULT", vault)
        .env_remove("BP_NOW")
        .env_remove("BP_ROLE")
        .env_remove("BP_SESSION")
        .env_remove("BP_HARNESS")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => return not_started(format!("bash did not start in {}: {e}", dir.display())),
    };
    let out = Arc::new(Mutex::new(Output {
        stdout: blake3::Hasher::new(),
        spool,
        spooled: 0,
        spool_failed: false,
        tail: Tail::default(),
        last: None,
        closed: false,
    }));
    let (read_tx, read_rx) = mpsc::channel();
    read(child.stdout.take(), true, &out, read_tx.clone());
    read(child.stderr.take(), false, &out, read_tx);
    let group = child.id();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait());
    });
    let end = if let Ok(status) = rx.recv_timeout(timeout) {
        let seconds = start.elapsed().as_secs_f64();
        End::Exited { code: status.ok().and_then(|s| s.code()), seconds, last: None }
    } else {
        kill(group, "TERM");
        let _ = rx.recv_timeout(GRACE);
        End::TimedOut(timeout)
    };
    // Every time, not only when the shell outlived the grace: a child that ignores `TERM` holds the pipe otherwise.
    kill(group, "KILL");
    let deadline = Instant::now() + DRAIN;
    for _ in 0..2 {
        if read_rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).is_err() {
            break;
        }
    }
    finish(end, &out)
}

/// An anonymous file for stderr: created under a fresh name, then unlinked while it stays open.
fn spool() -> std::io::Result<File> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    loop {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("bp-check-{}-{n}.stderr", std::process::id()));
        match File::options().read(true).write(true).create_new(true).open(&path) {
            Ok(file) => {
                std::fs::remove_file(&path)?;
                return Ok(file);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
}

fn finish(mut end: End, out: &Mutex<Output>) -> Ran {
    let mut out = lock(out);
    out.closed = true;
    if let End::Exited { last, .. } = &mut end {
        last.clone_from(&out.last);
    }
    let mut hasher = std::mem::take(&mut out.stdout);
    hasher.update(&[0]);
    let spooled = out.spooled;
    let whole = !out.spool_failed
        && out.spool.seek(SeekFrom::Start(0)).is_ok()
        && std::io::copy(&mut (&out.spool).take(spooled), &mut hasher).is_ok_and(|n| n == spooled);
    let evidence = whole.then(|| hasher.finalize().to_hex().to_string());
    let (tail, dropped) = out.tail.drain();
    Ran { end, tail, dropped, evidence }
}

/// Reads one stream in chunks until every holder of its write end has gone, or the run has closed; then says so.
fn read(pipe: Option<impl Read + Send + 'static>, stdout: bool, out: &Arc<Mutex<Output>>, done: mpsc::Sender<()>) {
    let out = Arc::clone(out);
    std::thread::spawn(move || {
        if let Some(mut pipe) = pipe {
            let mut buf = vec![0; 64 * 1024];
            let mut line = Vec::new();
            let mut cut = false;
            loop {
                let n = match pipe.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                    Err(_) => break,
                };
                let mut out = lock(&out);
                if out.closed {
                    return;
                }
                out.take(&buf[..n], stdout);
                for piece in buf[..n].split_inclusive(|b| *b == b'\n') {
                    let (text, ends) = piece.strip_suffix(b"\n").map_or((piece, false), |t| (t, true));
                    let room = LINE.saturating_sub(line.len());
                    cut |= text.len() > room;
                    line.extend_from_slice(&text[..text.len().min(room)]);
                    if ends {
                        out.push(&line, cut, stdout);
                        line.clear();
                        cut = false;
                    }
                }
            }
            let mut out = lock(&out);
            if out.closed {
                return;
            }
            if !line.is_empty() || cut {
                out.push(&line, cut, stdout);
            }
        }
        let _ = done.send(());
    });
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

    /// Whether a process whose command line holds `marker` is alive, after a moment for a killed one to be reaped.
    fn alive(marker: &str) -> bool {
        let found = || Command::new("pgrep").args(["-f", marker]).output().is_ok_and(|o| o.status.success());
        (0..20).all(|_| {
            let up = found();
            if up {
                std::thread::sleep(Duration::from_millis(50));
            }
            up
        })
    }

    #[test]
    fn output_is_kept_hashed_and_its_last_line_read() {
        let ran = run_here("echo one; echo two >&2; echo 3; echo", Duration::from_secs(10));
        let End::Exited { code, last, .. } = &ran.end else { panic!("{:?}", ran.end) };
        assert_eq!((*code, last.as_deref()), (Some(0), Some("3")), "the last non-blank stdout line");
        assert_eq!(ran.tail.len(), 4);
        let mut want = blake3::Hasher::new();
        want.update(b"one\n3\n\n\0two\n");
        assert_eq!(ran.evidence, Some(want.finalize().to_hex().to_string()), "stdout, NUL, stderr");
        let many = run_here("seq 150", Duration::from_secs(10));
        assert_eq!((many.tail.len(), many.dropped, many.tail[0].as_str()), (TAIL, 50, "51"));
    }

    #[test]
    fn the_same_output_twice_gives_the_same_evidence() {
        let script = "for i in $(seq 300); do echo out $i; echo err $i >&2; done";
        let one = run_here(script, Duration::from_secs(10));
        let two = run_here(script, Duration::from_secs(10));
        assert!(one.evidence.is_some());
        assert_eq!(one.evidence, two.evidence, "the streams hash apart, so their interleaving does not count");
    }

    #[test]
    fn a_long_line_is_cut_and_the_tail_bounded_in_bytes() {
        let ran = run_here("head -c 20000000 /dev/zero | tr '\\0' a; echo; seq 99", Duration::from_secs(30));
        assert!(matches!(ran.end, End::Exited { code: Some(0), .. }), "{:?}", ran.end);
        assert_eq!(ran.tail[0].len(), LINE + '…'.len_utf8(), "{}", &ran.tail[0][..20]);
        assert!(ran.tail[0].ends_with('…'));
        let wide = run_here(
            "for i in $(seq 100); do head -c 3000 /dev/zero | tr '\\0' b; echo; done",
            Duration::from_secs(30),
        );
        let bytes: usize = wide.tail.iter().map(|l| l.len() + 1).sum();
        assert!(bytes <= TAIL_BYTES && wide.dropped > 0, "{bytes} bytes, {} dropped", wide.dropped);
    }

    #[test]
    fn control_bytes_leave_the_tail_but_not_the_evidence() {
        let ran = run_here(r"printf 'a\000b\377\376c\r\n\033[31mred\tx\n'", Duration::from_secs(10));
        assert_eq!(ran.tail, ["ab\u{fffd}\u{fffd}c", "[31mred\tx"]);
        let mut want = blake3::Hasher::new();
        want.update(b"a\0b\xff\xfec\r\n\x1b[31mred\tx\n\0");
        assert_eq!(ran.evidence, Some(want.finalize().to_hex().to_string()));
    }

    #[test]
    fn bash_e_stops_at_the_first_failure_and_only_bp_vault_passes_through() {
        let ran = run_here("false\necho never", Duration::from_secs(10));
        assert!(matches!(ran.end, End::Exited { code: Some(1), .. }), "{:?}", ran.end);
        assert!(ran.tail.is_empty());
        let ran = run_here("test -n \"$BP_VAULT\"", Duration::from_secs(10));
        assert!(matches!(ran.end, End::Exited { code: Some(0), .. }), "{:?}", ran.end);
        let ran = run_here("test -z \"${BP_NOW-}${BP_ROLE-}${BP_SESSION-}${BP_HARNESS-}\"", Duration::from_secs(10));
        assert!(
            matches!(ran.end, End::Exited { code: Some(0), .. }),
            "a block sees no BP_NOW, BP_ROLE, BP_SESSION or BP_HARNESS: {:?}",
            ran.end
        );
    }

    #[test]
    fn the_timeout_kills_the_group_grandchildren_included() {
        let start = Instant::now();
        // The grandchild ignores TERM and holds stdout open; only the group's KILL lets the readers finish.
        let ran = run_here("(trap '' TERM; sleep 41.25) & sleep 30", Duration::from_secs(1));
        assert!(matches!(ran.end, End::TimedOut(_)), "{:?}", ran.end);
        assert!(start.elapsed() < Duration::from_secs(4), "{:?}", start.elapsed());
        assert!(!alive("sleep 41.25"), "the grandchild died with the group");
    }

    #[test]
    fn a_backgrounded_process_does_not_outlive_the_block() {
        let start = Instant::now();
        let ran = run_here("(trap '' TERM; sleep 42.25) &\necho started", Duration::from_secs(20));
        assert!(matches!(ran.end, End::Exited { code: Some(0), .. }), "{:?}", ran.end);
        assert!(start.elapsed() < Duration::from_secs(3), "{:?}", start.elapsed());
        assert!(!alive("sleep 42.25"), "the background child died with the group");
    }

    #[test]
    fn a_missing_directory_cannot_start() {
        let ran = run("true", Path::new("/no/such/dir"), Path::new("/"), Duration::from_secs(1));
        assert!(matches!(ran.end, End::NotStarted(_)), "{:?}", ran.end);
    }
}
