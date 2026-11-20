//! Headless UI tests: the real App driven with simulated pointer and
//! keyboard events (egui_kittest). No window, no GPU.

use super::*;
use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use pdfr_assist::memory::Store;
use std::time::{Duration, Instant};

/// How long to wait for background work (opening, detection, text,
/// thumbnails). Generous: CI runners have two slow cores shared by every
/// test's worker threads; the waits end as soon as the work is done.
const BG_WAIT: Duration = Duration::from_secs(60);

/// A sample's path, whether or not it exists (some are optional).
fn sample_path(name: &str) -> PathBuf {
    PathBuf::from(format!(
        "{}/../../samples/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
}

/// A committed sample. Fails at once if it isn't there (some local test
/// files are git-ignored and missing on CI).
fn sample(name: &str) -> PathBuf {
    let p = sample_path(name);
    assert!(p.exists(), "sample missing: {}", p.display());
    p
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
        // Frames as on screen, so two quick clicks are a double-click.
        .with_step_dt(1.0 / 60.0)
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
        assert!(start.elapsed() < BG_WAIT, "timed out waiting for detection");
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
    if !sample_path("big_300.pdf").exists() {
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
        assert!(start.elapsed() < BG_WAIT);
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
    use egui_kittest::kittest::Queryable;
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
    // The question in the current dialog style: "Don't save" opens the
    // other file without saving.
    h.get_by_label("Save changes to flat_form.pdf?");
    h.get_by_label("Don't save").click();
    h.run_steps(2);
    assert!(h.state().confirm.is_none());
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

// ------------------------------------------------------------ keyboard-first

fn ctrl(h: &mut Harness<'_, App>, k: Key) {
    h.key_press_modifiers(Modifiers::COMMAND, k);
    h.run_steps(2);
}

fn ctrl_shift(h: &mut Harness<'_, App>, k: Key) {
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, k);
    h.run_steps(2);
}

fn value_of(h: &Harness<'_, App>, id: u64) -> String {
    let d = h.state().doc.as_ref().unwrap();
    d.entries[d.index_of(id).unwrap()].f.value.clone()
}

#[test]
fn ctrl_n_jumps_between_empty_boxes() {
    let mut h = harness("flat_form.pdf", data_dir("next-empty", None));
    click_doc(&mut h, 300.0, 178.0);
    let full = entry_at(&h, 150.0, 168.0).id;
    type_text(&mut h, "Anna");
    key(&mut h, Key::Escape);
    assert_eq!(focused_id(&h), None);
    // From the page: the first box still to fill, skipping the filled one.
    ctrl(&mut h, Key::N);
    let a = focused_id(&h).expect("an empty box is focused");
    assert_ne!(a, full);
    assert_eq!(value_of(&h, a), "");
    ctrl(&mut h, Key::N);
    let b = focused_id(&h).unwrap();
    assert_ne!(b, a);
    ctrl_shift(&mut h, Key::N);
    assert_eq!(focused_id(&h), Some(a), "Ctrl+Shift+N goes back");
    // The chord never types into the box.
    assert_eq!(value_of(&h, a), "");
}

#[test]
fn ctrl_keys_scroll_page_and_zoom() {
    // Three A4 pages (committed; tools/gen_samples.py).
    let mut h = harness("links.pdf", data_dir("ctrl-nav", None));
    assert!(h.state().doc.as_ref().unwrap().sizes.len() > 1);
    let y0 = h.state().scroll.y;
    ctrl(&mut h, Key::D);
    let y1 = h.state().scroll.y;
    assert!(y1 > y0 + 100.0, "Ctrl+D: half a page down ({y0} -> {y1})");
    ctrl(&mut h, Key::J);
    assert!(h.state().scroll.y > y1, "Ctrl+J scrolls down");
    ctrl(&mut h, Key::K);
    assert!((h.state().scroll.y - y1).abs() < 0.5, "Ctrl+K scrolls back");
    ctrl(&mut h, Key::U);
    assert!(h.state().scroll.y < y1);
    // Pages, zathura style.
    ctrl(&mut h, Key::Home); // no binding: nothing happens
    let p0 = h.state().current_page();
    ctrl_shift(&mut h, Key::J);
    assert_eq!(h.state().current_page(), p0 + 1, "Ctrl+Shift+J: next page");
    let top = h.state().page_rects()[p0 + 1].top();
    assert!((top - (h.state().viewport.top() + PAD)).abs() < 1.0);
    ctrl_shift(&mut h, Key::K);
    assert_eq!(h.state().current_page(), p0, "Ctrl+Shift+K: previous page");
    // Zoom: Ctrl+2 fits the width, Ctrl+1 and Ctrl+0 are 100%.
    ctrl(&mut h, Key::Num2);
    let fit = h.state().fit_width_zoom();
    assert!((h.state().zoom - fit).abs() < 1e-3);
    ctrl(&mut h, Key::Num1);
    assert!((h.state().zoom - 1.0).abs() < 1e-3);
    ctrl(&mut h, Key::Num2);
    ctrl(&mut h, Key::Num0);
    assert!((h.state().zoom - 1.0).abs() < 1e-3);
}

#[test]
fn ctrl_j_browses_an_open_suggestion_list() {
    let mut store = Store::default();
    store.learn("full_name", "Anna Lindqvist");
    store.learn("full_name", "Per Berg");
    let mut h = harness("flat_form.pdf", data_dir("ctrl-j-list", Some(store)));
    click_doc(&mut h, 300.0, 178.0);
    let full = entry_at(&h, 150.0, 168.0).id;
    key(&mut h, Key::ArrowDown);
    let first = h.state().assist.list_choice(full).map(str::to_string);
    assert!(first.is_some(), "Down opens the list");
    let y = h.state().scroll.y;
    ctrl(&mut h, Key::J);
    let second = h.state().assist.list_choice(full).map(str::to_string);
    assert_ne!(first, second, "Ctrl+J moves the highlight");
    assert_eq!(h.state().scroll.y, y, "and does not scroll");
    ctrl(&mut h, Key::K);
    assert_eq!(
        h.state().assist.list_choice(full).map(str::to_string),
        first
    );
    key(&mut h, Key::Enter);
    assert_eq!(Some(value_of(&h, full)), first);
}

#[test]
fn drop_down_by_keyboard() {
    let mut h = harness("real_form.pdf", data_dir("choice-keys", None));
    let (id, options, before) = {
        let d = h.state().doc.as_ref().unwrap();
        let e = d
            .entries
            .iter()
            .find(|e| matches!(e.f.kind, FieldKind::Choice { .. }))
            .expect("real_form has a drop-down");
        let FieldKind::Choice { options, .. } = &e.f.kind else {
            unreachable!()
        };
        (e.id, options.clone(), e.f.value.clone())
    };
    assert!(options.len() > 1);
    let ctx = h.ctx.clone();
    h.state_mut().focus_field(id, true, &ctx);
    h.run_steps(2);
    // Alt+Down opens any drop-down.
    h.key_press_modifiers(Modifiers::ALT, Key::ArrowDown);
    h.run_steps(2);
    assert_eq!(h.state().choice_open, Some(id));
    let start = h.state().choice_sel;
    key(&mut h, Key::ArrowDown);
    let sel = h.state().choice_sel;
    assert_eq!(sel, (start + 1) % options.len());
    key(&mut h, Key::Enter);
    assert_eq!(h.state().choice_open, None, "Enter picks and closes");
    assert_eq!(value_of(&h, id), options[sel].0);
    assert_eq!(focused_id(&h), Some(id), "focus stays on the drop-down");
    // Esc closes without changing anything.
    h.key_press_modifiers(Modifiers::ALT, Key::ArrowDown);
    h.run_steps(2);
    key(&mut h, Key::ArrowDown);
    key(&mut h, Key::Escape);
    assert_eq!(h.state().choice_open, None);
    assert_eq!(value_of(&h, id), options[sel].0);
    assert_eq!(focused_id(&h), Some(id), "first Esc only closes the list");
    // One undo step back to the original value.
    ctrl(&mut h, Key::Z);
    assert_eq!(value_of(&h, id), before);
}

#[test]
fn ctrl_l_jump_labels() {
    let mut h = harness("flat_form.pdf", data_dir("hints", None));
    click_doc(&mut h, 300.0, 178.0);
    let full = entry_at(&h, 150.0, 168.0).id;
    // Esc puts the labels away; nothing is typed into the focused box.
    ctrl(&mut h, Key::L);
    assert!(h.state().hints.is_some());
    type_text(&mut h, "z");
    assert!(h.state().hints.is_none(), "no label starts with z");
    assert_eq!(value_of(&h, full), "", "letters never reach the box");
    ctrl(&mut h, Key::L);
    key(&mut h, Key::Escape);
    assert!(h.state().hints.is_none());
    assert_eq!(focused_id(&h), Some(full), "Esc only cancels the labels");
    // Typing a label focuses its box.
    ctrl(&mut h, Key::L);
    let (label, target) = h.state().hints.as_ref().unwrap().labels[2].clone();
    for c in label.chars() {
        type_text(&mut h, &c.to_string());
    }
    h.run_steps(1);
    assert!(h.state().hints.is_none());
    assert_eq!(focused_id(&h), Some(target));
    assert_eq!(value_of(&h, full), "");
}

#[test]
fn palette_runs_commands_and_never_types_into_boxes() {
    let mut h = harness("flat_form.pdf", data_dir("palette", None));
    click_doc(&mut h, 300.0, 178.0);
    let full = entry_at(&h, 150.0, 168.0).id;
    ctrl_shift(&mut h, Key::P);
    assert!(h.state().overlay.is_some());
    type_text(&mut h, "fit width");
    h.run_steps(1);
    assert_eq!(
        value_of(&h, full),
        "",
        "palette typing stays in the palette"
    );
    key(&mut h, Key::Enter);
    h.run_steps(1);
    assert!(h.state().overlay.is_none());
    let fit = h.state().fit_width_zoom();
    assert!((h.state().zoom - fit).abs() < 1e-3, "ran Fit width");
    // Esc closes without running anything; focus returns to the box.
    ctrl(&mut h, Key::Space);
    assert!(h.state().overlay.is_some());
    key(&mut h, Key::Escape);
    assert!(h.state().overlay.is_none());
    assert_eq!(focused_id(&h), Some(full));
    type_text(&mut h, "Anna");
    assert_eq!(value_of(&h, full), "Anna", "typing goes to the box again");
    // F1 shows the cheatsheet; Esc closes it.
    key(&mut h, Key::F1);
    assert!(matches!(
        h.state().overlay,
        Some(palette::Overlay::Cheatsheet)
    ));
    key(&mut h, Key::Escape);
    assert!(h.state().overlay.is_none());
}

#[test]
fn find_box_and_go_to_page() {
    let mut h = harness("flat_form.pdf", data_dir("find-box", None));
    ctrl_shift(&mut h, Key::F);
    // Box mode lists every box; Down + Enter focuses the second one.
    key(&mut h, Key::ArrowDown);
    key(&mut h, Key::Enter);
    h.run_steps(1);
    let second = h.state().editable_ids()[1];
    assert_eq!(focused_id(&h), Some(second));
    // Ctrl+G: a page number.
    ctrl(&mut h, Key::G);
    type_text(&mut h, "1");
    key(&mut h, Key::Enter);
    h.run_steps(1);
    assert!(h.state().overlay.is_none());
    assert_eq!(h.state().current_page(), 0);
}

#[test]
fn box_editing_chords() {
    let mut h = harness("flat_form.pdf", data_dir("box-chords", None));
    click_doc(&mut h, 300.0, 178.0);
    let full = entry_at(&h, 150.0, 168.0).id;
    type_text(&mut h, "Anna Maria");
    // Ctrl+Backspace deletes a word.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Backspace);
    h.run_steps(2);
    assert_eq!(value_of(&h, full), "Anna ");
    // Ctrl+Shift+Delete removes a box pdfr found; undo brings it back.
    let n = h.state().doc.as_ref().unwrap().entries.len();
    ctrl_shift(&mut h, Key::Delete);
    assert_eq!(h.state().doc.as_ref().unwrap().entries.len(), n - 1);
    assert_eq!(focused_id(&h), None);
    ctrl(&mut h, Key::Z);
    assert_eq!(h.state().doc.as_ref().unwrap().entries.len(), n);
}

#[test]
fn esc_puts_down_a_tool_and_cancels_the_unsaved_question() {
    let mut h = harness("flat_form.pdf", data_dir("esc", None));
    h.state_mut().run(commands::Command::AddTextField);
    h.run_steps(1);
    assert!(h.state().tool == Tool::AddText);
    key(&mut h, Key::Escape);
    assert!(h.state().tool == Tool::None);
    // The unsaved-changes question: Esc is Cancel.
    click_doc(&mut h, 300.0, 178.0);
    type_text(&mut h, "Anna");
    h.state_mut().request_open(sample("real_form.pdf"));
    h.run_steps(2);
    assert!(h.state().confirm.is_some());
    key(&mut h, Key::Escape);
    assert!(h.state().confirm.is_none());
    assert_eq!(entry_at(&h, 150.0, 168.0).f.value, "Anna");
}

fn drag(h: &mut Harness<'_, App>, a: Pos2, b: Pos2) {
    h.event(Event::PointerMoved(a));
    h.event(Event::PointerButton {
        pos: a,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(1);
    for k in 1..=6 {
        let t = k as f32 / 6.0;
        h.event(Event::PointerMoved(a + (b - a) * t));
        h.run_steps(1);
    }
    h.event(Event::PointerButton {
        pos: b,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
}

fn signatures(h: &Harness<'_, App>) -> Vec<Entry> {
    let d = h.state().doc.as_ref().unwrap();
    d.entries
        .iter()
        .filter(|e| e.f.kind.is_signature())
        .cloned()
        .collect()
}

#[test]
fn sign_place_move_remove_and_undo() {
    use egui_kittest::kittest::Queryable;
    let dir = data_dir("sign", None);
    let mut h = harness("flat_form.pdf", dir.clone());
    ctrl(&mut h, Key::I);
    assert!(h.state().sign.is_some(), "Ctrl+I opens the signing dialog");
    h.state_mut()
        .sign
        .as_mut()
        .unwrap()
        .type_name("Anna Lindqvist");
    h.run_steps(2);
    h.get_by_label("Place on the page").click();
    h.run_steps(2);
    assert!(h.state().sign.is_none());
    assert!(h.state().tool == Tool::PlaceSignature);
    click_doc(&mut h, 300.0, 600.0);
    let s = signatures(&h);
    assert_eq!(s.len(), 1);
    let (cx, cy) = s[0].f.rect.center();
    assert!((cx - 300.0).abs() < 1.0 && (cy - 600.0).abs() < 1.0);
    assert!(h.state().tool == Tool::None);
    // Saved for next time, in the data folder.
    let store = Store::load(&dir.join("memory.json"));
    assert_eq!(store.signatures.len(), 1);

    // Drag it 50 points down.
    let a = screen(&h, cx, cy);
    let b = screen(&h, cx, cy + 50.0);
    drag(&mut h, a, b);
    let moved = signatures(&h)[0].f.rect;
    assert!((moved.center().1 - (cy + 50.0)).abs() < 2.0, "{moved:?}");
    // Delete removes the selected signature; undo brings it back.
    key(&mut h, Key::Delete);
    assert!(signatures(&h).is_empty());
    ctrl(&mut h, Key::Z);
    assert_eq!(signatures(&h).len(), 1);
}

#[test]
fn ctrl_f_searches_the_text_and_remembers_the_query() {
    let mut h = harness("flat_form.pdf", data_dir("search", None));
    ctrl(&mut h, Key::F);
    type_text(&mut h, "thirty days");
    h.run_steps(3);
    {
        let s = h.state().search.as_ref().unwrap();
        assert_eq!(s.hits.len(), 1, "{:?}", s.hits);
        assert_eq!(s.current, Some(0), "the first hit is shown while typing");
        assert!(s.shown);
    }
    key(&mut h, Key::Enter);
    h.run_steps(1);
    assert!(h.state().overlay.is_none());
    assert!(h.state().search.as_ref().unwrap().shown, "highlights stay");
    // Esc on the page hides them; Ctrl+F brings the query back.
    key(&mut h, Key::Escape);
    assert!(!h.state().search.as_ref().unwrap().shown);
    ctrl(&mut h, Key::F);
    match &h.state().overlay {
        Some(palette::Overlay::Palette(f)) => assert_eq!(f.query, "thirty days"),
        _ => panic!("palette not open"),
    }
    // Boxes are found by label in the same search.
    key(&mut h, Key::Escape);
    ctrl(&mut h, Key::F);
    h.event(Event::Key {
        key: Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    });
    type_text(&mut h, "Occupation");
    h.run_steps(2);
    key(&mut h, Key::Enter);
    h.run_steps(1);
    let id = focused_id(&h).expect("the Occupation box is focused");
    let d = h.state().doc.as_ref().unwrap();
    let label = d.entries[d.index_of(id).unwrap()].f.label.clone();
    assert_eq!(label.as_deref(), Some("Occupation"));
}

#[test]
fn select_page_text_and_copy() {
    let mut h = harness("flat_form.pdf", data_dir("select", None));
    // Drag along the "Full name" label (it starts at x 50, baseline 184).
    let a = screen(&h, 45.0, 180.0);
    let b = screen(&h, 140.0, 180.0);
    drag(&mut h, a, b);
    assert_eq!(h.state().selection_text().as_deref(), Some("Full name"));
    assert!(h.state().focus.is_none(), "selecting never focuses a box");
    ctrl(&mut h, Key::C);
    assert_eq!(h.state().status, "Copied 9 characters.");
    // Double-click a word; a click elsewhere drops the selection.
    let p = screen(&h, 60.0, 180.0);
    click(&mut h, p);
    click(&mut h, p);
    assert_eq!(h.state().selection_text().as_deref(), Some("Full"));
    click_doc(&mut h, 30.0, 400.0);
    assert!(h.state().text_sel.is_none());
    // Dragging that starts on a box still edits the box.
    click_doc(&mut h, 300.0, 178.0);
    assert!(focused_id(&h).is_some());
}

#[test]
fn links_jump_and_go_back() {
    use pdfr_core::links::Target;
    let mut h = harness("links.pdf", data_dir("links", None));
    assert_eq!(h.state().doc.as_ref().unwrap().form.links.len(), 2);
    // "Continue in Part C" jumps to page 3; Ctrl+Shift+G comes back.
    click_doc(&mut h, 80.0, 176.0);
    assert_eq!(h.state().current_page(), 2);
    ctrl_shift(&mut h, Key::G);
    assert_eq!(h.state().current_page(), 0);
    // The printed address is a link too (not clicked: it would open a browser).
    let p = screen(&h, 300.0, 118.0);
    let rects = h.state().page_rects();
    let l = h.state().link_at(&rects, p).expect("printed address");
    assert_eq!(
        l.target,
        Target::Uri("https://example.org/form-help".into())
    );
}

#[test]
fn sidebar_thumbnails_and_outline() {
    use egui_kittest::kittest::Queryable;
    let mut h = harness("links.pdf", data_dir("sidebar", None));
    ctrl(&mut h, Key::B);
    assert!(h.state().sidebar_open());
    // Thumbnails arrive from the workers.
    let start = Instant::now();
    while h.state().doc.as_ref().unwrap().thumbs.len() < 3 {
        h.run_steps(1);
        std::thread::sleep(Duration::from_millis(10));
        assert!(start.elapsed() < BG_WAIT, "no thumbnails");
    }
    // The outline goes to its sections.
    h.state_mut().side_tab = sidebar::SideTab::Outline;
    h.run_steps(2);
    h.get_by_label("Part C: Signatures").click();
    h.run_steps(2);
    assert_eq!(h.state().current_page(), 2);
    // Ctrl+G finds sections by name.
    ctrl(&mut h, Key::G);
    type_text(&mut h, "household");
    key(&mut h, Key::Enter);
    h.run_steps(1);
    assert_eq!(h.state().current_page(), 1);
    ctrl(&mut h, Key::B);
    assert!(!h.state().sidebar_open());
}

fn open_and_wait(h: &mut Harness<'_, App>, file: &str) {
    h.state_mut().request_open(sample(file));
    let start = Instant::now();
    loop {
        h.run_steps(1);
        let s = h.state();
        if s.doc
            .as_ref()
            .is_some_and(|d| d.path.ends_with(file) && d.detected.iter().all(|x| *x))
            && s.assist.idle()
        {
            break;
        }
        assert!(start.elapsed() < BG_WAIT, "timed out opening {file}");
        std::thread::sleep(Duration::from_millis(10));
    }
    h.run_steps(3);
}

#[test]
fn recent_files_reopen_where_left() {
    let dir = data_dir("recent", None);
    let mut h = harness("links.pdf", dir.clone());
    h.state_mut().scroll_to_page(2);
    h.run_steps(1);
    open_and_wait(&mut h, "flat_form.pdf");
    let recent = &h.state().assist.a.store.settings.recent;
    assert_eq!(recent.len(), 2);
    assert!(recent[0].path.ends_with("flat_form.pdf"));
    assert_eq!(recent[1].page, 2, "links.pdf was left on page 3");
    // Ctrl+E lists links.pdf; Enter opens it where it was left.
    ctrl(&mut h, Key::E);
    key(&mut h, Key::Enter);
    let start = Instant::now();
    while !h.state().doc.as_ref().unwrap().path.ends_with("links.pdf") {
        h.run_steps(1);
        std::thread::sleep(Duration::from_millis(10));
        assert!(start.elapsed() < BG_WAIT);
    }
    h.run_steps(3);
    assert_eq!(h.state().current_page(), 2);
    assert!(h.state().status.starts_with("Back where you left off"));
}

#[test]
fn turn_a_page_undo_and_fit_page() {
    let mut h = harness("flat_form.pdf", data_dir("rotate", None));
    // Fill the "Full name" box, then turn the page.
    click_doc(&mut h, 300.0, 178.0);
    type_text(&mut h, "Anna");
    let id = focused_id(&h).unwrap();
    let before = {
        let d = h.state().doc.as_ref().unwrap();
        d.entries[d.index_of(id).unwrap()].f.rect
    };
    key(&mut h, Key::Escape);
    ctrl(&mut h, Key::R);
    {
        let d = h.state().doc.as_ref().unwrap();
        assert_eq!(d.form.pages[0].rotate, 90);
        assert!(d.sizes[0].0 > d.sizes[0].1, "landscape now");
        let e = &d.entries[d.index_of(id).unwrap()];
        assert_eq!(e.f.value, "Anna", "typed text stays");
        assert!((e.f.rect.width() - before.width()).abs() < 0.01);
        assert!(d.dirty);
    }
    // Detection runs again on the turned page.
    let start = Instant::now();
    while !h.state().doc.as_ref().unwrap().detected[0] {
        h.run_steps(1);
        std::thread::sleep(Duration::from_millis(10));
        assert!(start.elapsed() < BG_WAIT);
    }
    ctrl(&mut h, Key::Z);
    {
        let d = h.state().doc.as_ref().unwrap();
        assert_eq!(d.form.pages[0].rotate, 0, "undo turns it back");
        let e = &d.entries[d.index_of(id).unwrap()];
        assert!((e.f.rect.x0 - before.x0).abs() < 0.01 && (e.f.rect.y0 - before.y0).abs() < 0.01);
    }
    ctrl_shift(&mut h, Key::Z);
    assert_eq!(
        h.state().doc.as_ref().unwrap().form.pages[0].rotate,
        90,
        "redo"
    );
    // Fit page: the whole page is in the window.
    ctrl(&mut h, Key::Num3);
    let s = h.state();
    let r = s.page_rects()[0];
    assert!(r.height() <= s.viewport.height() && r.width() <= s.viewport.width());
}

#[test]
fn protected_file_asks_for_its_password() {
    let cli = CliOptions {
        file: Some(sample("encrypted_user.pdf")),
        no_ai: true,
        data_dir: Some(data_dir("password", None)),
        ..Default::default()
    };
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .build_eframe(move |cc| App::new(cc, cli));
    let wait = |h: &mut Harness<'_, App>, done: &dyn Fn(&App) -> bool| {
        let start = Instant::now();
        while !done(h.state()) {
            h.run_steps(1);
            std::thread::sleep(Duration::from_millis(10));
            assert!(start.elapsed() < BG_WAIT, "timed out");
        }
    };
    wait(&mut h, &|s| s.password.is_some());
    assert!(!h.state().password.as_ref().unwrap().wrong);
    type_text(&mut h, "guess");
    key(&mut h, Key::Enter);
    wait(&mut h, &|s| s.password.as_ref().is_some_and(|p| p.wrong));
    assert!(
        h.state().password.as_ref().unwrap().input.is_empty(),
        "never kept"
    );
    h.run_steps(2);
    type_text(&mut h, "pdfr");
    key(&mut h, Key::Enter);
    wait(&mut h, &|s| s.doc.is_some());
    assert!(h.state().password.is_none());
    let d = h.state().doc.as_ref().unwrap();
    assert!(d.form.was_protected);
    assert_eq!(d.entries.iter().filter(|e| e.f.is_real()).count(), 9);
}

#[test]
fn todays_date_and_signing_every_page() {
    use pdfr_assist::dates;
    let mut h = harness("flat_form.pdf", data_dir("stamps", None));
    // Into the focused box.
    click_doc(&mut h, 300.0, 178.0);
    let id = focused_id(&h).unwrap();
    ctrl_shift(&mut h, Key::D);
    assert_eq!(dates::parse(&value_of(&h, id), None), Some(dates::today()));
    // With no box focused, a click places it.
    key(&mut h, Key::Escape);
    ctrl_shift(&mut h, Key::D);
    assert!(h.state().tool == Tool::PlaceDate);
    let n = h.state().doc.as_ref().unwrap().entries.len();
    click_doc(&mut h, 400.0, 760.0);
    let d = h.state().doc.as_ref().unwrap();
    assert_eq!(d.entries.len(), n + 1);
    let placed = d.entries.last().unwrap();
    assert_eq!(dates::parse(&placed.f.value, None), Some(dates::today()));
    assert!(placed.f.rect.contains(402.0, 760.0));

    // Initials on every page of a three-page document.
    let mut h = harness("links.pdf", data_dir("every-page", None));
    ctrl_shift(&mut h, Key::I);
    assert!(h.state().sign.as_ref().is_some_and(|s| s.initials));
    key(&mut h, Key::Escape);
    let ink = Arc::new(pdfr_core::signature::Ink::typed("AL"));
    let r = ink.fit(&PRect::new(500.0, 780.0, 560.0, 810.0));
    h.state_mut().place_signature(0, r, ink);
    let id = signatures(&h)[0].id;
    h.state_mut().sign_every_page(id);
    let pages: Vec<usize> = signatures(&h).iter().map(|e| e.f.page).collect();
    assert_eq!(pages, vec![0, 1, 2]);
    ctrl(&mut h, Key::Z);
    assert_eq!(signatures(&h).len(), 1);
}

#[test]
fn dark_pages_redraw_and_stay_on() {
    let dir = data_dir("dark", None);
    let mut h = harness("flat_form.pdf", dir.clone());
    ctrl(&mut h, Key::M);
    assert!(h.state().dark_pages() && h.state().dark_applied);
    assert_eq!(
        h.state().page_fill(),
        Color32::from_gray(pdfr_core::render::DARK_PAPER)
    );
    // The page is drawn again (dark).
    let start = Instant::now();
    while !h.state().doc.as_ref().unwrap().tex.contains_key(&0) {
        h.run_steps(1);
        std::thread::sleep(Duration::from_millis(10));
        assert!(start.elapsed() < BG_WAIT, "page not redrawn");
    }
    assert!(
        Store::load(&dir.join("memory.json")).settings.dark_pages,
        "remembered"
    );
    ctrl(&mut h, Key::M);
    assert!(!h.state().dark_pages());
}

#[test]
fn new_window_opens_here_when_the_window_is_empty() {
    let cli = CliOptions {
        no_ai: true,
        data_dir: Some(data_dir("new-window", None)),
        ..Default::default()
    };
    let mut h = Harness::builder()
        .with_size(egui::vec2(800.0, 600.0))
        .with_step_dt(1.0 / 60.0)
        .build_eframe(move |cc| App::new(cc, cli));
    h.state_mut().open_in_new_window(sample("flat_form.pdf"));
    let start = Instant::now();
    while h.state().doc.is_none() {
        h.run_steps(1);
        std::thread::sleep(Duration::from_millis(10));
        assert!(start.elapsed() < BG_WAIT);
    }
    assert!(h.state().can(commands::Command::OpenNewWindow));
}

#[test]
fn two_pages_side_by_side_and_presenting() {
    let mut h = harness("links.pdf", data_dir("present", None));
    h.state_mut().run(commands::Command::TwoPages);
    h.run_steps(2);
    {
        let s = h.state();
        let r = s.page_rects();
        assert_eq!(r[1].top(), r[2].top(), "pages 2 and 3 side by side");
        assert!(r[2].left() > r[1].right());
        assert!(r[0].bottom() < r[1].top(), "page 1 alone, like a cover");
    }
    ctrl_shift(&mut h, Key::J);
    assert_eq!(
        h.state().current_page(),
        1,
        "next page goes to the next pair"
    );
    h.state_mut().run(commands::Command::TwoPages);
    h.run_steps(1);

    let zoom = h.state().zoom;
    h.state_mut().run(commands::Command::Present);
    h.run_steps(2);
    assert!(h.state().presenting());
    key(&mut h, Key::ArrowRight);
    h.run_steps(1);
    assert_eq!(h.state().current_page(), 2);
    {
        let s = h.state();
        let r = s.page_rects()[2];
        assert!(r.height() <= s.viewport.height() + 1.0, "the page fits");
    }
    key(&mut h, Key::Escape);
    assert!(!h.state().presenting());
    assert!((h.state().zoom - zoom).abs() < 1e-3, "zoom restored");
}

#[test]
fn settings_person_details_open() {
    use egui_kittest::kittest::Queryable;
    let mut store = Store::default();
    let mut values = std::collections::BTreeMap::new();
    values.insert("full_name".to_string(), "Anna Lindqvist".to_string());
    values.insert("email".to_string(), "anna@example.se".to_string());
    store.profiles.push(pdfr_assist::profiles::Profile {
        id: 1,
        values,
        ..Default::default()
    });
    let mut h = harness("flat_form.pdf", data_dir("people", Some(store)));
    h.state_mut().assist.open_settings("people");
    h.run_steps(3);
    // Used to panic: "add_space makes no sense in a grid layout".
    h.get_by_label("Show details").click();
    h.run_steps(3);
    h.get_by_label("anna@example.se");
}

#[test]
fn ctrl_h_hides_field_tints_and_remembers() {
    let dir = data_dir("marks", None);
    let mut h = harness("flat_form.pdf", dir.clone());
    assert!(h.state().is_on(commands::Command::FieldMarks));
    ctrl(&mut h, Key::H);
    assert!(!h.state().is_on(commands::Command::FieldMarks));
    assert!(!Store::load(&dir.join("memory.json")).settings.show_fields);
    // Boxes still work without their tint.
    click_doc(&mut h, 300.0, 178.0);
    type_text(&mut h, "Anna");
    assert_eq!(value_of(&h, focused_id(&h).unwrap()), "Anna");
    ctrl(&mut h, Key::H);
    assert!(h.state().is_on(commands::Command::FieldMarks));
}

#[test]
fn start_screen_cards_open_recent_files() {
    use egui_kittest::kittest::Queryable;
    let mut store = Store::default();
    store.settings.recent.push(pdfr_assist::memory::RecentFile {
        path: std::fs::canonicalize(sample("links.pdf"))
            .unwrap()
            .to_string_lossy()
            .into(),
        page: 0,
        offset: 0.0,
        zoom: 1.0,
        opened_at: pdfr_assist::memory::now(),
    });
    let cli = CliOptions {
        no_ai: true,
        data_dir: Some(data_dir("start", Some(store))),
        ..Default::default()
    };
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .build_eframe(move |cc| App::new(cc, cli));
    h.run_steps(3);
    h.get_by_label("Recent files");
    // A real click on the card (its name label is just text).
    let at = h.get_by_label("links").rect().center();
    click(&mut h, at);
    let start = Instant::now();
    while h.state().doc.is_none() {
        h.run_steps(1);
        std::thread::sleep(Duration::from_millis(10));
        assert!(start.elapsed() < BG_WAIT, "card did not open the file");
    }
}

/// Right-click printed text → "Edit this text": a field over it, holding
/// the text, ready to type; the paper's colour covers the old print once
/// the value changes.
#[test]
fn edit_printed_text() {
    use egui_kittest::kittest::Queryable;
    let mut h = harness("panels.pdf", data_dir("edit-printed", None));
    // The label "Portal" above the row of boxes (tools/gen_samples.py).
    let p = screen(&h, 46.0, 203.0);
    h.event(Event::PointerMoved(p));
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: p,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    h.run_steps(3);
    h.get_by_label("Edit this text").click();
    h.run_steps(3);
    let id = focused_id(&h).expect("the new field has the keyboard");
    let f = &h
        .state()
        .doc
        .as_ref()
        .unwrap()
        .entries
        .iter()
        .find(|e| e.id == id)
        .unwrap()
        .f;
    assert_eq!(f.value, "Portal");
    assert!(f.is_printed());
    type_text(&mut h, "s");
    let f = &h
        .state()
        .doc
        .as_ref()
        .unwrap()
        .entries
        .iter()
        .find(|e| e.id == id)
        .unwrap()
        .f;
    assert_eq!(f.value, "Portals");
    assert!(f.covers_print());
}
