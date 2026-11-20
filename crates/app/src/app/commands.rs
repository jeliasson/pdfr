//! Every user action as a command: one place that defines its name, icon,
//! keyboard shortcut, when it is available and what it does. The toolbar,
//! the keyboard shortcuts, the command palette and the F1 cheatsheet are all
//! built from this list, so a new feature is added here once and shows up
//! everywhere consistently.
//!
//! Keyboard-first: every new shortcut is a Ctrl chord (vim/zathura letters
//! where they fit: J/K scroll, D/U half pages, N next empty box), so nothing
//! typed into a box is ever taken as a command. No punctuation chords: they
//! move around on non-US layouts.

use super::{App, PAD, Tool};
use crate::icons::Icon;
use eframe::egui::{Key, KeyboardShortcut, Modifiers, pos2};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    Open,
    OpenNewWindow,
    Recent,
    Save,
    SaveAs,
    Print,
    Undo,
    Redo,
    ZoomOut,
    ZoomIn,
    ActualSize,
    FitWidth,
    FitPage,
    FieldMarks,
    TwoPages,
    Present,
    DarkPages,
    RotatePage,
    RotatePageBack,
    RotateAll,
    Sidebar,
    AddTextField,
    AddCheckbox,
    Sign,
    Initials,
    SignDigitally,
    TodaysDate,
    Settings,
    Register,
    ScrollDown,
    ScrollUp,
    HalfPageDown,
    HalfPageUp,
    NextPage,
    PrevPage,
    GoToPage,
    GoBack,
    NextEmpty,
    PrevEmpty,
    JumpHints,
    Copy,
    Find,
    NextMatch,
    PrevMatch,
    FindBox,
    ClearField,
    RemoveField,
    ToggleRemember,
    Palette,
    Cheatsheet,
}

/// How a command presents itself.
pub struct Spec {
    pub label: &'static str,
    /// Longer explanation for tooltips; empty if the label says it all.
    pub hint: &'static str,
    pub icon: Icon,
    pub shortcuts: &'static [KeyboardShortcut],
}

const fn ctrl(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::COMMAND, key)
}

const fn ctrl_shift(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), key)
}

const OPEN: &[KeyboardShortcut] = &[ctrl(Key::O)];
const RECENT: &[KeyboardShortcut] = &[ctrl(Key::E)];
const OPEN_NEW_WINDOW: &[KeyboardShortcut] = &[ctrl_shift(Key::O)];
const SAVE: &[KeyboardShortcut] = &[ctrl(Key::S)];
const SAVE_AS: &[KeyboardShortcut] = &[ctrl_shift(Key::S)];
const PRINT: &[KeyboardShortcut] = &[ctrl(Key::P)];
const UNDO: &[KeyboardShortcut] = &[ctrl(Key::Z)];
const REDO: &[KeyboardShortcut] = &[ctrl_shift(Key::Z), ctrl(Key::Y)];
const ZOOM_OUT: &[KeyboardShortcut] = &[ctrl(Key::Minus)];
const ZOOM_IN: &[KeyboardShortcut] = &[ctrl(Key::Plus), ctrl(Key::Equals)];
const ACTUAL_SIZE: &[KeyboardShortcut] = &[ctrl(Key::Num0), ctrl(Key::Num1)];
const FIT_WIDTH: &[KeyboardShortcut] = &[ctrl(Key::Num2)];
const SIDEBAR: &[KeyboardShortcut] = &[ctrl(Key::B)];
const FIELD_MARKS: &[KeyboardShortcut] = &[ctrl(Key::H)];
const FIT_PAGE: &[KeyboardShortcut] = &[ctrl(Key::Num3)];
const DARK_PAGES: &[KeyboardShortcut] = &[ctrl(Key::M)];
const ROTATE: &[KeyboardShortcut] = &[ctrl(Key::R)];
const ROTATE_ALL: &[KeyboardShortcut] = &[ctrl_shift(Key::R)];
const SETTINGS: &[KeyboardShortcut] = &[ctrl(Key::Comma)];
const SCROLL_DOWN: &[KeyboardShortcut] = &[ctrl(Key::J)];
const SCROLL_UP: &[KeyboardShortcut] = &[ctrl(Key::K)];
const HALF_DOWN: &[KeyboardShortcut] = &[ctrl(Key::D)];
const HALF_UP: &[KeyboardShortcut] = &[ctrl(Key::U)];
const NEXT_PAGE: &[KeyboardShortcut] = &[ctrl_shift(Key::J)];
const PREV_PAGE: &[KeyboardShortcut] = &[ctrl_shift(Key::K)];
const GO_TO_PAGE: &[KeyboardShortcut] = &[ctrl(Key::G)];
const GO_BACK: &[KeyboardShortcut] = &[ctrl_shift(Key::G)];
const NEXT_EMPTY: &[KeyboardShortcut] = &[ctrl(Key::N)];
const PREV_EMPTY: &[KeyboardShortcut] = &[ctrl_shift(Key::N)];
const JUMP_HINTS: &[KeyboardShortcut] = &[ctrl(Key::L)];
const FIND: &[KeyboardShortcut] = &[ctrl(Key::F)];
const COPY: &[KeyboardShortcut] = &[ctrl(Key::C)];
const FIND_BOX: &[KeyboardShortcut] = &[ctrl_shift(Key::F)];
const SIGN: &[KeyboardShortcut] = &[ctrl(Key::I)];
const INITIALS: &[KeyboardShortcut] = &[ctrl_shift(Key::I)];
const SIGN_DIGITALLY: &[KeyboardShortcut] = &[ctrl_shift(Key::C)];
const TODAY: &[KeyboardShortcut] = &[ctrl_shift(Key::D)];
const REMOVE_FIELD: &[KeyboardShortcut] = &[ctrl_shift(Key::Delete)];
const PALETTE: &[KeyboardShortcut] = &[ctrl_shift(Key::P), ctrl(Key::Space)];
const CHEATSHEET: &[KeyboardShortcut] = &[KeyboardShortcut::new(Modifiers::NONE, Key::F1)];
const NONE: &[KeyboardShortcut] = &[];

