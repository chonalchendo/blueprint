//! A diagram's SVG for the view to inline (28, design change 12; 32, design change 8): the `<svg>` element of the page
//! `bp diagram` wrote, with the page's style moved inside it. The page's style is written for a page of its own: it
//! sets `:root`, `body` and `h1`, and keys its dark values to `prefers-color-scheme`. Inlined as it is, it would
//! restyle the view and turn the diagram light on a light system. So the style keeps only the rules that select
//! inside `.bp`, the faces, and the variables, set on the `svg.bp` element, the dark ones always (19: dark by
//! default). The SVG's own markup is the renderer's, less any `<script>` element and `on*` attribute: a page under
//! `diagrams/` is vault content, as untrusted as a body, and the route serves it in the view's origin. The route's
//! content policy is the first guard; this is the second.

use std::fmt::Write;

use bp_vault::{Code, Index, Refusal, Vault};

/// The SVG of the diagram `name` of the design `id` in `project`, its style scoped. A design the project lacks, or a
/// name its `diagrams/` directory does not render, refuses `no_node`.
pub fn diagram(vault: &Vault, index: &Index, project: &str, id: &str, name: &str) -> Result<String, Refusal> {
    let (design, _) = crate::design::design_id(index, project, id)?;
    let path = format!("{design}/diagrams/{name}.html");
    if !crate::design::pages(vault, design.as_str()).iter().any(|n| n == name) {
        return Err(Refusal::new(
            Code::NoNode,
            path,
            "a diagram is a page <design>/diagrams/<name>.html that bp diagram wrote",
            format!("the design page lists its diagrams: /api/{project}/design/{design}"),
        ));
    }
    let page = std::fs::read_to_string(vault.root.join(&path)).map_err(|e| Refusal::failed(&path, e.to_string()))?;
    inline(&page).map(|svg| inert(&svg)).ok_or_else(|| {
        Refusal::failed(&path, format!("the page holds no <svg> element; bp diagram {design} {name} renders it again"))
    })
}

/// The page's first `<svg>` element, with the page's scoped style as its first child; `None` without one.
fn inline(page: &str) -> Option<String> {
    let start = page.find("<svg")?;
    let end = close(page, start)?;
    let svg = &page[start..end];
    let css = page[..start].split_once("<style").and_then(|(_, s)| s.split_once('>')).map(|(_, s)| s);
    let css = css.and_then(|s| s.split_once("</style>")).map(|(s, _)| scope(s)).unwrap_or_default();
    if css.is_empty() {
        return Some(svg.to_string());
    }
    let open = svg.find('>')? + 1;
    Some(format!("{}<style>{css}</style>{}", &svg[..open], &svg[open..]))
}

/// `svg` with no `<script>` element, its text with it, and no `on*` attribute. A comment, a CDATA section and text pass
/// as they are: none of them is an element.
fn inert(svg: &str) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        rest = &rest[lt..];
        let end = if rest.starts_with("<!--") {
            rest.find("-->").map(|e| e + 3)
        } else if rest.starts_with("<![CDATA[") {
            rest.find("]]>").map(|e| e + 3)
        } else {
            tag_len(rest)
        };
        // An unclosed tag or comment ends the drawing.
        let Some(end) = end else { return out };
        let tag = &rest[..end];
        rest = &rest[end..];
        if tag.starts_with("<!") || tag.starts_with("<?") {
            out.push_str(tag);
            continue;
        }
        let name: String = tag[1..].trim_start_matches('/').chars().take_while(|c| is_name(*c)).collect();
        if !name.rsplit(':').next().is_some_and(|local| local.eq_ignore_ascii_case("script")) {
            out.push_str(&no_handlers(tag));
        } else if !tag.starts_with("</") && !tag.ends_with("/>") {
            // The script's text goes with it, to its end tag or the end of the drawing.
            let close = format!("</{}", name.to_ascii_lowercase());
            let at = rest.to_ascii_lowercase().find(&close);
            rest = at.and_then(|a| rest[a..].find('>').map(|e| &rest[a + e + 1..])).unwrap_or("");
        }
    }
    out.push_str(rest);
    out
}

fn is_name(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, ':' | '-' | '_' | '.')
}

