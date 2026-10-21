//! The sequence type: parties in columns by order, messages in rows by order, no cells (28, design change 1).
//! `back` writes the dashed return on the next row; activation bars come from each call and its return; a step
//! inserted in the spec moves only the rows below it.

use std::collections::BTreeMap;

use crate::font::Font;
use crate::geom::{Point, Rect};
use crate::layout::heads;
use crate::nodes::up4;
use crate::scene::{Run, SEdge, SLabel, SNode, SNote, Scene, Shape};
use crate::spec::Spec;

const TOP: i32 = 24;
const HEAD_H: i32 = 52;
const ROW: i32 = 40;
const LOOP_ROW: i32 = 52;
const SIDE: i32 = 48;

/// One drawn message: a call, or the return a step's `back` adds.
struct Msg<'a> {
    id: String,
    step: &'a str,
    from: usize,
    to: usize,
    say: &'a str,
    ret: bool,
    note: Option<&'a str>,
}

pub(crate) fn lay_out(spec: &Spec) -> Scene {
    let msgs = messages(spec);
    let labels: Vec<(Run, Run)> = msgs
        .iter()
        .enumerate()
        .map(|(k, m)| {
            (Run::new(Font::Mono, 10, 0, "chip", (k + 1).to_string()), Run::new(Font::Body, 12, 0, "elabel", m.say))
        })
        .collect();
    let mut parties: Vec<SNode> = spec.nodes.iter().map(|n| header(n, spec.focus.contains(&n.id))).collect();
    let cx = columns(&parties, &msgs, &labels);
    // Rows: a self call takes a taller row for its loop.
    let mut ys = Vec::with_capacity(msgs.len());
    let mut y = TOP + HEAD_H + 44;
    for m in &msgs {
        ys.push(y);
        y += if m.from == m.to { LOOP_ROW } else { ROW };
    }
    let bottom = y;
    let bars = activations(&msgs, &ys, parties.len());
    let edge_x = |p: usize, row: usize, toward: i32| {
        let open = bars[p].iter().any(|b| b.0 <= ys[row] && ys[row] <= b.1);
        cx[p] + if open { 4 * toward.signum() } else { 0 }
    };
    let mut edges = Vec::new();
    let mut notes = Vec::new();
    let mut right = cx.last().copied().unwrap_or(0) + parties.last().map_or(0, |h| h.rect.w / 2) + SIDE;
    for (k, m) in msgs.iter().enumerate() {
        let y = ys[k];
        let (chip, text) = labels[k].clone();
        let lw = chip.w + 6 + text.w + 8;
        let pts: Vec<Point>;
        let rect;
        if m.from == m.to {
            let x = edge_x(m.from, k, 1);
            pts = vec![(x, y), (x + 28, y), (x + 28, y + 20), (x, y + 20)];
            rect = Rect::new(x + 40, y + 2, lw, 16);
        } else {
            let dir = (cx[m.to] - cx[m.from]).signum();
            let (a, b) = (edge_x(m.from, k, dir), edge_x(m.to, k, -dir));
            pts = vec![(a, y), (b, y)];
            rect = Rect::new(a.midpoint(b) - lw / 2, y - 6 - 16, lw, 16);
        }
        let runs = vec![chip.at(rect.x + 4, rect.y + 12), text.at(rect.x + 4 + labels[k].0.w + 6, rect.y + 12)];
        let focus = spec.focus.iter().any(|f| f == m.step);
        let mut class = vec![if m.ret { "return" } else { "call" }];
        if focus {
            class.push("focus");
        }
        if let Some(n) = m.note {
            // The note names the callee's state effect, beside its lifeline, away from the message.
            let callee = if m.ret { m.from } else { m.to };
            let other = if m.ret { m.to } else { m.from };
            let run = Run::new(Font::Mono, 10, 0, "note", n);
            let w = up4(run.w + 16);
            let x = if cx[other] < cx[callee] || other == callee { cx[callee] + 12 } else { cx[callee] - 12 - w };
            let r = Rect::new(x, y - 12, w, 24);
            right = right.max(r.right() + SIDE);
            notes.push(SNote {
                id: format!("{}:note", m.id),
                of: m.id.clone(),
                rect: r,
                runs: vec![run.at(x + 8, y + 4)],
                step: k + 1,
            });
        }
        edges.push(SEdge {
            id: m.id.clone(),
            from: spec.nodes[m.from].id.clone(),
            to: spec.nodes[m.to].id.clone(),
            dashed: m.ret,
            heads: heads(&pts, false),
            pieces: vec![pts],
            bridges: Vec::new(),
            labels: vec![SLabel { id: format!("{}:label", m.id), rect, runs, pill: false }],
            class,
            step: Some(k + 1),
        });
    }
    for (i, h) in parties.iter_mut().enumerate() {
        h.rect.x = cx[i] - h.rect.w / 2;
        for r in &mut h.runs {
            r.x += h.rect.x;
            r.y += TOP;
        }
        h.lifeline = Some((cx[i], TOP + HEAD_H, bottom));
        h.bars = bars[i].iter().map(|&(a, b)| Rect::new(cx[i] - 4, a - 8, 8, b - a + 16)).collect();
    }
    Scene {
        w: right,
        h: bottom + 8,
        nodes: parties,
        edges,
        notes,
        legend_y: bottom + 8,
        steps: msgs.iter().map(|m| m.id.clone()).collect(),
        ..Scene::default()
    }
}

