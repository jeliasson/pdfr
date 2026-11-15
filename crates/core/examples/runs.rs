//! Dev tool: print the text runs of one page with their character positions.
//! cargo run --release -p pdfr-core --example runs -- in.pdf page
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let l = pdfr_core::pipeline::load(std::fs::read(&a[1]).unwrap()).unwrap();
    let page: usize = a[2].parse().unwrap();
    for r in l.text.get(&page).into_iter().flatten() {
        println!(
            "{:6.1},{:6.1} {:6.1},{:6.1} {:?} xs={:?}",
            r.rect.x0,
            r.rect.y0,
            r.rect.x1,
            r.rect.y1,
            r.text,
            r.xs.iter().map(|x| *x as i32).collect::<Vec<_>>()
        );
    }
}
