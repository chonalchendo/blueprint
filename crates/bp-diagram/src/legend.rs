//! The legend strip: exactly the roles, styles and marks in use (`legend/truthful`). The author may name each entry;
//! a named legend that lists a key not in use, or misses one in use, is refused.

use std::fmt::Write as _;

use crate::Finding;
use crate::font::Font;
use crate::nodes::{role_words, up4};
use crate::scene::{Key, Scene};
use crate::spec::{Spec, Type};
use crate::svg::esc;

/// The keys in use, in order of first use.
fn in_use(spec: &Spec) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    let mut add = |k: &str| {
        if !keys.iter().any(|x| x == k) {
            keys.push(k.to_string());
        }
    };
    if !spec.focus.is_empty() {
        add("focus");
    }
    if spec.ty == Type::Sequence {
        add("call");
        if spec.steps.iter().any(|s| s.ret || s.back.is_some()) {
            add("return");
        }
        if spec.steps.iter().any(|s| s.note.is_some()) {
            add("note");
        }
        return keys;
    }
    for n in &spec.nodes {
        add(&n.role);
    }
    if spec.edges.iter().any(|e| e.dashed) {
        add("dashed");
    }
    keys
}

pub(crate) fn rule(spec: &Spec) -> Vec<Finding> {
    let Some(named) = &spec.legend else { return Vec::new() };
    let used = in_use(spec);
    let mut out = Vec::new();
    for (k, _) in named.iter().filter(|(k, _)| !used.contains(k)) {
        out.push(Finding::error("legend/truthful", "legend", format!("`{k}` is in the legend but not in the diagram")));
    }
    for k in used.iter().filter(|k| !named.iter().any(|(n, _)| n == *k)) {
        out.push(Finding::error("legend/truthful", "legend", format!("`{k}` is in the diagram but not in the legend")));
    }
    out
}

/// Places the legend under the drawing and grows the scene to hold it.
pub(crate) fn lay_out(scene: &mut Scene, spec: &Spec) {
    let named = |k: &str| spec.legend.as_ref().and_then(|l| l.iter().find(|(n, _)| n == k)).map(|(_, l)| l.clone());
    let mut keys: Vec<(String, String)> = in_use(spec)
        .into_iter()
        .map(|k| {
            let words = named(&k).unwrap_or_else(|| default_words(spec, &k));
            (k, words)
        })
        .collect();
    if spec.ty == Type::Er && spec.level.as_deref() != Some("conceptual") {
        keys.push(("marks".into(), "# key · → reference · #→ key part that is a reference".into()));
    }
    // A tagged role's swatch is the tag itself, as its nodes draw it: the reader matches words, not fills.
    let tag = |k: &str| match k {
        "elsewhere" => spec.nodes.iter().find(|n| n.role == k).and_then(|n| n.owner.clone()),
        _ => role_words(k).0.map(str::to_string),
    };
    let (mut x, mut y) = (88, scene.legend_y + 20);
    for (key, label) in keys {
        let tag = tag(&key);
        // A badge that already says the words needs no words after it.
        let label = if tag.as_ref().is_some_and(|t| t.eq_ignore_ascii_case(&label)) { String::new() } else { label };
        let swatch = tag.as_ref().map_or(20, |t| Font::Body.width(t, 11, 0) + 12);
        let w = up4(swatch + 8 + Font::Body.width(&label, 11, 0) + 28);
        if x + w > scene.w - 24 && x > 88 {
            (x, y) = (88, y + 24);
        }
        scene.legend.push(Key { kind: key, tag, label, x, y });
        x += w;
    }
    scene.h = y + 36;
}

fn default_words(spec: &Spec, key: &str) -> String {
    match key {
        "focus" => "Focus".into(),
        "call" => "Call".into(),
        "return" => "Return".into(),
        "note" => "State the tool records".into(),
        "dashed" => {
            let mut words: Vec<&str> = Vec::new();
            for e in spec.edges.iter().filter(|e| e.dashed && !e.label.is_empty()) {
                if !words.contains(&e.label.as_str()) {
                    words.push(&e.label);
                }
            }
            let text = if words.is_empty() { "a weaker tie".to_string() } else { words.join(", ") };
            let mut c = text.chars();
            c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
        }
        role => role_words(role).1.into(),
    }
}

/// The strip as SVG: a hairline, `Legend`, then one swatch and its words per entry.
pub(crate) fn draw(scene: &Scene) -> String {
    let Some(first) = scene.legend.first() else { return String::new() };
    let mut out = format!(
        "<g class=\"legend\"><path class=\"hair\" d=\"M24,{}H{}\"/><text class=\"t-strong glabel\" x=\"24\" y=\"{}\" font-size=\"12\">Legend</text>",
        first.y - 8,
        scene.w - 24,
        first.y + 10
    );
    for k in &scene.legend {
        let (x, y) = (k.x, k.y);
        let swatch = match k.kind.as_str() {
            "dashed" | "call" | "return" => {
                let dashed = if k.kind == "call" { "" } else { " dashed" };
                format!("<g class=\"swatch edge{dashed}\"><path class=\"stroke\" d=\"M{x},{}H{}\"/></g>", y + 4, x + 20)
            }
            "note" => format!(
                "<g class=\"swatch note\"><rect class=\"notebox\" x=\"{x}\" y=\"{y}\" width=\"20\" height=\"12\" rx=\"3\"/></g>"
            ),
            "marks" => String::new(),
            role => match &k.tag {
                Some(t) => format!(
                    "<g class=\"swatch role-{role}\"><rect class=\"tagbox\" x=\"{x}\" y=\"{}\" width=\"{}\" height=\"18\" rx=\"9\"/><text class=\"t-body tag\" x=\"{}\" y=\"{}\" font-size=\"11\">{}</text></g>",
                    y - 3,
                    Font::Body.width(t, 11, 0) + 12,
                    x + 6,
                    y + 10,
                    esc(t)
                ),
                // A sequence's focus is a message, so its swatch is a line.
                None if role == "focus" && !scene.steps.is_empty() => format!(
                    "<g class=\"swatch edge focus\"><path class=\"stroke\" d=\"M{x},{}H{}\"/></g>",
                    y + 4,
                    x + 20
                ),
                None => {
                    let class = if role == "focus" { "focus".to_string() } else { format!("role-{role}") };
                    format!(
                        "<g class=\"swatch {class}\"><rect class=\"box\" x=\"{x}\" y=\"{y}\" width=\"20\" height=\"12\" rx=\"3\"/></g>"
                    )
                }
            },
        };
        let tx = match (&k.tag, k.kind.as_str()) {
            (_, "marks") => x,
            (Some(t), _) => x + Font::Body.width(t, 11, 0) + 20,
            _ => x + 28,
        };
        out.push_str(&swatch);
        if !k.label.is_empty() {
            let _ = write!(
                out,
                "<text class=\"t-body key\" x=\"{tx}\" y=\"{}\" font-size=\"11\">{}</text>",
                y + 10,
                esc(&k.label)
            );
        }
    }
    out.push_str("</g>");
    out
}
