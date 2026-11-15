#!/usr/bin/env python3
"""Screen vs file: compares typed text in an app screenshot with a render of
the saved PDF at the same scale.

usage: compare_screen.py screenshot.png render.png page_x page_y scale fields.json
fields.json: list of [x0, y0, x1, y1] display-space rects (points) to compare.
For each field it finds dark (text) pixels in both images and reports the
offset of the ink bounding box and centroid. Pass if every offset <= 1 px.
"""
import json, sys
import numpy as np
from PIL import Image

shot = np.asarray(Image.open(sys.argv[1]).convert("L")).astype(int)
ref = np.asarray(Image.open(sys.argv[2]).convert("L")).astype(int)
px, py, s = float(sys.argv[3]), float(sys.argv[4]), float(sys.argv[5])
rects = json.load(open(sys.argv[6]))
worst = 0.0
ok = True
for r in rects:
    x0, y0, x1, y1 = r[:4]
    name = r[4] if len(r) > 4 else ""
    # Inset slightly so drawn box borders are excluded.
    ix0, iy0, ix1, iy1 = [int(round(v * s)) for v in (x0 + 1.2, y0 + 1.2, x1 - 1.2, y1 - 1.2)]
    a = shot[int(round(py)) + iy0:int(round(py)) + iy1, int(round(px)) + ix0:int(round(px)) + ix1]
    b = ref[iy0:iy1, ix0:ix1]
    h = min(a.shape[0], b.shape[0]); w = min(a.shape[1], b.shape[1])
    a, b = a[:h, :w], b[:h, :w]
    ma, mb = a < 110, b < 110
    if ma.sum() < 5 or mb.sum() < 5:
        print(f"{name:24s} no ink (screen {ma.sum()}, file {mb.sum()})")
        ok = False
        continue
    def stats(m):
        ys, xs = np.nonzero(m)
        return np.array([xs.min(), ys.min(), xs.max(), ys.max(), xs.mean(), ys.mean()])
    sa, sb = stats(ma), stats(mb)
    d = sa - sb
    # Best-aligning shift between the two darkness images (cross-correlation
    # over integer shifts, then parabolic sub-pixel refinement). This is the
    # text's displacement, independent of how heavy each rasteriser draws.
    da = (255 - a).astype(float); db = (255 - b).astype(float)
    best = None; scores = {}
    for dy in range(-3, 4):
        for dx in range(-3, 4):
            sh = np.roll(np.roll(db, dy, axis=0), dx, axis=1)
            c = float((da * sh).sum()) / (np.linalg.norm(da) * np.linalg.norm(sh) + 1e-9)
            scores[(dx, dy)] = c
            if best is None or c > best[0]:
                best = (c, dx, dy)
    _, bx, by = best
    def refine(c_m, c_0, c_p):
        den = c_m - 2 * c_0 + c_p
        return 0.0 if den == 0 else 0.5 * (c_m - c_p) / den
    sx = bx + (refine(scores.get((bx-1,by),best[0]), best[0], scores.get((bx+1,by),best[0])) if abs(bx) < 3 else 0)
    sy = by + (refine(scores.get((bx,by-1),best[0]), best[0], scores.get((bx,by+1),best[0])) if abs(by) < 3 else 0)
    m = max(abs(sx), abs(sy))
    worst = max(worst, m)
    flag = "OK " if m <= 1.0 else "BAD"
    if m > 1.0: ok = False
    print(f"{flag} {name:26s} shift dx={sx:+.2f} dy={sy:+.2f} px   bbox dx0={d[0]:+.0f} dy0={d[1]:+.0f} dx1={d[2]:+.0f} dy1={d[3]:+.0f}")
print(f"worst offset {worst:.2f} px -> {'PASS' if ok else 'FAIL'}")
