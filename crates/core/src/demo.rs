//! Sample values for `--demo` and tests.

use crate::field::{Field, FieldKind, TextMode};

/// Fills every empty, editable field with a plausible sample value.
pub fn fill_demo(fields: &mut [Field]) {
    let mut radio_groups = std::collections::HashSet::new();
    for (i, f) in fields.iter_mut().enumerate() {
        if f.read_only || !f.is_empty() {
            continue;
        }
        match &f.kind {
            FieldKind::Text { mode, .. } => {
                f.value = match mode {
                    TextMode::Comb(cells) => "4815162342ABCDEFGHJK"
                        .chars()
                        .cycle()
                        .skip(i % 7)
                        .take(cells.len())
                        .collect(),
                    TextMode::Multi => "Multi-line notes wrap inside the box and shrink when there is a lot to say. Ångström, señor, smörgåsbord.".into(),
                    TextMode::OnLine => "Anna Lindqvist".into(),
                    TextMode::Single => {
                        let samples = ["Anna Lindqvist", "anna@example.se", "Storgatan 12", "Göteborg", "42", "1 250,00", "Engineer"];
                        samples[i % samples.len()].into()
                    }
                };
                if let Some(cap) = f.capacity() {
                    f.value = f.value.chars().take(cap).collect();
                }
            }
            FieldKind::Checkbox { .. } => f.checked = i % 2 == 0,
            FieldKind::Radio { group, .. } => {
                if radio_groups.insert(group.clone()) {
                    f.checked = true;
                }
            }
            FieldKind::Choice { options, .. } => {
                if let Some(o) = options.get(1).or(options.first()) {
                    f.value = o.0.clone();
                }
            }
        }
    }
}
