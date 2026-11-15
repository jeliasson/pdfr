//! Geometry in *display space*: points, origin at the top-left of the page as
//! shown on screen, y pointing down, after `/Rotate` and the CropBox.
//!
//! PDF user space (origin bottom-left, y up, unrotated) is only used when
//! reading and writing the file, via [`PageGeom`].

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Rect {
    pub const fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Rect { x0, y0, x1, y1 }
    }
    /// Builds a rect from two arbitrary corners.
    pub fn from_points(a: (f32, f32), b: (f32, f32)) -> Self {
        Rect::new(a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1))
    }
    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }
    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }
    pub fn center(&self) -> (f32, f32) {
        ((self.x0 + self.x1) * 0.5, (self.y0 + self.y1) * 0.5)
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }
    pub fn inset(&self, d: f32) -> Rect {
        Rect::new(self.x0 + d, self.y0 + d, self.x1 - d, self.y1 - d)
    }
    pub fn intersect(&self, o: &Rect) -> Option<Rect> {
        let r = Rect::new(
            self.x0.max(o.x0),
            self.y0.max(o.y0),
            self.x1.min(o.x1),
            self.y1.min(o.y1),
        );
        (r.x1 > r.x0 && r.y1 > r.y0).then_some(r)
    }
    pub fn area(&self) -> f32 {
        self.width().max(0.0) * self.height().max(0.0)
    }
    /// Intersection area divided by the smaller rect's area.
    pub fn overlap_ratio(&self, o: &Rect) -> f32 {
        let a = self.area().min(o.area());
        if a <= 0.0 {
            return 0.0;
        }
        self.intersect(o).map_or(0.0, |r| r.area() / a)
    }
}

/// Page geometry: maps between PDF user space and display space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageGeom {
    /// Effective crop box in user space: `[llx, lly, urx, ury]`.
    pub crop: [f32; 4],
    /// Clockwise rotation in degrees: 0, 90, 180 or 270.
    pub rotate: u16,
}

impl PageGeom {
    pub fn new(crop: [f32; 4], rotate: i64) -> Self {
        let c = [
            crop[0].min(crop[2]),
            crop[1].min(crop[3]),
            crop[0].max(crop[2]),
            crop[1].max(crop[3]),
        ];
        let r = (rotate.rem_euclid(360) / 90 * 90) as u16;
        PageGeom { crop: c, rotate: r }
    }

    fn unrotated_size(&self) -> (f32, f32) {
        (self.crop[2] - self.crop[0], self.crop[3] - self.crop[1])
    }

    /// Size of the page as displayed, in points.
    pub fn display_size(&self) -> (f32, f32) {
        let (w, h) = self.unrotated_size();
        if self.rotate % 180 == 90 {
            (h, w)
        } else {
            (w, h)
        }
    }

    /// User space -> display space.
    pub fn to_display(&self, ux: f32, uy: f32) -> (f32, f32) {
        let (w, h) = self.unrotated_size();
        // Unrotated display coordinates (y down).
        let x = ux - self.crop[0];
        let y = self.crop[3] - uy;
        match self.rotate {
            90 => (h - y, x),
            180 => (w - x, h - y),
            270 => (y, w - x),
            _ => (x, y),
        }
    }

    /// Display space -> user space.
    pub fn to_user(&self, dx: f32, dy: f32) -> (f32, f32) {
        let (w, h) = self.unrotated_size();
        let (x, y) = match self.rotate {
            90 => (dy, h - dx),
            180 => (w - dx, h - dy),
            270 => (w - dy, dx),
            _ => (dx, dy),
        };
        (x + self.crop[0], self.crop[3] - y)
    }

    /// A user-space rectangle `[llx, lly, urx, ury]` in display space.
    pub fn rect_to_display(&self, r: [f32; 4]) -> Rect {
        Rect::from_points(self.to_display(r[0], r[1]), self.to_display(r[2], r[3]))
    }

    /// A display rectangle as a normalised user-space `[llx, lly, urx, ury]`.
    pub fn rect_to_user(&self, r: &Rect) -> [f32; 4] {
        let a = self.to_user(r.x0, r.y0);
        let b = self.to_user(r.x1, r.y1);
        [a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)]
    }

    /// Size of a display rectangle as seen in the unrotated form space
    /// (what an appearance stream's BBox must be).
    pub fn form_size(&self, r: &Rect) -> (f32, f32) {
        if self.rotate % 180 == 90 {
            (r.height(), r.width())
        } else {
            (r.width(), r.height())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all_rotations() {
        for rot in [0, 90, 180, 270, -90, 450] {
            let g = PageGeom::new([10.0, 20.0, 622.0, 812.0], rot);
            for &(x, y) in &[(10.0, 20.0), (100.0, 700.0), (622.0, 812.0), (300.5, 40.25)] {
                let (dx, dy) = g.to_display(x, y);
                let (ux, uy) = g.to_user(dx, dy);
                assert!((ux - x).abs() < 1e-3 && (uy - y).abs() < 1e-3, "rot {rot}");
                let (w, h) = g.display_size();
                assert!(dx >= -1e-3 && dx <= w + 1e-3 && dy >= -1e-3 && dy <= h + 1e-3);
            }
        }
    }

    #[test]
    fn rotate_90_maps_top_left() {
        // A page rotated 90° clockwise: the user-space lower-left corner is shown top-left.
        let g = PageGeom::new([0.0, 0.0, 612.0, 792.0], 90);
        assert_eq!(g.display_size(), (792.0, 612.0));
        assert_eq!(g.to_display(0.0, 0.0), (0.0, 0.0));
        assert_eq!(g.to_display(0.0, 792.0), (792.0, 0.0));
        assert_eq!(g.to_display(612.0, 0.0), (0.0, 612.0));
    }
}
