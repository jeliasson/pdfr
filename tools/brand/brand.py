"""pdfr's app icon: a white form page with a folded corner on the coral
tile, two lines of a form and one field being filled in (outlined in rose,
with a cursor). No text, like other apps' icons. Small sizes draw fewer,
bolder parts. Writes path-only SVGs."""
import os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = sys.argv[1] if len(sys.argv) > 1 else "out"
os.makedirs(OUT, exist_ok=True)
TILE = open(os.path.join(HERE, "squircle.txt")).read().strip()
CORAL_A, CORAL_B, SHADOW = "#FF9259", "#E5336D", "#6B0E33"
ROSE, LINE, FOLD, TITLE = "#E5336D", "#F4C3B4", "#FFD3C2", "#EE9A86"


def defs():
    return f'''<defs>
    <linearGradient id="bg" x1="0.15" y1="0" x2="0.85" y2="1"><stop offset="0" stop-color="{CORAL_A}"/><stop offset="1" stop-color="{CORAL_B}"/></linearGradient>
    <radialGradient id="glow" cx="0.3" cy="0.12" r="0.75"><stop offset="0" stop-color="#FFFFFF" stop-opacity="0.28"/><stop offset="1" stop-color="#FFFFFF" stop-opacity="0"/></radialGradient>
    <linearGradient id="paper" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#FFFFFF"/><stop offset="1" stop-color="#FFF1EC"/></linearGradient>
    <filter id="sh" x="-30%" y="-30%" width="160%" height="160%"><feGaussianBlur stdDeviation="20"/></filter>
  </defs>'''


def svg(body):
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">\n  {defs()}\n{body}\n</svg>\n'


def tile():
    return f'  <path d="{TILE}" fill="url(#bg)"/>\n  <path d="{TILE}" fill="url(#glow)"/>'


def page(h, aspect=0.78, detail=2, shadow=True):
    """The page, `h` units tall and centred (nudged up for its shadow), with
    a folded top-right corner, `detail` form lines (0-2) and the field being
    filled: outlined in rose, with a caret. Everything inside keeps one
    margin, so the page reads as a form at every size."""
    w = round(h * aspect)
    x0, y0 = 512 - w / 2, 512 - h / 2 - 10
    x1, y1 = x0 + w, y0 + h
    r = round(h * 0.06)
    fold = round(w * 0.27)
    pad = round(w * 0.15)
    path = (
        f"M{x0 + r} {y0}H{x1 - fold}L{x1} {y0 + fold}V{y1 - r}"
        f"a{r} {r} 0 0 1 {-r} {r}H{x0 + r}a{r} {r} 0 0 1 {-r} {-r}V{y0 + r}"
        f"a{r} {r} 0 0 1 {r} {-r}z"
    )
    fr = round(fold * 0.18)
    flap = f"M{x1 - fold} {y0}V{y0 + fold - fr}a{fr} {fr} 0 0 0 {fr} {fr}H{x1}z"
    out = []
    if shadow:
        out.append(f'  <path d="{path}" fill="{SHADOW}" fill-opacity="0.30" filter="url(#sh)" transform="translate(0 20)"/>')
    out.append(f'  <path d="{path}" fill="url(#paper)"/>')
    # The flap casts a soft shade on the page under it.
    out.append(f'  <path d="{flap}" fill="{SHADOW}" fill-opacity="0.10" transform="translate(-6 8)"/>')
    out.append(f'  <path d="{flap}" fill="{FOLD}"/>')
    inner = w - 2 * pad
    # Content, top to bottom: a title, form lines, the field being filled.
    lh = round(h * (0.045 if detail == 2 else 0.07))
    th = round(lh * 1.35)
    fh = round(h * (0.15 if detail == 2 else 0.22))
    stroke = round(fh * (0.2 if detail == 2 else 0.24))
    gap = round(h * 0.06)
    items = []  # (kind, width share, height)
    if detail >= 1:
        items.append(("title", 0.5, th))
    if detail == 2:
        items += [("line", 0.86, lh), ("line", 0.66, lh)]
    items.append(("field", 1.0, fh))
    block = sum(i[2] for i in items) + gap * (len(items) - 1) + (gap * 0.6 if detail >= 1 else 0)
    # Centred in the page below the fold's reach.
    top, bottom = y0 + fold * 0.55, y1 - pad * 0.6
    y = top + (bottom - top - block) / 2
    for k, (kind, share, ih) in enumerate(items):
        if kind == "title":
            out.append(f'  <rect x="{x0 + pad}" y="{y}" width="{inner * share}" height="{ih}" rx="{ih / 2}" fill="{TITLE}"/>')
            y += gap * 0.6
        elif kind == "line":
            out.append(f'  <rect x="{x0 + pad}" y="{y}" width="{inner * share}" height="{ih}" rx="{ih / 2}" fill="{LINE}"/>')
        else:
            out.append(
                f'  <rect x="{x0 + pad + stroke / 2}" y="{y + stroke / 2}" width="{inner - stroke}" height="{ih - stroke}" '
                f'rx="{(ih - stroke) * 0.3}" fill="none" stroke="{ROSE}" stroke-width="{stroke}"/>'
            )
            cw, ch = stroke * 0.85, (ih - 2 * stroke) * 0.7
            out.append(
                f'  <rect x="{x0 + pad + stroke + ih * 0.22}" y="{y + (ih - ch) / 2}" width="{cw}" height="{ch}" rx="{cw / 3}" fill="{ROSE}"/>'
            )
        y += ih + gap
    return "\n".join(out)


full = tile() + "\n" + page(600)
small = tile() + "\n" + page(660, detail=1)
tiny = tile() + "\n" + page(720, detail=0, shadow=False)
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
