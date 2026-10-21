//! The self-contained page: the embedded faces, the house style, the header, the SVG and the viewer. No network
//! reference and no `src=` attribute, so the file opens anywhere and loads nothing.

use crate::Head;
use crate::svg::esc;

const STYLE: &str = include_str!("style.css");
const VIEWER: &str = include_str!("../viewer/viewer.js");

pub(crate) fn page(head: &Head, svg: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<meta name=\"generator\" content=\"bp-diagram\">\n<title>{title}</title>\n<style>\n{faces}{STYLE}</style>\n</head>\n<body>\n<main class=\"frame\">\n<p class=\"eyebrow\">{eyebrow}</p>\n<h1>{title}</h1>\n<p class=\"lede\">{lede}</p>\n{svg}\n</main>\n<script>\n{VIEWER}</script>\n</body>\n</html>\n",
        title = esc(&head.title),
        lede = esc(&head.lede),
        eyebrow = esc(&head.eyebrow),
        faces = crate::font::faces_css(),
    )
}
