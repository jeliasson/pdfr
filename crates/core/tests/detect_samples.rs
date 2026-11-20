//! End-to-end detection on the generated samples, checked against the
//! expected fields recorded by tools/gen_samples.py.

use pdfr_core::detect::detect_page;
use pdfr_core::field::{Field, FieldKind, TextMode};
use pdfr_core::render::{RenderCache, open};
use serde_json::Value;

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../samples/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn detect(name: &str) -> Vec<Field> {
    let pdf = open(sample(name)).unwrap();
    let cache = RenderCache::new();
    let page = &pdf.pages()[0];
    detect_page(page, &cache, 0)
}

fn kind_of(f: &Field) -> &'static str {
    match &f.kind {
        FieldKind::Checkbox { .. } => "checkbox",
        FieldKind::Text { mode, .. } => match mode {
            TextMode::Single => "text",
            TextMode::Multi => "multi",
            TextMode::OnLine => "line",
            TextMode::Comb(_) => "comb",
        },
        _ => "other",
    }
}

fn check(name: &str, tol: f32) {
    let found = detect(name);
    let exp: Value = serde_json::from_slice(&sample("flat_form.json")).unwrap();
    let exp = exp["fields"].as_array().unwrap();
    let mut matched = vec![false; found.len()];
    let mut missing = vec![];
    for e in exp {
        let kind = e["kind"].as_str().unwrap();
        let r: Vec<f32> = e["rect"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap() as f32)
            .collect();
        let hit = found.iter().enumerate().position(|(i, f)| {
            if matched[i] || kind_of(f) != kind {
                return false;
            }
            let fr = f.rect;
            let ok = if kind == "line" {
                // Write-on fields sit on the line: compare the extent and the bottom.
                (fr.x0 - r[0]).abs() <= tol
                    && (fr.x1 - r[2]).abs() <= tol
                    && (fr.y1 - r[1]).abs() <= tol
            } else {
                (fr.x0 - r[0]).abs() <= tol
                    && (fr.y0 - r[1]).abs() <= tol
                    && (fr.x1 - r[2]).abs() <= tol
                    && (fr.y1 - r[3]).abs() <= tol
            };
            let cells_ok = match (e.get("cells"), f.kind.text_mode()) {
                (Some(n), Some(TextMode::Comb(c))) => n.as_u64().unwrap() as usize == c.len(),
                (None, _) => true,
                _ => false,
            };
            ok && cells_ok
        });
        match hit {
            Some(i) => matched[i] = true,
            None => missing.push(format!("{kind} {r:?} {:?}", e.get("label"))),
        }
    }
    let extra: Vec<String> = found
        .iter()
        .zip(&matched)
        .filter(|(_, m)| !**m)
        .map(|(f, _)| format!("{} {:?}", kind_of(f), f.rect))
        .collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{name}: missing {missing:#?}\nextra {extra:#?}"
    );
    // Nothing in the heading, the paragraph or the filled bars.
    for f in &found {
        let y = f.rect.center().1;
        assert!(
            !(40.0..160.0).contains(&y) && !(440.0..456.0).contains(&y),
            "{name}: field in text area {:?}",
            f.rect
        );
    }
}

#[test]
fn flat_form_fields() {
    check("flat_form.pdf", 2.5);
}

#[test]
fn rotated_form_fields_are_upright_in_display_space() {
    check("flat_rotated.pdf", 2.5);
}

#[test]
fn scanned_form_fields() {
    // 0.25° skew moves page corners by up to ~2 pt.
    check("scanned_form.pdf", 4.0);
}

#[test]
fn real_form_text_page_has_no_false_boxes_outside_widgets() {
    // perf_text is a dense text page: no fields at all.
    assert!(detect("perf_text.pdf").is_empty());
}

/// A realistic scan (warm paper, uneven light, skew, blur, JPEG and
/// scanner dust; tools/gen_media_form.py): every place a value goes must
/// be inside a detected field.
#[test]
fn scanned_household_form_finds_every_field() {
    let fields = detect("household_scanned.pdf");
    let vals: Value = serde_json::from_slice(&sample("household_values.json")).unwrap();
    let mut missed = Vec::new();
    for v in vals.as_array().unwrap() {
        let (x, y) = (
            v["x"].as_f64().unwrap() as f32,
            v["y"].as_f64().unwrap() as f32,
        );
        let hit = fields.iter().any(|f| {
            let r = f.rect;
            r.x0 - 2.0 <= x && x <= r.x1 + 2.0 && r.y0 - 6.0 <= y && y <= r.y1 + 6.0
        });
        if !hit {
            missed.push(v["value"].to_string());
        }
    }
    assert!(missed.is_empty(), "no field for: {missed:?}");
}

/// Boxes drawn by hand in a paint program (wobbly, slanted, overshooting
/// corners, labels touching the outline): each one is a field, and the loop
/// of the "d" in "ZipCode" is not.
#[test]
fn hand_drawn_boxes() {
    let fields = detect("handwritten_by_kid.pdf");
    // A point well inside each drawn box or above each line, in points.
    let spots = [
        ("Name box", 90.0, 125.0),
        ("Age box", 280.0, 134.0),
        ("ZipCode box", 290.0, 195.0),
        ("City box", 105.0, 220.0),
        ("Name line", 330.0, 330.0),
        ("Sign line", 310.0, 368.0),
    ];
    for (what, x, y) in spots {
        assert!(
            fields.iter().any(|f| f.rect.contains(x, y)),
            "no field for {what}: {:?}",
            fields.iter().map(|f| f.rect).collect::<Vec<_>>()
        );
    }
    assert_eq!(
        fields.len(),
        spots.len(),
        "{:?}",
        fields.iter().map(|f| f.rect).collect::<Vec<_>>()
    );
}

/// A tax-return-like scan (200 dpi, blurred, JPEG; tools/gen_samples.py):
/// letter tops in the paragraphs are no lines to write on, and table cells
/// holding a label are no boxes, even where a slanted row line stops the
/// ruled-box finder. The empty amount column and the signature line stay.
#[test]
fn scanned_table_has_no_fields_on_text() {
    let fields = detect("scanned_table.pdf");
    let exp: Value = serde_json::from_slice(&sample("scanned_table.json")).unwrap();
    let rect = |v: &Value| -> pdfr_core::geom::Rect {
        let a: Vec<f32> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_f64().unwrap() as f32)
            .collect();
        pdfr_core::geom::Rect::new(a[0], a[1], a[2], a[3])
    };
    for t in exp["text"].as_array().unwrap() {
        let t = rect(t);
        for f in &fields {
            let r = f.rect;
            let overlap = r.x0 < t.x1 && t.x0 < r.x1 && r.y0 < t.y1 && t.y0 < r.y1;
            // A field may end just above the text; it must not cover it,
            // nor sit in the label's cell beside it.
            let beside = r.y0 < t.y1 && t.y0 < r.y1 && r.x0 < 430.0;
            assert!(!overlap && !beside, "field {:?} on text {:?}", r, t);
        }
    }
    for want in exp["fields"].as_array().unwrap() {
        let w = rect(want);
        let (cx, cy) = ((w.x0 + w.x1) / 2.0, (w.y0 + w.y1) / 2.0);
        assert!(
            fields.iter().any(|f| f.rect.x0 - 2.0 <= cx
                && cx <= f.rect.x1 + 2.0
                && f.rect.y0 - 4.0 <= cy
                && cy <= f.rect.y1 + 4.0),
            "no field at {w:?}"
        );
    }
    assert_eq!(
        fields.len(),
        exp["fields"].as_array().unwrap().len(),
        "{fields:#?}"
    );
}
