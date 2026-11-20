//! Toolbar icons, drawn as vector strokes in the theme's text colour. Sharp
//! at any display scale, no image files, and they follow theme changes.
//!
//! Every icon is designed on a 24×24 grid (y down) and scaled into the
//! target rectangle. To add one: add a variant and its strokes in `paint`.

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Open,
    Save,
    SaveAs,
    Print,
    Undo,
    Redo,
    ZoomIn,
    ZoomOut,
    ActualSize,
    FitWidth,
    TextField,
    Checkbox,
    FillAll,
    Settings,
    Close,
    Trash,
    Person,
    List,
    Sparkle,
    Storage,
    Search,
    Brain,
    Key,
    Signature,
    Sidebar,
    FitPage,
    Rotate,
    Moon,
    Highlight,
}

/// Maps the 24×24 design grid into a rectangle.
struct Grid {
    origin: Pos2,
    s: f32,
}

impl Grid {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.origin.x + x * self.s, self.origin.y + y * self.s)
    }
    fn pts(&self, v: &[(f32, f32)]) -> Vec<Pos2> {
        v.iter().map(|&(x, y)| self.p(x, y)).collect()
    }
}

/// Draws `icon` centred in `rect`.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let side = rect.width().min(rect.height());
    let g = Grid {
        origin: rect.center() - vec2(side, side) / 2.0,
        s: side / 24.0,
    };
    // Thin, like SF Symbols at regular weight.
    let w = (1.5 * g.s).max(1.0);
    let st = Stroke::new(w, color);
    let line = |pts: &[(f32, f32)]| painter.add(Shape::line(g.pts(pts), st));
    let closed = |pts: &[(f32, f32)]| painter.add(Shape::closed_line(g.pts(pts), st));
    let circle = |x: f32, y: f32, r: f32| painter.circle_stroke(g.p(x, y), r * g.s, st);
    // A rounded rectangle outline.
    let rrect = |x: f32, y: f32, rw: f32, rh: f32, r: f32| {
        closed(&rounded(x, y, rw, rh, r));
    };
    // An open arc around (cx, cy), angles in radians (y down).
    let arc = |cx: f32, cy: f32, r: f32, a0: f32, a1: f32| -> Vec<(f32, f32)> {
        let n = ((a1 - a0).abs() * r * 1.5).ceil().max(6.0) as usize;
        (0..=n)
            .map(|k| {
                let a = a0 + (a1 - a0) * k as f32 / n as f32;
                (cx + r * a.cos(), cy + r * a.sin())
            })
            .collect()
    };
    let magnifier = |plus: bool, minus: bool| {
        circle(10.0, 10.0, 6.0);
        line(&[(14.5, 14.5), (20.0, 20.0)]);
        if plus || minus {
            line(&[(7.0, 10.0), (13.0, 10.0)]);
        }
        if plus {
            line(&[(10.0, 7.0), (10.0, 13.0)]);
        }
    };
    use std::f32::consts::PI;
    match icon {
        Icon::Open => {
            // A folder, its tab on the left.
            closed(&smooth_corners(&[
                (3.0, 7.0),
                (3.0, 5.5),
                (9.0, 5.5),
                (11.0, 7.5),
                (21.0, 7.5),
                (21.0, 19.0),
                (3.0, 19.0),
            ]));
            line(&[(3.0, 10.0), (21.0, 10.0)]);
        }
        Icon::Save => {
            // An arrow down into a tray.
            line(&[(12.0, 3.5), (12.0, 14.0)]);
            line(&[(8.0, 10.0), (12.0, 14.0), (16.0, 10.0)]);
            let mut tray = vec![(5.0, 13.5)];
            tray.extend(arc(7.0, 18.0, 2.0, PI, PI / 2.0));
            tray.extend(arc(17.0, 18.0, 2.0, PI / 2.0, 0.0));
            tray.push((19.0, 13.5));
            line(&tray);
        }
        Icon::SaveAs => {
            // A page with a pencil writing on it.
            let mut page = vec![(13.0, 4.0)];
            page.extend(arc(6.0, 6.0, 2.0, -PI / 2.0, -PI));
            page.extend(arc(6.0, 18.0, 2.0, PI, PI / 2.0));
            page.extend(arc(18.0, 18.0, 2.0, PI / 2.0, 0.0));
            page.push((20.0, 11.0));
            line(&page);
            closed(&[
                (10.0, 14.0),
                (10.6, 11.4),
                (18.6, 3.4),
                (20.6, 5.4),
                (12.6, 13.4),
            ]);
        }
        Icon::Print => {
            // A printer: paper in, a rounded body, paper out.
            line(&[(7.0, 9.0), (7.0, 4.5), (17.0, 4.5), (17.0, 9.0)]);
            let mut body = vec![(7.0, 17.0)];
            body.extend(arc(5.5, 14.5, 2.5, PI / 2.0, PI));
            body.extend(arc(5.5, 11.5, 2.5, PI, 1.5 * PI));
            body.extend(arc(18.5, 11.5, 2.5, 1.5 * PI, 2.0 * PI));
            body.extend(arc(18.5, 14.5, 2.5, 0.0, PI / 2.0));
            body.push((17.0, 17.0));
            line(&body);
            rrect(7.0, 13.5, 10.0, 6.5, 1.0);
            painter.circle_filled(g.p(17.6, 11.6), 0.8 * g.s, color);
        }
        Icon::Undo => {
            // A U-turn arrow back to the left.
            line(&[(8.5, 13.5), (4.0, 9.0), (8.5, 4.5)]);
            let mut p = vec![(4.0, 9.0), (14.5, 9.0)];
            p.extend(arc(14.5, 14.5, 5.5, -PI / 2.0, PI / 2.0));
            p.push((11.0, 20.0));
            line(&p);
        }
        Icon::Redo => {
            line(&[(15.5, 13.5), (20.0, 9.0), (15.5, 4.5)]);
            let mut p = vec![(20.0, 9.0), (9.5, 9.0)];
            p.extend(arc(9.5, 14.5, 5.5, -PI / 2.0, -1.5 * PI));
            p.push((13.0, 20.0));
            line(&p);
        }
        Icon::ZoomIn => magnifier(true, false),
        Icon::ZoomOut => magnifier(false, true),
        Icon::ActualSize => {
            rrect(3.0, 5.0, 18.0, 14.0, 2.5);
            // "1:1"
            line(&[(7.5, 10.0), (9.0, 9.0), (9.0, 15.0)]);
            line(&[(14.5, 10.0), (16.0, 9.0), (16.0, 15.0)]);
            painter.circle_filled(g.p(12.0, 10.5), 0.8 * g.s, color);
            painter.circle_filled(g.p(12.0, 13.8), 0.8 * g.s, color);
        }
        Icon::FitWidth => {
            rrect(3.0, 5.0, 18.0, 14.0, 2.5);
            line(&[(7.0, 12.0), (17.0, 12.0)]);
            line(&[(9.2, 9.8), (7.0, 12.0), (9.2, 14.2)]);
            line(&[(14.8, 9.8), (17.0, 12.0), (14.8, 14.2)]);
        }
        Icon::TextField => {
            // A text box with a T and a caret.
            rrect(3.0, 6.0, 18.0, 12.0, 2.5);
            line(&[(7.0, 10.0), (12.5, 10.0)]);
            line(&[(9.75, 10.0), (9.75, 15.0)]);
            line(&[(15.5, 9.0), (15.5, 15.0)]);
        }
        Icon::Checkbox => {
            rrect(4.0, 4.0, 16.0, 16.0, 4.0);
            line(&[(8.0, 12.3), (10.8, 15.1), (16.0, 9.6)]);
        }
        Icon::FillAll => {
            // A wand with sparkles.
            let wand = Stroke::new(w * 1.3, color);
            painter.add(Shape::line(g.pts(&[(4.0, 20.0), (15.0, 9.0)]), wand));
            let spark = |x: f32, y: f32, r: f32| {
                line(&[(x - r, y), (x + r, y)]);
                line(&[(x, y - r), (x, y + r)]);
            };
            spark(18.0, 6.0, 2.5);
            spark(12.0, 4.0, 1.5);
            spark(20.0, 13.0, 1.5);
        }
        Icon::Signature => {
            // A signature's flourish over the line it signs on.
            line(&[(3.5, 20.0), (20.5, 20.0)]);
            line(&smooth(&[
                (3.5, 16.5),
                (5.5, 13.0),
                (7.0, 9.0),
                (8.8, 7.6),
                (9.4, 10.5),
                (8.6, 14.0),
                (9.6, 14.6),
                (11.6, 12.4),
                (13.6, 11.4),
                (14.6, 13.4),
                (16.2, 13.8),
                (20.5, 12.5),
            ]));
        }
        Icon::Highlight => {
            // A field's dashed outline with a line of text: show the fields.
            let pts = g.pts(&rounded(3.0, 7.0, 18.0, 10.0, 2.5));
            let mut pts = pts;
            pts.push(pts[0]);
            painter.extend(Shape::dashed_line(&pts, st, 2.6 * g.s, 2.4 * g.s));
            line(&[(7.5, 12.0), (13.5, 12.0)]);
        }
        Icon::Moon => {
            // A crescent: an outer arc and an inner one.
            let arc = |cx: f32, cy: f32, r: f32, a0: f32, a1: f32| -> Vec<(f32, f32)> {
                (0..=16)
                    .map(|i| {
                        let a = a0 + (a1 - a0) * i as f32 / 16.0;
                        (cx + r * a.cos(), cy + r * a.sin())
                    })
                    .collect()
            };
            let pi = std::f32::consts::PI;
            let mut pts = arc(12.0, 12.0, 8.0, -0.35 * pi, 1.35 * pi);
            let mut inner = arc(15.5, 9.5, 6.5, 1.12 * pi, -0.12 * pi);
            pts.append(&mut inner);
            closed(&pts);
        }
        Icon::FitPage => {
            // A page inside rounded corner marks.
            rrect(7.5, 6.5, 9.0, 11.0, 1.5);
            line(&[(3.5, 8.0), (3.5, 5.5)]);
            line(&arc(5.5, 5.5, 2.0, PI, 1.5 * PI));
            line(&[(5.5, 3.5), (8.0, 3.5)]);
            line(&[(16.0, 3.5), (18.5, 3.5)]);
            line(&arc(18.5, 5.5, 2.0, 1.5 * PI, 2.0 * PI));
            line(&[(20.5, 5.5), (20.5, 8.0)]);
            line(&[(20.5, 16.0), (20.5, 18.5)]);
            line(&arc(18.5, 18.5, 2.0, 0.0, PI / 2.0));
            line(&[(18.5, 20.5), (16.0, 20.5)]);
            line(&[(8.0, 20.5), (5.5, 20.5)]);
            line(&arc(5.5, 18.5, 2.0, PI / 2.0, PI));
            line(&[(3.5, 18.5), (3.5, 16.0)]);
        }
        Icon::Rotate => {
            // A clockwise arrow.
            line(&arc(12.0, 12.0, 7.5, -0.25 * PI, 1.75 * PI - 0.35));
            line(&[(19.5, 3.5), (19.5, 8.0), (15.0, 8.0)]);
        }
        Icon::Sidebar => {
            rrect(3.0, 4.5, 18.0, 15.0, 3.0);
            line(&[(9.5, 4.5), (9.5, 19.5)]);
            line(&[(5.5, 8.5), (7.5, 8.5)]);
            line(&[(5.5, 11.0), (7.5, 11.0)]);
        }
        Icon::Close => {
            line(&[(6.0, 6.0), (18.0, 18.0)]);
            line(&[(6.0, 18.0), (18.0, 6.0)]);
        }
        Icon::Trash => {
            line(&[(4.0, 6.5), (20.0, 6.5)]);
            line(&[(9.5, 6.5), (9.5, 4.0), (14.5, 4.0), (14.5, 6.5)]);
            line(&[(6.0, 6.5), (7.0, 20.0), (17.0, 20.0), (18.0, 6.5)]);
            line(&[(10.0, 10.0), (10.0, 16.5)]);
            line(&[(14.0, 10.0), (14.0, 16.5)]);
        }
        Icon::Person => {
            circle(12.0, 8.0, 4.0);
            let n = 16;
            let pts: Vec<(f32, f32)> = (0..=n)
                .map(|k| {
                    let a = std::f32::consts::PI * (1.0 + k as f32 / n as f32);
                    (12.0 + 7.5 * a.cos(), 21.0 + 7.5 * a.sin())
                })
                .collect();
            line(&pts);
        }
        Icon::List => {
            for y in [6.0, 12.0, 18.0] {
                painter.circle_filled(g.p(5.0, y), 1.2 * g.s, color);
                line(&[(9.0, y), (20.0, y)]);
            }
        }
        Icon::Sparkle => {
            // A four-pointed star and a small one.
            closed(&[
                (11.0, 3.0),
                (13.0, 9.0),
                (19.0, 11.0),
                (13.0, 13.0),
                (11.0, 19.0),
                (9.0, 13.0),
                (3.0, 11.0),
                (9.0, 9.0),
            ]);
            line(&[(18.5, 16.0), (18.5, 22.0)]);
            line(&[(15.5, 19.0), (21.5, 19.0)]);
        }
        Icon::Storage => {
            // A database cylinder.
            let ellipse = |cy: f32, half: bool| {
                let n = 20;
                let (a0, a1) = if half {
                    (0.0, std::f32::consts::PI)
                } else {
                    (0.0, std::f32::consts::TAU)
                };
                let pts: Vec<(f32, f32)> = (0..=n)
                    .map(|k| {
                        let a = a0 + (a1 - a0) * k as f32 / n as f32;
                        (12.0 + 7.5 * a.cos(), cy + 2.8 * a.sin())
                    })
                    .collect();
                line(&pts);
            };
            ellipse(6.0, false);
            ellipse(12.0, true);
            ellipse(18.0, true);
            line(&[(4.5, 6.0), (4.5, 18.0)]);
            line(&[(19.5, 6.0), (19.5, 18.0)]);
        }
        Icon::Search => magnifier(false, false),
        Icon::Key => {
            // A round bow and a shaft with two teeth.
            circle(7.5, 16.5, 4.0);
            line(&[(10.4, 13.6), (20.0, 4.0)]);
            line(&[(17.0, 7.0), (19.5, 9.5)]);
            line(&[(14.5, 9.5), (16.5, 11.5)]);
        }
        Icon::Brain => {
            // Two hemispheres seen from the front, each with a few folds;
            // the left half is drawn and mirrored.
            let outline = [
                (12.0, 4.8),
                (9.6, 3.6),
                (7.2, 4.4),
                (6.2, 6.8),
                (4.0, 8.2),
                (3.6, 11.4),
                (5.0, 13.2),
                (3.9, 15.4),
                (4.9, 18.2),
                (7.6, 19.8),
                (10.4, 19.7),
                (12.0, 18.6),
            ];
            let folds: [&[(f32, f32)]; 2] = [
                &[(6.2, 6.8), (8.0, 7.9), (8.8, 9.6)],
                &[(5.0, 13.2), (7.4, 13.0), (8.8, 14.6)],
            ];
            for mirror in [false, true] {
                let m = |v: &[(f32, f32)]| -> Vec<(f32, f32)> {
                    let pts: Vec<(f32, f32)> = v
                        .iter()
                        .map(|&(x, y)| if mirror { (24.0 - x, y) } else { (x, y) })
                        .collect();
                    smooth(&pts)
                };
                line(&m(&outline));
                for f in folds {
                    line(&m(f));
                }
            }
            line(&[(12.0, 4.8), (12.0, 18.6)]);
        }
        Icon::Settings => {
            // A gear: one outline with eight rounded teeth, and the hub.
            let pts: Vec<(f32, f32)> = (0..96)
                .map(|i| {
                    let a = i as f32 / 96.0 * std::f32::consts::TAU;
                    let tooth = ((a * 8.0).cos() + 1.0) / 2.0;
                    let r = 7.0 + 2.0 * ((tooth - 0.25) * 2.0).clamp(0.0, 1.0);
                    (12.0 + r * a.cos(), 12.0 + r * a.sin())
                })
                .collect();
            closed(&pts);
            circle(12.0, 12.0, 3.0);
        }
    }
}

