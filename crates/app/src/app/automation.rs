//! Hidden command-line automation for tests and measurements: --demo,
//! --save, --screenshot (prints timings and positions to stdout).

use super::*;

impl App {
    /// `--values file.json`: `[{"page":0,"x":..,"y":..,"value":"text"|true}]`
    /// with display-space points; each value goes into the smallest field
    /// at its point (README screenshots with made-up data).
    fn fill_values(&mut self, path: &std::path::Path) {
        let list: Vec<serde_json::Value> = std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        for v in list {
            let Some(doc) = self.doc.as_ref() else { return };
            let page = v["page"].as_u64().unwrap_or(0) as usize;
            let (x, y) = (
                v["x"].as_f64().unwrap_or(0.0) as f32,
                v["y"].as_f64().unwrap_or(0.0) as f32,
            );
            let best = doc
                .entries
                .iter()
                .filter(|e| {
                    let r = e.f.rect;
                    e.f.page == page
                        && r.x0 - 2.0 <= x
                        && x <= r.x1 + 2.0
                        && r.y0 - 6.0 <= y
                        && y <= r.y1 + 6.0
                })
                .min_by(|a, b| {
                    let area = |e: &Entry| e.f.rect.width() * e.f.rect.height();
                    area(a).total_cmp(&area(b))
                })
                .map(|e| e.id);
            let Some(mut id) = best else {
                println!("PDFR_VALUES no field at {x},{y}");
                continue;
            };
            match &v["value"] {
                serde_json::Value::Bool(b) => {
                    let doc = self.doc.as_mut().unwrap();
                    let i = doc.index_of(id).unwrap();
                    doc.entries[i].f.checked = *b;
                }
                serde_json::Value::String(t) => {
                    // Like typing: what does not fit a row of boxes carries
                    // on in the next group (dates, IBANs).
                    let mut rest: String = t.clone();
                    loop {
                        let doc = self.doc.as_mut().unwrap();
                        let i = doc.index_of(id).unwrap();
                        let f = &mut doc.entries[i].f;
                        let cap = f.capacity().unwrap_or(usize::MAX);
                        let comb = f.kind.is_comb();
                        f.value = rest.chars().take(cap).collect();
                        rest = rest.chars().skip(cap).collect();
                        if rest.is_empty() || !comb {
                            break;
                        }
                        match self.adjacent_comb(id, true) {
                            Some(next) => id = next,
                            None => break,
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(doc) = self.doc.as_mut() {
            doc.dirty = true;
        }
    }

    /// Hidden CLI automation: --demo, --save, --screenshot.
    pub(super) fn automation(&mut self, ctx: &egui::Context) {
        if self.cli.demo
            || self.cli.values.is_some()
            || self.cli.save.is_some()
            || self.cli.screenshot.is_some()
        {
        } else {
            return;
        }
        ctx.request_repaint();
        let ppp = ctx.pixels_per_point();
        // No file: a screenshot of the empty window (theme pictures).
        let empty = self.cli.file.is_none() && !self.cli.demo;
        match self.auto_stage {
            0 if empty => {
                self.auto_stage = 1;
                self.auto_frames = 0;
            }
            0 => {
                let ready = self
                    .doc
                    .as_ref()
                    .is_some_and(|d| d.detected.iter().all(|x| *x))
                    && self.assist.idle();
                if ready {
                    let d = self.doc.as_ref().unwrap();
                    println!(
                        "PDFR_TIMING first_page_ms={:.0} detect_ms={:.0} pages={} fields={}",
                        d.first_page_ms.unwrap_or(-1.0),
                        d.detect_done_ms.unwrap_or(0.0),
                        d.sizes.len(),
                        d.entries.len()
                    );
                    if self.cli.demo {
                        let doc = self.doc.as_mut().unwrap();
                        let mut fields = doc.fields();
                        pdfr_core::demo::fill_demo(&mut fields);
                        for (e, f) in doc.entries.iter_mut().zip(fields) {
                            e.f = f;
                        }
                        doc.dirty = true;
                    }
                    if let Some(path) = self.cli.values.clone() {
                        self.fill_values(&path);
                    }
                    if self.cli.ask_remember {
                        let doc = self.doc.as_mut().unwrap();
                        self.assist.show_remember_dialog(doc);
                    }
                    if let Some(name) = self.cli.sign.clone() {
                        self.open_sign(false, None);
                        if let Some(d) = self.sign.as_mut() {
                            d.type_name(&name);
                        }
                    }
                    self.auto_stage = 1;
                    self.auto_frames = 0;
                }
            }
            1 => {
                self.auto_frames += 1;
                // Dialogs fade in: give them time before a screenshot.
                let settle = if self.cli.ask_remember || self.cli.sign.is_some() {
                    40
                } else {
                    5
                };
                let rendered = if empty {
                    self.auto_frames > 20
                } else {
                    self.all_visible_rendered(ppp)
                };
                if rendered && self.auto_frames > settle {
                    if let Some(path) = self.cli.screenshot.clone() {
                        let (tx, rx) = channel();
                        ctx.request_screenshot(move |img| {
                            let _ = tx.send(img);
                        });
                        self.screenshot_rx = Some(rx);
                        // Report where each visible page sits in the screenshot (physical pixels).
                        for (i, r) in self.page_rects().iter().enumerate() {
                            if r.intersects(self.viewport) {
                                println!(
                                    "PDFR_PAGE {i} {:.2} {:.2} {:.4}",
                                    r.left() * ppp,
                                    r.top() * ppp,
                                    self.zoom * ppp
                                );
                            }
                        }
                        let _ = path;
                    }
                    self.auto_stage = 2;
                }
            }
            2 => {
                if let Some(rx) = &self.screenshot_rx {
                    if let Ok(img) = rx.try_recv() {
                        let path = self.cli.screenshot.clone().unwrap();
                        let f = std::fs::File::create(&path).expect("screenshot file");
                        let mut enc = png::Encoder::new(f, img.size[0] as u32, img.size[1] as u32);
                        enc.set_color(png::ColorType::Rgba);
                        let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                        enc.write_header()
                            .unwrap()
                            .write_image_data(&bytes)
                            .unwrap();
                        println!("PDFR_SCREENSHOT {}", path.display());
                        self.screenshot_rx = None;
                    } else {
                        return;
                    }
                }
                if let Some(p) = self.cli.save.clone() {
                    let ok = self.save_to(p.clone());
                    println!("PDFR_SAVED {} {ok}", p.display());
                }
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                self.auto_stage = 3;
            }
            _ => {}
        }
    }
}
