//! Dev tool: one real AI call on a sample form, reporting tokens.
//! cargo run --release -p pdfr-assist --example ai_call -- samples/flat_form.pdf [--all-labels]
//! Normal mode follows the app (dictionary and cache first; usually 0 calls on
//! the sample). `--all-labels` sends every label of the form in one request,
//! bypassing the dictionary, to measure the cost of a call.
use pdfr_assist::{AiJob, Assistant, ai, memory::Store};
use std::sync::Arc;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let l = pdfr_core::pipeline::load(std::fs::read(&args[1]).unwrap()).unwrap();
    let mut store = Store::default();
    store.settings.ai_consent = Some(true);
    let backend: Arc<dyn ai::Backend> = ai::detect("", "", "").expect("no AI tool installed");
    println!("backend: {}", backend.name());
    let a = Assistant::new(store, None, Some(backend.clone()));
    let an = a.analyse(&l.fields, &l.text, &l.form.pages);
    let pending = a.pending_labels(&an);
    println!(
        "labels on the form: {}",
        an.info.iter().filter(|i| i.label.is_some()).count()
    );
    println!(
        "unresolved after dictionary/cache: {} {:?}",
        pending.len(),
        pending
    );
    let labels: Vec<String> = if args.iter().any(|x| x == "--all-labels") {
        let mut v: Vec<String> = an.info.iter().filter_map(|i| i.label.clone()).collect();
        v.sort();
        v.dedup();
        v
    } else {
        pending
    };
    if labels.is_empty() {
        println!("nothing to ask: 0 calls");
        return;
    }
    let job = AiJob {
        labels: labels.clone(),
        vocab: a.store.vocabulary(),
        backend,
        people: None,
    };
    let t = std::time::Instant::now();
    let out = job.run();
    println!(
        "request:\n{}",
        out.requests.first().cloned().unwrap_or_default()
    );
    println!(
        "calls: {}  time: {:.1}s  error: {:?}",
        out.calls,
        t.elapsed().as_secs_f64(),
        out.error
    );
    println!("reply:\n{}", out.replies.join("\n---\n"));
    for (n, i, o) in &out.usage {
        println!("usage: {n} labels, {i} input tokens, {o} output tokens");
    }
    for (l, a) in &out.answers {
        println!("  {l:40} -> {a:?}");
    }
}
