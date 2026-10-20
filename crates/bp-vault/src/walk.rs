//! The walk (15): from a seed, every node within `depth` hops along the walk's links, forward and backward at 0.3
//! of the weight, ranked by personalized PageRank from the seed, then filled into a token budget first fit.
//! Records and inbox entries are not walked: a record is evidence about its parent, and an inbox entry is a note.
//!
//! The index keeps the walk's graph as one adjacency blob, rebuilt by each refresh that changed a file and kept up
//! to date by a write. The reach and the rank then run in memory: spike 2 found the adjacency layout, not the
//! language, sets the walk's cost.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::OptionalExtension;

use crate::index::{count_at, failed};
use crate::{Id, Index, Kind, Refusal};

/// The forward weight of each link the walk follows, as `analysis/walk3.py` reads them. A body wikilink counts as a
/// cite, and one link type between two nodes counts once. `targets` takes the weight of `answers`; walk3 had no
/// `targets`. `authorizes` and `supersedes` are not walked: they point at history, not at context.
const WEIGHTS: [(&str, f64); 5] =
    [("under", 1.0), ("depends", 1.0), ("answers", 0.6), ("targets", 0.6), ("cites", 0.4)];
const BACKWARD: f64 = 0.3;
const DAMPING: f64 = 0.85;
const ITERATIONS: usize = 50;
/// The characters an id line adds beyond its id, kind and title: the JSON keys, quotes and a full-precision rank.
const ID_LINE_CHARS: usize = 56;

/// One required link from a seed: at least one target of `field` must be a node of `kind` with a body (21, rule 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Requires {
    /// The seed's link field.
    pub field: &'static str,
    /// The kind its target must be.
    pub kind: Kind,
}

/// What `complete` needs of an issue: its `under` milestone and a cited design. The integrity check fails a live
/// issue that lacks either through the same [`Index::context_gap`], so `blocked` holds before a walk could refuse.
pub const ISSUE_CONTEXT: [Requires; 2] =
    [Requires { field: "under", kind: Kind::Milestone }, Requires { field: "cites", kind: Kind::Design }];

/// Why a requirement does not hold: no target of its kind, or the first such target that is absent or empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gap {
    /// No target of the kind.
    None,
    /// A target of the kind's path that no node or alias holds.
    Absent(String),
    /// A node of the kind with an empty body.
    Empty(String),
}

impl Gap {
    /// The gap in one line, as the walk's refusal and the integrity finding both say it.
    pub fn detail(&self, req: Requires) -> String {
        match self {
            Self::None => format!("no {} under `{}`", req.kind, req.field),
            Self::Absent(t) => format!("the {} {t} is absent", req.kind),
            Self::Empty(t) => format!("the {} {t} is empty", req.kind),
        }
    }
}

/// One reached node, in rank order.
#[derive(Debug, Clone)]
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
    pub tokens: usize,
}

impl Reached {
    /// What this node's id line costs, at four characters a token.
    pub fn line_tokens(&self) -> usize {
        let chars =
            ID_LINE_CHARS + self.id.len() + self.kind.as_str().len() + self.title.as_ref().map_or(0, String::len);
        chars.div_ceil(4)
    }
}

/// The walk's graph by node row number: for each node, its neighbours with the summed weight of every arc between
/// them, forward at full weight and backward at `BACKWARD`; and whether the walk may enter it. Built in memory by a
/// refresh and stored as one blob.
struct Graph {
    offsets: Vec<u32>,
    arcs: Vec<(u32, f64)>,
    walkable: Vec<bool>,
}

impl Graph {
    /// Little-endian: node count, arc count, the offsets, the arcs, then one byte per node for `walkable`.
    fn encode(&self) -> Vec<u8> {
        let n = self.walkable.len();
        let mut out = Vec::with_capacity(8 + 4 * (n + 1) + 12 * self.arcs.len() + n);
        for x in [n, self.arcs.len()] {
            out.extend(u32::try_from(x).expect("a vault holds fewer than 2^32 nodes and arcs").to_le_bytes());
        }
        out.extend(self.offsets.iter().flat_map(|o| o.to_le_bytes()));
        for (v, w) in &self.arcs {
            out.extend(v.to_le_bytes());
            out.extend(w.to_le_bytes());
        }
        out.extend(self.walkable.iter().map(|&b| u8::from(b)));
        out
    }
}

