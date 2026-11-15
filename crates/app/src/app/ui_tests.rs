//! Headless UI tests: the real App driven with simulated pointer and
//! keyboard events (egui_kittest). No window, no GPU.

use super::*;
use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use pdfr_assist::memory::Store;
use std::time::{Duration, Instant};

fn sample(name: &str) -> PathBuf {
    PathBuf::from(format!(
        "{}/../../samples/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
}

fn data_dir(tag: &str, store: Option<Store>) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pdfr-ui-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    if let Some(s) = store {
        s.save(&d.join("memory.json")).unwrap();
    }
    d
}

fn harness(file: &str, dir: PathBuf) -> Harness<'static, App> {
    let cli = CliOptions {
        file: Some(sample(file)),
        no_ai: true,
        data_dir: Some(dir),
        zoom: Some(1.2),
        ..Default::default()
    };
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 1100.0))
        .build_eframe(move |cc| App::new(cc, cli));
    // Wait for the document and detection (background threads).
    let start = Instant::now();
    loop {
        h.run_steps(1);
        let s = h.state();
        if s.doc
            .as_ref()
            .is_some_and(|d| d.detected.iter().all(|x| *x))
            && s.assist.idle()
        {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "timed out waiting for detection"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    h.run_steps(3);
    h
}

/// Screen position of a display-space point on page 0.
fn screen(h: &Harness<'_, App>, x: f32, y: f32) -> Pos2 {
    let s = h.state();
    let r = s.page_rects()[0];
    pos2(r.left() + x * s.zoom, r.top() + y * s.zoom)
}

fn click(h: &mut Harness<'_, App>, p: Pos2) {
    h.event(Event::PointerMoved(p));
    h.event(Event::PointerButton {
        pos: p,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.event(Event::PointerButton {
        pos: p,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
}

/// Hovers a field (its button appears), then clicks the button.
fn click_field_button(h: &mut Harness<'_, App>, f: &Field) {
    let b = {
        let s = h.state();
        s.field_button_rect(&s.page_rects(), f)
            .expect("field has room for a button")
    };
    h.event(Event::PointerMoved(b.center()));
    h.run_steps(2);
    click(h, b.center());
}

fn click_doc(h: &mut Harness<'_, App>, x: f32, y: f32) {
    let p = screen(h, x, y);
    click(h, p);
}

fn type_text(h: &mut Harness<'_, App>, t: &str) {
    h.event(Event::Text(t.into()));
    h.run_steps(1);
}

fn key(h: &mut Harness<'_, App>, k: Key) {
    h.key_press(k);
    h.run_steps(1);
}

/// The entry whose rect matches (within 2pt) the given display rect.
fn entry_at<'a>(h: &'a Harness<'_, App>, x0: f32, y0: f32) -> &'a Entry {
    let d = h.state().doc.as_ref().unwrap();
    d.entries
        .iter()
        .find(|e| (e.f.rect.x0 - x0).abs() < 2.5 && (e.f.rect.y0 - y0).abs() < 2.5)
        .unwrap_or_else(|| panic!("no field at {x0},{y0}"))
}

fn focused_id(h: &Harness<'_, App>) -> Option<u64> {
    h.state().focus.as_ref().map(|f| f.id)
}

#[test]
fn click_type_tab_and_combs() {
    let mut h = harness("flat_form.pdf", data_dir("type", None));
    // Click into "Full name" and type.
    click_doc(&mut h, 300.0, 178.0);
    let full = entry_at(&h, 150.0, 168.0).id;
    assert_eq!(focused_id(&h), Some(full));
    type_text(&mut h, "Anna Lindqvist");
    assert_eq!(entry_at(&h, 150.0, 168.0).f.value, "Anna Lindqvist");
    // Click between letters: the caret lands where clicked (shared layout hit test).
    let f = entry_at(&h, 150.0, 168.0).f.clone();
    let l = pdfr_core::layout::layout(&f, &f.value);
    let g = &l.lines[0].glyphs[4]; // the space after "Anna"
    click_doc(&mut h, g.x + 0.2, l.lines[0].baseline - 2.0);
    assert_eq!(h.state().focus.as_ref().unwrap().ed.caret, 4);
    type_text(&mut h, "-Maria");
    assert_eq!(entry_at(&h, 150.0, 168.0).f.value, "Anna-Maria Lindqvist");
    // Tab moves to the next field in reading order: the personal number boxes.
    key(&mut h, Key::Tab);
    let pn = entry_at(&h, 150.0, 200.0).id;
    assert_eq!(focused_id(&h), Some(pn));
    // One character per box; never more than there are boxes.
    type_text(&mut h, "8501011234XYZ");
    assert_eq!(entry_at(&h, 150.0, 200.0).f.value, "8501011234");
    // Date of birth: three rows of boxes on one line. Filling one moves on.
    let day = entry_at(&h, 190.0, 232.0).id;
    let month = entry_at(&h, 232.0, 232.0).id;
    let year = entry_at(&h, 274.0, 232.0).id;
    click_doc(&mut h, 195.0, 241.0);
    assert_eq!(focused_id(&h), Some(day));
    type_text(&mut h, "1");
    type_text(&mut h, "2");
    assert_eq!(
        focused_id(&h),
        Some(month),
        "full row moves focus to the next row of boxes"
    );
    type_text(&mut h, "031985");
    assert_eq!(entry_at(&h, 190.0, 232.0).f.value, "12");
    assert_eq!(entry_at(&h, 232.0, 232.0).f.value, "03");
    assert_eq!(entry_at(&h, 274.0, 232.0).f.value, "1985");
    assert_eq!(focused_id(&h), Some(year));
    // Backspace at the start of a row goes back into the previous one.
    click_doc(&mut h, 236.0, 241.0);
    assert_eq!(focused_id(&h), Some(month));
    key(&mut h, Key::Home);
    key(&mut h, Key::Backspace);
    assert_eq!(focused_id(&h), Some(day));
    assert_eq!(entry_at(&h, 190.0, 232.0).f.value, "1");
    // Overwrite in a full row: click the first box of the year and type.
    click_doc(&mut h, 278.0, 241.0);
    type_text(&mut h, "2");
    assert_eq!(entry_at(&h, 274.0, 232.0).f.value, "2985");
    // Checkbox toggles on click.
    click_doc(&mut h, 245.0, 273.0);
    assert!(entry_at(&h, 240.0, 268.0).f.checked);
    // Undo restores the previous state.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert!(!entry_at(&h, 240.0, 268.0).f.checked);
    // Escape leaves the field.
    key(&mut h, Key::Escape);
    assert_eq!(focused_id(&h), None);
}

#[test]
fn ghost_text_tab_accepts_and_clear_button_undoes() {
    let mut store = Store::default();
    store.learn("full_name", "Anna Lindqvist");
    store.learn("job_title", "Engineer");
    store.learn("email", "anna@example.se");
    store.learn("date_of_birth", "1985-03-12");
    let mut h = harness("flat_form.pdf", data_dir("ghost", Some(store)));
    // Focus "Full name": the stored name appears as ghost text.
    click_doc(&mut h, 300.0, 178.0);
    assert_eq!(
        h.state().focus.as_ref().unwrap().ghost.as_deref(),
        Some("Anna Lindqvist")
    );
    // Nothing is written until accepted.
    assert_eq!(entry_at(&h, 150.0, 168.0).f.value, "");
    key(&mut h, Key::Tab);
    assert_eq!(entry_at(&h, 150.0, 168.0).f.value, "Anna Lindqvist");
    // Escape dismisses a ghost; typing replaces it.
    click_doc(&mut h, 300.0, 305.0); // Occupation
    assert_eq!(
        h.state().focus.as_ref().unwrap().ghost.as_deref(),
        Some("Engineer")
    );
    type_text(&mut h, "Dr");
    assert_eq!(entry_at(&h, 150.0, 295.3).f.value, "Dr");
    assert!(h.state().focus.as_ref().unwrap().ghost.is_none());
    // Hovering a filled field shows ✕: it clears the field, one undo step.
    let occupation = entry_at(&h, 150.0, 295.3).f.clone();
    click_field_button(&mut h, &occupation);
    assert_eq!(entry_at(&h, 150.0, 295.3).f.value, "");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(entry_at(&h, 150.0, 295.3).f.value, "Dr");
}

#[test]
fn scrolling_and_zoom_keep_pages() {
    let mut h = harness("flat_form.pdf", data_dir("scroll", None));
    let before = h.state().page_rects()[0];
    h.event(Event::PointerMoved(pos2(500.0, 500.0)));
    h.event(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -120.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
    });
    h.run_steps(30);
    let after = h.state().page_rects()[0];
    assert!(
        after.top() < before.top(),
        "scrolled: {before:?} -> {after:?}"
    );
    // Zoom around the pointer: the document point under it stays put.
    let p = pos2(400.0, 400.0);
    let (r0, z0) = (h.state().page_rects()[0], h.state().zoom);
    let doc_pt = (p - r0.min) / z0;
    h.state_mut().zoom_to(z0 * 1.5, p);
    let (r1, z1) = (h.state().page_rects()[0], h.state().zoom);
    let p1 = r1.min + doc_pt * z1;
    assert!((p1 - p).length() < 1.0, "anchor moved {p:?} -> {p1:?}");
    // The old texture is kept (stretched) until the re-render arrives.
    assert!(h.state().doc.as_ref().unwrap().tex.contains_key(&0));
}

/// Scrolls through the 300-page file at a steady speed and counts frames in
/// which a visible page had no texture yet (shown as blank paper).
/// Needs samples/big_300.pdf (tools/gen_samples.py --big); skipped otherwise.
#[test]
fn steady_scroll_through_300_pages() {
    if !sample("big_300.pdf").exists() {
        eprintln!("skipped: samples/big_300.pdf not generated");
        return;
    }
    let cli = CliOptions {
        file: Some(sample("big_300.pdf")),
        no_ai: true,
        data_dir: Some(data_dir("big", None)),
        zoom: Some(1.0),
        ..Default::default()
    };
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 900.0))
        .build_eframe(move |cc| App::new(cc, cli));
    let start = Instant::now();
    while h.state().doc.is_none() {
        h.run_steps(1);
        std::thread::sleep(Duration::from_millis(5));
        assert!(start.elapsed() < Duration::from_secs(20));
    }
    let mut blank = 0;
    let mut frames = 0;
    let speed = 3000.0; // points per second (about 3.5 pages a second)
    let t0 = Instant::now();
    let mut last = Instant::now();
    while t0.elapsed() < Duration::from_secs(8) {
        let dt = last.elapsed().as_secs_f32();
        last = Instant::now();
        h.state_mut().scroll.y += speed * dt;
        h.run_steps(1);
        frames += 1;
        let s = h.state();
        let d = s.doc.as_ref().unwrap();
        for (i, r) in s.page_rects().iter().enumerate() {
            if r.intersects(s.viewport) && !d.tex.contains_key(&i) {
                blank += 1;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    let pages = h.state().scroll.y / (842.0 + GAP);
    eprintln!(
        "steady scroll: {frames} frames, {blank} with a blank visible page, {pages:.0} pages scrolled"
    );
    assert!(
        blank * 100 <= frames,
        "more than 1% of frames had a blank page: {blank}/{frames}"
    );
}

#[test]
fn space_toggles_checkbox_and_keeps_focus() {
    let mut h = harness("flat_form.pdf", data_dir("space", None));
    click_doc(&mut h, 245.0, 273.0);
    let cb = entry_at(&h, 240.0, 268.0).id;
    assert!(entry_at(&h, 240.0, 268.0).f.checked);
    assert_eq!(focused_id(&h), Some(cb));
    // A real keyboard sends the key and the typed " " together.
    h.key_press(Key::Space);
    h.event(egui::Event::Text(" ".into()));
    h.run_steps(2);
    assert!(!entry_at(&h, 240.0, 268.0).f.checked, "Space toggles");
    assert_eq!(focused_id(&h), Some(cb), "Space stays on the checkbox");
    h.key_press(Key::Space);
    h.event(egui::Event::Text(" ".into()));
    h.run_steps(2);
    assert!(entry_at(&h, 240.0, 268.0).f.checked);
    assert_eq!(focused_id(&h), Some(cb));
}

#[test]
fn opening_another_file_asks_when_there_are_unsaved_changes() {
    let mut h = harness("flat_form.pdf", data_dir("dirty-open", None));
    click_doc(&mut h, 300.0, 178.0);
    type_text(&mut h, "Anna");
    assert!(h.state().doc.as_ref().unwrap().dirty);
    h.state_mut().request_open(sample("real_form.pdf"));
    h.run_steps(2);
    assert!(
        matches!(h.state().confirm, Some(Confirm::Open(_))),
        "asks before discarding"
    );
    // Nothing replaced while the question is open.
    for _ in 0..20 {
        h.run_steps(1);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(entry_at(&h, 150.0, 168.0).f.value, "Anna");
}

#[test]
fn suggestion_list_and_ghost_never_leak_into_another_field() {
    let mut store = Store::default();
    store.learn("full_name", "Anna Lindqvist");
    store.learn("full_name", "Per Berg");
    store.learn("job_title", "Engineer");
    let mut h = harness("flat_form.pdf", data_dir("list-leak", Some(store)));
    // Open the alternatives in "Full name", then click "Occupation".
    click_doc(&mut h, 300.0, 178.0);
    key(&mut h, Key::ArrowDown);
    assert!(h.state().assist.list_open());
    click_doc(&mut h, 300.0, 305.0);
    assert!(
        !h.state().assist.list_open(),
        "the list closes when focus moves"
    );
    key(&mut h, Key::Enter);
    assert_ne!(entry_at(&h, 150.0, 295.3).f.value, "Anna Lindqvist");

    // Undo restores a value: no stale ghost is left to overwrite it on Tab.
    click_doc(&mut h, 300.0, 178.0);
    type_text(&mut h, "abc");
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    click_doc(&mut h, 300.0, 178.0);
    key(&mut h, Key::End);
    for _ in 0..3 {
        key(&mut h, Key::Backspace);
    }
    assert!(
        h.state().focus.as_ref().unwrap().ghost.is_some(),
        "emptied field shows a ghost"
    );
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(entry_at(&h, 150.0, 168.0).f.value, "abc");
    assert!(
        h.state().focus.as_ref().unwrap().ghost.is_none(),
        "no ghost over a restored value"
    );
}

#[test]
fn suggestion_button_and_arrow_keys_browse_alternatives() {
    let mut store = Store::default();
    store.learn("full_name", "Anna Lindqvist");
    store.learn("full_name", "Erik Lindqvist");
    store.learn("full_name", "Maja Berg");
    let mut h = harness("flat_form.pdf", data_dir("browse", Some(store)));
    // The empty "Full name" offers its suggestions on hover.
    let name = entry_at(&h, 150.0, 168.0).f.clone();
    click_field_button(&mut h, &name);
    let fid = focused_id(&h).expect("the button focuses the field");
    let first = h
        .state()
        .assist
        .list_choice(fid)
        .map(str::to_string)
        .expect("list open");
    // Down and Up move through the alternatives and wrap around; the
    // highlighted one is previewed, nothing is written yet.
    key(&mut h, Key::ArrowDown);
    let second = h
        .state()
        .assist
        .list_choice(fid)
        .map(str::to_string)
        .unwrap();
    assert_ne!(first, second);
    key(&mut h, Key::ArrowUp);
    assert_eq!(h.state().assist.list_choice(fid), Some(first.as_str()));
    key(&mut h, Key::ArrowUp);
    let last = h
        .state()
        .assist
        .list_choice(fid)
        .map(str::to_string)
        .unwrap();
    assert!(
        last != first && last != second,
        "Up from the first wraps to the last"
    );
    assert_eq!(entry_at(&h, 150.0, 168.0).f.value, "");
    // Enter takes it.
    key(&mut h, Key::Enter);
    assert_eq!(entry_at(&h, 150.0, 168.0).f.value, last);
    // Escape, then Up opens the list again from the end.
    key(&mut h, Key::Escape);
    click_doc(&mut h, 300.0, 178.0);
    key(&mut h, Key::ArrowUp);
    assert!(h.state().assist.list_open());
}

#[test]
fn settings_general_section() {
    use egui_kittest::kittest::Queryable;
    let mut h = harness("flat_form.pdf", data_dir("general", None));
    h.state_mut().assist.open_settings("general");
    h.run_steps(3);
    for label in ["General", "Default PDF app", "Terminal command", "Updates"] {
        h.get_by_label(label);
    }
    // The other sections sit under the Autocomplete heading.
    h.get_by_label("AUTOCOMPLETE");
    h.state_mut().assist.open_settings("autocomplete");
    h.run_steps(3);
    h.get_by_label("When you save a filled form");
}
