//! Every text colour the house style draws reaches WCAG AA against each fill it sits on, in light and in dark: 4.5:1,
//! and 3:1 for large text. The ratios come from the CSS variables in `style.css`, so a token change that drops a pair
//! below its bound fails here.

use std::collections::BTreeMap;

/// Each text token, the fills it sits on, and the bound. A row is a text class on the surface that the style puts
/// under it: names and fields on every node fill, muted text on fills and pills, each kind's tag on its pill and on
/// the paper a removed node's faded pill leaves.
const PAIRS: &[(&str, &[&str], f64)] = &[
    (
        "ink",
        &[
            "paper",
            "node",
            "recess",
            "canvas",
            "accent-tint",
            "added-tint",
            "violet-fill",
            "indigo-fill",
            "teal-fill",
            "plum-fill",
        ],
        4.5,
    ),
    (
        "muted",
        &[
            "paper",
            "node",
            "recess",
            "canvas",
            "soft",
            "tag",
            "chip",
            "accent-tint",
            "added-tint",
            "violet-fill",
            "indigo-fill",
            "teal-fill",
            "plum-fill",
            "grey-tag",
        ],
        4.5,
    ),
    ("accent-ink", &["paper", "accent-tint"], 4.5),
    ("on-accent", &["accent"], 4.5),
    ("added-ink", &["paper"], 4.5),
    ("violet-text", &["violet-tag", "paper"], 4.5),
    ("indigo-text", &["indigo-tag", "paper"], 4.5),
    ("teal-text", &["teal-tag", "paper"], 4.5),
    ("plum-text", &["plum-tag", "paper"], 4.5),
    ("grey-text", &["grey-tag", "paper"], 4.5),
];

/// The `--name:#hex` declarations of the first `:root{...}` at or after `from`.
fn tokens(css: &str, from: usize) -> BTreeMap<String, [u8; 3]> {
    let start = css[from..].find(":root{").unwrap() + from + 6;
    let body = &css[start..start + css[start..].find('}').unwrap()];
    body.split(';')
        .filter_map(|d| d.trim().strip_prefix("--")?.split_once(":#"))
        .map(|(name, hex)| {
            let hex: String = if hex.len() == 3 { hex.chars().flat_map(|c| [c, c]).collect() } else { hex.into() };
            let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap();
            (name.to_string(), [byte(0), byte(2), byte(4)])
        })
        .collect()
}

fn luminance(rgb: [u8; 3]) -> f64 {
    let lin = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.039_28 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(rgb[0]) + 0.7152 * lin(rgb[1]) + 0.0722 * lin(rgb[2])
}

fn ratio(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[test]
fn every_text_reaches_its_contrast_in_light_and_dark() {
    let css = include_str!("../src/style.css");
    let light = tokens(css, 0);
    let dark = tokens(css, css.find("prefers-color-scheme: dark").unwrap());
    let mut low = Vec::new();
    for (scheme, vars) in [("light", &light), ("dark", &dark)] {
        // A dark token that the dark block does not restate keeps its light value.
        let get = |name: &str| *vars.get(name).or_else(|| light.get(name)).unwrap_or_else(|| panic!("no --{name}"));
        for (text, fills, min) in PAIRS {
            for fill in *fills {
                let r = ratio(get(text), get(fill));
                if r < *min {
                    low.push(format!("{scheme}: --{text} on --{fill} is {r:.2}, below {min}"));
                }
            }
        }
    }
    assert!(low.is_empty(), "{low:#?}");
}
