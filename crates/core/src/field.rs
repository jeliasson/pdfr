//! The field model shared by real (AcroForm) fields and detected fields.

use crate::geom::Rect;
use crate::signature::Ink;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

impl Align {
    /// From the PDF `/Q` value.
    pub fn from_q(q: i64) -> Self {
        match q {
            1 => Align::Center,
            2 => Align::Right,
            _ => Align::Left,
        }
    }
    pub fn to_q(self) -> i64 {
        match self {
            Align::Left => 0,
            Align::Center => 1,
            Align::Right => 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TextMode {
    /// A single line, vertically centred on cap height.
    Single,
    /// Text written on a ruled line: baseline 2 pt above the rect's bottom edge.
    OnLine,
    /// Top-aligned, word-wrapped.
    Multi,
    /// One character per cell. Each cell's x range in display space.
    Comb(Vec<(f32, f32)>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum FieldKind {
    Text {
        mode: TextMode,
        password: bool,
        max_len: Option<usize>,
    },
    Checkbox {
        /// The appearance state name used for "on" (e.g. `Yes`).
        on_state: String,
    },
    Radio {
        on_state: String,
        /// Fields with the same group are mutually exclusive.
        group: String,
    },
    Choice {
        /// `(export value, display text)`.
        options: Vec<(String, String)>,
        editable: bool,
    },
    /// A hand-drawn or typed signature placed by the user. `lock` draws it
    /// into the page content when saving instead of as an annotation.
    Signature { ink: Arc<Ink>, lock: bool },
}

impl FieldKind {
    pub fn is_text(&self) -> bool {
        matches!(self, FieldKind::Text { .. })
    }
    pub fn text_mode(&self) -> Option<&TextMode> {
        match self {
            FieldKind::Text { mode, .. } => Some(mode),
            _ => None,
        }
    }
    pub fn is_comb(&self) -> bool {
        matches!(self.text_mode(), Some(TextMode::Comb(_)))
    }
    pub fn is_multiline(&self) -> bool {
        matches!(self.text_mode(), Some(TextMode::Multi))
    }
    pub fn is_password(&self) -> bool {
        matches!(self, FieldKind::Text { password: true, .. })
    }
    pub fn is_toggle(&self) -> bool {
        matches!(self, FieldKind::Checkbox { .. } | FieldKind::Radio { .. })
    }
    pub fn is_signature(&self) -> bool {
        matches!(self, FieldKind::Signature { .. })
    }
    /// A drop-down whose value must be one of its options (no free text).
    pub fn is_fixed_choice(&self) -> bool {
        matches!(self, FieldKind::Choice { editable: false, .. })
    }
    /// Short tag for fingerprints and debugging.
    pub fn tag(&self) -> &'static str {
        match self {
            FieldKind::Text { mode, .. } => match mode {
                TextMode::Single => "t",
                TextMode::OnLine => "l",
                TextMode::Multi => "m",
                TextMode::Comb(_) => "c",
            },
            FieldKind::Checkbox { .. } => "x",
            FieldKind::Radio { .. } => "r",
            FieldKind::Choice { .. } => "d",
            FieldKind::Signature { .. } => "s",
        }
    }
}

/// PDF object id `(number, generation)`.
pub type ObjId = (u32, u16);

#[derive(Clone, Debug, PartialEq)]
pub enum Origin {
    /// A widget annotation of a real form field.
    AcroForm {
        widget: ObjId,
        /// The terminal field dictionary holding `/V` (may equal `widget`).
        field: ObjId,
        /// Fully qualified field name.
        name: String,
    },
    /// Found by detection on a flat or scanned page.
    Detected,
    /// Drawn by the user with a manual tool.
    Manual,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub page: usize,
    /// Display-space rectangle in points.
    pub rect: Rect,
    pub kind: FieldKind,
    /// Text or choice value.
    pub value: String,
    /// Checkbox / radio state.
    pub checked: bool,
    /// Fixed font size; `None` means automatic.
    pub font_size: Option<f32>,
    pub align: Align,
    pub read_only: bool,
    pub origin: Origin,
    /// What a person would read to know what goes here (filled in by `assist`).
    pub label: Option<String>,
    /// The `/TU` tooltip of a real field.
    pub tooltip: Option<String>,
}

impl Field {
    pub fn new_text(page: usize, rect: Rect, mode: TextMode, origin: Origin) -> Self {
        Field {
            page,
            rect,
            kind: FieldKind::Text {
                mode,
                password: false,
                max_len: None,
            },
            value: String::new(),
            checked: false,
            font_size: None,
            align: Align::Left,
            read_only: false,
            origin,
            label: None,
            tooltip: None,
        }
    }

    pub fn new_checkbox(page: usize, rect: Rect, origin: Origin) -> Self {
        Field {
            kind: FieldKind::Checkbox {
                on_state: "Yes".into(),
            },
            ..Field::new_text(page, rect, TextMode::Single, origin)
        }
    }

    /// Maximum number of characters the field accepts, if limited.
    pub fn capacity(&self) -> Option<usize> {
        match &self.kind {
            FieldKind::Text {
                mode: TextMode::Comb(cells),
                max_len,
                ..
            } => Some(max_len.map_or(cells.len(), |m| m.min(cells.len()))),
            FieldKind::Text { max_len, .. } => *max_len,
            _ => None,
        }
    }

    pub fn is_real(&self) -> bool {
        matches!(self.origin, Origin::AcroForm { .. })
    }

    /// The text shown for the value: a fixed drop-down shows the option's
    /// display text ("Sweden") for its export value ("SE").
    pub fn shown_text(&self) -> &str {
        if let FieldKind::Choice {
            options,
            editable: false,
        } = &self.kind
            && let Some((_, display)) = options.iter().find(|(e, _)| *e == self.value)
        {
            return display;
        }
        &self.value
    }

    pub fn is_empty(&self) -> bool {
        if self.kind.is_signature() {
            false
        } else if self.kind.is_toggle() {
            !self.checked
        } else {
            self.value.is_empty()
        }
    }

    /// Splits `rect` into `n` equal comb cells.
    pub fn equal_cells(rect: &Rect, n: usize) -> Vec<(f32, f32)> {
        let w = rect.width() / n.max(1) as f32;
        (0..n)
            .map(|i| (rect.x0 + w * i as f32, rect.x0 + w * (i + 1) as f32))
            .collect()
    }
}