/// A smooth curve through `pts` (Catmull-Rom, 6 steps per segment).
fn smooth(pts: &[(f32, f32)]) -> Vec<(f32, f32)> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let at = |i: isize| pts[i.clamp(0, pts.len() as isize - 1) as usize];
    let mut out = Vec::new();
    for i in 0..pts.len() as isize - 1 {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        for k in 0..6 {
            let t = k as f32 / 6.0;
            let (t2, t3) = (t * t, t * t * t);
            let f = |a: f32, b: f32, c: f32, d: f32| {
                0.5 * (2.0 * b
                    + (c - a) * t
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
                    + (3.0 * b - a - 3.0 * c + d) * t3)
            };
            out.push((f(p0.0, p1.0, p2.0, p3.0), f(p0.1, p1.1, p2.1, p3.1)));
        }
    }
    out.push(*pts.last().unwrap());
    out
}

/// The outline of a rounded rectangle, clockwise from the top-left corner.
fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Vec<(f32, f32)> {
    use std::f32::consts::PI;
    let mut out = Vec::new();
    for (cx, cy, a0) in [
        (x + r, y + r, PI),
        (x + w - r, y + r, 1.5 * PI),
        (x + w - r, y + h - r, 0.0),
        (x + r, y + h - r, 0.5 * PI),
    ] {
        for k in 0..=6 {
            let a = a0 + 0.5 * PI * k as f32 / 6.0;
            out.push((cx + r * a.cos(), cy + r * a.sin()));
        }
    }
    out
}

/// Rounds every corner of a polygon a little (radius 1.6), for folder-like
/// outlines.
fn smooth_corners(pts: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let n = pts.len();
    let mut out = Vec::new();
    for i in 0..n {
        let (p, c, q) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
        let d = |a: (f32, f32), b: (f32, f32)| ((b.0 - a.0).hypot(b.1 - a.1)).max(1e-3);
        let r = 1.6f32.min(d(p, c) / 2.0).min(d(c, q) / 2.0);
        let a = (
            c.0 + (p.0 - c.0) / d(p, c) * r,
            c.1 + (p.1 - c.1) / d(p, c) * r,
        );
        let b = (
            c.0 + (q.0 - c.0) / d(c, q) * r,
            c.1 + (q.1 - c.1) / d(c, q) * r,
        );
        // A quadratic curve from a through the corner to b.
        for k in 0..=4 {
            let t = k as f32 / 4.0;
            let u = 1.0 - t;
            out.push((
                u * u * a.0 + 2.0 * u * t * c.0 + t * t * b.0,
                u * u * a.1 + 2.0 * u * t * c.1 + t * t * b.1,
            ));
        }
    }
    out
}
