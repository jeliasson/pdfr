"""A made-up, realistic paper form for README screenshots: a household
benefit application from the fictional Municipality of Northvale, printed,
filled by nobody, then "scanned" (slightly skewed, warm paper, blur, noise,
JPEG). Also writes the values to type into it, by position, for
`pdfr --values` (made-up people only).

    uv run --with reportlab --with pillow --with numpy python tools/gen_media_form.py

Writes samples/household_form.pdf (clean), samples/household_scanned.pdf and
samples/household_values.json.
"""

import io
import json
import math
import os
import random

import numpy as np
from PIL import Image, ImageFilter
from reportlab.lib.pagesizes import A4
from reportlab.lib.utils import ImageReader
from reportlab.pdfgen import canvas

W, H = A4
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "samples")
INK = (0.12, 0.14, 0.2)
ACCENT = (0.16, 0.3, 0.5)

values = []  # (x, y top-down, value)


class Form:
    def __init__(self, c):
        self.c = c

    def Y(self, y):
        return H - y

    def text(self, x, y, s, size=8.5, font="Helvetica", color=INK):
        self.c.setFillColorRGB(*color)
        self.c.setFont(font, size)
        self.c.drawString(x, self.Y(y), s)

    def rect(self, x, y, w, h, lw=0.7, fill=None, stroke=INK):
        c = self.c
        c.setLineWidth(lw)
        c.setStrokeColorRGB(*stroke)
        if fill:
            c.setFillColorRGB(*fill)
        c.rect(x, self.Y(y + h), w, h, stroke=1, fill=1 if fill else 0)

    def line(self, x0, y0, x1, y1, lw=0.6, dash=None):
        c = self.c
        c.setLineWidth(lw)
        c.setStrokeColorRGB(*INK)
        c.setDash(*(dash or ([], 0)))
        c.line(x0, self.Y(y0), x1, self.Y(y1))
        c.setDash([], 0)

    def field(self, x, y, w, h, label, value=None):
        """A box with a small label at its top-left corner inside."""
        self.rect(x, y, w, h)
        self.text(x + 3, y + 8, label, size=6.2, color=(0.3, 0.32, 0.38))
        if value is not None:
            values.append((x + w / 2, y + h * 0.62, value))

    def comb(self, x, y, n, label, value=None, cell=14, h=18, groups=None):
        self.text(x, y - 3, label, size=7)
        cx = x
        for i in range(n):
            if groups and i in groups:
                cx += 6
            self.rect(cx, y, cell, h, lw=0.6)
            cx += cell
        if value is not None:
            values.append((x + cell / 2, y + h / 2, value))
        return cx

    def date(self, x, y, label, value=None):
        """DD MM YYYY as three separate groups of squares."""
        self.text(x, y - 3, label, size=7)
        cx = x
        for n, hint in ((2, "DD"), (2, "MM"), (4, "YYYY")):
            for i in range(n):
                self.rect(cx, y, 13, 17, lw=0.6)
                cx += 13
            self.text(cx - n * 13, y + 25, hint, size=5, color=(0.45, 0.45, 0.5))
            cx += 8
        if value is not None:
            values.append((x + 6.5, y + 8.5, value))

    def checkbox(self, x, y, label, checked=False):
        self.rect(x, y, 9, 9, lw=0.7)
        self.text(x + 14, y + 7.5, label, size=8)
        if checked:
            values.append((x + 4.5, y + 4.5, True))

    def section(self, y, n, title):
        self.rect(36, y, W - 72, 16, lw=0, fill=(0.88, 0.9, 0.93), stroke=(0.88, 0.9, 0.93))
        self.text(42, y + 11.5, f"{n}", size=9, font="Helvetica-Bold", color=ACCENT)
        self.text(56, y + 11.5, title.upper(), size=8.5, font="Helvetica-Bold")


