//! `cargo run -p bp-diagram --example lint < geometry.json`: runs `bp`'s geometric rules on another tool's drawing.
//! The judgement kit extracts each plugin render's boxes, routes and label masks into JSON, so both sides of the
//! blind comparison meet the same rules. A route comes as its visible pieces, cut where a label masks the stroke.
//! Prints one finding per line as `rule id: detail`.

use std::io::Read;

use bp_diagram::geom::{Geometry, Label, Rect, Route};
use serde_json::Value;

/// A coordinate: the extractor writes whole pixels.
fn int(v: &Value) -> i32 {
    v.as_i64().and_then(|i| i32::try_from(i).ok()).unwrap_or(0)
}

fn rect(v: &Value) -> Rect {
    Rect::new(int(&v["x"]), int(&v["y"]), int(&v["w"]), int(&v["h"]))
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).expect("read stdin");
    let g: Value = serde_json::from_str(&input).expect("the geometry is JSON");
    let list = |k: &str| g[k].as_array().cloned().unwrap_or_default();
    let geometry = Geometry {
        boxes: list("boxes").iter().map(|b| (text(&b["id"]), rect(b))).collect(),
        routes: list("routes")
            .iter()
            .map(|r| Route {
                id: text(&r["id"]),
                from: text(&r["from"]),
                to: text(&r["to"]),
                pieces: r["pieces"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|piece| piece.as_array().into_iter().flatten().map(|p| (int(&p[0]), int(&p[1]))).collect())
                    .collect(),
                bridges: Vec::new(),
            })
            .collect(),
        labels: list("labels")
            .iter()
            .map(|l| Label { id: text(&l["id"]), of: text(&l["of"]), rect: rect(l) })
            .collect(),
        texts: Vec::new(),
    };
    for f in bp_diagram::lint(&geometry) {
        println!("{} {}: {}", f.rule, f.id, f.detail);
    }
}
