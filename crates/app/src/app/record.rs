//! Hidden demo recorder for the README videos: `--record <dir> --script
//! <name>` plays a scripted scene (pointer moves, clicks, typing at a human
//! pace) by feeding input events to the app, draws a visible pointer, and
//! saves every frame with its time. `tools/make_demo.sh` turns the frames
//! into a video. Development only; never used in normal runs.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// One step of a scene. Positions are page-1 points (as in the PDF view).
#[derive(Clone, Debug)]
pub enum Step {
    Wait(f32),
    /// Glide the pointer to a page point over `secs`.
    MoveTo(f32, f32, f32),
    Click,
    /// Type text at `chars per second`.
    Type(&'static str, f32),
    Press(Key),
}

pub fn script(name: &str) -> Option<Vec<Step>> {
    use Step::*;
    Some(match name {
        // On the scanned household form (samples/household_scanned.pdf):
        // click and type; text lands in the boxes, one letter per square,
        // the date flowing across its three groups.
        "fill" => vec![
            Wait(0.8),
            MoveTo(150.0, 158.0, 0.7),
            Wait(0.2),
            Click,
            Type("Anna Lindqvist", 11.0),
            Wait(0.4),
            MoveTo(358.0, 159.0, 0.5),
            Click,
            Type("12031985", 6.0),
            Wait(0.4),
            MoveTo(60.0, 193.0, 0.6),
            Click,
            Type("198503124328", 8.0),
            Wait(0.4),
            MoveTo(409.5, 303.5, 0.6),
            Wait(0.2),
            Click,
            Wait(0.4),
            MoveTo(150.0, 232.0, 0.6),
            Click,
            Type("Storgatan 12", 10.0),
            Wait(1.5),
        ],
        // Remembered people: the hover button, browsing alternatives, a
        // suggestion taken with Tab, and clearing.
        "remember" => vec![
            Wait(0.8),
            // Hover the empty name: the sparkle button appears; open the list.
            MoveTo(150.0, 158.0, 0.7),
            Wait(0.4),
            MoveTo(329.0, 158.0, 0.5),
            Wait(0.5),
            Click,
            Wait(0.9),
            // Browse: each one is previewed in the field.
            Press(Key::ArrowDown),
            Wait(0.9),
            Press(Key::ArrowUp),
            Wait(0.7),
            Press(Key::Enter),
            Wait(0.7),
            // The e-mail already knows this person: Tab takes it.
            MoveTo(150.0, 302.0, 0.6),
            Wait(0.2),
            Click,
            Wait(1.1),
            Press(Key::Tab),
            Wait(0.8),
            // The clear button empties a filled field.
            MoveTo(329.0, 158.0, 0.7),
            Wait(0.6),
            Click,
            Wait(1.5),
        ],
        _ => return None,
    })
}

pub struct Recorder {
    steps: Vec<Step>,
    dir: PathBuf,
    /// Current step, when it started, and the pointer position then.
    at: usize,
    step_start: Option<Instant>,
    from: Pos2,
    typed: usize,
    pub pointer: Pos2,
    start: Option<Instant>,
    finished_at: Option<Instant>,
    sent: usize,
    written: Arc<AtomicUsize>,
    times: Arc<Mutex<Vec<(usize, f64)>>>,
}

impl Recorder {
    pub fn new(steps: Vec<Step>, dir: PathBuf) -> Recorder {
        let _ = std::fs::create_dir_all(&dir);
        Recorder {
            steps,
            dir,
            at: 0,
            step_start: None,
            from: pos2(900.0, 120.0),
            typed: 0,
            pointer: pos2(900.0, 120.0),
            start: None,
            finished_at: None,
            sent: 0,
            written: Arc::new(AtomicUsize::new(0)),
            times: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl App {
    fn recording_ready(&self) -> bool {
        self.doc
            .as_ref()
            .is_some_and(|d| d.detected.iter().all(|x| *x) && !d.tex.is_empty())
            && self.assist.idle()
    }

    /// Feeds the scene's input for this frame (called before the frame).
    pub(super) fn record_input(&mut self, raw: &mut egui::RawInput) {
        if self.rec.is_none() || !self.recording_ready() {
            return;
        }
        let page = self.page_rects().first().copied();
        let zoom = self.zoom;
        let rec = self.rec.as_mut().unwrap();
        let now = Instant::now();
        rec.start.get_or_insert(now);
        let to_screen = |x: f32, y: f32| {
            page.map_or(pos2(x, y), |r| {
                pos2(r.left() + x * zoom, r.top() + y * zoom)
            })
        };
        'step: {
            let Some(step) = rec.steps.get(rec.at).cloned() else {
                rec.finished_at.get_or_insert(now);
                break 'step;
            };
            let started = *rec.step_start.get_or_insert_with(|| {
                rec.from = rec.pointer;
                now
            });
            let t = now.duration_since(started).as_secs_f32();
            let done = match step {
                Step::Wait(s) => t >= s,
                Step::MoveTo(x, y, s) => {
                    let k = (t / s).min(1.0);
                    let e = k * k * (3.0 - 2.0 * k); // ease in-out
                    let target = to_screen(x, y);
                    rec.pointer = rec.from + (target - rec.from) * e;
                    raw.events.push(egui::Event::PointerMoved(rec.pointer));
                    k >= 1.0
                }
                Step::Click => {
                    for pressed in [true, false] {
                        raw.events.push(egui::Event::PointerButton {
                            pos: rec.pointer,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Modifiers::NONE,
                        });
                    }
                    true
                }
                Step::Type(text, cps) => {
                    let due = ((t * cps) as usize + 1).min(text.chars().count());
                    for c in text.chars().skip(rec.typed).take(due - rec.typed) {
                        raw.events.push(egui::Event::Text(c.to_string()));
                    }
                    rec.typed = due;
                    due >= text.chars().count()
                }
                Step::Press(key) => {
                    for pressed in [true, false] {
                        raw.events.push(egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers: Modifiers::NONE,
                        });
                    }
                    true
                }
            };
            if !done {
                break 'step;
            }
            rec.at += 1;
            rec.step_start = None;
            rec.typed = 0;
            // One step per frame: a click must take effect (focus) before
            // the next keystroke arrives.
        }
    }

    /// Draws the pointer and saves this frame; closes the app when done.
    pub(super) fn record_frame(&mut self, ctx: &egui::Context) {
        let Some(rec) = &mut self.rec else { return };
        ctx.request_repaint();
        let Some(start) = rec.start else { return };
        draw_pointer(ctx, rec.pointer);
        // Keep recording a moment after the last step, then finish once
        // every frame is on disk.
        if let Some(end) = rec.finished_at
            && end.elapsed() > Duration::from_millis(300)
        {
            if rec.written.load(Ordering::SeqCst) >= rec.sent {
                let mut times = rec.times.lock().unwrap().clone();
                times.sort_by_key(|t| t.0);
                write_concat(&rec.dir, &times);
                println!(
                    "PDFR_RECORDED {} frames in {}",
                    times.len(),
                    rec.dir.display()
                );
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }
        let idx = rec.sent;
        rec.sent += 1;
        let t = start.elapsed().as_secs_f64();
        let (dir, written, times) = (rec.dir.clone(), rec.written.clone(), rec.times.clone());
        ctx.request_screenshot(move |img| {
            std::thread::spawn(move || {
                let path = dir.join(format!("frame_{idx:05}.png"));
                if let Ok(f) = std::fs::File::create(&path) {
                    let mut enc = png::Encoder::new(
                        std::io::BufWriter::new(f),
                        img.size[0] as u32,
                        img.size[1] as u32,
                    );
                    enc.set_color(png::ColorType::Rgba);
                    enc.set_compression(png::Compression::Fast);
                    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                    if let Ok(mut w) = enc.write_header() {
                        let _ = w.write_image_data(&bytes);
                    }
                }
                times.lock().unwrap().push((idx, t));
                written.fetch_add(1, Ordering::SeqCst);
            });
        });
    }
}

/// An arrow pointer like the system's, with a white outline.
pub(super) fn draw_pointer(ctx: &egui::Context, p: Pos2) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        Id::unique("pdfr-record-pointer"),
    ));
    let s = 1.15;
    let pts: Vec<Pos2> = [
        (0.0, 0.0),
        (0.0, 17.0),
        (4.2, 13.2),
        (7.0, 19.5),
        (9.6, 18.4),
        (6.9, 12.3),
        (12.2, 12.3),
    ]
    .iter()
    .map(|&(x, y)| p + vec2(x * s, y * s))
    .collect();
    // Not convex: an outline, then the fill as triangles.
    painter.add(egui::Shape::Path(egui::epaint::PathShape::closed_line(
        pts.clone(),
        Stroke::new(3.0, Color32::WHITE),
    )));
    let mesh_tris = [(0, 1, 2), (0, 2, 5), (0, 5, 6), (2, 3, 4), (2, 4, 5)];
    let mut mesh = egui::Mesh::default();
    for q in &pts {
        mesh.colored_vertex(*q, Color32::BLACK);
    }
    for (a, b, c) in mesh_tris {
        mesh.add_triangle(a, b, c);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// ffmpeg concat list with each frame's display time, so the video plays at
/// the real speed even when frames were uneven.
fn write_concat(dir: &std::path::Path, times: &[(usize, f64)]) {
    let mut s = String::from("ffconcat version 1.0\n");
    for (i, (idx, t)) in times.iter().enumerate() {
        let next = times.get(i + 1).map_or(t + 1.0 / 30.0, |n| n.1);
        s.push_str(&format!(
            "file 'frame_{idx:05}.png'\nduration {:.4}\n",
            (next - t).max(0.001)
        ));
    }
    if let Some((idx, _)) = times.last() {
        s.push_str(&format!("file 'frame_{idx:05}.png'\n"));
    }
    let _ = std::fs::write(dir.join("frames.ffconcat"), s);
}
