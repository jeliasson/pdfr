"""Icon concepts A (page with comb boxes), B (the field, chosen) and C
(monogram), rendered in docs/brand/concepts.png. Shared helpers for brand.py."""
import subprocess, sys, os
D = os.path.dirname(os.path.abspath(__file__))
def glyphs(font, text, size, x, base, track=0.0):
    r = subprocess.run([sys.executable, f"{D}/glyphs.py", f"{D}/fonts/{font}", text, str(size), str(x), str(base), str(track)],
                       capture_output=True, text=True, check=True)
    end = float(r.stderr.split()[-1])
    return r.stdout.strip(), end

def width(font, text, size, track=0.0):
    return glyphs(font, text, size, 0, 0, track)[1]

TILE = open(os.path.join(os.path.dirname(os.path.abspath(__file__)),'squircle.txt')).read()

# ---------- Concept A: page with comb boxes ----------
def concept_a(small=False):
    boxes = []
    bx, by, bw, bh, gap = 300, 470, 98, 124, 16
    letters = "pdf"
    out = []
    for i in range(4):
        x = bx + i * (bw + gap)
        out.append(f'<rect x="{x}" y="{by}" width="{bw}" height="{bh}" rx="14" fill="#EEF2FF" stroke="#4C6EF5" stroke-width="8"/>')
        if i < 3 and not small:
            w = width("InterDisplay-Bold.ttf", letters[i], 92)
            d, _ = glyphs("InterDisplay-Bold.ttf", letters[i], 92, x + (bw - w) / 2, by + 90)
            out.append(f'<path d="{d}" fill="#1B1F3B"/>')
    cx = bx + 3 * (bw + gap) + bw / 2 - 4
    out.append(f'<rect x="{cx}" y="{by+22}" width="9" height="{bh-44}" rx="4.5" fill="#FF6B3D"/>')
    lines = '' if small else '''
      <rect x="300" y="300" width="250" height="26" rx="13" fill="#D6DCEB"/>
      <rect x="300" y="360" width="380" height="26" rx="13" fill="#E3E7F2"/>
      <rect x="300" y="660" width="424" height="26" rx="13" fill="#E3E7F2"/>
      <rect x="300" y="720" width="300" height="26" rx="13" fill="#E3E7F2"/>'''
    if small:
        out = []
        for i in range(3):
            x = 290 + i * 150
            out.append(f'<rect x="{x}" y="440" width="132" height="160" rx="20" fill="#EEF2FF" stroke="#4C6EF5" stroke-width="22"/>')
        out.append('<rect x="644" y="470" width="22" height="100" rx="11" fill="#FF6B3D"/>')
        lines = '<rect x="290" y="300" width="300" height="44" rx="22" fill="#D6DCEB"/><rect x="290" y="680" width="440" height="44" rx="22" fill="#E3E7F2"/>'
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#5C7CFA"/><stop offset="1" stop-color="#5F3DC4"/></linearGradient>
    <filter id="sh" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur stdDeviation="18"/></filter>
  </defs>
  <path d="{TILE}" fill="url(#bg)"/>
  <g transform="rotate(-5 512 512)">
    <path d="M250 170H660L790 300V870H250Z" fill="#1A1446" opacity="0.35" filter="url(#sh)" transform="translate(0 22)"/>
    <path d="M250 170H660L790 300V870H250Z" fill="#FFFFFF"/>
    <path d="M660 170V270Q660 300 690 300H790Z" fill="#DCE2F5"/>
    {lines}
    {"".join(out)}
  </g>
</svg>'''

# ---------- Concept B: the field ----------
def concept_b(small=False):
    fw = width("InterDisplay-Bold.ttf", "pdf", 250, -0.02)
    tx = 236
    d, end = glyphs("InterDisplay-Bold.ttf", "pdf", 250, tx, 590, -0.02)
    text = '' if small else f'<path d="{d}" fill="#2A1636"/>'
    cur_x = (end + 22) if not small else 330
    label = '<rect x="180" y="300" width="230" height="34" rx="17" fill="#FFFFFF" opacity="0.75"/>'
    if small:
        label = '<rect x="170" y="290" width="300" height="56" rx="28" fill="#FFFFFF" opacity="0.8"/>'
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#FF8F5A"/><stop offset="1" stop-color="#E8366B"/></linearGradient>
    <filter id="sh" x="-20%" y="-30%" width="140%" height="160%"><feGaussianBlur stdDeviation="20"/></filter>
  </defs>
  <path d="{TILE}" fill="url(#bg)"/>
  {label}
  <rect x="170" y="400" width="684" height="290" rx="64" fill="#5A0F2E" opacity="0.35" filter="url(#sh)" transform="translate(0 20)"/>
  <rect x="170" y="390" width="684" height="290" rx="64" fill="#FFFFFF"/>
  {text}
  <rect x="{cur_x}" y="{430 if not small else 450}" width="{18 if not small else 40}" height="{210 if not small else 170}" rx="9" fill="#E8366B"/>
</svg>'''

# ---------- Concept C: monogram ----------
def concept_c(small=False):
    sw = 76 if not small else 110
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#272B45"/><stop offset="1" stop-color="#14162A"/></linearGradient>
    <linearGradient id="mint" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#63E6BE"/><stop offset="1" stop-color="#12B886"/></linearGradient>
  </defs>
  <path d="{TILE}" fill="url(#bg)"/>
  <!-- the bowl of the p is a form box -->
  <rect x="{300+sw/2}" y="{250+sw/2}" width="{360-sw}" height="{360-sw}" rx="{70 - sw/4}" fill="#63E6BE" opacity="0.12"/>
  <rect x="{300+sw/2}" y="{250+sw/2}" width="{360-sw}" height="{360-sw}" rx="{70 - sw/4}" fill="none" stroke="url(#mint)" stroke-width="{sw}"/>
  <rect x="300" y="250" width="{sw}" height="540" rx="{sw/2}" fill="url(#mint)"/>
  <rect x="{720}" y="{290}" width="{sw*0.55}" height="280" rx="{sw*0.275}" fill="#FFB547"/>
</svg>'''

if __name__ == "__main__":
  os.makedirs(f"{D}/out", exist_ok=True)
  for name, fn in [("a", concept_a), ("b", concept_b), ("c", concept_c)]:
    open(f"{D}/out/{name}.svg", "w").write(fn())
    open(f"{D}/out/{name}-small.svg", "w").write(fn(True))
