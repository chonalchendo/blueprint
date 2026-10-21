"""Makes the three embedded faces and their advance table (28, design change 3: measure with the font you draw with).

Run once when the faces or the character set change, from the crate directory, with fontTools and a brotli module:

    python3 fonts/subset.py <inter.woff2> <geist-mono.woff2>

The sources are the human view's variable fonts (OFL-1.1). Each face is pinned to one weight, cut to CHARS, and stripped
of hinting and of every layout table, so the browser draws each glyph at its plain advance: no kerning, no ligature.
The width the crate measures is then the width the browser draws. It writes `fonts/*.woff2` and `src/metrics.rs`.
"""
import sys
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont
from fontTools import subset

CHARS = [chr(c) for c in range(0x20, 0x7F)] + list(" ·×é–—‘’“”•…←↑→↓↔−")
FACES = [("INTER_400", "inter-400", 0, {"wght": 400, "opsz": 14}), ("INTER_600", "inter-600", 0, {"wght": 600, "opsz": 14}),
         ("MONO_400", "geist-mono-400", 1, {"wght": 400})]


def main(sources):
    rows = []
    for const, name, src, axes in FACES:
        font = instantiateVariableFont(TTFont(sources[src]), axes)
        opts = subset.Options()
        opts.layout_features, opts.hinting, opts.name_IDs, opts.notdef_outline = [], False, [0, 1, 2, 13], True
        opts.drop_tables += ["GDEF", "GPOS", "GSUB", "kern", "STAT", "MVAR", "HVAR", "meta", "gasp", "prep", "DSIG"]
        opts.flavor = "woff2"
        sub = subset.Subsetter(opts)
        sub.populate(unicodes=[ord(c) for c in CHARS])
        sub.subset(font)
        subset.save_font(font, f"fonts/{name}.woff2", opts)
        cmap, hmtx = font.getBestCmap(), font["hmtx"]
        advances = ", ".join(f"({ord(c)}, {hmtx[cmap[ord(c)]][0]})" for c in CHARS if ord(c) in cmap)
        rows.append(f"pub(crate) static {const}: Face = Face {{\n    upm: {font['head'].unitsPerEm},\n"
                    f"    cap: {font['OS/2'].sCapHeight},\n    advances: &[{advances}],\n}};\n")
    with open("src/metrics.rs", "w") as out:
        out.write("//! The advance of each embedded glyph, in font units. Written by `fonts/subset.py`; do not edit.\n\n")
        out.write("use crate::font::Face;\n\n" + "\n".join(rows))


if __name__ == "__main__":
    main(sys.argv[1:])
