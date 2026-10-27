//! The diff of a Change page: the file stat of the pull request against its base in the project's clone, and the text
//! on request, cut at [`DIFF_CAP`].

use std::path::Path;
use std::process::Command;

use serde::Serialize;
use ts_rs::TS;

/// The diff text the page carries at most; past it the text is cut at a line and `truncated` is set.
pub const DIFF_CAP: usize = 200 * 1024;

/// The pull request's diff: the file stat, and the text on request.
#[derive(Default, Serialize, TS)]
#[ts(optional_fields)]
pub struct Diff {
    /// One row per changed file; empty when no clone holds the head, and `reason` says why.
    pub files: Vec<DiffFile>,
    /// Why no file stat was read: no clone holds the project, or the clone lacks the base or the head.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The diff text, asked for with `?diff=1`, at most [`DIFF_CAP`] bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The text was cut at the cap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
}

/// One changed file: its lines added and removed; a binary file counts none.
#[derive(Serialize, TS)]
pub struct DiffFile {
    /// The path in the repository.
    pub path: String,
    /// Lines added.
    pub additions: u64,
    /// Lines removed.
    pub deletions: u64,
}

/// The diff of `p` against its base as the clone at `top` holds it: `git diff` from their merge base, the file stat,
/// and the text when asked. A clone that lacks the head or the base gives no files.
pub(crate) fn diff(top: &Path, p: &bp_forge::PullRequest, text: bool) -> Diff {
    let range = format!("refs/remotes/origin/{}...{}", p.base_ref_name.as_deref().unwrap_or("HEAD"), p.head_ref_oid);
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(top)
            .args(["diff", "--no-ext-diff", "--no-color"])
            .args(args)
            .args(["--end-of-options", &range])
            .output()
            .ok()?;
        out.status.success().then_some(out.stdout)
    };
    let Some(stat) = git(&["--numstat"]) else {
        let (base, head) =
            (p.base_ref_name.as_deref().unwrap_or("HEAD"), &p.head_ref_oid[..p.head_ref_oid.len().min(7)]);
        return Diff::none(&format!("the clone lacks origin/{base} or {head}; git fetch in the clone"));
    };
    let files = {
        String::from_utf8_lossy(&stat)
            .lines()
            .filter_map(|l| {
                let mut f = l.splitn(3, '\t');
                let (add, del, path) = (f.next()?, f.next()?, f.next()?);
                Some(DiffFile {
                    path: path.into(),
                    additions: add.parse().unwrap_or(0),
                    deletions: del.parse().unwrap_or(0),
                })
            })
            .collect()
    };
    let full = if text { git(&[]) } else { None };
    let (text, truncated) = match full {
        None => (None, None),
        Some(bytes) if bytes.len() <= DIFF_CAP => (Some(String::from_utf8_lossy(&bytes).into_owned()), None),
        Some(bytes) => {
            // Cut at the last line end inside the cap, so no line shows half.
            let end = bytes[..DIFF_CAP].iter().rposition(|&b| b == b'\n').map_or(DIFF_CAP, |i| i + 1);
            (Some(String::from_utf8_lossy(&bytes[..end]).into_owned()), Some(true))
        }
    };
    Diff { files, reason: None, text, truncated }
}

impl Diff {
    /// No file stat, for `reason`.
    pub(crate) fn none(reason: &str) -> Self {
        Self { reason: Some(reason.into()), ..Self::default() }
    }
}
