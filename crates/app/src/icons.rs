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
    let w = (1.7 * g.s).max(1.0);
    let st = Stroke::new(w, color);
    let line = |pts: &[(f32, f32)]| painter.add(Shape::line(g.pts(pts), st));
    let closed = |pts: &[(f32, f32)]| painter.add(Shape::closed_line(g.pts(pts), st));
    let circle = |x: f32, y: f32, r: f32| painter.circle_stroke(g.p(x, y), r * g.s, st);
    let fill =
        |pts: &[(f32, f32)]| painter.add(Shape::convex_polygon(g.pts(pts), color, Stroke::NONE));
    // A curved arrow: an arc from `a0` to `a1` (radians, y down) around
    // (12, 13) with an arrowhead at the end.
    let arc_arrow = |a0: f32, a1: f32| {
        let (cx, cy, r) = (12.0, 13.0, 6.5);
        let n = 14;
        let pts: Vec<(f32, f32)> = (0..=n)
            .map(|k| {
                let a = a0 + (a1 - a0) * k as f32 / n as f32;
                (cx + r * a.cos(), cy + r * a.sin())
            })
            .collect();
        line(&pts);
        let (ex, ey) = (cx + r * a0.cos(), cy + r * a0.sin());
        // Arrowhead at the start point, pointing along the tangent.
        let dir = if a1 > a0 { -1.0 } else { 1.0 };
        let (tx, ty) = (-a0.sin() * dir, a0.cos() * dir);
        let (nx, ny) = (-ty, tx);
        fill(&[
            (ex + tx * 3.5, ey + ty * 3.5),
            (ex + nx * 2.8, ey + ny * 2.8),
            (ex - nx * 2.8, ey - ny * 2.8),
        ]);
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
    let floppy = || {
        closed(&[
            (4.0, 4.0),
            (17.0, 4.0),
            (20.0, 7.0),
            (20.0, 20.0),
            (4.0, 20.0),
        ]);
        closed(&[(8.0, 4.0), (8.0, 9.0), (15.0, 9.0), (15.0, 4.0)]);
        closed(&[(7.0, 13.5), (17.0, 13.5), (17.0, 20.0), (7.0, 20.0)]);
    };
    match icon {
        Icon::Open => {
            // Folder, open.
            line(&[
                (3.0, 19.0),
                (3.0, 6.0),
                (9.0, 6.0),
                (11.0, 8.0),
                (18.0, 8.0),
                (18.0, 11.0),
            ]);
            closed(&[(3.0, 19.0), (6.5, 11.0), (21.5, 11.0), (18.0, 19.0)]);
        }
        Icon::Save => floppy(),
        Icon::SaveAs => {
            floppy();
            // A pencil over the corner.
            let wpen = Stroke::new(w * 1.1, color);
            painter.add(Shape::line(
                g.pts(&[
                    (13.0, 22.0),
                    (14.0, 18.5),
                    (21.5, 11.0),
                    (24.0, 13.5),
                    (16.5, 21.0),
                    (13.0, 22.0),
                ]),
                wpen,
            ));
        }
        Icon::Print => {
            // A printer: paper in, body, paper out.
            closed(&[(7.0, 3.5), (17.0, 3.5), (17.0, 8.5), (7.0, 8.5)]);
            // The body, open at the bottom where the paper comes out.
            line(&[
                (7.0, 16.5),
                (3.5, 16.5),
                (3.5, 8.5),
                (20.5, 8.5),
                (20.5, 16.5),
                (17.0, 16.5),
            ]);
            closed(&[(7.0, 13.5), (17.0, 13.5), (17.0, 20.5), (7.0, 20.5)]);
            line(&[(9.0, 17.0), (15.0, 17.0)]);
            painter.circle_filled(g.p(17.5, 11.2), 0.9 * g.s, color);
        }
        Icon::Undo => arc_arrow(std::f32::consts::PI, std::f32::consts::TAU),
        Icon::Redo => arc_arrow(std::f32::consts::TAU, std::f32::consts::PI),
        Icon::ZoomIn => magnifier(true, false),
        Icon::ZoomOut => magnifier(false, true),
        Icon::ActualSize => {
            closed(&[(4.0, 5.0), (20.0, 5.0), (20.0, 19.0), (4.0, 19.0)]);
            // "1:1"
            line(&[(8.0, 9.5), (9.5, 8.5), (9.5, 15.5)]);
            line(&[(14.5, 8.5), (16.0, 7.5), (16.0, 15.5)]);
            painter.circle_filled(g.p(12.0, 10.0), 0.9 * g.s, color);
            painter.circle_filled(g.p(12.0, 14.0), 0.9 * g.s, color);
        }
        Icon::FitWidth => {
            closed(&[(6.0, 3.0), (18.0, 3.0), (18.0, 21.0), (6.0, 21.0)]);
            line(&[(2.0, 12.0), (22.0, 12.0)]);
            fill(&[(1.0, 12.0), (4.5, 9.5), (4.5, 14.5)]);
            fill(&[(23.0, 12.0), (19.5, 9.5), (19.5, 14.5)]);
        }
        Icon::TextField => {
            closed(&[(2.5, 7.0), (21.5, 7.0), (21.5, 17.0), (2.5, 17.0)]);
            // A text caret with serifs.
            line(&[(8.0, 9.5), (8.0, 14.5)]);
            line(&[(6.5, 9.5), (9.5, 9.5)]);
            line(&[(6.5, 14.5), (9.5, 14.5)]);
        }
        Icon::Checkbox => {
            closed(&[(4.0, 4.0), (20.0, 4.0), (20.0, 20.0), (4.0, 20.0)]);
            line(&[(8.0, 8.0), (16.0, 16.0)]);
            line(&[(8.0, 16.0), (16.0, 8.0)]);
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
            // A pen's loop over the line it signs on.
            line(&[(3.0, 19.5), (21.0, 19.5)]);
            line(&[
                (4.0, 15.0),
                (5.5, 10.5),
                (7.5, 6.5),
                (9.0, 6.0),
                (9.2, 9.0),
                (7.8, 14.5),
                (9.0, 15.0),
                (11.0, 11.0),
                (12.5, 10.0),
                (12.8, 13.0),
                (14.5, 12.0),
                (16.5, 11.0),
                (17.5, 13.0),
                (20.0, 12.0),
            ]);
        }
        Icon::Highlight => {
            // A text box with a highlighter's marks: the tint of a field.
            closed(&[(3.0, 8.0), (21.0, 8.0), (21.0, 16.0), (3.0, 16.0)]);
            fill(&[(5.0, 10.0), (19.0, 10.0), (19.0, 14.0), (5.0, 14.0)]);
            line(&[(6.0, 19.5), (18.0, 19.5)]);
            line(&[(8.0, 4.5), (16.0, 4.5)]);
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
            // A page with corner marks around it.
            closed(&[(8.0, 6.0), (16.0, 6.0), (16.0, 18.0), (8.0, 18.0)]);
            line(&[(3.0, 6.0), (3.0, 3.0), (6.0, 3.0)]);
            line(&[(18.0, 3.0), (21.0, 3.0), (21.0, 6.0)]);
            line(&[(21.0, 18.0), (21.0, 21.0), (18.0, 21.0)]);
            line(&[(6.0, 21.0), (3.0, 21.0), (3.0, 18.0)]);
        }
        Icon::Rotate => {
            // A page under a clockwise arrow.
            closed(&[(5.0, 10.0), (13.0, 10.0), (13.0, 21.0), (5.0, 21.0)]);
            let n = 10;
            let pts: Vec<(f32, f32)> = (0..=n)
                .map(|i| {
                    let a = std::f32::consts::PI * (1.0 + 0.5 * i as f32 / n as f32);
                    (16.0 + 6.0 * a.cos(), 10.0 + 6.0 * a.sin())
                })
                .collect();
            line(&pts);
            line(&[(19.5, 2.5), (16.0, 4.0), (19.0, 6.5)]);
        }
        Icon::Sidebar => {
            closed(&[(3.5, 5.0), (20.5, 5.0), (20.5, 19.0), (3.5, 19.0)]);
            line(&[(9.0, 5.0), (9.0, 19.0)]);
            line(&[(5.5, 8.5), (7.0, 8.5)]);
            line(&[(5.5, 11.5), (7.0, 11.5)]);
            line(&[(5.5, 14.5), (7.0, 14.5)]);
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
            // A gear: ring, hub and eight teeth.
            circle(12.0, 12.0, 6.0);
            circle(12.0, 12.0, 2.3);
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                let (c, s) = (a.cos(), a.sin());
                painter.add(Shape::line_segment(
                    [
                        g.p(12.0 + 6.0 * c, 12.0 + 6.0 * s),
                        g.p(12.0 + 9.0 * c, 12.0 + 9.0 * s),
                    ],
                    Stroke::new(w * 1.6, color),
                ));
            }
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
