//! The self-contained page: the embedded faces, the house style, the header, the SVG and the viewer. No network
//! reference and no `src=` attribute, so the file opens anywhere and loads nothing.

use crate::Head;
use crate::svg::esc;

const STYLE: &str = include_str!("style.css");
const VIEWER: &str = include_str!("../viewer/viewer.js");

pub(crate) fn page(head: &Head, svg: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<meta name=\"generator\" content=\"bp-diagram\">\n<title>{title}</title>\n<style>\n{faces}{STYLE}</style>\n</head>\n<body>\n<main class=\"frame\">\n<header class=\"masthead\">\n<div class=\"title\"><h1>{title}</h1><span class=\"tag\">{tag}</span></div>\n<p class=\"lede\">{lede}</p>\n</header>\n{svg}\n</main>\n<script>\n{VIEWER}</script>\n</body>\n</html>\n",
        title = esc(&head.title),
        lede = esc(&head.lede),
        tag = esc(&head.tag),
        faces = crate::font::faces_css(),
    )
}
