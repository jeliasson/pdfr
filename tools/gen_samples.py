#!/usr/bin/env python3
"""Generates the sample PDFs used by pdfr's tests.

Run with:  uv run --with reportlab --with pillow --with numpy tools/gen_samples.py [outdir]

Writes (default outdir: samples/):
  real_form.pdf       AcroForm: text, comb, checkbox, radio group, drop-down, multi-line
  flat_form.pdf       the same kind of form, only drawn (no fields)
  flat_form.json      expected fields on the flat form, in display space (points, y down)
  flat_rotated.pdf    flat_form drawn on a landscape page with /Rotate 90 (displays upright)
  scanned_form.pdf    flat_form rasterised at 150 dpi, blurred, noisy, skewed 0.25 deg
  perf_text.pdf       one dense text page (embedded TrueType font)
  perf_image.pdf      one page dominated by large photographic images
  big_300.pdf         (only with --big) 300 pages: flat forms and text pages
"""

import io
import json
import math
import os
import random
import subprocess
import sys
import tempfile

import numpy as np
from PIL import Image, ImageFilter
from reportlab.lib.colors import Color, black, white
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.ttfonts import TTFont
from reportlab.pdfgen import canvas

W, H = 595.0, 842.0  # A4 portrait, points
OUT = "samples"
random.seed(7)
np.random.seed(7)

BODY = (
    "Please complete this form in capital letters using black ink. All information is "
    "treated confidentially and will only be used to process your application. Fields "
    "marked as optional may be left blank. Return the signed form to the address shown "
    "at the bottom of this page within thirty days of receipt."
)


class Flat:
    """Draws the flat form in display coordinates (y down) and records expected fields."""

    def __init__(self, c):
        self.c = c
        self.expected = []

    # display -> reportlab user coordinates (y up)
    def Y(self, y):
        return H - y

    def text(self, x, y, s, size=9, font="Helvetica"):
        self.c.setFont(font, size)
        self.c.drawString(x, self.Y(y), s)

    def rect(self, x0, y0, x1, y1, lw=0.8, fill=None):
        c = self.c
        c.setLineWidth(lw)
        if fill is not None:
            c.setFillColor(fill)
            c.rect(x0, self.Y(y1), x1 - x0, y1 - y0, stroke=0, fill=1)
            c.setFillColor(black)
        else:
            c.rect(x0, self.Y(y1), x1 - x0, y1 - y0, stroke=1, fill=0)

    def line(self, x0, y0, x1, y1, lw=0.8):
        self.c.setLineWidth(lw)
        self.c.line(x0, self.Y(y0), x1, self.Y(y1))

    def expect(self, kind, rect, label=None, cells=None):
        e = {"kind": kind, "rect": [round(v, 2) for v in rect]}
        if cells:
            e["cells"] = len(cells)
        if label:
            e["label"] = label
        self.expected.append(e)

    def draw(self):
        c = self.c
        # Underlined heading: must NOT become a field.
        self.text(50, 60, "Membership Application", size=18, font="Helvetica-Bold")
        hw = pdfmetrics.stringWidth("Membership Application", "Helvetica-Bold", 18)
        self.line(50, 64, 50 + hw, 64, lw=1.2)

        # Body paragraph: must NOT become fields.
        words, lines, cur = BODY.split(), [], ""
        for w in words:
            t = (cur + " " + w).strip()
            if pdfmetrics.stringWidth(t, "Helvetica", 9) > 495:
                lines.append(cur)
                cur = w
            else:
                cur = t
        lines.append(cur)
        for i, ln in enumerate(lines):
            self.text(50, 88 + i * 12, ln)

        # Filled decorative bar with white text: must NOT become a field.
        self.rect(50, 140, 545, 156, fill=Color(0.25, 0.25, 0.3))
        c.setFillColor(white)
        self.text(56, 152, "SECTION 1  PERSONAL DETAILS", size=10, font="Helvetica-Bold")
        c.setFillColor(black)

        # Plain box.
        self.text(50, 182, "Full name")
        self.rect(150, 168, 545, 188)
        self.expect("text", [150, 168, 545, 188], "Full name")

        # Shared-edge character cells (10 x 14pt).
        self.text(50, 214, "Personal number")
        x0, cw = 150, 14
        self.rect(x0, 200, x0 + 10 * cw, 218)
        for i in range(1, 10):
            self.line(x0 + i * cw, 200, x0 + i * cw, 218)
        self.expect("comb", [x0, 200, x0 + 10 * cw, 218], "Personal number", cells=range(10))

        # Separate date squares in groups of 2, 2 and 4.
        self.text(50, 246, "Date of birth (DD MM YYYY)")
        x = 190
        for n in (2, 2, 4):
            gx = x
            for i in range(n):
                self.rect(x, 232, x + 14, 250)
                x += 14 + 2
            self.expect("comb", [gx, 232, x - 2, 250], "Date of birth", cells=range(n))
            x += 10

        # Checkboxes.
        self.text(50, 278, "Gender")
        for i, lab in enumerate(["Female", "Male", "Other"]):
            bx = 150 + i * 90
            self.rect(bx, 268, bx + 10, 278)
            self.text(bx + 14, 277, lab)
            self.expect("checkbox", [bx, 268, bx + 10, 278], lab)

        # Write-on line.
        self.text(50, 312, "Occupation")
        self.line(150, 314, 545, 314)
        self.expect("line", [150, 314, 545, 314], "Occupation")

        # Underscore line made of text characters.
        self.text(50, 340, "Email ____________________________________________________")
        uw0 = 50 + pdfmetrics.stringWidth("Email ", "Helvetica", 9)
        uw1 = 50 + pdfmetrics.stringWidth(
            "Email ____________________________________________________", "Helvetica", 9)
        self.expect("line", [uw0, 340, uw1, 341], "Email")

        # Tick-mark ruler: 6 ticks -> 5 cells.
        self.text(50, 372, "Postcode")
        self.line(150, 374, 250, 374)
        for i in range(6):
            tx = 150 + i * 20
            self.line(tx, 374, tx, 368)
        self.expect("comb", [150, 362, 250, 374], "Postcode", cells=range(5))

        # Label-on-top cells.
        y0, y1 = 392, 422
        cols = [(50, 300, "Street address"), (300, 400, "Postal code"), (400, 545, "City")]
        self.rect(50, y0, 545, y1)
        for (a, b, lab) in cols:
            if a > 50:
                self.line(a, y0, a, y1)
            self.text(a + 3, y0 + 8, lab, size=6.5)
            self.expect("text", [a, y0 + 10, b, y1], lab)

        # Filled bar 2.
        self.rect(50, 440, 545, 456, fill=Color(0.25, 0.25, 0.3))
        c.setFillColor(white)
        self.text(56, 452, "SECTION 2  ITEMS", size=10, font="Helvetica-Bold")
        c.setFillColor(black)

        # Empty table grid with a header row.
        tx = [50, 250, 350, 445, 545]
        ty = [466, 482, 500, 518, 536]
        for y in ty:
            self.line(tx[0], y, tx[-1], y)
        for x in tx:
            self.line(x, ty[0], x, ty[-1])
        for x, h in zip(tx, ["Description", "Quantity", "Unit price", "Total"]):
            self.text(x + 3, 477, h, font="Helvetica-Bold", size=8)
        for r in range(1, 4):
            for col in range(4):
                self.expect("text", [tx[col], ty[r], tx[col + 1], ty[r + 1]],
                            ["Description", "Quantity", "Unit price", "Total"][col])

        # Large notes box -> multi-line.
        self.text(50, 560, "Notes")
        self.rect(50, 566, 545, 676)
        self.expect("multi", [50, 566, 545, 676], "Notes")

        # Signature line and date at the bottom.
        self.text(50, 720, "Signature")
        self.line(110, 722, 300, 722)
        self.expect("line", [110, 722, 300, 722], "Signature")
        self.text(320, 720, "Date")
        self.line(350, 722, 545, 722)
        self.expect("line", [350, 722, 545, 722], "Date")

        self.text(50, 790, "Return to: Example Association, Box 123, 111 22 Stockholm, Sweden",
                  size=8)