def crest(f, x, y):
    """A made-up municipal crest: a shield with a tower and two waves."""
    c = f.c
    c.setFillColorRGB(*ACCENT)
    c.setStrokeColorRGB(*ACCENT)
    p = c.beginPath()
    p.moveTo(x, f.Y(y))
    p.lineTo(x + 34, f.Y(y))
    p.lineTo(x + 34, f.Y(y + 22))
    p.curveTo(x + 34, f.Y(y + 34), x + 20, f.Y(y + 40), x + 17, f.Y(y + 42))
    p.curveTo(x + 14, f.Y(y + 40), x, f.Y(y + 34), x, f.Y(y + 22))
    p.close()
    c.drawPath(p, fill=1, stroke=0)
    c.setFillColorRGB(1, 1, 1)
    c.rect(x + 12, f.Y(y + 26), 10, 16, fill=1, stroke=0)
    for i in range(3):
        c.rect(x + 11 + i * 4.5, f.Y(y + 10), 3, 3, fill=1, stroke=0)
    c.setStrokeColorRGB(1, 1, 1)
    c.setLineWidth(1.4)
    for wy in (30, 35):
        p = c.beginPath()
        p.moveTo(x + 6, f.Y(y + wy))
        for k in range(4):
            p.curveTo(x + 8 + k * 6, f.Y(y + wy - 2), x + 10 + k * 6, f.Y(y + wy + 2), x + 12 + k * 6, f.Y(y + wy))
        c.drawPath(p, fill=0, stroke=1)


def draw(c):
    f = Form(c)
    crest(f, 36, 34)
    f.text(80, 48, "Municipality of Northvale", size=13, font="Helvetica-Bold")
    f.text(80, 62, "Social Services Department  ·  Box 120, 413 01 Northvale", size=7.5, color=(0.35, 0.37, 0.42))
    f.text(W - 36 - 150, 46, "APPLICATION FOR", size=7.5, color=(0.35, 0.37, 0.42))
    f.text(W - 36 - 150, 60, "Household benefit", size=12, font="Helvetica-Bold")
    f.rect(W - 36 - 150, 70, 150, 14, lw=0.5)
    f.text(W - 36 - 147, 80, "Case no. (office use)", size=5.8, color=(0.4, 0.4, 0.45))
    f.line(36, 92, W - 36, 92, lw=1.2)
    f.text(36, 106, "Fill in the form in block capitals. Read the guidance notes before you start. Fields marked * are required.", size=7.5)

    # 1 Applicant
    y = 116
    f.section(y, 1, "Applicant")
    y += 24
    f.field(36, y, 300, 28, "Full name *", "Anna Lindqvist")
    f.date(350, y + 9, "Date of birth *", "12031985")
    y += 44
    f.comb(36, y, 12, "Personal identity number *", "198503124328", groups={8})
    f.field(260, y - 9, W - 36 - 260, 28, "Nationality", "Swedish")
    y += 30
    f.field(36, y, 330, 28, "Street address *", "Storgatan 12")
    f.field(370, y, W - 36 - 370, 28, "Apartment no.", "1102")
    y += 40
    f.comb(36, y, 5, "Postcode *", "41301")
    f.field(130, y - 9, 200, 28, "Town / city *", "Northvale")
    f.field(336, y - 9, W - 36 - 336, 28, "Telephone", "070-123 45 67")
    y += 30
    f.field(36, y, 300, 28, "E-mail", "anna@example.se")
    f.text(350, y + 10, "Marital status", size=7)
    f.checkbox(350, y + 15, "Single")
    f.checkbox(405, y + 15, "Married / partner", checked=True)
    f.checkbox(495, y + 15, "Other")

    # 2 Household
    y += 42
    f.section(y, 2, "Others living in the household")
    y += 22
    cols = [(36, "Name"), (226, "Personal identity number"), (356, "Relation to applicant"), (466, "Date of birth")]
    f.rect(36, y, W - 72, 14, lw=0.6, fill=(0.95, 0.96, 0.97))
    for x, t in cols:
        f.text(x + 3, y + 10, t, size=6.8, font="Helvetica-Bold")
    rows = [
        ("Erik Lindqvist", "20140602-1473", "Son", "02.06.2014"),
        ("Maja Lindqvist", "20170915-2286", "Daughter", "15.09.2017"),
        (None, None, None, None),
    ]
    y += 14
    for r in rows:
        f.rect(36, y, W - 72, 20, lw=0.6)
        for (x, _), v in zip(cols, r):
            if x > 36:
                f.line(x, y, x, y + 20)
            if v:
                values.append((x + 40, y + 10, v))
        y += 20

    # 3 Income
    y += 14
    f.section(y, 3, "Household income per month")
    y += 26
    f.checkbox(36, y, "Salary / wages", checked=True)
    f.checkbox(160, y, "Pension")
    f.checkbox(250, y, "Student finance")
    f.checkbox(36, y + 18, "Unemployment benefit")
    f.checkbox(160, y + 18, "Parental benefit", checked=True)
    f.checkbox(250, y + 18, "Other:")
    f.line(298, y + 26, 360, y + 26, dash=([1, 1.6], 0))
    f.field(380, y - 4, W - 36 - 380, 28, "Total before tax (SEK) *", "32 400")
    y += 40
    f.field(36, y, W - 72, 46, "Other information about your situation", "Parental leave with Maja until 31 January 2027; back to full-time work in February.")

    # 4 Payment
    y += 60
    f.section(y, 4, "Payment")
    y += 30
    f.comb(36, y, 24, "IBAN (account for payment) *", "SE4550000000058398257466", cell=13, groups={4, 8, 12, 16, 20})
    f.field(400, y - 9, W - 36 - 400, 28, "Bank", "Northvale Savings Bank")

    # 5 Declaration
    y += 36
    f.section(y, 5, "Declaration")
    y += 22
    f.checkbox(36, y, "I confirm that the information I have given is true and complete,", checked=True)
    f.text(50, y + 18, "and I will tell the Social Services Department about any changes.", size=8)
    y += 40
    f.text(36, y + 4, "Place", size=8)
    f.line(64, y + 6, 220, y + 6, dash=([1, 1.6], 0))
    values.append((140, y + 1, "Northvale"))
    f.date(240, y - 10, "Date", "16112026")
    f.line(400, y + 6, W - 36, y + 6)
    f.text(400, y + 16, "Signature of applicant", size=6.5, color=(0.35, 0.37, 0.42))

    # 6 Attachments
    y += 34
    f.section(y, 6, "Attachments")
    y += 24
    f.checkbox(36, y, "Payslips for the last three months", checked=True)
    f.checkbox(250, y, "Rental agreement", checked=True)
    f.checkbox(36, y + 18, "Decision on parental benefit", checked=True)
    f.checkbox(250, y + 18, "Other:")
    f.line(298, y + 26, W - 36, y + 26, dash=([1, 1.6], 0))

    f.line(36, H - 46, W - 36, H - 46, lw=0.5)
    f.text(36, H - 34, "Form SS-114 (2026)  ·  This is a fictional form made for pdfr's screenshots.", size=6.5, color=(0.4, 0.4, 0.45))
    f.text(W - 70, H - 34, "Page 1 of 1", size=6.5, color=(0.4, 0.4, 0.45))


