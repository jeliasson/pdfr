# pdfr

**Fill in PDF forms the way they were meant to be filled in.**

pdfr is a fast, friendly PDF viewer for filling in forms: tax forms, school
applications, permits, anything with boxes to fill. Click a box, type, and
your text lands neatly inside it at the right size. That's it.

Free for personal use. Made for [Omarchy](https://omarchy.org) first, and it
works on Linux, macOS and Windows too.

---

## Why pdfr?

### Forms that actually work

You know the struggle: a form that's just lines and boxes, so you end up
dragging text boxes around, guessing the font size, or tapping the space bar
to line letters up with little squares.

pdfr fixes that.

- **Click and type.** Your text appears exactly where you clicked, inside the
  box, at a size that fits.
- **One letter per square.** Rows of little boxes (dates, ID numbers, postcodes)
  get one character per box, automatically. When a row is full, typing carries
  on in the next one.
- **Every kind of form.** It works with PDFs that have proper fill-in fields,
  with forms that are only drawn on the page, and even with scanned paper forms.
  pdfr finds the boxes and lines for you.
- **What you see is what you get.** The saved and printed form looks just like
  the screen, and other PDF apps can still open and edit it.

### Fast

pdfr opens straight away. The first page is on screen in about a tenth of a
second, even for a 300-page document, and scrolling stays smooth with no blank
pages.

### Works everywhere

pdfr was made for Omarchy first and fits right in: it follows your Omarchy
theme and even switches live when you change it. It also runs on any Linux
desktop, on macOS and on Windows, in light or dark mode.

### Remembers you, privately

pdfr can remember the details you type (names, addresses, ID numbers, e-mail)
and offer them on the next form, in any language.

- **Each person stays separate.** Your details, your partner's and your
  kids' are kept apart, so a form about your child gets your child's details,
  not yours.
- **You decide.** After you save, pdfr shows what it would like to remember,
  and you pick. Or tell it to always or never remember.
- **It stays on your computer.** Nothing is uploaded. Passwords and card
  numbers are never stored.

---

## How it compares

pdfr does one thing, filling in forms, and tries to do it better than anyone.
The big PDF editors do much more. Here's an honest side-by-side.

|  | **pdfr** | Adobe Acrobat | Foxit PDF Editor | PDF-XChange Editor |
|---|:---:|:---:|:---:|:---:|
| **Price** | Free for personal use | Free Reader; Pro about $20/month | About $11/month, or one-time | Free version; paid one-time licence |
| **Linux** | ✅ Made for Omarchy | ❌ | ❌ | ❌ |
| **macOS** | ✅ | ✅ | ✅ | ❌ |
| **Windows** | ✅ | ✅ | ✅ | ✅ |
| **Fill in fillable forms** | ✅ | ✅ | ✅ | ✅ |
| **Finds the boxes on forms that aren't fillable** | ✅ When you open the file | ◐ "Prepare Form" step, paid | ◐ "Form Field Recognition" step | ◐ "Identify Forms", paid Plus edition |
| **Works on scanned paper forms** | ✅ | ◐ | ◐ | ◐ |
| **One letter per square in rows of boxes** | ✅ Automatic | ◐ Set up per field | ◐ Set up per field | ◐ Set up per field |
| **Remembers each person's details separately** (you, partner, kids) | ✅ | ❌ | ❌ | ❌ |
| **Print** | ◐ Linux for now | ✅ | ✅ | ✅ |
| **Edit text and images in a PDF** | Not yet | ✅ | ✅ | ✅ |
| **Recognise text in scans (OCR)** | Not yet | ✅ Paid | ✅ | ✅ |
| **Comments and highlights** | Not yet | ✅ | ✅ | ✅ |
| **Digital signatures** | Not yet | ✅ | ✅ | ✅ |
| **Merge, split and reorder pages** | Not yet | ✅ Paid | ✅ | ✅ |

✅ yes · ◐ partly, with an extra step or a paid edition · ❌ no

*Based on each company's public information and reviews, November 2026.
Prices are approximate and in US dollars. Spotted a mistake?
[Let us know](https://github.com/OWNER/pdfr/issues).*

---

## Install

**Omarchy and Arch Linux**

```sh
yay -S pdfr
```

**macOS and Linux (Homebrew)**

```sh
brew tap OWNER/tap https://github.com/OWNER/homebrew-tap
brew install OWNER/tap/pdfr
```

**Debian and Ubuntu**

Download the `.deb` file from the
[releases page](https://github.com/OWNER/pdfr/releases), then:

```sh
sudo apt install ./pdfr_*_amd64.deb
```

**Windows and others**

Download pdfr for your system from the
[releases page](https://github.com/OWNER/pdfr/releases).

---

## Filling in a form

1. **Open a PDF:** drag it onto the window, press **Ctrl+O**, or run
   `pdfr form.pdf`.
2. **Click a box and type.** Press **Tab** to jump to the next box.
3. **Save:** press **Ctrl+S**. pdfr never overwrites your original file without
   asking. The first time, it suggests a new name like `form-filled.pdf`.
4. **Print:** press **Ctrl+P**.

### Handy tricks

- **Suggestions:** in an empty box, pdfr may show a grey suggestion. Press
  **Tab** to take it. Press **↑** or **↓** to look through other suggestions.
- **Hover buttons:** hover over a box to get a small button: **✕** clears it,
  **✨** shows suggestions.
- **Missed a box?** Use the toolbar to add a text box (drag) or a checkbox
  (click) yourself. Right-click a box pdfr added by mistake to remove it.
- **Undo anything** with **Ctrl+Z**.

### Keyboard shortcuts

| What | Keys |
|---|---|
| Open, save, save as, print | Ctrl+O, Ctrl+S, Ctrl+Shift+S, Ctrl+P |
| Next / previous box | Tab / Shift+Tab |
| Tick a checkbox | Space or click |
| Take the suggestion | Tab |
| Browse suggestions | ↑ / ↓ |
| Leave a box | Esc |
| Undo / redo | Ctrl+Z / Ctrl+Shift+Z |
| Zoom in / out / 100% | Ctrl+= / Ctrl+− / Ctrl+0 (or Ctrl+scroll) |

---

## Your data stays yours

- Everything pdfr remembers is kept in one file on your computer that only you
  can read. Look through it, or delete people and values, any time in
  **Settings**.
- **Optional AI helper.** If you have
  [Claude Code](https://claude.com/claude-code) installed, pdfr can ask it what
  an unusual box label means, or which boxes belong to which person on a
  family form. It only ever sends the printed labels and headings, **never what
  you type**. pdfr asks before using it the first time, and you can switch it
  off in Settings. Without it, everything else works just the same.

---

## Not there yet

- Suggestions on scanned forms only work for forms you've filled in before
  (pdfr doesn't read scanned text yet).
- Password-protected PDFs, signature fields and some unusual form types.
- Alphabets beyond Latin, Greek and Cyrillic (for example Chinese or Arabic).
- Printing on macOS and Windows is coming. Until then, save and print from
  another app.

Found a bug or a form that doesn't work?
[Open an issue](https://github.com/OWNER/pdfr/issues). A sample PDF helps a lot
(a blank form, never one with your personal details).

---

## Free for personal use

pdfr is **free for personal and non-commercial use**, and for schools,
charities and public institutions.

The source code is public: you're welcome to read it, learn from it, and
[contribute](CONTRIBUTING.md). It isn't "open source" in the strict sense,
though: you may not copy, redistribute, sell, or publish changed versions of
pdfr.

Want to use pdfr in your business? Paid licences are planned. Get in touch
through the [project page](https://github.com/OWNER/pdfr).

The full terms are in [LICENSE.md](LICENSE.md) (PolyForm Strict 1.0.0).

---

## Thanks

pdfr types your text in [Noto Sans](https://notofonts.github.io/) (SIL Open
Font License) and is built on many excellent open-source Rust libraries, each
under its own licence.