/// The length of the tag at the head of `s`, which opens with `<`: to its first `>` outside a quoted value.
fn tag_len(s: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in s.char_indices().skip(1) {
        match quote {
            Some(q) if c == q => quote = None,
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '>' => return Some(i + 1),
            _ => {}
        }
    }
    None
}

/// The tag with each attribute whose name opens with `on`, in any case, removed: an event handler.
fn no_handlers(tag: &str) -> String {
    let mut out = String::with_capacity(tag.len());
    let mut rest = tag;
    let name = 1 + rest[1..].find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(rest.len() - 1);
    out.push_str(&rest[..name]);
    rest = &rest[name..];
    loop {
        let ws = rest.find(|c: char| !c.is_whitespace()).unwrap_or(rest.len());
        let (space, after) = rest.split_at(ws);
        if after.is_empty() || after.starts_with('>') || after.starts_with("/>") {
            out.push_str(rest);
            return out;
        }
        // A lone `/` ends no tag: a parser reads the attributes after it.
        if let Some(more) = after.strip_prefix('/') {
            out.push_str(space);
            out.push('/');
            rest = more;
            continue;
        }
        let attr = after.find(|c: char| c.is_whitespace() || c == '=' || c == '>' || c == '/').unwrap_or(after.len());
        let mut len = attr;
        let value = after[attr..].trim_start();
        if let Some(v) = value.strip_prefix('=') {
            let v = v.trim_start();
            let skipped = after.len() - v.len();
            let vlen = match v.chars().next() {
                Some(q @ ('"' | '\'')) => v[1..].find(q).map_or(v.len(), |e| e + 2),
                _ => v.find(|c: char| c.is_whitespace() || c == '>').unwrap_or(v.len()),
            };
            len = skipped + vlen;
        }
        if !after[..attr].to_ascii_lowercase().starts_with("on") {
            out.push_str(space);
            out.push_str(&after[..len]);
        }
        rest = &after[len..];
    }
}

/// The end of the `<svg>` element that opens at `start`, past a nested one: the viewer script after it may hold the
/// text `</svg>`.
fn close(page: &str, start: usize) -> Option<usize> {
    let mut depth = 0_usize;
    let mut at = start;
    loop {
        let open = page[at..].find("<svg").map(|i| at + i);
        let shut = page[at..].find("</svg>").map(|i| at + i)?;
        match open {
            Some(o) if o < shut => (depth, at) = (depth + 1, o + 4),
            _ if depth == 1 => return Some(shut + "</svg>".len()),
            _ => (depth, at) = (depth - 1, shut + "</svg>".len()),
        }
    }
}

/// The rules of a page's style that hold inside the view: the faces; `:root`'s variables on `svg.bp`, then the dark
/// ones after them, unconditionally; each rule whose selectors name `.bp` or a text class, and the reduced-motion
/// block with the same filter. Every other rule styles the page around the diagram, and goes.
fn scope(css: &str) -> String {
    let mut out = String::new();
    for (prelude, block) in rules(css) {
        if prelude.starts_with("@font-face") {
            let _ = write!(out, "{prelude}{{{block}}}");
        } else if prelude == ":root" {
            let _ = write!(out, "svg.bp{{{block}}}");
        } else if prelude.starts_with("@media") && prelude.contains("prefers-color-scheme: dark") {
            for (_, vars) in rules(block).into_iter().filter(|(p, _)| p == ":root") {
                let _ = write!(out, "svg.bp{{{vars}}}");
            }
        } else if prelude.starts_with("@media") {
            let inner: String =
                rules(block).into_iter().filter_map(|(p, b)| kept(&p).map(|p| format!("{p}{{{b}}}"))).collect();
            if !inner.is_empty() {
                let _ = write!(out, "{prelude}{{{inner}}}");
            }
        } else if let Some(p) = kept(&prelude) {
            let _ = write!(out, "{p}{{{block}}}");
        }
    }
    out
}

/// The selectors of `prelude` that select inside the diagram, the text classes scoped to it; `None` when none do.
fn kept(prelude: &str) -> Option<String> {
    let inside = |s: &&str| {
        s.starts_with("svg.bp") || *s == ".bp" || [".bp ", ".bp.", ".bp>", ".bp:"].iter().any(|p| s.starts_with(p))
    };
    let kept: Vec<String> = prelude
        .split(',')
        .map(str::trim)
        .filter_map(|s| if s.starts_with(".t-") { Some(format!("svg.bp {s}")) } else { inside(&s).then(|| s.into()) })
        .collect();
    (!kept.is_empty()).then(|| kept.join(","))
}