impl Command {
    pub const ALL: &'static [Command] = &[
        Command::Open,
        Command::OpenNewWindow,
        Command::Recent,
        Command::Save,
        Command::SaveAs,
        Command::Print,
        Command::Undo,
        Command::Redo,
        Command::ZoomOut,
        Command::ZoomIn,
        Command::ActualSize,
        Command::FitWidth,
        Command::FitPage,
        Command::FieldMarks,
        Command::TwoPages,
        Command::Present,
        Command::DarkPages,
        Command::RotatePage,
        Command::RotatePageBack,
        Command::RotateAll,
        Command::Sidebar,
        Command::AddTextField,
        Command::AddCheckbox,
        Command::Sign,
        Command::Initials,
        Command::SignDigitally,
        Command::TodaysDate,
        Command::Settings,
        Command::Register,
        Command::ScrollDown,
        Command::ScrollUp,
        Command::HalfPageDown,
        Command::HalfPageUp,
        Command::NextPage,
        Command::PrevPage,
        Command::GoToPage,
        Command::GoBack,
        Command::NextEmpty,
        Command::PrevEmpty,
        Command::JumpHints,
        Command::Copy,
        Command::Find,
        Command::NextMatch,
        Command::PrevMatch,
        Command::FindBox,
        Command::ClearField,
        Command::RemoveField,
        Command::ToggleRemember,
        Command::Palette,
        Command::Cheatsheet,
    ];

    pub fn spec(self) -> Spec {
        use Command::*;
        let (label, hint, icon, shortcuts) = match self {
            Open => ("Open", "Open a PDF", Icon::Open, OPEN),
            OpenNewWindow => (
                "Open in new window",
                "Open another PDF next to this one, in its own window",
                Icon::Open,
                OPEN_NEW_WINDOW,
            ),
            Recent => (
                "Recent files…",
                "Open a file you had open before, where you left it",
                Icon::Open,
                RECENT,
            ),
            Save => (
                "Save",
                "Save the filled form (the original is never overwritten silently)",
                Icon::Save,
                SAVE,
            ),
            SaveAs => ("Save as", "Save under a new name", Icon::SaveAs, SAVE_AS),
            Print => (
                "Print",
                if super::print::AVAILABLE {
                    "Print the filled form"
                } else {
                    "Printing is not available on this system yet"
                },
                Icon::Print,
                PRINT,
            ),
            Undo => ("Undo", "", Icon::Undo, UNDO),
            Redo => ("Redo", "", Icon::Redo, REDO),
            ZoomOut => ("Zoom out", "", Icon::ZoomOut, ZOOM_OUT),
            ZoomIn => ("Zoom in", "", Icon::ZoomIn, ZOOM_IN),
            ActualSize => ("Actual size", "Zoom to 100%", Icon::ActualSize, ACTUAL_SIZE),
            FitWidth => (
                "Fit width",
                "Zoom so the page fills the window's width",
                Icon::FitWidth,
                FIT_WIDTH,
            ),
            FitPage => (
                "Fit page",
                "Zoom so the whole page fits in the window",
                Icon::FitPage,
                FIT_PAGE,
            ),
            FieldMarks => (
                "Show fields",
                "Tint the boxes you can fill in. Off: see the form as other PDF apps show it",
                Icon::Highlight,
                FIELD_MARKS,
            ),
            TwoPages => (
                "Two pages side by side",
                "Show pages in pairs, like an open book",
                Icon::FitWidth,
                NONE,
            ),
            Present => (
                "Present",
                "One page at a time, full screen. Arrow keys turn pages; Esc ends",
                Icon::FitPage,
                NONE,
            ),
            DarkPages => (
                "Dark pages",
                "Show pages light on dark (screen only; saving and printing are unchanged)",
                Icon::Moon,
                DARK_PAGES,
            ),
            RotatePage => (
                "Turn page clockwise",
                "Turn this page a quarter turn (saved turned too)",
                Icon::Rotate,
                ROTATE,
            ),
            RotatePageBack => (
                "Turn page anticlockwise",
                "Turn this page a quarter turn the other way",
                Icon::Rotate,
                NONE,
            ),
            RotateAll => (
                "Turn all pages clockwise",
                "Turn every page a quarter turn",
                Icon::Rotate,
                ROTATE_ALL,
            ),
            Sidebar => (
                "Page sidebar",
                "Thumbnails of every page, and the document's outline",
                Icon::Sidebar,
                SIDEBAR,
            ),
            AddTextField => (
                "Add text field",
                "Drag a box on the page where text should go",
                Icon::TextField,
                NONE,
            ),
            AddCheckbox => (
                "Add checkbox",
                "Click on the page where a box should be ticked",
                Icon::Checkbox,
                NONE,
            ),
            Sign => (
                "Sign",
                "Draw or type your signature, then click where it goes",
                Icon::Signature,
                SIGN,
            ),
            SignDigitally => (
                "Sign with certificate",
                "Sign digitally with your certificate, ID card or .p12 file",
                Icon::Key,
                SIGN_DIGITALLY,
            ),
            Initials => (
                "Initials",
                "Draw or type your initials, then click where they go",
                Icon::Signature,
                INITIALS,
            ),
            TodaysDate => (
                "Today's date",
                "Into the box you're in, or click where it goes",
                Icon::TextField,
                TODAY,
            ),
            Settings => (
                "Settings",
                "Autocomplete, AI and remembered people",
                Icon::Settings,
                SETTINGS,
            ),
            Register => (
                "Register pdfr",
                "Get a free lifetime licence key at pdfr.app, or enter one",
                Icon::Key,
                NONE,
            ),
            ScrollDown => ("Scroll down", "", Icon::List, SCROLL_DOWN),
            ScrollUp => ("Scroll up", "", Icon::List, SCROLL_UP),
            HalfPageDown => ("Half a page down", "", Icon::List, HALF_DOWN),
            HalfPageUp => ("Half a page up", "", Icon::List, HALF_UP),
            NextPage => ("Next page", "", Icon::List, NEXT_PAGE),
            PrevPage => ("Previous page", "", Icon::List, PREV_PAGE),
            GoToPage => (
                "Go to page…",
                "Type a page number",
                Icon::Search,
                GO_TO_PAGE,
            ),
            GoBack => (
                "Go back",
                "Back to where you were before following a link",
                Icon::Undo,
                GO_BACK,
            ),
            NextEmpty => (
                "Next empty box",
                "Jump to the next box that still needs filling",
                Icon::TextField,
                NEXT_EMPTY,
            ),
            PrevEmpty => ("Previous empty box", "", Icon::TextField, PREV_EMPTY),
            JumpHints => (
                "Jump to a box",
                "Letters appear on every box you can see: type one to go there",
                Icon::Sparkle,
                JUMP_HINTS,
            ),
            Copy => (
                "Copy",
                "Copy the text selected on the page (drag over it, or double-click a word)",
                Icon::List,
                COPY,
            ),
            Find => (
                "Search…",
                "Find words in the document, and boxes by their label",
                Icon::Search,
                FIND,
            ),
            NextMatch => ("Next match", "", Icon::Search, NONE),
            PrevMatch => ("Previous match", "", Icon::Search, NONE),
            FindBox => (
                "Find a box…",
                "Search the boxes by their label",
                Icon::Search,
                FIND_BOX,
            ),
            ClearField => ("Clear this box", "", Icon::Close, NONE),
            RemoveField => (
                "Remove this box",
                "Only boxes pdfr or you added",
                Icon::Trash,
                REMOVE_FIELD,
            ),
            ToggleRemember => (
                "Don't remember this box",
                "Turn remembering on or off for this box",
                Icon::Brain,
                NONE,
            ),
            Palette => (
                "Command palette",
                "Run any command by name",
                Icon::Search,
                PALETTE,
            ),
            Cheatsheet => ("Keyboard shortcuts", "", Icon::List, CHEATSHEET),
        };
        Spec {
            label,
            hint,
            icon,
            shortcuts,
        }
    }

    /// Toolbar layout: groups separated by dividers.
    pub const TOOLBAR: &'static [&'static [Command]] = &[
        &[Command::Sidebar],
        &[
            Command::Open,
            Command::Save,
            Command::SaveAs,
            Command::Print,
        ],
        &[Command::Undo, Command::Redo],
        &[
            Command::ZoomOut,
            Command::ZoomIn,
            Command::ActualSize,
            Command::FitWidth,
            Command::FitPage,
            Command::RotatePage,
        ],
        &[
            Command::AddTextField,
            Command::AddCheckbox,
            Command::Sign,
            Command::FieldMarks,
        ],
    ];
}

