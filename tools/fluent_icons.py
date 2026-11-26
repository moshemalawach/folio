#!/usr/bin/env python3
"""Subset the Fluent UI System Icons fonts to the icons Folio uses.

Usage: python3 tools/fluent_icons.py SOURCE_DIR

SOURCE_DIR holds FluentSystemIcons-{Regular,Filled}.{ttf,json} from
https://github.com/microsoft/fluentui-system-icons, tag 1.1.344 (its fonts/ folder).
Reads assets/fonts/fluent/icons.txt (one icon name per line, # starts a comment) and
writes, next to it, FluentIcons-Regular.ttf, FluentIcons-Filled.ttf and codepoints.txt
(name, size, style, code point). Needs fontTools (pip install fonttools).
"""
import json
import pathlib
import sys

from fontTools import subset

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "fonts" / "fluent"
SIZES = {"Regular": (16, 20, 24, 32), "Filled": (20, 24)}


def main() -> None:
    source = pathlib.Path(sys.argv[1])
    names = [
        line.split("#")[0].strip()
        for line in (OUT / "icons.txt").read_text().splitlines()
        if line.split("#")[0].strip()
    ]
    table = []
    missing = []
    for style, sizes in SIZES.items():
        cmap = json.loads((source / f"FluentSystemIcons-{style}.json").read_text())
        unicodes = []
        for name in names:
            found = False
            for size in sizes:
                key = f"ic_fluent_{name}_{size}_{style.lower()}"
                if key in cmap:
                    unicodes.append(cmap[key])
                    table.append(f"{name}\t{size}\t{style.lower()}\tU+{cmap[key]:04X}")
                    found = True
            if not found and style == "Regular":
                missing.append(name)
        options = subset.Options()
        options.glyph_names = False
        options.notdef_outline = True
        options.layout_features = []
        font = subset.load_font(str(source / f"FluentSystemIcons-{style}.ttf"), options)
        subsetter = subset.Subsetter(options)
        subsetter.populate(unicodes=sorted(set(unicodes)))
        subsetter.subset(font)
        subset.save_font(font, str(OUT / f"FluentIcons-{style}.ttf"), options)
    (OUT / "codepoints.txt").write_text("\n".join(table) + "\n")
    if missing:
        sys.exit(f"not in the Fluent font: {', '.join(missing)}")


if __name__ == "__main__":
    main()
