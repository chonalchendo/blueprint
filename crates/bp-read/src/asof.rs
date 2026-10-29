//! A design's body at a version tag (18, As-of diff; 32, design change 9). The body at a tag comes from git, and the
//! diff against the current body is section by section (18, Open: the diff grain).

use bp_vault::{Body, Code, Id, Refusal, Vault};
use serde::Serialize;
use ts_rs::TS;

use crate::version::Tags;
use crate::{Show, show};

/// What changed from a design's body at a tag to its current body, section by section.
#[derive(Default, Serialize, TS)]
pub struct BodyDiff {
    /// The heading of each section whose text differs, in body order; `""` is the text before the first heading.
    pub sections: Vec<String>,
    /// The lines the current body holds that the body at the tag lacks, one entry per section that has any.
    pub added: Vec<SectionLines>,
    /// The lines the body at the tag holds that the current body lacks, one entry per section that has any.
    pub removed: Vec<SectionLines>,
}

/// The lines of one section that differ.
#[derive(Serialize, TS)]
pub struct SectionLines {
    /// The section's heading; `""` for the text before the first heading.
    pub section: String,
    /// The lines, in their order, blank lines left out.
    pub lines: Vec<String>,
}

/// The `show` object of `id` as the tag holds it; `None` when the file is not at the tag (the design was added or
/// renamed since). Only a version tag of the project, `blueprint/<project>/<n>`, is read: any other value, or a tag the
/// vault lacks, refuses `no_tag`. So no revision expression (`~1`, `^`, `:`, `@{}`) reads another commit, and no
/// other project's tag reads as this design's version.
pub(crate) fn show_at(vault: &Vault, id: &Id, tag: &str, project: &str) -> Result<Option<Show>, Refusal> {
    let read = if Tags::n(tag, project).is_some() {
        show(vault, None, id, Some(&format!("refs/tags/{tag}")), None, false)
    } else {
        Err(no_tag(tag, project))
    };
    match read {
        Ok(shown) => Ok(Some(shown)),
        Err(e) if e.code == Code::NoNode => Ok(None),
        Err(e) if e.code == Code::UnknownValue => Err(no_tag(tag, project)),
        Err(e) => Err(e),
    }
}

fn no_tag(tag: &str, project: &str) -> Refusal {
    Refusal::new(
        Code::NoTag,
        format!("?at={tag}"),
        "?at names a version tag of the project, blueprint/<project>/<n>",
        format!("git tag -l 'blueprint/{project}/*' lists the versions"),
    )
}

/// The section diff from `then` to `now`. A section pairs with the section of the same heading and the same rank
/// among its namesakes. A section only `then` holds sits after the section it followed there.
pub(crate) fn diff(then: &Body, now: &Body) -> BodyDiff {
    let (old, new) = (parts(then), parts(now));
    let pair: Vec<Option<usize>> = new.iter().map(|p| old.iter().position(|o| o.key() == p.key())).collect();
    let mut out = BodyDiff::default();
    // A section whose only change is a blank line has no lines to show, so it is not listed.
    let mut emit = |heading: &str, a: &str, b: &str| {
        let (removed, added) = if a == b { Default::default() } else { lines(a, b) };
        if removed.is_empty() && added.is_empty() {
            return;
        }
        out.sections.push(heading.to_string());
        for (to, lines) in [(&mut out.removed, removed), (&mut out.added, added)] {
            if !lines.is_empty() {
                to.push(SectionLines { section: heading.to_string(), lines });
            }
        }
    };
    for (p, j) in new.iter().zip(&pair) {
        emit(p.heading, j.map_or("", |j| old[j].text), p.text);
        // The lead always pairs, so every section only `then` holds follows a paired one.
        let Some(j) = *j else { continue };
        for o in old[j + 1..].iter().enumerate().take_while(|(i, _)| !pair.contains(&Some(j + 1 + i))) {
            emit(o.1.heading, o.1.text, "");
        }
    }
    out
}

/// One section of a body: the lead is the section `""`.
struct Part<'a> {
    heading: &'a str,
    rank: usize,
    text: &'a str,
}

impl Part<'_> {
    fn key(&self) -> (&str, usize) {
        (self.heading, self.rank)
    }
}

fn parts(body: &Body) -> Vec<Part<'_>> {
    let mut out = vec![Part { heading: "", rank: 0, text: &body.lead }];
    for s in &body.sections {
        let rank = out.iter().filter(|p| p.heading == s.heading).count();
        out.push(Part { heading: &s.heading, rank, text: &s.text });
    }
    out
}

