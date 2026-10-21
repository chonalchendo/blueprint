//! Cells to pixels (`m5-diagram-tools.md` §5.3, steps 1 and 2). Each column takes its widest member and each row
//! its tallest; gutters take a base width plus room for a group's border, and grow when a label needs it.

use std::collections::BTreeMap;

use crate::Finding;
use crate::nodes::up4;

/// The room between a group's border and its members, at the sides and at the top (which holds its label).
pub(crate) const GROUP_SIDE: i32 = 24;
pub(crate) const GROUP_TOP: i32 = 40;
/// The outer channel: routes may run around the whole grid.
pub(crate) const MARGIN: i32 = 48;

/// One box to place: its cell, span and natural size.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Item {
    pub(crate) col: i32,
    pub(crate) row: i32,
    pub(crate) cspan: i32,
    pub(crate) rspan: i32,
    pub(crate) w: i32,
    pub(crate) h: i32,
}

/// A group's cell rectangle: first and last column and row.
pub(crate) type Span = (i32, i32, i32, i32);

/// The placed grid: each column's left and width, each row's top and height, the gutters between them.
#[derive(Clone, Debug, Default)]
pub(crate) struct Grid {
    pub(crate) colx: Vec<i32>,
    pub(crate) colw: Vec<i32>,
    pub(crate) rowy: Vec<i32>,
    pub(crate) rowh: Vec<i32>,
    pub(crate) w: i32,
    pub(crate) h: i32,
}

impl Grid {
    /// The pixel box of a cell span.
    pub(crate) fn cell(&self, col: i32, row: i32, cspan: i32, rspan: i32) -> (i32, i32, i32, i32) {
        let (c, r) = (idx(col), idx(row));
        let (c2, r2) = (idx(col + cspan - 1), idx(row + rspan - 1));
        let x = self.colx[c];
        let y = self.rowy[r];
        (x, y, self.colx[c2] + self.colw[c2] - x, self.rowy[r2] + self.rowh[r2] - y)
    }

    /// The free vertical bands (between columns and at both sides), as `(left, right)`.
    pub(crate) fn xbands(&self) -> Vec<(i32, i32)> {
        bands(&self.colx, &self.colw, self.w)
    }

    pub(crate) fn ybands(&self) -> Vec<(i32, i32)> {
        bands(&self.rowy, &self.rowh, self.h)
    }
}

fn idx(v: i32) -> usize {
    usize::try_from(v).unwrap_or(0)
}

fn bands(at: &[i32], size: &[i32], end: i32) -> Vec<(i32, i32)> {
    let mut out = vec![(0, at[0])];
    for i in 0..at.len() {
        let right = at.get(i + 1).copied().unwrap_or(end);
        out.push((at[i] + size[i], right));
    }
    out
}

/// Places `items`. `cgap` and `rgap` are the base gutters; `wide` adds to single gutters, by column or row index
/// of the gutter's left or upper side.
pub(crate) fn place(items: &[Item], groups: &[Span], cgap: i32, rgap: i32, wide: &BTreeMap<(bool, i32), i32>) -> Grid {
    let ncols = items.iter().map(|i| i.col + i.cspan).max().unwrap_or(1);
    let nrows = items.iter().map(|i| i.row + i.rspan).max().unwrap_or(1);
    // Gutter `k` sits after column `k`; gutter -1 is the outer margin before column 0.
    let gap = |horizontal: bool, k: i32, n: i32| {
        let mut g = if k < 0 || k == n - 1 {
            MARGIN
        } else if horizontal {
            cgap
        } else {
            rgap
        };
        for &(c0, r0, c1, r1) in groups {
            let (first, last) = if horizontal { (c0, c1) } else { (r0, r1) };
            if last == k {
                g += GROUP_SIDE;
            }
            if first == k + 1 {
                g += if horizontal { GROUP_SIDE } else { GROUP_TOP };
            }
        }
        up4(g + wide.get(&(horizontal, k)).copied().unwrap_or(0))
    };
    let (lefts, widths) = axis(items, ncols, |i| (i.col, i.cspan, i.w), |k| gap(true, k, ncols));
    let (tops, heights) = axis(items, nrows, |i| (i.row, i.rspan, i.h), |k| gap(false, k, nrows));
    let last = |at: &[i32], size: &[i32], n: i32| at[idx(n - 1)] + size[idx(n - 1)];
    let w = last(&lefts, &widths, ncols) + gap(true, ncols - 1, ncols);
    let h = last(&tops, &heights, nrows) + gap(false, nrows - 1, nrows);
    Grid { colx: lefts, colw: widths, rowy: tops, rowh: heights, w, h }
}

