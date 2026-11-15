# Prints SVG path data for text set in an Inter Display weight.
# usage: glyphs.py FONT TEXT SIZE X BASELINE [tracking_em]
import sys
from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
font = TTFont(sys.argv[1]); text = sys.argv[2]; size = float(sys.argv[3])
x = float(sys.argv[4]); base = float(sys.argv[5]); track = float(sys.argv[6]) if len(sys.argv) > 6 else 0
upm = font['head'].unitsPerEm; s = size / upm
gs = font.getGlyphSet(); cmap = font.getBestCmap(); hmtx = font['hmtx']
pen = SVGPathPen(gs)
for ch in text:
    g = cmap[ord(ch)]
    gs[g].draw(TransformPen(pen, (s, 0, 0, -s, x, base)))
    x += hmtx[g][0] * s + track * size
print(pen.getCommands())
print("width_end", x, file=sys.stderr)
