"""pdfr's app icon: a text cursor (I-beam) standing on the line of a form
field, white on the coral tile: click and type. No text, like other apps'
icons. Small sizes draw it bolder. Writes path-only SVGs."""
import os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = sys.argv[1] if len(sys.argv) > 1 else "out"
os.makedirs(OUT, exist_ok=True)
TILE = open(os.path.join(HERE, "squircle.txt")).read().strip()
CORAL_A, CORAL_B, SHADOW = "#FF9259", "#E5336D", "#6B0E33"


def defs():
    return f'''<defs>
    <linearGradient id="bg" x1="0.15" y1="0" x2="0.85" y2="1"><stop offset="0" stop-color="{CORAL_A}"/><stop offset="1" stop-color="{CORAL_B}"/></linearGradient>
    <radialGradient id="glow" cx="0.3" cy="0.12" r="0.75"><stop offset="0" stop-color="#FFFFFF" stop-opacity="0.28"/><stop offset="1" stop-color="#FFFFFF" stop-opacity="0"/></radialGradient>
    <filter id="sh" x="-30%" y="-30%" width="160%" height="160%"><feGaussianBlur stdDeviation="20"/></filter>
  </defs>'''


def svg(body):
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">\n  {defs()}\n{body}\n</svg>\n'


def tile():
    return f'  <path d="{TILE}" fill="url(#bg)"/>\n  <path d="{TILE}" fill="url(#glow)"/>'


def cursor(cap_w, cap_h, stem_w, top, bottom, line_w, line_h, line_y, shadow=True):
    """The I-beam (caps and stem) and the field line under it, centred."""
    c = 512
    ibeam = (
        f"M{c - cap_w / 2} {top}h{cap_w}v{cap_h}h{-(cap_w - stem_w) / 2}"
        f"V{bottom - cap_h}h{(cap_w - stem_w) / 2}v{cap_h}h{-cap_w}v{-cap_h}"
        f"h{(cap_w - stem_w) / 2}V{top + cap_h}h{-(cap_w - stem_w) / 2}z"
    )
    line = f'<rect x="{c - line_w / 2}" y="{line_y}" width="{line_w}" height="{line_h}" rx="{line_h / 2}"'
    out = []
    if shadow:
        out.append(
            f'  <g fill="{SHADOW}" fill-opacity="0.35" filter="url(#sh)" transform="translate(0 20)">'
            f'<path d="{ibeam}"/>{line}/></g>'
        )
    out.append(f'  <path d="{ibeam}" fill="#FFFFFF"/>')
    out.append(f'  {line} fill="#FFFFFF" fill-opacity="0.85"/>')
    return "\n".join(out)


full = tile() + "\n" + cursor(240, 64, 64, 236, 764, 624, 44, 808)
small = tile() + "\n" + cursor(300, 84, 96, 210, 770, 660, 64, 812)
tiny = tile() + "\n" + cursor(340, 110, 130, 190, 760, 700, 90, 806, shadow=False)
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
