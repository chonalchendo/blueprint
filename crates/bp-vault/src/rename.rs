//! What a rename reads and rewrites (12, A rename): the files that mention a node, found in the index, and each
//! one's text with every whole mention of the old id replaced and every other byte kept.

use std::path::Path;

use crate::index::failed;
use crate::{Id, Index, Refusal};

impl Index {
    /// The live aliases of node `id`, as the past path ids they hold.
    pub fn aliases_of(&self, id: &Id) -> Result<Vec<Id>, Refusal> {
        let run = || -> rusqlite::Result<Vec<Id>> {
            self.conn
                .prepare_cached("select alias from aliases where id = ?1 order by alias")?
                .query_map([id.as_str()], |r| r.get(0).map(Id::from_trusted))?
                .collect()
        };
        run().map_err(|e| failed(Path::new(".bp/index.db"), &e))
    }

    /// The files that name `id`, sorted: a link to it, to one of its aliases or to a file under its directory, and
    /// any other mention of one of those ids in a header or body, code and prose included (12, A rename). A stored
    /// text that holds the id as part of a longer one is a candidate too; [`retarget`] leaves it as it was.
    pub fn inbound(&self, id: &Id) -> Result<Vec<Id>, Refusal> {
        let run = || -> rusqlite::Result<Vec<Id>> {
            // `0` follows `/`, so the range holds exactly the ids under the directory, and the index serves it. A
            // path under the directory holds the id, so the text scans need only the names.
            self.conn
                .prepare_cached(
                    "with names(n) as (select ?1 union select alias from aliases where id = ?1) \
                     select src from links where dst in (select n from names) or (dst > ?1 || '/' and dst < ?1 || '0') \
                     union select f.id from fts f, names where instr(f.body, n) > 0 \
                     union select x.id from nodes x, names where instr(x.header, n) > 0 \
                     union select r.id from records r, names where instr(r.body, n) > 0 or instr(r.header, n) > 0 \
                     order by 1",
                )?
                .query_map([id.as_str()], |r| r.get(0).map(Id::from_trusted))?
                .collect()
        };
        run().map_err(|e| failed(Path::new(".bp/index.db"), &e))
    }
}

/// `text` with each mention of the id `from` naming `to` instead, and every other byte as it was. A mention is the
/// id whole: not inside a longer path or slug, except after `](/`, where a markdown link opens a root-relative path.
/// What follows it stays, so a `|label`, a `#section`, `.md` and a path under the id's directory survive.
pub fn retarget(text: &str, from: &str, to: &str) -> String {
    let path = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '/');
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (at, _) in text.match_indices(from) {
        let opens = text[..at].chars().next_back().is_none_or(|c| !path(c)) || text[..at].ends_with("](/");
        let rest = &text[at + from.len()..];
        let rest = rest.strip_prefix(".md").unwrap_or(rest);
        let closes = match rest.chars().next() {
            None | Some('/') => true,
            // A sentence may end on the id; a slug may hold a dot.
            Some('.') => !rest[1..].starts_with(path),
            Some(c) => !path(c),
        };
        if opens && closes {
            out.push_str(&text[last..at]);
            out.push_str(to);
            last = at + from.len();
        }
    }
    out.push_str(&text[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mention_keeps_its_label_section_and_path() {
        let (o, n) = ("shared/threads/a.1", "shared/threads/b");
        let text = "[[shared/threads/a.1|A]] [[shared/threads/a.1#Why]]\ncites: [\"shared/threads/a.1\"]\n\
                    [d](/shared/threads/a.1/diagrams/x.html) [[shared/threads/a.1.md]] ends shared/threads/a.1.\n";
        let want = "[[shared/threads/b|A]] [[shared/threads/b#Why]]\ncites: [\"shared/threads/b\"]\n\
                    [d](/shared/threads/b/diagrams/x.html) [[shared/threads/b.md]] ends shared/threads/b.\n";
        assert_eq!(retarget(text, o, n), want);
    }

    #[test]
    fn a_longer_id_or_another_path_is_not_a_mention() {
        let o = "shared/threads/a";
        for text in
            ["[[shared/threads/a-b]]", "[[shared/threads/a.1]]", "x/shared/threads/a", "https://h/shared/threads/a"]
        {
            assert_eq!(retarget(text, o, "shared/threads/z"), text);
        }
    }
}