impl App {
    /// Whether a command can run now.
    pub(crate) fn can(&self, c: Command) -> bool {
        use Command::*;
        let doc = self.doc.as_ref();
        let editable = doc.is_some_and(|d| d.form.editable());
        match c {
            // One file dialog at a time.
            Open | OpenNewWindow | Recent => !self.dialog_open(),
            Settings => true,
            Register => !self.license.registered(),
            Save | SaveAs => editable && !self.dialog_open(),
            Print => doc.is_some() && super::print::AVAILABLE && !self.dialog_open(),
            AddTextField | AddCheckbox | Sign | Initials | TodaysDate => editable,
            SignDigitally => editable && !self.digital.busy() && !self.dialog_open(),
            Undo => doc.is_some() && !self.undo.is_empty(),
            Redo => doc.is_some() && !self.redo.is_empty(),
            ZoomOut | ZoomIn | ActualSize | FitWidth | FitPage | Sidebar => doc.is_some(),
            DarkPages => true,
            TwoPages | Present | FieldMarks => doc.is_some(),
            RotatePage | RotatePageBack | RotateAll => editable,
            ScrollDown | ScrollUp | HalfPageDown | HalfPageUp | NextPage | PrevPage | GoToPage
            | Find | FindBox | JumpHints => doc.is_some(),
            Copy => self.focus.is_none() && self.text_sel.is_some(),
            NextMatch | PrevMatch => self.search.as_ref().is_some_and(|s| !s.hits.is_empty()),
            GoBack => doc.is_some() && !self.back.is_empty(),
            NextEmpty | PrevEmpty => editable,
            ClearField => editable && self.focused_editable(),
            RemoveField => editable && self.focused_editable() && self.focused_added(),
            ToggleRemember => self.focus.is_some(),
            Palette | Cheatsheet => true,
        }
    }

