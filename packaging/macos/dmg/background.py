#!/usr/bin/env python3
"""Write background.svg, the Tinta Office macOS DMG window background (660 x 400 pt).

The text is outlined with Sora (OFL-1.1, the Tinta Office wordmark font), so the SVG renders the same
everywhere without fonts. Fonts are never committed here: pass the Sora variable font
(Sora[wght].ttf from Google Fonts / craft-fonts). Needs `pip install fonttools`.

    python3 packaging/macos/dmg/background.py KIND NAME packaging/macos/dmg/background.svg Sora.ttf

KIND is pdf, texto, calculo or presenta; NAME is the part after "Tinta" (PDF, Texto, ...).
Then run generate.py to render background.tiff.
"""
import sys
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen

IVORY, PAGE, EDGE, INK, MUTED = "#F3EDE1", "#FBF8F2", "#D8CBB2", "#1E2547", "#5A6080"
COLORS = {"pdf": "#B03A2E", "texto": "#2D4BA0", "calculo": "#4F7A28", "presenta": "#C7780F"}
_fonts = {}


def font(wght):
    if wght not in _fonts:
        _fonts[wght] = instantiateVariableFont(TTFont(sys.argv[4]), {"wght": wght})
    return _fonts[wght]


def text(s, x, y, size, wght, fill, tracking=0.0):
    """Outlined text: one path, baseline at y. Returns (svg, advance)."""
    f = font(wght)
    gs, cmap, hmtx = f.getGlyphSet(), f.getBestCmap(), f["hmtx"]
    k = size / f["head"].unitsPerEm
    pen = SVGPathPen(gs, ntos=lambda v: f"{v:.2f}")
    cx = x
    for ch in s:
        g = cmap.get(ord(ch))
        if g is None:
            continue
        gs[g].draw(TransformPen(pen, (k, 0, 0, -k, cx, y)))
        cx += hmtx[g][0] * k + tracking
    return f'<path fill="{fill}" d="{pen.getCommands()}"/>', cx - x - tracking


def background(kind, name):
    c = COLORS[kind]
    out = ['<svg xmlns="http://www.w3.org/2000/svg" width="660" height="400" viewBox="0 0 660 400">',
           '<defs><clipPath id="panel"><rect width="232" height="400"/></clipPath></defs>',
           f'<rect width="660" height="400" fill="{PAGE}"/>']
    # Left panel: a wall of azulejo tiles. Quarter circles meet across tile joints as full circles.
    t = 116
    tiles = []
    for row in range(-1, 5):
        for col in range(0, 2):
            x, y = col * t, row * t + 26
            tiles.append(f'<rect x="{x}" y="{y}" width="{t}" height="{t}" fill="{IVORY}" stroke="{EDGE}" stroke-width="1.5"/>')
    for row in range(-1, 6):
        for col in range(0, 3):
            tiles.append(f'<circle cx="{col * t}" cy="{row * t + 26}" r="36" fill="{c}"/>')
    for row in range(-1, 5):
        for col in range(0, 2):
            cx, cy = col * t + t / 2, row * t + 26 + t / 2
            tiles.append(f'<rect x="{cx - 9}" y="{cy - 9}" width="18" height="18" rx="3" transform="rotate(45 {cx} {cy})" fill="{c}"/>')
    out.append(f'<g clip-path="url(#panel)">{"".join(tiles)}</g>')
    out.append(f'<line x1="232.5" y1="0" x2="232.5" y2="400" stroke="{INK}" stroke-width="1.2"/>')
    # Title: "Tinta" bold + app name regular, as the wordmark.
    a, w = text("Tinta", 267, 72, 30, 700, INK)
    b, _ = text(name, 267 + w + 8, 72, 30, 400, INK)
    out += [a, b]
    out.append(f'<line x1="267" y1="88.5" x2="624" y2="88.5" stroke="{INK}" stroke-width="0.6"/>')
    out.append(f'<circle cx="271" cy="88.5" r="4.5" fill="{c}"/>')
    s, _ = text("Tinta Office", 267, 110, 11, 400, MUTED, tracking=0.6)
    out.append(s)
    # Arrow between the app (326, 205) and Applications (574, 205), outside both icon boxes.
    out.append(f'<rect x="408" y="197" width="78" height="16" rx="8" fill="{c}" fill-opacity="0.35"/>'
               f'<line x1="414" y1="205" x2="482" y2="205" stroke="{INK}" stroke-width="1.6"/>'
               f'<path d="M499.1,205.0 L486.1,211.0 L486.1,199.0 Z" fill="{INK}"/>')
    out.append(f'<line x1="267" y1="335.5" x2="624" y2="335.5" stroke="{INK}" stroke-width="0.6"/>')
    s, _ = text("DRAG ONTO APPLICATIONS TO INSTALL", 267, 360, 10.5, 500, MUTED, tracking=1.6)
    out.append(s)
    out.append("</svg>")
    return "\n".join(out) + "\n"


if __name__ == "__main__":
    open(sys.argv[3], "w").write(background(sys.argv[1], sys.argv[2]))