/// The drawn messages in time order: each step's call, then its `back` return on the next row.
fn messages(spec: &Spec) -> Vec<Msg<'_>> {
    let at: BTreeMap<&str, usize> = spec.nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
    let mut msgs = Vec::new();
    for s in &spec.steps {
        let (from, to) = (at[s.from.as_str()], at[s.to.as_str()]);
        let note = s.note.as_deref();
        msgs.push(Msg {
            id: s.id.clone(),
            step: &s.id,
            from,
            to,
            say: &s.say,
            ret: s.ret,
            note: note.filter(|_| s.back.is_none()),
        });
        if let Some(back) = &s.back {
            msgs.push(Msg {
                id: format!("{}.back", s.id),
                step: &s.id,
                from: to,
                to: from,
                say: back,
                ret: true,
                note,
            });
        }
    }
    msgs
}

/// A party's header box at the top, its name over its role, at x 0 until the columns are known.
fn header(n: &crate::spec::Node, focus: bool) -> SNode {
    let name = Run::new(Font::Strong, 13, 0, "name", &n.label);
    let sub = n.note.as_deref().map(|s| Run::new(Font::Mono, 10, 0, "kind", s));
    let w = ((name.w.max(sub.as_ref().map_or(0, |s| s.w)) + 32 + 7) / 8 * 8).max(120);
    let mut runs = vec![name.centred(w / 2, if sub.is_some() { 24 } else { 31 })];
    runs.extend(sub.map(|s| s.centred(w / 2, 40)));
    SNode {
        id: n.id.clone(),
        role: n.role.clone(),
        term: n.term.clone(),
        noun: n.noun.clone(),
        label: n.label.clone(),
        rect: Rect::new(0, TOP, w, HEAD_H),
        shape: Shape::Box,
        dashed: false,
        runs,
        rules: Vec::new(),
        lifeline: None,
        bars: Vec::new(),
        class: if focus { vec!["focus"] } else { Vec::new() },
    }
}

/// Lifeline x per party: header widths apart by 40 at least, then each message's label given room between its two
/// lifelines, shortest spans first, the shortfall spread over the gaps it crosses.
fn columns(heads: &[SNode], msgs: &[Msg<'_>], labels: &[(Run, Run)]) -> Vec<i32> {
    let n = heads.len();
    let mut gap: Vec<i32> = (1..n).map(|i| up4(heads[i - 1].rect.w.midpoint(heads[i].rect.w) + 40)).collect();
    let mut needs: Vec<(usize, usize, i32)> = msgs
        .iter()
        .zip(labels)
        .map(|(m, (chip, text))| {
            let w = chip.w + 6 + text.w + 8;
            if m.from == m.to {
                (m.from, (m.from + 1).min(n - 1), w + 56)
            } else {
                (m.from.min(m.to), m.from.max(m.to), w + 48)
            }
        })
        .filter(|(a, b, _)| a < b)
        .collect();
    needs.sort_by_key(|&(a, b, _)| b - a);
    for (a, b, w) in needs {
        let have: i32 = gap[a..b].iter().sum();
        if w > have {
            let k = i32::try_from(b - a).unwrap_or(1);
            let each = up4((w - have + k - 1) / k);
            for g in &mut gap[a..b] {
                *g += each;
            }
        }
    }
    let mut cx = vec![SIDE + heads.first().map_or(0, |h| up4(h.rect.w / 2))];
    for g in gap {
        cx.push(cx[cx.len() - 1] + g);
    }
    cx
}

/// Each party's activation bars as `(first row y, last row y)`: a call opens one on the callee, its return closes
/// it, and a caller with none open holds one from its first message to its last.
fn activations(msgs: &[Msg<'_>], ys: &[i32], n: usize) -> Vec<Vec<(i32, i32)>> {
    let mut open: Vec<Vec<i32>> = vec![Vec::new(); n];
    let mut bars: Vec<Vec<(i32, i32)>> = vec![Vec::new(); n];
    let mut last = vec![0; n];
    for (m, &y) in msgs.iter().zip(ys) {
        last[m.from] = y;
        last[m.to] = y;
        if m.from == m.to {
            continue;
        }
        if open[m.from].is_empty() && !m.ret {
            open[m.from].push(y);
        }
        if m.ret {
            if let Some(start) = open[m.from].pop() {
                bars[m.from].push((start, y));
            }
        } else {
            open[m.to].push(y);
        }
    }
    for (p, stack) in open.into_iter().enumerate() {
        for start in stack {
            bars[p].push((start, last[p].max(start)));
        }
    }
    for b in &mut bars {
        b.sort_unstable();
        // Bars that overlap merge: the drawing shows one bar while the party holds control.
        let mut merged: Vec<(i32, i32)> = Vec::new();
        for &(a, z) in b.iter() {
            match merged.last_mut() {
                Some(m) if a <= m.1 => m.1 = m.1.max(z),
                _ => merged.push((a, z)),
            }
        }
        *b = merged;
    }
    bars
}
