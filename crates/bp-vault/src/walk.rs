//! The walk (15): from a seed, every node within `depth` hops along the walk's links, forward and backward at 0.3
//! of the weight, ranked by personalized PageRank from the seed, then filled into a token budget first fit.
//! Records and inbox entries are not walked: a record is evidence about its parent, and an inbox entry is a note.

use std::fmt::Write as _;
use std::path::Path;

use serde::Serialize;

use crate::index::{count_at, failed};
use crate::{Index, Kind, Refusal};

/// The forward weight of each link the walk follows (analysis/walk3.py). `body` is a wikilink in the body, read as
/// a cite. `authorizes` and `supersedes` are not walked: they point at history, not at context.
const WEIGHTS: [(&str, f64); 6] =
    [("under", 1.0), ("depends", 1.0), ("answers", 0.6), ("targets", 0.6), ("cites", 0.4), ("body", 0.4)];
const BACKWARD: f64 = 0.3;
const DAMPING: f64 = 0.85;
const ITERATIONS: usize = 50;
/// What an id line costs in the budget: the id, the kind and the title.
pub const ID_TOKENS: usize = 12;

/// One reached node, in rank order.
#[derive(Debug, Clone, Serialize)]
pub struct Reached {
    /// The path id.
    pub id: String,
    /// The kind.
    pub kind: Kind,
    /// The title.
    pub title: Option<String>,
    /// The status.
    pub status: Option<String>,
    /// Personalized PageRank from the seed; the seed ranks first.
    pub rank: f64,
    /// The body's cost, at four characters a token.
    #[serde(skip)]
    pub tokens: usize,
}

impl Index {
    /// Every node within `depth` hops of `seed`, the seed first, then by rank.
    pub fn reach(&self, seed: &str, depth: usize) -> Result<Vec<Reached>, Refusal> {
        self.reach_(seed, depth).map_err(|e| failed(Path::new(".bp/index.db"), &e))
    }