/// The top-level rules of a style sheet: each prelude, comments removed and trimmed, and the text inside its braces.
fn rules(css: &str) -> Vec<(String, &str)> {
    let b = css.as_bytes();
    let (mut out, mut depth, mut start, mut open, mut i) = (Vec::new(), 0_usize, 0, 0, 0);
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i = css[i + 2..].find("*/").map_or(b.len(), |e| i + 2 + e + 2);
                continue;
            }
            q @ (b'"' | b'\'') => {
                i += 1;
                while i < b.len() && b[i] != q {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'{' => {
                if depth == 0 {
                    open = i;
                }
                depth += 1;
            }
            b'}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    out.push((uncomment(&css[start..open]), &css[open + 1..i]));
                    start = i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// A prelude without its comments, trimmed.
fn uncomment(prelude: &str) -> String {
    let mut out = String::new();
    let mut rest = prelude;
    while let Some((before, after)) = rest.split_once("/*") {
        out.push_str(before);
        rest = after.split_once("*/").map_or("", |(_, a)| a);
    }
    out.push_str(rest);
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "<html><head><style>\n@font-face{font-family:bp-inter;src:url(\"data:x{y}\")}\n\
        :root{color-scheme:light dark;--paper:#fff}\n@media (prefers-color-scheme: dark){:root{--paper:#000}}\n\
        /* the page */ body{margin:0}h1{font:600 24px x}\n.bp .box,.frame{fill:var(--paper)}.t-body{font-weight:400}\n\
        .bp-panel{opacity:0}\n@media (prefers-reduced-motion:reduce){.bp *{transition:none}body{x:y}}\n</style></head>\
        <body><svg class=\"bp\"><g data-id=\"a\"><svg></svg></g></svg><script>let s='</svg>';</script></body></html>";

    #[test]
    fn the_svg_carries_the_rules_that_select_inside_it_and_the_dark_values_always() {
        let svg = inline(PAGE).unwrap();
        assert_eq!(
            svg,
            "<svg class=\"bp\"><style>@font-face{font-family:bp-inter;src:url(\"data:x{y}\")}\
             svg.bp{color-scheme:light dark;--paper:#fff}svg.bp{--paper:#000}.bp .box{fill:var(--paper)}\
             svg.bp .t-body{font-weight:400}@media (prefers-reduced-motion:reduce){.bp *{transition:none}}</style>\
             <g data-id=\"a\"><svg></svg></g></svg>"
        );
    }

    #[test]
    fn no_script_element_or_handler_survives_and_the_rest_passes_as_it_is() {
        let hostile = "<svg class=\"bp\" ONLOAD=\"fetch('/api/projects')\" viewBox='0 0 1 1'><script>document.title=\
                       \"ran\"</script><g data-id=\"a\" onclick = 'x()' data-x=\"on > off\"><text>a &lt; b</text>\
                       </g><svg:script xlink:href=\"x.js\"/><SCRIPT type=\"x\">alert(1)</Script><!-- <script> -->\
                       <a href=\"#\" onmouseover=go>t</a><g / onload=x></g></svg>";
        assert_eq!(
            inert(hostile),
            "<svg class=\"bp\" viewBox='0 0 1 1'><g data-id=\"a\" data-x=\"on > off\"><text>a &lt; b</text></g>\
             <!-- <script> --><a href=\"#\">t</a><g /></g></svg>"
        );
        let clean = inline(PAGE).unwrap();
        assert_eq!(inert(&clean), clean, "the renderer's markup passes byte for byte");
        assert_eq!(inert("<svg><script>x"), "<svg>", "an unclosed script takes the rest");
    }

    #[test]
    fn a_page_with_no_style_gives_its_svg_and_one_with_no_svg_none() {
        assert_eq!(
            inline("<p>x</p><svg viewBox=\"0 0 1 1\"></svg>").as_deref(),
            Some("<svg viewBox=\"0 0 1 1\"></svg>")
        );
        assert_eq!(inline("<p>no drawing</p>"), None);
        assert_eq!(inline("<svg><g>"), None, "an unclosed element is no drawing");
    }
}