/// One axis: sizes from single-span members, then each spanning member's shortfall added to its last track.
fn axis(
    items: &[Item],
    n: i32,
    of: impl Fn(&Item) -> (i32, i32, i32),
    gap: impl Fn(i32) -> i32,
) -> (Vec<i32>, Vec<i32>) {
    // An empty column or row stays as a channel the router may use.
    let mut size = vec![0; idx(n)];
    for i in items {
        let (at, span, s) = of(i);
        if span == 1 {
            size[idx(at)] = size[idx(at)].max(up4(s));
        }
    }
    for s in &mut size {
        if *s == 0 {
            *s = 48;
        }
    }
    let mut spanning: Vec<&Item> = items.iter().filter(|i| of(i).1 > 1).collect();
    spanning.sort_by_key(|i| of(i).1);
    for i in spanning {
        let (at, span, s) = of(i);
        let have: i32 = (at..at + span).map(|k| size[idx(k)]).sum::<i32>() + (at..at + span - 1).map(&gap).sum::<i32>();
        if s > have {
            size[idx(at + span - 1)] += up4(s - have);
        }
    }
    let mut pos = Vec::with_capacity(size.len());
    let mut x = gap(-1);
    for (k, s) in size.iter().enumerate() {
        pos.push(x);
        x += s + gap(i32::try_from(k).unwrap_or(0));
    }
    (pos, size)
}

/// The cell rules: no two boxes share a cell (`layout/overlap`), and a group's members fill one rectangle that holds
/// no other box (`layout/group-shape`).
pub(crate) fn cell_rules(cells: &[(String, Item)], groups: &[(String, Vec<String>)]) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut owner: BTreeMap<(i32, i32), &str> = BTreeMap::new();
    for (id, i) in cells {
        for c in i.col..i.col + i.cspan {
            for r in i.row..i.row + i.rspan {
                if let Some(first) = owner.insert((c, r), id) {
                    out.push(Finding::error("layout/overlap", id, format!("cell [{c}, {r}] is also `{first}`'s")));
                    owner.insert((c, r), first);
                }
            }
        }
    }
    for (gid, members) in groups {
        let mine: Vec<&Item> = cells.iter().filter(|(id, _)| members.contains(id)).map(|(_, i)| i).collect();
        let Some(span) = span_of(&mine) else { continue };
        let area: i32 = mine.iter().map(|i| i.cspan * i.rspan).sum();
        let (c0, r0, c1, r1) = span;
        let holes = area != (c1 - c0 + 1) * (r1 - r0 + 1);
        let intruder = owner
            .iter()
            .find(|((c, r), id)| (c0..=c1).contains(c) && (r0..=r1).contains(r) && !members.iter().any(|m| m == *id));
        if let Some((_, id)) = intruder {
            out.push(Finding::error(
                "layout/group-shape",
                gid,
                format!("`{id}` sits inside the group but is not a member"),
            ));
        } else if holes {
            out.push(Finding::error(
                "layout/group-shape",
                gid,
                "the members do not fill one rectangle of cells".into(),
            ));
        }
    }
    out
}

/// The cell rectangle around `items`.
pub(crate) fn span_of(items: &[&Item]) -> Option<Span> {
    let c0 = items.iter().map(|i| i.col).min()?;
    let r0 = items.iter().map(|i| i.row).min()?;
    let c1 = items.iter().map(|i| i.col + i.cspan - 1).max()?;
    let r1 = items.iter().map(|i| i.row + i.rspan - 1).max()?;
    Some((c0, r0, c1, r1))
}
