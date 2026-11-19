use hayro::hayro_interpret::InterpreterCache;
use pdfr_core::render::open;
use pdfr_core::text::extract;

fn runs(name: &str) -> Vec<pdfr_core::text::TextRun> {
    let pdf = open(
        std::fs::read(format!(
            "{}/../../samples/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
    )
    .unwrap();
    let cache = InterpreterCache::new();
    extract(&pdf.pages()[0], &cache)
}

#[test]
fn labels_with_positions() {
    for name in ["flat_form.pdf", "flat_rotated.pdf"] {
        let r = runs(name);
        let find = |t: &str| {
            r.iter().find(|x| x.text == t).unwrap_or_else(|| {
                panic!(
                    "{name}: {t} not in {:#?}",
                    r.iter().map(|x| &x.text).collect::<Vec<_>>()
                )
            })
        };
        let full = find("Full name");
        assert!(
            (full.rect.x0 - 50.0).abs() < 1.0 && (full.rect.y1 - 184.0).abs() < 2.5,
            "{:?}",
            full.rect
        );
        find("Personal number");
        find("Date of birth (DD MM YYYY)");
        find("Street address");
        assert!(r.iter().any(|x| x.text.starts_with("Email")));
    }
    // The scan has no text layer.
    assert!(runs("scanned_form.pdf").is_empty());
}

#[test]
fn character_boxes_and_search_on_a_real_page() {
    for name in ["flat_form.pdf", "flat_rotated.pdf"] {
        let r = runs(name);
        for run in &r {
            assert_eq!(run.ws.len(), run.text.chars().count(), "{name}");
            assert!(run.ws.iter().all(|w| *w > 0.0 && *w < 2.0 * run.size));
        }
        // "Full name" sits at x 50: its "name" is about four letters wide.
        let hits = pdfr_core::search::search_page(0, &r, "name");
        let full = r.iter().find(|x| x.text == "Full name").unwrap();
        let h = hits
            .iter()
            .find(|h| h.rects[0].y1 == full.rect.y1)
            .expect("name in Full name");
        let w = h.rects[0].width();
        assert!(w > 1.5 * full.size && w < 3.5 * full.size, "{name}: {w}");
        assert!((h.rects[0].x1 - full.rect.x1).abs() < 0.5, "{name}");
        assert_eq!(pdfr_core::search::search_page(0, &r, "membership application").len(), 1);
    }
}
