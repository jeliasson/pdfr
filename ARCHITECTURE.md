# Architecture

How pdfr is put together, and where new features go.

## Crates

| Crate | Owns | Never |
|---|---|---|
| `crates/core` (`pdfr-core`) | PDF reading (`forms`), saving (`save`, `pdf_font`), comments and markup (`markup`), field model (`field`), the one text layout used on screen *and* in saved files (`layout`), the caret editor (`edit`), field detection (`detect`), text extraction (`text`), redaction (`redact`: rewrites page content so text, pixels and drawings under boxes are gone), rendering wrapper (`render`) | UI, network, user data |
| `crates/assist` (`pdfr-assist`) | Labels, meanings, value memory and profiles, suggestions, the AI backend | UI, PDF writing |
| `crates/license` (`pdfr-license`) | Licence keys: offline Ed25519 verification, the key/state files, the `pdfr-license` tool (format: `docs/license-keys.md`) | UI, network |
| `crates/app` (`pdfr`) | The window, input, rendering threads, theming, dialogs | PDF structure details |

## The app crate

`app.rs` holds the state types (`App`, `Doc`, `Entry`, `Focus`, `Tool`,
`Confirm`) and the frame loop. Behaviour lives in child modules, each one
`impl App` block:

| Module | Concern |
|---|---|
| `app/commands.rs` | **Every user action** as a `Command`: label, icon, shortcuts, `can` (enabled), `is_on` (toggles), `run` |
| `app/toolbar.rs` | Icon toolbar built from `Command::TOOLBAR` |
| `icons.rs` | Vector icons (24×24 grid, theme coloured) |
| `app/files.rs` | Open (with unsaved-changes prompt), save, file dialogs parented to the window |
| `app/view.rs` | Page geometry, zoom, scroll, scrollbar, texture requests |
| `app/history.rs` | Undo/redo snapshots |
| `app/editing.rs` | Focused field: keys, typing, Tab order, ghost text, value writes |
| `app/canvas.rs` | Drawing pages and fields, pointer input, context menu, popups |
| `app/redact.rs` | The redact tool: marks (dragged, from the selection or search), the apply dialog, writing the redacted copy |
| `app/markup.rs` | Comments and markup (notes, highlights, text boxes, shapes): placing, selecting, moving, the comment card, the Comments tab; saved by `core/markup.rs` as standard annotations with appearances |
| `app/automation.rs` | Hidden `--demo/--save/--screenshot` flags for tests |
| `worker.rs` | Render/detect thread pool (newest request per page wins) |
| `assist_ui.rs` | Ghost text, suggestion list, consent and remember dialogs, settings |
| `license_ui.rs` | Registering: the reminder for unregistered copies, the register dialog, Settings → License, the daily online check (`license_ui/online.rs`) |
| `theme.rs` | Omarchy theme following |
| `system/` | Desktop integration: default PDF app, app menu registration, portals (colour scheme, links, which dialogs exist), install detection, macOS open events |
| `updates.rs` | Opt-in daily check for a newer version (notify only, pure-Rust TLS) |

### Adding a feature

1. Add a `Command` variant with its `spec()` (label, hint, icon, shortcuts),
   `can`, `run`, and (for tools/panels) `is_on`.
2. Add an `Icon` variant and draw it in `icons::paint`.
3. Put it in a `Command::TOOLBAR` group. Shortcuts work automatically and are
   ignored while a text box or dialog has the keyboard
   (`keyboard_elsewhere`).
4. Put the behaviour in the module for its concern, or a new `app/<name>.rs`.

### Rules that keep it correct

- All field value writes go through `Doc::set_value_at` (keeps widgets of one
  field in sync) inside an undo step (`snapshot`/`before_edit`).
- Fields found by detection are added to every undo state: undo never
  removes them.
- The assist analysis is only used while its field ids match the document's
  exactly; structural changes re-analyse.
- Keyboard input goes to the PDF field only when no other widget or dialog
  has the keyboard.

## Privacy boundary (assist)

- Sent to the AI: field labels (`labels::plausible_label`) and section headings
  (`profiles::heading_like`, minus any heading containing a stored value), with
  row numbers. Built only in `ai::build_prompt`,
  `profiles::build_people_prompt` and `check::explain_prompt` ("Explain
  this form": headings and labels, minus anything containing one of the
  form's answers).
- The checks before saving (`check::check`) are local: nothing is sent.
- Never stored: password fields, secret labels (`is_secret_label`), card
  numbers including across rows of boxes (`never_store`), fields marked
  "don't remember", values deselected in the save dialog.
- AI replies must answer every line exactly once, or are discarded.

## Next structural steps (for Acrobat-class features)

These are known limits, in rough order of need:

1. **Edit-based history.** Replace whole-snapshot undo with `Edit` diffs
   (`SetValue`, `AddField`, `RemoveField`, `Batch`), so annotations and page
   moves can be undone the same way.
2. **Document model.** A `Document` with pages, a field tree (parent/kids),
   and annotations as typed collections, mutated only via `apply(Edit)`;
   save becomes per-kind writers instead of one large match.
3. **Incremental save.** Append an update instead of rewriting the file, so
   existing digital signatures stay valid (a prerequisite for signing).
4. **Tabs.** `Tab { doc, view, history, focus, assist state }` with
   `App { tabs, active }`; the worker keys requests by tab.
5. **Tool trait.** `trait Tool { pointer, paint, cursor }` with Select/Fill,
   AddText, AddCheckbox, then Highlight, Note, Ink, Redact.
6. **Modal queue.** One `enum Modal` shown at a time instead of separate flags.
7. **Store.** Versioned schema with locking (or a small embedded database)
   instead of one JSON file.
