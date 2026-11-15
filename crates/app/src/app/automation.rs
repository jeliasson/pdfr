//! Hidden command-line automation for tests and measurements: --demo,
//! --save, --screenshot (prints timings and positions to stdout).

use super::*;

impl App {
    /// Hidden CLI automation: --demo, --save, --screenshot.
    pub(super) fn automation(&mut self, ctx: &egui::Context) {
        if self.cli.demo || self.cli.save.is_some() || self.cli.screenshot.is_some() {
        } else {
            return;
        }
        ctx.request_repaint();
        let ppp = ctx.pixels_per_point();
        match self.auto_stage {
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
                    if self.cli.ask_remember {
                        let doc = self.doc.as_mut().unwrap();
                        self.assist.show_remember_dialog(doc);
                    }
                    self.auto_stage = 1;
                    self.auto_frames = 0;
                }
            }
            1 => {
                self.auto_frames += 1;
                // Dialogs fade in: give them time before a screenshot.
                let settle = if self.cli.ask_remember { 40 } else { 5 };
                if self.all_visible_rendered(ppp) && self.auto_frames > settle {
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