/// A read-only view over a stored graph blob. It decodes nothing up front: a walk touches a few hundred of the
/// vault's nodes, so it reads only their arcs.
struct Blob<'a> {
    bytes: &'a [u8],
    n: usize,
    arcs_at: usize,
}

impl<'a> Blob<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        let n = u32_at(bytes, 0) as usize;
        Self { bytes, n, arcs_at: 8 + 4 * (n + 1) }
    }

    fn neighbours(&self, u: usize) -> impl Iterator<Item = (usize, f64)> + '_ {
        let (from, to) = (u32_at(self.bytes, 8 + 4 * u) as usize, u32_at(self.bytes, 12 + 4 * u) as usize);
        (from..to).map(|i| {
            let at = self.arcs_at + 12 * i;
            let w = f64::from_le_bytes(self.bytes[at + 4..at + 12].try_into().expect("eight bytes"));
            (u32_at(self.bytes, at) as usize, w)
        })
    }

    fn walkable(&self, u: usize) -> bool {
        let m = u32_at(self.bytes, 4) as usize;
        self.bytes[self.arcs_at + 12 * m + u] == 1
    }
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().expect("four bytes"))
}

impl Index {
    /// Every node within `depth` hops of `seed`, the seed first, then by rank.
    pub fn reach(&self, seed: &str, depth: usize) -> Result<Vec<Reached>, Refusal> {
        self.reach_(seed, depth).map_err(|e| failed(Path::new(".bp/index.db"), &e))
    }