    fn reach_(&self, seed: &str, depth: usize) -> rusqlite::Result<Vec<Reached>> {
        // The breadth-first reach runs inside SQLite over integer edges; an inbox entry is never reached.
        self.conn.execute_batch(
            "pragma temp_store = memory;
             create temp table if not exists reach_set (row integer primary key, id text, kind text, title text,
               status text, tokens integer);
             delete from reach_set;",
        )?;
        self.conn
            .prepare_cached(
                "insert into reach_set
                 with recursive r(row, d) as (
                   select rowid, 0 from nodes where id = ?1
                   union
                   select n.rowid, r.d + 1 from r join edges e on e.src = r.row join nodes n on n.rowid = e.dst
                     where r.d < ?2 and n.kind != 'inbox'
                   union
                   select n.rowid, r.d + 1 from r join edges e on e.dst = r.row join nodes n on n.rowid = e.src
                     where r.d < ?2 and n.kind != 'inbox')
                 select n.rowid, n.id, n.kind, n.title, n.status, n.tokens from (select distinct row from r) x
                   join nodes n on n.rowid = x.row",
            )?
            .execute(rusqlite::params![seed, i64::try_from(depth).expect("a depth is small")])?;
        let nodes: Vec<(i64, Reached)> = self
            .conn
            .prepare_cached("select id, kind, title, status, tokens, row from reach_set order by row")?
            .query_map([], |r| Ok((r.get(5)?, reached(r)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let at = |row: i64| nodes.binary_search_by_key(&row, |(r, _)| *r).expect("both ends are reached nodes");
        // `cross join` fixes the order: the temp table has no statistics, so the planner otherwise scans every edge.
        let edges: Vec<(usize, usize, f64)> = self
            .conn
            .prepare_cached(
                "select e.src, e.dst, e.weight from reach_set a cross join edges e on e.src = a.row
                 cross join reach_set b on b.row = e.dst where e.src != e.dst",
            )?
            .query_map([], |r| Ok((at(r.get(0)?), at(r.get(1)?), r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let seed_at = nodes.iter().position(|(_, n)| n.id == seed).expect("the seed is a node");
        let rank = pagerank(seed_at, nodes.len(), &edges);
        let mut out: Vec<Reached> =
            nodes.into_iter().zip(rank).map(|((_, node), rank)| Reached { rank, ..node }).collect();
        out.sort_by(|a, b| (b.id == seed).cmp(&(a.id == seed)).then(b.rank.total_cmp(&a.rank)).then(a.id.cmp(&b.id)));
        Ok(out)
    }

    /// The conventions a project node cites: 21 puts them in every walk's id tail, reached or not. Rank 0.
    pub fn cited_conventions(&self, project: &str) -> Result<Vec<Reached>, Refusal> {
        let run = || -> rusqlite::Result<Vec<Reached>> {
            self.conn
                .prepare_cached(
                    "select n.id, n.kind, n.title, n.status, n.tokens from links l join nodes n on n.id = l.dst
                     where l.src = ?1 and l.type in ('cites', 'body') and n.kind = 'convention' order by n.id",
                )?
                .query_map([project], reached)?
                .collect()
        };
        run().map_err(|e| failed(Path::new(".bp/index.db"), &e))
    }

    /// A node's body as the index holds it.
    pub fn body(&self, id: &str) -> Result<String, Refusal> {
        self.conn
            .query_row("select f.body from nodes n join fts f on f.rowid = n.rowid where n.id = ?1", [id], |r| r.get(0))
            .map_err(|e| failed(Path::new(".bp/index.db"), &e))
    }

    /// The targets of one link field of a node, as written, whether or not a node holds them.
    pub fn links_of(&self, id: &str, field: &str) -> Result<Vec<String>, Refusal> {
        let run = || -> rusqlite::Result<Vec<String>> {
            let mut st = self.conn.prepare_cached("select dst from links where src = ?1 and type = ?2")?;
            st.query_map([id, field], |r| r.get(0))?.collect()
        };
        run().map_err(|e| failed(Path::new(".bp/index.db"), &e))
    }
}

/// A node row as `id, kind, title, status, tokens`, unranked.
fn reached(r: &rusqlite::Row) -> rusqlite::Result<Reached> {
    let kind: String = r.get(1)?;
    let kind = kind.parse::<Kind>().expect("the index stores kinds from the closed set");
    Ok(Reached { id: r.get(0)?, kind, title: r.get(2)?, status: r.get(3)?, rank: 0.0, tokens: count_at(r, 4)? })
}

/// Rebuilds `edges`, the walk's links between nodes as row numbers with their forward weight. A refresh that changed
/// any file runs it: row numbers move when a node is parsed again, so a partial update would point at stale rows.
pub(crate) fn rebuild_edges(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    let weight = WEIGHTS.iter().fold(String::new(), |mut s, (t, w)| {
        let _ = write!(s, " when '{t}' then {w:?}");
        s
    });
    let types = WEIGHTS.iter().map(|(t, _)| format!("'{t}'")).collect::<Vec<_>>().join(", ");
    conn.execute_batch(&format!(
        "delete from edges;
         insert into edges select a.rowid, b.rowid, case l.type{weight} end
           from links l join nodes a on a.id = l.src join nodes b on b.id = l.dst where l.type in ({types});"
    ))
}

/// Personalized PageRank from the seed over the reached nodes; `edges` holds (from, to, weight) by position. One flat
/// arc array: the walk runs on every agent turn, inside the call budget (22, constraint 8).
fn pagerank(seed: usize, n: usize, edges: &[(usize, usize, f64)]) -> Vec<f64> {
    let mut arcs: Vec<(usize, usize, f64)> =
        edges.iter().flat_map(|&(s, d, w)| [(s, d, w), (d, s, w * BACKWARD)]).collect();
    arcs.sort_by_key(|a| a.0);
    let mut total = vec![0.0; n];
    for &(u, _, w) in &arcs {
        total[u] += w;
    }
    for (u, _, w) in &mut arcs {
        *w /= total[*u];
    }
    let sinks: Vec<usize> = (0..n).filter(|&u| total[u] == 0.0).collect();
    let mut p = vec![0.0; n];
    p[seed] = 1.0;
    for _ in 0..ITERATIONS {
        let mut q = vec![0.0; n];
        let dangling: f64 = sinks.iter().map(|&u| p[u]).sum();
        for &(u, v, w) in &arcs {
            q[v] += DAMPING * p[u] * w;
        }
        q[seed] += (1.0 - DAMPING) + DAMPING * dangling;
        p = q;
    }
    p
}

/// The fill (15): in rank order, a body for each node of a body kind that still fits, first fit; an id line for
/// every other node. The seed is always a body. Returns the bodies, the id lines, the cut and the tokens used.
pub fn fill(reached: &[Reached], bodies: &[Kind], budget: usize) -> Fill {
    let mut f = Fill::default();
    for (i, r) in reached.iter().enumerate() {
        if i == 0 || bodies.contains(&r.kind) && r.kind != Kind::Convention {
            if i == 0 || f.used + r.tokens <= budget {
                f.used += r.tokens;
                f.bodies.push(r.id.clone());
            } else {
                f.cut.push(r.id.clone());
            }
        } else if f.used + ID_TOKENS <= budget {
            f.used += ID_TOKENS;
            f.ids.push(r.id.clone());
        } else {
            f.cut.push(r.id.clone());
        }
    }
    // The project's conventions are always in the id tail (15).
    let (conventions, rest): (Vec<_>, Vec<_>) =
        f.ids.drain(..).partition(|id| reached.iter().any(|r| &r.id == id && r.kind == Kind::Convention));
    f.ids = rest.into_iter().chain(conventions).collect();
    f
}

/// What the fill chose.
#[derive(Debug, Default, PartialEq)]
pub struct Fill {
    /// Ids given a body, in rank order.
    pub bodies: Vec<String>,
    /// Ids given an id line, in rank order, conventions last.
    pub ids: Vec<String>,
    /// Ids the budget left out.
    pub cut: Vec<String>,
    /// Tokens used.
    pub used: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(id: &str, kind: Kind, tokens: usize) -> Reached {
        Reached { id: id.into(), kind, title: None, status: None, rank: 0.0, tokens }
    }

    #[test]
    fn the_fill_is_first_fit_and_ends_with_conventions() {
        let reached = [
            r("seed", Kind::Issue, 100),
            r("conv", Kind::Convention, 50),
            r("big", Kind::Design, 900),
            r("small", Kind::Design, 50),
            r("note", Kind::Thread, 500),
        ];
        let f = fill(&reached, &[Kind::Issue, Kind::Design, Kind::Convention], 200);
        assert_eq!(f.bodies, ["seed", "small"], "a body that does not fit is skipped, and the fill goes on");
        assert_eq!(f.ids, ["note", "conv"], "a convention is an id line, last");
        assert_eq!(f.cut, ["big"]);
        assert_eq!(f.used, 100 + 50 + 2 * ID_TOKENS);
    }
}
