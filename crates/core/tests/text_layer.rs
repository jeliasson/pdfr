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
