---
name: pdfr
description: >
  Fill in, read, check, sign and save PDF forms with pdfr, the desktop PDF app,
  through its MCP tools. Use when the user asks to fill in or complete a PDF form
  (real form fields, flat forms or scans), to read what a PDF form asks for, to
  check a filled-in form, or to open, print or save a PDF in pdfr. Triggers:
  pdfr, PDF form, fill in this form, fill out, application form, tax form, tick
  the box, sign this PDF.
---

<!-- pdfr-skill v2 (pdfr updates this file; delete this line to keep your own changes) -->

# Using pdfr

pdfr is a desktop app for viewing PDFs and filling in forms. Its tools (the MCP
server named `pdfr`) drive a real pdfr window on the user's screen. The user
watches the pointer move and can take over at any time.

## Filling in a form

Work with the form tools. They are faster and more reliable than clicking, and
they work on real form fields, flat forms and scans alike.

1. `open` with the PDF's absolute path. It starts pdfr if needed and waits until
   the form's boxes have been found.
2. `read_form` lists every box in reading order: its `ref` (like `f12`), page,
   kind, label and current value. On a long form, narrow it with `page`,
   `only_empty` or `filter`.
3. Fill each box by its `ref`:
   - `fill` for text. It replaces what is there. Rows of squares (kind `boxes`)
     take one character each, and a long value carries on into the next row.
     The answer says if something did not fit.
   - `set_checked` for checkboxes and radio buttons.
   - `choose` for drop-downs, by the option's text or value.
4. `check_form` lists answers that look wrong (an impossible date, a bad IBAN)
   and boxes still empty. Fix what it reports, or tell the user.
5. `save_as` with an absolute path. Save to a new file unless the user asked to
   replace the original.

Do not guess what the user would write. If a box asks for something you were
not given, call `suggest` with its `ref`: it returns what pdfr remembers the
user usually writes there, best first. If that is empty, ask the user.

## Reading a PDF

- `read_text` gives the text printed on a page; `find` searches the document.
- A label in `read_form` may be short. Read the page's text when you need the
  instructions around a box.

## Seeing and using the window

Use these when the form tools cannot do it: a dialog, a toolbar button, drawing
a signature, moving or resizing something on the page.

- `screenshot` shows the window as the user sees it.
- `read_ui` lists the buttons, labels and text boxes of the window and any open
  dialog, each with its position (`at`) ready to click.
- `computer` moves the pointer and types: `click`, `double_click`, `move`,
  `drag`, `scroll`, `type`, `key`. Its `x` and `y` are pixels of the latest
  screenshot. `read_form` gives the same kind of position for each visible box.
  Take a screenshot afterwards to see what happened.
- `view` goes to a page or a box, or changes the zoom. `run_command` runs one
  of pdfr's own commands by name (Undo, Redo, FitWidth, NextEmpty, …); with no
  name it lists the ones that can run now.

## Rules

- Pages are numbered from 1.
- Every answer ends with the window's `state`: the file, the page, whether
  there are unsaved changes, and any dialog that is open. Read it before the
  next step.
- Some actions ask the user first, in the window: signing digitally and saving
  over an existing file by default, printing if the user chose so. The call
  waits for their answer. `declined` means they said no: do not retry, and do
  not look for another way to do the same thing.
- `paused_by_user` means the user took over the window. Stop and ask them
  before going on.
- `no_window` means no pdfr window is open: call `open`. `busy` means pdfr is
  in the middle of something: `wait_idle`, then try again.
- `needs_confirmation` on `open` or `close` means the document has unsaved
  changes. Save it first. Pass `discard` only if the user said those changes
  can go; pdfr then asks them in the window too.
- pdfr's Settings, its licence dialog and its question about using AI are
  the user's. While they are open, clicks, keys and screenshots are refused
  with `busy`: ask the user to close them.
- `print` opens the print dialog for the user to finish.