def flat_form(path, rotated=False):
    if rotated:
        # reportlab swaps the MediaBox when the page is rotated by 90, so pass
        # the portrait size to get a landscape MediaBox (checked with pdfinfo).
        c = canvas.Canvas(path, pagesize=(W, H))
        c.setPageRotation(90)
        # Portrait drawing coords (px, py) -> landscape user space (H - py, px),
        # which /Rotate 90 displays upright.
        c.transform(0, 1, -1, 0, H, 0)
    else:
        c = canvas.Canvas(path, pagesize=(W, H))
    f = Flat(c)
    f.draw()
    c.showPage()
    c.save()
    return f.expected


def real_form(path):
    c = canvas.Canvas(path, pagesize=(W, H))
    form = c.acroForm
    Y = lambda y: H - y
    c.setFont("Helvetica-Bold", 16)
    c.drawString(50, Y(60), "Real Form Sample")
    c.setFont("Helvetica", 9)

    def label(y, s):
        c.setFont("Helvetica", 9)
        c.drawString(50, Y(y), s)

    label(102, "Full name")
    form.textfield(name="fullname", tooltip="Full name", x=160, y=Y(108), width=380,
                   height=20, borderWidth=1, fontSize=0, fillColor=Color(0.93, 0.95, 1))
    label(132, "Email")
    form.textfield(name="contact.email", x=160, y=Y(138), width=380, height=20,
                   borderWidth=1, fontSize=10)
    label(162, "Postcode (comb)")
    form.textfield(name="postcode", tooltip="Postal code", x=160, y=Y(168), width=120,
                   height=20, borderWidth=1, maxlen=6, fieldFlags="comb", fontSize=0)
    label(192, "Subscribe")
    form.checkbox(name="subscribe", tooltip="Subscribe to newsletter", x=160, y=Y(196),
                  size=12, buttonStyle="check", borderWidth=1)
    label(222, "Plan")
    for i, v in enumerate(["basic", "plus", "pro"]):
        form.radio(name="plan", value=v, x=160 + i * 70, y=Y(226), size=12,
                   buttonStyle="circle", borderWidth=1, selected=False)
        c.drawString(176 + i * 70, Y(222), v)
    label(252, "Country")
    form.choice(name="country", tooltip="Country", x=160, y=Y(258), width=180, height=20,
                options=["Sweden", "Spain", "Norway", "Denmark"], value="Sweden",
                borderWidth=1, fontSize=10)
    label(282, "Comments")
    form.textfield(name="comments", x=160, y=Y(360), width=380, height=84,
                   borderWidth=1, fieldFlags="multiline", fontSize=0)
    c.showPage()
    c.save()


