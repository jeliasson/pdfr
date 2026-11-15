//! Dev tool: print each field's label and meaning, and the exact labels the
//! AI would be asked about. Makes no AI call.
//! cargo run --release -p pdfr-assist --example labels -- form.pdf [page]
use pdfr_assist::{Assistant, memory::Store};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let only: Option<usize> = args.get(2).and_then(|s| s.parse().ok());
    let l = pdfr_core::pipeline::load(std::fs::read(&args[1]).unwrap()).unwrap();
    let a = Assistant::new(Store::default(), None, None);
    let an = a.analyse(&l.fields, &l.text, &l.form.pages);
    for (i, f) in l.fields.iter().enumerate() {
        if only.is_some_and(|p| p != f.page) {
            continue;
        }
        let m = an.meanings[i]
            .as_ref()
            .map(|m| format!("{m:?}"))
            .unwrap_or_default();
        println!(
            "p{} {:>5.0},{:<5.0} {:4} person {:2} {:50} {}",
            f.page + 1,
            f.rect.x0,
            f.rect.y0,
            f.kind.tag(),
            an.person[i]
                .map(|p| (p + 1).to_string())
                .unwrap_or_else(|| "-".into()),
            an.info[i].label.as_deref().unwrap_or("-"),
            m
        );
    }
    match a.people_request(&an, &l.fields) {
        Some(r) => println!(
            "\npeople request ({} people found locally):\n{}",
            an.people.len(),
            r.prompt
        ),
        None => println!(
            "\nno people request ({} people found locally)",
            an.people.len()
        ),
    }
    let pending = a.pending_labels(&an);
    println!("\nwould ask the AI about {} labels:", pending.len());
    for p in pending {
        println!("  {p}");
    }
}