    fn reach_(&self, seed: &str, depth: usize) -> rusqlite::Result<Vec<Reached>> {
        let bytes: Vec<u8> = self.conn.query_row("select csr from graph", [], |r| r.get(0))?;
        let graph = Blob::new(&bytes);
        let start: i64 = self.conn.query_row("select rowid from nodes where id = ?1", [seed], |r| r.get(0))?;
        let start = usize::try_from(start).expect("a rowid is positive");
        // Breadth first, both directions, entering no inbox entry; `at` maps a row to its place in `rows`.
        let mut at = vec![u32::MAX; graph.n];
        let mut rows = vec![start];
        at[start] = 0;
        let mut frontier = 0..1;
        for _ in 0..depth {
            for i in frontier.clone() {
                for (v, _) in graph.neighbours(rows[i]) {
                    if at[v] == u32::MAX && graph.walkable(v) {
                        at[v] = u32::try_from(rows.len()).expect("a reach is far below 2^32 nodes");
                        rows.push(v);
                    }
                }
            }
            frontier = frontier.end..rows.len();
        }
        let wanted = format!("[{}]", rows.iter().map(usize::to_string).collect::<Vec<_>>().join(","));
        let mut nodes: Vec<Reached> = vec![];
        let mut by_row: BTreeMap<usize, Reached> = self
            .conn
            .prepare_cached(
                "select id, kind, title, status, tokens, rowid from nodes where rowid in (select value from json_each(?1))",
            )?
            .query_map([wanted], |r| Ok((usize::try_from(r.get::<_, i64>(5)?).expect("a rowid is positive"), reached(r)?)))?
            .collect::<rusqlite::Result<_>>()?;
        nodes.extend(rows.iter().map(|r| by_row.remove(r).expect("every reached row is a node")));
        let rank = pagerank(&graph, &rows, &at);
        let mut out: Vec<Reached> = nodes.into_iter().zip(rank).map(|(node, rank)| Reached { rank, ..node }).collect();
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

    /// The one predicate of 21 rule 3, which the walk and the integrity check share: `None` when `req.field` names at
    /// least one node of `req.kind` and each such target resolves, through an alias if need be (13, rule 1), to a
    /// node with a body. Otherwise the first gap, in link order.
    pub fn context_gap(&self, id: &str, req: Requires) -> Result<Option<Gap>, Refusal> {
        let run = || -> rusqlite::Result<Option<Gap>> {
            let targets: Vec<String> = self
                .conn
                .prepare_cached("select dst from links where src = ?1 and type = ?2 order by rowid")?
                .query_map([id, req.field], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            let mut resolve = self.conn.prepare_cached(
                "select kind, tokens from nodes where id = ?1 union all select n.kind, n.tokens from aliases a \
                 join nodes n on n.id = a.id where a.alias = ?1 limit 1",
            )?;
            let mut any = false;
            for t in targets {
                let node: Option<(String, i64)> = resolve.query_row([&t], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
                match node {
                    Some((kind, tokens)) if kind == req.kind.as_str() => {
                        if tokens == 0 {
                            return Ok(Some(Gap::Empty(t)));
                        }
                        any = true;
                    }
                    None if Id::parse(&t).is_ok_and(|i| req.kind.fits(&i)) => return Ok(Some(Gap::Absent(t))),
                    // A target of another kind is not this requirement's, present or not.
                    Some(_) | None => {}
                }
            }
            Ok((!any).then_some(Gap::None))
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

/// Rebuilds the walk's graph from `links` and `nodes`. A refresh that changed any file runs it: row numbers move when
/// a node is parsed again, so a partial update would point at stale rows.
pub(crate) fn rebuild_graph(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    let n: i64 = conn.query_row("select coalesce(max(rowid), 0) + 1 from nodes", [], |r| r.get(0))?;
    let n = usize::try_from(n).expect("a rowid is positive");
    let mut walkable = vec![false; n];
    for row in conn.prepare("select rowid from nodes where kind != 'inbox'")?.query_map([], |r| r.get::<_, i64>(0))? {
        walkable[usize::try_from(row?).expect("a rowid is positive")] = true;
    }
    // One arc per link type between two nodes, a body wikilink read as a cite; then summed per pair and direction.
    let mut typed = std::collections::BTreeSet::new();
    let mut st = conn.prepare(
        "select a.rowid, b.rowid, case l.type when 'body' then 'cites' else l.type end
         from links l join nodes a on a.id = l.src join nodes b on b.id = l.dst
         where l.type in ('under', 'depends', 'answers', 'targets', 'cites', 'body') and a.rowid != b.rowid",
    )?;
    for row in st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))? {
        typed.insert(row?);
    }
    let mut pairs: BTreeMap<(u32, u32), f64> = BTreeMap::new();
    for (s, d, t) in typed {
        let w = WEIGHTS.iter().find(|(k, _)| *k == t).expect("the query asks only for the walk's links").1;
        let (s, d) = (u32::try_from(s).expect("a rowid fits u32"), u32::try_from(d).expect("a rowid fits u32"));
        *pairs.entry((s, d)).or_default() += w;
        *pairs.entry((d, s)).or_default() += w * BACKWARD;
    }
    let mut offsets = vec![0_u32; n + 1];
    for &(s, _) in pairs.keys() {
        offsets[s as usize + 1] += 1;
    }
    for i in 0..n {
        offsets[i + 1] += offsets[i];
    }
    let arcs = pairs.into_iter().map(|((_, d), w)| (d, w)).collect();
    let blob = Graph { offsets, arcs, walkable }.encode();
    conn.execute("delete from graph", [])?;
    conn.execute("insert into graph values (?1)", [blob])?;
    Ok(())
}

/// What the graph holds of one id: its row and whether the walk enters it, and every arc that starts or ends there,
/// as `src dst type` over ids.
#[derive(PartialEq, Eq)]
pub(crate) struct Around {
    node: Option<(i64, bool)>,
    arcs: std::collections::BTreeSet<(String, String, String)>,
}

/// [`Around`] for one id, read inside the write's transaction.
pub(crate) fn around(conn: &rusqlite::Connection, id: &str) -> rusqlite::Result<Around> {
    let node = conn
        .prepare_cached("select rowid, kind != 'inbox' from nodes where id = ?1")?
        .query_row([id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let arcs = conn
        .prepare_cached(
            "select a.id, b.id, case l.type when 'body' then 'cites' else l.type end
             from links l join nodes a on a.id = l.src join nodes b on b.id = l.dst
             where (l.src = ?1 or l.dst = ?1)
               and l.type in ('under', 'depends', 'answers', 'targets', 'cites', 'body') and a.rowid != b.rowid",
        )?
        .query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Around { node, arcs })
}

/// Brings the graph up to date after a write took each id from `before` to `after`. A label or a body that moves no
/// link leaves it as it was, and a new node with no arc, such as an inbox entry, only adds its row; anything else
/// rebuilds it. Rebuilding on every write cost a fifth of a one-file write (25, issue 9). A node keeps its row
/// across an edit, so the rows of the stored graph still name the same nodes.
pub(crate) fn update_graph(conn: &rusqlite::Connection, before: &[Around], after: &[Around]) -> rusqlite::Result<()> {
    let mut added = Vec::new();
    for (b, a) in before.iter().zip(after) {
        match (b.node, a.node) {
            _ if b.arcs != a.arcs => return rebuild_graph(conn),
            (x, y) if x == y => {}
            (None, Some(row)) => added.push(row),
            _ => return rebuild_graph(conn),
        }
    }
    if added.is_empty() {
        return Ok(());
    }
    let old: Vec<u8> = conn.query_row("select csr from graph", [], |r| r.get(0))?;
    let blob = Blob::new(&old);
    let (n, m) = (blob.n, u32_at(&old, 4) as usize);
    let rows = added.iter().map(|(row, _)| usize::try_from(*row).expect("a rowid is positive"));
    let grown = rows.clone().map(|r| r + 1).chain([n]).max().expect("the chain holds n");
    let mut out = Vec::with_capacity(old.len() + 5 * (grown - n));
    out.extend(u32::try_from(grown).expect("a vault holds fewer than 2^32 nodes").to_le_bytes());
    out.extend(&old[4..blob.arcs_at]);
    // Each new row starts and ends its arcs where the last row ended them: at `m`, with none.
    for _ in n..grown {
        out.extend(&old[4..8]);
    }
    out.extend(&old[blob.arcs_at..]);
    let walkable = blob.arcs_at + 4 * (grown - n) + 12 * m;
    out.resize(walkable + grown, 0);
    for (r, (_, walk)) in rows.zip(&added) {
        out[walkable + r] = u8::from(*walk);
    }
    conn.execute("update graph set csr = ?1", [out])?;
    Ok(())
}

/// Personalized PageRank from the seed, `rows[0]`, over the reached nodes in `rows` order; `at` maps a row to its
/// place, `u32::MAX` outside the reach. Each node's arcs inside the reach share its outflow by weight.
fn pagerank(graph: &Blob, rows: &[usize], at: &[u32]) -> Vec<f64> {
    let n = rows.len();
    let mut arcs: Vec<(usize, usize, f64)> = Vec::new();
    let mut sinks = Vec::new();
    for (u, &row) in rows.iter().enumerate() {
        let start = arcs.len();
        arcs.extend(graph.neighbours(row).filter(|(v, _)| at[*v] != u32::MAX).map(|(v, w)| (u, at[v] as usize, w)));
        let total: f64 = arcs[start..].iter().map(|a| a.2).sum();
        if total == 0.0 {
            sinks.push(u);
        }
        for a in &mut arcs[start..] {
            a.2 /= total;
        }
    }
    let mut p = vec![0.0; n];
    p[0] = 1.0;
    for _ in 0..ITERATIONS {
        let mut q = vec![0.0; n];
        let dangling: f64 = sinks.iter().map(|&u| p[u]).sum();
        for &(u, v, w) in &arcs {
            q[v] += DAMPING * p[u] * w;
        }
        q[0] += (1.0 - DAMPING) + DAMPING * dangling;
        p = q;
    }
    p
}

/// The fill (15). The seed is always a body, so `used` passes `budget` when the seed alone does. The conventions'
/// id lines come next and close the tail (21). Then in rank order, first fit: a node of a body kind gets its body
/// when it fits, else its id line; any other node gets its id line. What fits neither counts as cut. A body costs
/// its own tokens plus its id line.
///
/// # Panics
///
/// When `reached` is empty: the reach always holds the seed.
pub fn fill(reached: &[Reached], bodies: &[Kind], budget: usize) -> Fill {
    let (seed, rest) = reached.split_first().expect("the seed is always reached");
    let mut f = Fill { bodies: vec![seed.id.clone()], used: seed.tokens + seed.line_tokens(), ..Fill::default() };
    let mut tail = Vec::new();
    for c in rest.iter().filter(|r| r.kind == Kind::Convention) {
        if f.used + c.line_tokens() <= budget {
            f.used += c.line_tokens();
            tail.push(c.id.clone());
        } else {
            f.cut += 1;
        }
    }
    for r in rest.iter().filter(|r| r.kind != Kind::Convention) {
        let line = r.line_tokens();
        if bodies.contains(&r.kind) && f.used + r.tokens + line <= budget {
            f.used += r.tokens + line;
            f.bodies.push(r.id.clone());
        } else if f.used + line <= budget {
            f.used += line;
            f.ids.push(r.id.clone());
        } else {
            f.cut += 1;
        }
    }
    f.ids.extend(tail);
    f
}

/// What the fill chose.
#[derive(Debug, Default, PartialEq)]
pub struct Fill {
    /// Ids given a body, the seed first, then in rank order.
    pub bodies: Vec<String>,
    /// Ids given an id line, in rank order, conventions last.
    pub ids: Vec<String>,
    /// How many reached nodes fit neither as a body nor as an id line.
    pub cut: usize,
    /// Tokens used.
    pub used: usize,
}

/// The graph as sorted `src dst weight` lines over ids, for comparing a refreshed index with a rebuilt one.
#[cfg(test)]
pub(crate) fn graph_rows(conn: &rusqlite::Connection) -> Vec<String> {
    let bytes: Vec<u8> = conn.query_row("select csr from graph", [], |r| r.get(0)).unwrap();
    let g = Blob::new(&bytes);
    let ids: BTreeMap<usize, String> = conn
        .prepare("select rowid, id from nodes")
        .unwrap()
        .query_map([], |r| Ok((usize::try_from(r.get::<_, i64>(0)?).unwrap(), r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut out: Vec<String> = (0..g.n)
        .flat_map(|u| g.neighbours(u).map(move |(v, w)| (u, v, w)).collect::<Vec<_>>())
        .map(|(u, v, w)| format!("graph {} {} {w:.6}", ids[&u], ids[&v]))
        .collect();
    out.extend(ids.iter().filter(|(u, _)| g.walkable(**u)).map(|(_, id)| format!("walkable {id}")));
    out.sort();
    out
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
            r("small", Kind::Design, 50),
            r("conv", Kind::Convention, 0),
            r("big", Kind::Design, 900),
            r("note", Kind::Thread, 500),
        ];
        let line = |id: &str, kind: Kind| r(id, kind, 0).line_tokens();
        let budget = 100
            + 50
            + line("seed", Kind::Issue)
            + line("small", Kind::Design)
            + line("conv", Kind::Convention)
            + line("big", Kind::Design);
        let f = fill(&reached, &[Kind::Design], budget);
        assert_eq!(f.bodies, ["seed", "small"]);
        assert_eq!(f.ids, ["big", "conv"], "a body that does not fit is an id line; the convention closes the tail");
        assert_eq!((f.cut, f.used), (1, budget), "the thread fits neither way");
    }

    #[test]
    fn the_blob_reads_back_the_graph() {
        let g = Graph { offsets: vec![0, 1, 2], arcs: vec![(1, 0.4), (0, 0.12)], walkable: vec![true, false] };
        let bytes = g.encode();
        let b = Blob::new(&bytes);
        assert_eq!(b.neighbours(0).collect::<Vec<_>>(), [(1, 0.4)]);
        assert_eq!(b.neighbours(1).collect::<Vec<_>>(), [(0, 0.12)]);
        assert!(b.walkable(0) && !b.walkable(1));
    }
}
