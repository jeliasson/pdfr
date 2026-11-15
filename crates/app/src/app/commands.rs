//! Every user action as a command: one place that defines its name, icon,
//! keyboard shortcut, when it is available and what it does. The toolbar,
//! the keyboard shortcuts (and later menus, context menus and a command
//! palette) are all built from this list, so a new feature is added here
//! once and shows up everywhere consistently.

use super::{App, Tool};
use crate::icons::Icon;
use eframe::egui::{Key, KeyboardShortcut, Modifiers, pos2};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    Open,
    Save,
    SaveAs,
    Print,
    Undo,
    Redo,
    ZoomOut,
    ZoomIn,
    ActualSize,
    FitWidth,
    AddTextField,
    AddCheckbox,
    Settings,
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
const SAVE: &[KeyboardShortcut] = &[ctrl(Key::S)];
const SAVE_AS: &[KeyboardShortcut] = &[ctrl_shift(Key::S)];
const PRINT: &[KeyboardShortcut] = &[ctrl(Key::P)];
const UNDO: &[KeyboardShortcut] = &[ctrl(Key::Z)];
const REDO: &[KeyboardShortcut] = &[ctrl_shift(Key::Z), ctrl(Key::Y)];
const ZOOM_OUT: &[KeyboardShortcut] = &[ctrl(Key::Minus)];
const ZOOM_IN: &[KeyboardShortcut] = &[ctrl(Key::Plus), ctrl(Key::Equals)];
const ACTUAL_SIZE: &[KeyboardShortcut] = &[ctrl(Key::Num0)];
const NONE: &[KeyboardShortcut] = &[];

impl Command {
    pub const ALL: &'static [Command] = &[
        Command::Open,
        Command::Save,
        Command::SaveAs,
        Command::Print,
        Command::Undo,
        Command::Redo,
        Command::ZoomOut,
        Command::ZoomIn,
        Command::ActualSize,
        Command::FitWidth,
        Command::AddTextField,
        Command::AddCheckbox,
        Command::Settings,
    ];

    pub fn spec(self) -> Spec {
        use Command::*;
        let (label, hint, icon, shortcuts) = match self {
            Open => ("Open", "Open a PDF", Icon::Open, OPEN),
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
                NONE,
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
            Settings => (
                "Settings",
                "Autocomplete, AI and remembered people",
                Icon::Settings,
                NONE,
            ),
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
        ],
        &[Command::AddTextField, Command::AddCheckbox],
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
            Open => !self.dialog_open(),
            Settings => true,
            Save | SaveAs => editable && !self.dialog_open(),
            Print => doc.is_some() && super::print::AVAILABLE && !self.dialog_open(),
            AddTextField | AddCheckbox => editable,
            Undo => doc.is_some() && !self.undo.is_empty(),
            Redo => doc.is_some() && !self.redo.is_empty(),
            ZoomOut | ZoomIn | ActualSize | FitWidth => doc.is_some(),
        }
    }

    /// Whether a toggle-like command is currently on (tools, open panels).
    pub(crate) fn is_on(&self, c: Command) -> bool {
        match c {
            Command::AddTextField => self.tool == Tool::AddText,
            Command::AddCheckbox => self.tool == Tool::AddCheckbox,
            Command::Settings => self.assist.settings_open,
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
            Open => self.pick_and_open(),
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
            Settings => self.assist.settings_open = !self.assist.settings_open,
        }
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
    let keys: Vec<String> = s.shortcuts.iter().map(|k| ctx.format_shortcut(k)).collect();
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
