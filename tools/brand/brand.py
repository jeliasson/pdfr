"""pdfr brand icon: a form field where "pdf" has been typed and the caret
waits for the "r". Writes path-only SVGs (no fonts needed to render)."""
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from concepts import glyphs, width, TILE
OUT = sys.argv[1] if len(sys.argv) > 1 else "out"
os.makedirs(OUT, exist_ok=True)
F = "InterDisplay-Bold.ttf"
INK, CORAL_A, CORAL_B = "#2B1533", "#FF9259", "#E5336D"

def defs():
    return f'''<defs>
    <linearGradient id="bg" x1="0.15" y1="0" x2="0.85" y2="1"><stop offset="0" stop-color="{CORAL_A}"/><stop offset="1" stop-color="{CORAL_B}"/></linearGradient>
    <radialGradient id="glow" cx="0.3" cy="0.12" r="0.75"><stop offset="0" stop-color="#FFFFFF" stop-opacity="0.28"/><stop offset="1" stop-color="#FFFFFF" stop-opacity="0"/></radialGradient>
    <linearGradient id="field" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#FFFFFF"/><stop offset="1" stop-color="#FFF4F2"/></linearGradient>
    <filter id="sh" x="-20%" y="-40%" width="140%" height="180%"><feGaussianBlur stdDeviation="22"/></filter>
  </defs>'''

def svg(body, view="0 0 1024 1024"):
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{view}">\n  {defs()}\n{body}\n</svg>\n'

def tile():
    return f'  <path d="{TILE}" fill="url(#bg)"/>\n  <path d="{TILE}" fill="url(#glow)"/>'

def field(x, y, w, h, r, text=None, size=0, caret_w=18, label=True, second=True):
    out = []
    if label:
        out.append(f'  <rect x="{x+8}" y="{y-74}" width="{w*0.36:.0f}" height="34" rx="17" fill="#FFFFFF" fill-opacity="0.8"/>')
    out.append(f'  <rect x="{x}" y="{y+18}" width="{w}" height="{h}" rx="{r}" fill="#6B0E33" fill-opacity="0.38" filter="url(#sh)"/>')
    out.append(f'  <rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{r}" fill="url(#field)"/>')
    pad = h * 0.24
    if text:
        base = y + h / 2 + size * 0.36
        d, end = glyphs(F, text, size, x + pad * 0.95, base, -0.03)
        out.append(f'  <path d="{d}" fill="{INK}"/>')
        cx = end + size * 0.07
    else:
        cx = x + pad
    ch = h * 0.66
    out.append(f'  <rect x="{cx:.1f}" y="{y + (h-ch)/2:.1f}" width="{caret_w}" height="{ch:.1f}" rx="{caret_w/2}" fill="{CORAL_B}"/>')
    if second:
        y2 = y + h + 64
        out.append(f'  <rect x="{x+8}" y="{y2}" width="{w*0.24:.0f}" height="30" rx="15" fill="#FFFFFF" fill-opacity="0.45"/>')
        out.append(f'  <rect x="{x}" y="{y2+56}" width="{w}" height="{h*0.42:.0f}" rx="{r*0.6:.0f}" fill="#FFFFFF" fill-opacity="0.22"/>')
    return "\n".join(out)

full = tile() + "\n" + field(168, 330, 688, 276, 60, "pdf", 232, 22)
small = tile() + "\n" + field(150, 340, 724, 340, 76, "p", 270, 40, label=True, second=False)
tiny = tile() + "\n" + field(140, 352, 744, 320, 80, None, 0, 64, label=False, second=False)
open(f"{OUT}/pdfr.svg", "w").write(svg(full))
open(f"{OUT}/pdfr-small.svg", "w").write(svg(small))
open(f"{OUT}/pdfr-tiny.svg", "w").write(svg(tiny))
# macOS: Apple's grid puts the 824 px body inside the 1024 canvas, with a drop shadow.
mac = f'''  <filter id="macsh" x="-10%" y="-10%" width="120%" height="125%"><feGaussianBlur stdDeviation="14"/></filter>
  <g transform="translate(512 512) scale(0.9196) translate(-512 -512)">
  <path d="{TILE}" fill="#000000" fill-opacity="0.3" filter="url(#macsh)" transform="translate(0 12)"/>
{full}
  </g>'''
open(f"{OUT}/pdfr-macos.svg", "w").write(svg(mac))