def scan(clean_pdf, path):
    """Print-and-scan look: 200 dpi, skew, warm paper, uneven light, blur, noise."""
    import subprocess
    import tempfile

    rng = np.random.default_rng(7)
    with tempfile.TemporaryDirectory() as td:
        subprocess.run(["pdftoppm", "-r", "200", "-png", clean_pdf, os.path.join(td, "p")], check=True)
        png = [x for x in os.listdir(td) if x.endswith(".png")][0]
        img = Image.open(os.path.join(td, png)).convert("RGB")
    img = img.rotate(-0.35, resample=Image.BICUBIC, fillcolor=(255, 255, 255), translate=(6, 4))
    img = img.filter(ImageFilter.GaussianBlur(0.75))
    a = np.asarray(img).astype(np.float32) / 255.0
    h, w, _ = a.shape
    yy, xx = np.mgrid[0:h, 0:w]
    # Uneven scanner light: darker towards one edge, a soft shadow at the binding.
    light = 0.985 - 0.05 * (xx / w) ** 2 - 0.035 * (yy / h)
    light -= 0.10 * np.exp(-((xx - 0) / (w * 0.025)) ** 2)
    paper = np.array([0.985, 0.965, 0.92])
    a = a * paper * light[..., None]
    a += rng.normal(0, 0.022, a.shape)
    for _ in range(600):
        y, x = rng.integers(0, h), rng.integers(0, w)
        a[y : y + 2, x : x + 2] *= 0.45
    a = np.clip(a, 0, 1)
    img = Image.fromarray((a * 255).astype(np.uint8), "RGB")
    buf = io.BytesIO()
    img.save(buf, "JPEG", quality=72)
    buf.seek(0)
    c = canvas.Canvas(path, pagesize=(W, H))
    c.drawImage(ImageReader(buf), 0, 0, W, H)
    c.showPage()
    c.save()


def main():
    random.seed(7)
    clean = os.path.join(OUT, "household_form.pdf")
    c = canvas.Canvas(clean, pagesize=A4)
    c.setTitle("Household benefit application (fictional)")
    draw(c)
    c.showPage()
    c.save()
    scan(clean, os.path.join(OUT, "household_scanned.pdf"))
    with open(os.path.join(OUT, "household_values.json"), "w") as fh:
        json.dump(
            [{"page": 0, "x": round(x, 1), "y": round(y, 1), "value": v} for x, y, v in values],
            fh,
            indent=1,
            ensure_ascii=False,
        )
    print(f"{len(values)} values")


if __name__ == "__main__":
    main()