    /// Whether a toggle-like command is currently on (tools, open panels).
    pub(crate) fn is_on(&self, c: Command) -> bool {
        match c {
            Command::AddTextField => self.tool == Tool::AddText,
            Command::AddCheckbox => self.tool == Tool::AddCheckbox,
            Command::Sign => {
                (self.tool == Tool::PlaceSignature && !self.placing_initials)
                    || self.sign.as_ref().is_some_and(|s| !s.initials)
            }
            Command::SignDigitally => {
                self.digital.dialog.is_some()
                    || self.digital.pending.is_some()
                    || self.digital.busy()
            }
            Command::Initials => {
                (self.tool == Tool::PlaceSignature && self.placing_initials)
                    || self.sign.as_ref().is_some_and(|s| s.initials)
            }
            Command::TodaysDate => self.tool == Tool::PlaceDate,
            Command::Settings => self.assist.settings_open,
            Command::Sidebar => self.sidebar_open(),
            Command::DarkPages => self.dark_pages(),
            Command::TwoPages => self.two_up,
            Command::FieldMarks => self.assist.a.store.settings.show_fields,
            Command::Present => self.presenting(),
            Command::ToggleRemember => self.focused_entry().is_some_and(|e| e.no_memory),
            _ => false,
        }
    }

    pub(crate) fn run(&mut self, c: Command) {
        if !self.can(c) {
            return;
        }
        use Command::*;
        let center = self.viewport.center();
        match c {
            Open => self.pick_and_open(false),
            OpenNewWindow => self.pick_and_open(true),
            Recent => self.open_palette(super::palette::Scope::Recent),
            Save => self.save(false),
            SaveAs => self.save(true),
            Print => self.print(),
            Undo => self.undo(false),
            Redo => self.undo(true),
            ZoomOut => self.zoom_to(self.zoom / 1.2, center),
            ZoomIn => self.zoom_to(self.zoom * 1.2, center),
            ActualSize => self.zoom_to(1.0, center),
            FitWidth => {
                let z = self.fit_width_zoom();
                self.zoom_to(z, pos2(center.x, self.viewport.top()));
            }
            AddTextField => {
                self.tool = if self.tool == Tool::AddText {
                    Tool::None
                } else {
                    Tool::AddText
                }
            }
            AddCheckbox => {
                self.tool = if self.tool == Tool::AddCheckbox {
                    Tool::None
                } else {
                    Tool::AddCheckbox
                }
            }
            Sign => {
                if self.tool == Tool::PlaceSignature {
                    self.tool = Tool::None;
                    self.placing = None;
                } else {
                    let target = self.focused_sign_target();
                    self.open_sign(false, target);
                }
            }
            FitPage => self.fit_page(),
            TwoPages => self.toggle_two_up(),
            FieldMarks => {
                let s = &mut self.assist.a.store.settings.show_fields;
                *s = !*s;
                let on = *s;
                self.assist.a.save();
                self.status = if on {
                    "Fields are tinted.".into()
                } else {
                    "Fields untinted: the form as other PDF apps show it. Ctrl+H tints them again."
                        .into()
                };
            }
            Present => {
                let ctx = self.egui_ctx.clone();
                if self.presenting() {
                    self.stop_presenting(&ctx);
                } else {
                    self.start_presenting(&ctx);
                }
            }
            DarkPages => {
                let on = !self.dark_pages();
                self.set_dark_pages(on);
            }
            RotatePage | RotatePageBack => {
                let p = self.current_page();
                self.rotate_pages(vec![p], if c == RotatePage { 1 } else { -1 });
            }
            RotateAll => {
                let n = self.doc.as_ref().map_or(0, |d| d.sizes.len());
                self.rotate_pages((0..n).collect(), 1);
            }
            Sidebar => self.toggle_sidebar(),
            SignDigitally => {
                if self.digital.pending.is_some() {
                    self.digital.pending = None;
                    self.tool = Tool::None;
                    self.placing = None;
                } else {
                    let target = self.focused_sign_target();
                    self.open_digital(target);
                }
            }
            Initials => {
                if self.tool == Tool::PlaceSignature {
                    self.tool = Tool::None;
                    self.placing = None;
                } else {
                    self.open_sign(true, None);
                }
            }
            TodaysDate => {
                if self.tool == Tool::PlaceDate {
                    self.tool = Tool::None;
                } else {
                    self.stamp_date();
                }
            }
            Settings => self.assist.settings_open = !self.assist.settings_open,
            Register => self.license.open_register(),
            // Inside an open list (suggestions, drop-down) J/K move the
            // highlight, like fzf; otherwise they scroll.
            ScrollDown | ScrollUp => {
                let down = c == ScrollDown;
                if !self.move_in_open_list(down) {
                    self.scroll_by(if down { 60.0 } else { -60.0 });
                }
            }
            HalfPageDown => self.scroll_by(self.viewport.height() * 0.5),
            HalfPageUp => self.scroll_by(-self.viewport.height() * 0.5),
            NextPage => {
                let next = self.row_after(self.current_page());
                self.scroll_to_page(next);
            }
            PrevPage => {
                // Like zathura: back to this page's top first, if below it.
                let cur = self.current_page();
                let top = self.page_rects().get(cur).map(|r| r.top());
                let at_top = top.is_some_and(|t| t >= self.viewport.top() + PAD - 2.0);
                let to = if at_top { self.row_before(cur) } else { cur };
                self.scroll_to_page(to);
            }
            GoToPage => self.open_palette(super::palette::Scope::Page),
            GoBack => self.go_back(),
            NextEmpty => self.focus_empty(false),
            PrevEmpty => self.focus_empty(true),
            JumpHints => self.start_hints(),
            Copy => {
                let ctx = self.egui_ctx.clone();
                self.copy_selection(&ctx);
            }
            Find => self.open_palette(super::palette::Scope::Find),
            NextMatch => self.step_hit(false),
            PrevMatch => self.step_hit(true),
            FindBox => self.open_palette(super::palette::Scope::Boxes),
            ClearField => {
                if let Some(id) = self.focus.as_ref().map(|f| f.id) {
                    self.clear_field(id);
                }
            }
            RemoveField => {
                if let Some(id) = self.focus.as_ref().map(|f| f.id) {
                    self.remove_field(id);
                }
            }
            ToggleRemember => {
                if let Some(id) = self.focus.as_ref().map(|f| f.id) {
                    self.toggle_memory(id);
                }
            }
            Palette => self.open_palette(super::palette::Scope::Commands),
            Cheatsheet => self.overlay = Some(super::palette::Overlay::Cheatsheet),
        }
    }

