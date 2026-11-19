//! Writes a signed copy of a sample (one editable, one locked signature).
//! `cargo run -p pdfr-core --example sign_demo -- in.pdf out.pdf`
use pdfr_core::field::Field;
use pdfr_core::forms::FormDoc;
use pdfr_core::geom::Rect;
use pdfr_core::signature::Ink;
use std::sync::Arc;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let fd = FormDoc::open(std::fs::read(&a[1]).unwrap()).unwrap();
    let typed = Arc::new(Ink::typed("Anna Lindqvist"));
    let drawn = Arc::new(Ink::from_drawing(
        &[
            vec![
                [0.0, 30.0],
                [10.0, 5.0],
                [18.0, 0.0],
                [22.0, 10.0],
                [15.0, 40.0],
                [25.0, 38.0],
                [35.0, 20.0],
                [45.0, 30.0],
                [60.0, 18.0],
                [80.0, 25.0],
            ],
            vec![[30.0, 45.0], [70.0, 42.0]],
        ],
        2.5,
    ));
    let f1 = Field::new_signature(
        0,
        typed.fit(&Rect::new(300.0, 600.0, 520.0, 650.0)),
        typed,
        false,
    );
    let f2 = Field::new_signature(
        0,
        drawn.fit(&Rect::new(60.0, 600.0, 220.0, 650.0)),
        drawn,
        true,
    );
    std::fs::write(&a[2], pdfr_core::save::save(&fd, &[f1, f2]).unwrap()).unwrap();
}