def scanned_form(flat_pdf, path):
    with tempfile.TemporaryDirectory() as td:
        subprocess.run(["pdftoppm", "-r", "150", "-gray", "-png", flat_pdf,
                        os.path.join(td, "p")], check=True)
        png = [f for f in os.listdir(td) if f.endswith(".png")][0]
        img = Image.open(os.path.join(td, png)).convert("L")
    img = img.rotate(-0.25, resample=Image.BICUBIC, fillcolor=255, expand=False)
    img = img.filter(ImageFilter.GaussianBlur(0.7))
    a = np.asarray(img).astype(np.float32)
    a += np.random.normal(0, 9, a.shape)
    # Scanner dust: a few isolated dark specks.
    for _ in range(400):
        y, x = np.random.randint(0, a.shape[0]), np.random.randint(0, a.shape[1])
        a[y, x] = 60
    a = np.clip(a * 0.97 + 6, 0, 255).astype(np.uint8)
    img = Image.fromarray(a, "L")
    buf = io.BytesIO()
    img.save(buf, "JPEG", quality=85)
    buf.seek(0)
    from reportlab.lib.utils import ImageReader
    c = canvas.Canvas(path, pagesize=(W, H))
    c.drawImage(ImageReader(buf), 0, 0, W, H)
    c.showPage()
    c.save()


def perf_text(path):
    font = "/usr/share/fonts/liberation/LiberationSerif-Regular.ttf"
    pdfmetrics.registerFont(TTFont("LibSerif", font))
    c = canvas.Canvas(path, pagesize=(W, H))
    words = (BODY + " " + BODY.upper()).split()
    c.setFont("LibSerif", 7)
    for col in range(2):
        x = 36 + col * 268
        y = H - 40
        while y > 36:
            ln, cur = "", 0
            while True:
                w = words[random.randrange(len(words))]
                t = (ln + " " + w).strip()
                if pdfmetrics.stringWidth(t, "LibSerif", 7) > 255:
                    break
                ln = t
            c.drawString(x, y, ln)
            y -= 8.2
    c.showPage()
    c.save()


def photo(w, h):
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)
    img = np.stack([
        128 + 100 * np.sin(xx / 37.0 + yy / 91.0),
        128 + 100 * np.sin(yy / 53.0),
        128 + 100 * np.cos((xx + yy) / 71.0),
    ], axis=-1)
    img += np.random.normal(0, 25, img.shape)
    return Image.fromarray(np.clip(img, 0, 255).astype(np.uint8), "RGB")


def perf_image(path):
    from reportlab.lib.utils import ImageReader
    c = canvas.Canvas(path, pagesize=(W, H))
    for (x, y, w, h, pw, ph) in [(36, 430, 523, 376, 2400, 1700), (36, 36, 250, 370, 1100, 1600),
                                 (309, 36, 250, 370, 1100, 1600)]:
        buf = io.BytesIO()
        photo(pw, ph).save(buf, "JPEG", quality=88)
        buf.seek(0)
        c.drawImage(ImageReader(buf), x, y, w, h)
    c.showPage()
    c.save()


def big(path, pages=300):
    c = canvas.Canvas(path, pagesize=(W, H))
    for p in range(pages):
        if p % 3 == 0:
            Flat(c).draw()
        else:
            c.setFont("Helvetica", 9)
            y = H - 50
            while y > 50:
                c.drawString(50, y, BODY[: 100 + (int(y) % 20)])
                y -= 12
        c.setFont("Helvetica", 8)
        c.drawString(W / 2 - 10, 20, f"{p + 1}")
        c.showPage()
    c.save()


def main():
    out = OUT
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if args:
        out = args[0]
    os.makedirs(out, exist_ok=True)
    p = lambda n: os.path.join(out, n)
    real_form(p("real_form.pdf"))
    expected = flat_form(p("flat_form.pdf"))
    with open(p("flat_form.json"), "w") as fh:
        json.dump({"page_size": [W, H], "fields": expected}, fh, indent=1)
    flat_form(p("flat_rotated.pdf"), rotated=True)
    scanned_form(p("flat_form.pdf"), p("scanned_form.pdf"))
    perf_text(p("perf_text.pdf"))
    perf_image(p("perf_image.pdf"))
    if "--big" in sys.argv:
        big(p("big_300.pdf"))
    print("wrote samples to", out)


if __name__ == "__main__":
    main()