    pub(super) fn focused_entry(&self) -> Option<&super::Entry> {
        let doc = self.doc.as_ref()?;
        let id = self.focus.as_ref()?.id;
        doc.entries.iter().find(|e| e.id == id)
    }

    fn focused_editable(&self) -> bool {
        self.focused_entry().is_some_and(|e| !e.f.read_only)
    }

    /// The focused box was added by pdfr or the user (not in the PDF).
    fn focused_added(&self) -> bool {
        self.focused_entry().is_some_and(|e| !e.f.is_real())
    }

    /// Runs commands whose shortcut was pressed. Shortcuts with more
    /// modifiers are checked first, so Ctrl+Shift+S is not taken as Ctrl+S.
    pub(crate) fn command_shortcuts(&mut self, ctx: &eframe::egui::Context) {
        let mut all: Vec<(KeyboardShortcut, Command)> = Command::ALL
            .iter()
            .flat_map(|&c| c.spec().shortcuts.iter().map(move |s| (*s, c)))
            .collect();
        all.sort_by_key(|(s, _)| std::cmp::Reverse(modifier_count(s.modifiers)));
        let mut fired: Vec<Command> = Vec::new();
        ctx.input_mut(|i| {
            for (s, c) in &all {
                if i.consume_shortcut(s) && !fired.contains(c) {
                    fired.push(*c);
                }
            }
        });
        for c in fired {
            self.run(c);
        }
    }
}

fn modifier_count(m: Modifiers) -> u32 {
    m.alt as u32 + m.ctrl as u32 + m.shift as u32 + m.mac_cmd as u32 + m.command as u32
}

/// "Save as (Ctrl+Shift+S)" plus the hint, for tooltips.
pub fn tooltip(ctx: &eframe::egui::Context, c: Command) -> String {
    let s = c.spec();
    let keys: Vec<String> = s.shortcuts.iter().map(|k| key_text(ctx, k)).collect();
    let mut t = s.label.to_string();
    if !keys.is_empty() {
        t.push_str(&format!("  ({})", keys.join(", ")));
    }
    if !s.hint.is_empty() {
        t.push('\n');
        t.push_str(s.hint);
    }
    t
}

/// A shortcut as people read it: "Ctrl+−" rather than egui's "Ctrl+Minus".
pub fn key_text(ctx: &eframe::egui::Context, k: &KeyboardShortcut) -> String {
    let t = ctx.format_shortcut(k);
    for (name, sym) in [
        ("Minus", "−"),
        ("Plus", "+"),
        ("Equals", "="),
        ("Comma", ","),
    ] {
        if let Some(head) = t.strip_suffix(name) {
            return format!("{head}{sym}");
        }
    }
    t
}