/// The lines only `a` holds and the lines only `b` holds, by a longest common subsequence of their lines.
fn lines(a: &str, b: &str) -> (Vec<String>, Vec<String>) {
    let (a, b): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    // The common head and tail need no table; most edits touch a few lines of a long section.
    let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[head..].iter().rev().zip(b[head..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[head..a.len() - tail], &b[head..b.len() - tail]);
    // `lcs[i][j]`: the longest common subsequence of `a[i..]` and `b[j..]`.
    let mut lcs = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let (mut only_a, mut only_b) = (Vec::new(), Vec::new());
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            (i, j) = (i + 1, j + 1);
        } else if j == b.len() || (i < a.len() && lcs[i + 1][j] >= lcs[i][j + 1]) {
            only_a.push(a[i]);
            i += 1;
        } else {
            only_b.push(b[j]);
            j += 1;
        }
    }
    let kept = |v: Vec<&str>| v.into_iter().filter(|l| !l.trim().is_empty()).map(str::to_string).collect();
    (kept(only_a), kept(only_b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_diff_names_each_changed_added_and_removed_section_in_body_order() {
        let then = Body::parse("Lead.\n\n## A\n\none\ntwo\nthree\n\n## Gone\n\nold text\n\n## B\n\nsame\n");
        let now = Body::parse("Lead.\n\n## A\n\none\n2\nthree\n\n## B\n\nsame\n\n## New\n\nfresh\n\nlines\n");
        let d = diff(&then, &now);
        assert_eq!(d.sections, ["A", "Gone", "New"]);
        let got = |v: &[SectionLines]| v.iter().map(|s| (s.section.clone(), s.lines.join("|"))).collect::<Vec<_>>();
        assert_eq!(got(&d.removed), [("A".into(), "two".into()), ("Gone".into(), "old text".into())]);
        assert_eq!(got(&d.added), [("A".into(), "2".into()), ("New".into(), "fresh|lines".into())]);
        assert!(diff(&now, &now).sections.is_empty(), "a body equals itself");
        let spaced = Body::parse("Lead.\n\n## A\n\none\n\n\ntwo\nthree\n\n## Gone\n\nold text\n\n## B\n\nsame\n");
        let d = diff(&then, &spaced);
        assert!(d.sections.is_empty() && d.added.is_empty() && d.removed.is_empty(), "a blank line only: no section");
    }

    #[test]
    fn the_lead_is_the_section_without_a_heading_and_namesakes_pair_by_rank() {
        let then = Body::parse("Old lead.\n\n## X\n\na\n\n## X\n\nb\n");
        let now = Body::parse("New lead.\n\n## X\n\na\n\n## X\n\nc\n");
        let d = diff(&then, &now);
        assert_eq!(d.sections, ["", "X"]);
        assert_eq!(
            (d.removed[1].lines.as_slice(), d.added[1].lines.as_slice()),
            (["b".to_string()].as_slice(), ["c".to_string()].as_slice())
        );
    }
}

#[cfg(test)]
mod page_tests {
    use std::path::Path;
    use std::process::Command;

    use bp_vault::Index;

    use super::*;

    const SYSTEM: &str =
        "---\nkind: design\ntitle: S\nstatus: active\n---\n\nLead.\n\n## TL;DR\n\nShort.\n\n## Old\n\nGone soon.\n";

    fn git(dir: &Path, args: &[&str]) {
        let mut git = Command::new("git");
        git.arg("-C").arg(dir).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args);
        assert!(git.status().unwrap().success(), "git {args:?}");
    }

    fn write(dir: &Path, rel: &str, text: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// A vault in git with a project and its system design, committed.
    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "projects/bp/project.md", "---\nkind: project\ntitle: B\nstatus: active\n---\n");
        write(dir.path(), "shared/.keep", "");
        write(dir.path(), ".gitignore", ".bp/\n");
        write(dir.path(), "projects/bp/designs/system.md", SYSTEM);
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["add", "-A"]);
        git(dir.path(), &["commit", "-qm", "one"]);
        let vault = Vault { root: dir.path().to_path_buf() };
        (dir, vault)
    }

    /// The version the Design page of the system design reads.
    fn version(v: &Vault) -> String {
        crate::design(v, &Index::open(v).unwrap(), "bp", "projects/bp/designs/system", None).unwrap().version
    }

    /// A version record `n`, written by hand as `stamp` writes it, whose manifest pins the system design at `hash`.
    fn record(v: &Vault, n: u64, hash: &str) {
        let text = format!(
            "---\nkind: blueprint-version\nunder: projects/bp/project\nmilestone: projects/bp/milestones/m\nn: {n}\n\
             tag: blueprint/bp/{n}\naccepted_at: 2026-10-2{n}T00:00:00Z\nmanifest:\n  projects/bp/designs/system: {hash}\n---\n"
        );
        write(&v.root, &format!("projects/bp/versions/{n}.md"), &text);
    }

    #[test]
    fn the_banner_reads_the_record_else_the_newest_tag_else_no_version() {
        let (_d, v) = vault();
        assert_eq!(version(&v), "no version", "no record and no tag");
        for tag in ["blueprint/bp/2", "blueprint/bp/10", "blueprint/bp/x", "blueprint/other/99"] {
            git(&v.root, &["tag", tag]);
        }
        assert_eq!(version(&v), "changed since blueprint/bp/10", "the tag alone, by number: no manifest is read");
        let hash = v.read(&crate::node_id("projects/bp/designs/system").unwrap()).unwrap().hash;
        record(&v, 1, &hash);
        assert_eq!(version(&v), "accepted at blueprint/bp/1", "the record wins over the newer tags");
        record(&v, 2, "0000");
        assert_eq!(version(&v), "accepted at blueprint/bp/1", "the newest version that holds the hash");
        write(&v.root, "projects/bp/designs/system.md", &SYSTEM.replace("title: S", "title: S2"));
        assert_eq!(version(&v), "changed since blueprint/bp/2", "a header edit changes the hash");
    }

    /// The version words Home's design row, Home, Projects and the Design banner read, in that order.
    fn pages(v: &Vault) -> [Option<String>; 4] {
        let index = Index::open(v).unwrap();
        let home = crate::home(v, &index, "bp").unwrap();
        let row = home.designs.iter().find(|d| d.row.row.id == "projects/bp/designs/system").unwrap().version.clone();
        let project = crate::projects(v, &index).unwrap().rows.remove(0).version;
        [Some(row), home.version, project, Some(version(v))]
    }

    #[test]
    fn home_projects_and_design_read_one_version_and_a_missing_tag_drops_the_link() {
        let (_d, v) = vault();
        let none = [Some("no version".to_string()), None, None, Some("no version".to_string())];
        assert_eq!(pages(&v), none);
        git(&v.root, &["tag", "blueprint/bp/1"]);
        let since = Some("changed since blueprint/bp/1".to_string());
        let tag = Some("blueprint/bp/1".to_string());
        assert_eq!(
            pages(&v),
            [since.clone(), tag.clone(), tag.clone(), since],
            "a tag alone: one version on all three"
        );
        let design = || crate::design(&v, &Index::open(&v).unwrap(), "bp", "projects/bp/designs/system", None).unwrap();
        assert_eq!((design().since, design().tag_fix), (tag, None));
        // A record whose tag the vault's git lacks: the words stay, the link goes, the fix shows.
        record(&v, 3, "0000");
        let d = design();
        assert_eq!(
            (d.version.as_str(), d.version_tag.as_deref(), d.since, d.tag_fix.as_deref()),
            (
                "changed since blueprint/bp/3",
                Some("blueprint/bp/3"),
                None,
                Some("blueprint/bp/3 is not in this vault's git: git fetch --tags")
            )
        );
        assert_eq!(pages(&v)[1..3], [Some("blueprint/bp/3".to_string()), Some("blueprint/bp/3".to_string())]);
    }

    #[test]
    fn at_a_tag_the_body_is_gits_and_the_diff_names_the_changed_sections() {
        let (_d, v) = vault();
        git(&v.root, &["tag", "blueprint/bp/1"]);
        let now =
            SYSTEM.replace("Short.", "Short.\n\nA probe line.").replace("## Old\n\nGone soon.\n", "## New\n\nFresh.\n");
        write(&v.root, "projects/bp/designs/system.md", &now);
        write(&v.root, "projects/bp/designs/product.md", "---\nkind: design\ntitle: P\nstatus: active\n---\n\nP.\n");
        git(&v.root, &["add", "-A"]);
        git(&v.root, &["commit", "-qm", "two"]);
        let index = Index::open(&v).unwrap();
        let page = |id: &str, at: &str| crate::design(&v, &index, "bp", id, Some(at));
        let d = page("projects/bp/designs/system", "blueprint/bp/1").unwrap();
        assert_eq!(
            (d.version.as_str(), d.show.body.as_deref()),
            ("at blueprint/bp/1", Some("Lead.\n\n## TL;DR\n\nShort.\n\n## Old\n\nGone soon."))
        );
        let diff = d.diff.unwrap();
        assert_eq!(diff.sections, ["TL;DR", "Old", "New"]);
        let lines =
            |v: &[SectionLines]| v.iter().map(|s| format!("{}: {}", s.section, s.lines.join("|"))).collect::<Vec<_>>();
        assert_eq!(
            (lines(&diff.removed), lines(&diff.added)),
            (vec!["Old: Gone soon.".to_string()], vec!["TL;DR: A probe line.".to_string(), "New: Fresh.".to_string()])
        );
        let product = page("projects/bp/designs/product", "blueprint/bp/1").unwrap();
        assert_eq!(
            (product.version.as_str(), product.show.body.as_deref(), product.outline.len()),
            ("not at blueprint/bp/1", Some(""), 0)
        );
        assert!(product.diff.is_some_and(|d| d.sections.is_empty() && d.added.is_empty()), "an empty diff");
        // Another project's tag and a tag outside `blueprint/` exist, and still refuse.
        git(&v.root, &["tag", "blueprint/mentat/7", "blueprint/bp/1"]);
        git(&v.root, &["tag", "v-probe", "blueprint/bp/1"]);
        for tag in ["blueprint/bp/9", "blueprint/bp/1~1", "", "HEAD", "blueprint/mentat/7", "v-probe", "blueprint/bp/"]
        {
            let refusal = page("projects/bp/designs/system", tag).err().unwrap();
            assert_eq!(
                (refusal.code, refusal.fix.as_str()),
                (Code::NoTag, "git tag -l 'blueprint/bp/*' lists the versions"),
                "{tag:?}"
            );
        }
    }
}
