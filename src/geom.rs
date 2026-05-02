//! 2D geometry: points, lines, regions, and Huzita's first four axioms.

use std::ops::{Add, Mul, Sub};

pub const EPS: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    pub fn dot(self, o: Vec2) -> f64 {
        self.x * o.x + self.y * o.y
    }
    pub fn cross(self, o: Vec2) -> f64 {
        self.x * o.y - self.y * o.x
    }
    pub fn norm(self) -> f64 {
        self.dot(self).sqrt()
    }
    pub fn normalized(self) -> Vec2 {
        let n = self.norm();
        if n < EPS {
            self
        } else {
            self * (1.0 / n)
        }
    }
    pub fn perp(self) -> Vec2 {
        Vec2::new(-self.y, self.x)
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}
impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f64> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f64) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
    pub fn from_xy(p: Vec2, z: f64) -> Self {
        Self::new(p.x, p.y, z)
    }
    pub fn xy(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    pub fn norm(self) -> f64 {
        self.dot(self).sqrt()
    }
    pub fn normalized(self) -> Vec3 {
        let n = self.norm();
        if n < EPS {
            self
        } else {
            self * (1.0 / n)
        }
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f64> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f64) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}

/// Rotate `v` around an axis through the origin with unit direction `k` by
/// `theta` radians (Rodrigues' formula).
pub fn rodrigues(v: Vec3, k: Vec3, theta: f64) -> Vec3 {
    let k = k.normalized();
    let c = theta.cos();
    let s = theta.sin();
    v * c + k.cross(v) * s + k * (k.dot(v) * (1.0 - c))
}

/// An infinite line stored as a point + unit direction. Represents both the
/// line and (via offset/normal) a half-plane test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    pub p: Vec2,
    pub dir: Vec2, // unit
}

impl Line {
    pub fn from_two_points(a: Vec2, b: Vec2) -> Self {
        let dir = (b - a).normalized();
        Line { p: a, dir }
    }

    pub fn normal(&self) -> Vec2 {
        self.dir.perp()
    }

    /// Signed distance from a point to the line (positive on the normal side).
    pub fn signed_dist(&self, q: Vec2) -> f64 {
        (q - self.p).dot(self.normal())
    }

    /// Reflect a point across this line.
    pub fn reflect(&self, q: Vec2) -> Vec2 {
        let n = self.normal();
        q - n * (2.0 * (q - self.p).dot(n))
    }

    /// Reflect a line across this line.
    pub fn reflect_line(&self, l: Line) -> Line {
        let p = self.reflect(l.p);
        let q = self.reflect(l.p + l.dir);
        Line::from_two_points(p, q)
    }

    pub fn parallel_to(&self, other: &Line) -> bool {
        self.dir.cross(other.dir).abs() < 1e-7
    }
}

/// Intersection of two lines, or None if parallel.
pub fn intersect_lines(a: &Line, b: &Line) -> Option<Vec2> {
    let denom = a.dir.cross(b.dir);
    if denom.abs() < 1e-9 {
        return None;
    }
    let t = (b.p - a.p).cross(b.dir) / denom;
    Some(a.p + a.dir * t)
}

// ---------- Huzita's axioms 1-4 ----------

/// A1: line through two points.
pub fn crease_lbp(p1: Vec2, p2: Vec2) -> Line {
    Line::from_two_points(p1, p2)
}

/// A2: perpendicular bisector of segment p1-p2 (fold p1 onto p2).
pub fn crease_p2p(p1: Vec2, p2: Vec2) -> Line {
    let mid = (p1 + p2) * 0.5;
    let dir = (p2 - p1).perp().normalized();
    Line { p: mid, dir }
}

/// A3: angle bisector of two lines (fold l1 onto l2). When parallel, the line
/// midway between them. With intersecting lines two bisectors exist; we return
/// the one whose direction is the sum of the unit directions of l1 and l2 (the
/// canonical "internal" bisector).
pub fn crease_l2l(l1: &Line, l2: &Line) -> Line {
    if l1.parallel_to(l2) {
        // Midway parallel line: move from l1.p halfway toward l2.
        let d = l2.signed_dist(l1.p);
        let p = l1.p - l2.normal() * (d * 0.5);
        return Line { p, dir: l1.dir };
    }
    let pt = intersect_lines(l1, l2).unwrap();
    // normalize directions consistently to pick a canonical bisector
    let d1 = l1.dir;
    let d2 = if d1.dot(l2.dir) >= 0.0 { l2.dir } else { l2.dir * -1.0 };
    let dir = (d1 + d2).normalized();
    Line { p: pt, dir }
}

/// A4: through p1, perpendicular to l1.
pub fn crease_l2s(l1: &Line, p1: Vec2) -> Line {
    Line {
        p: p1,
        dir: l1.normal(), // perpendicular to l1.dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: Vec2, b: Vec2) -> bool {
        (a - b).norm() < 1e-6
    }

    #[test]
    fn p2p_bisects() {
        let l = crease_p2p(Vec2::new(0.0, 0.0), Vec2::new(2.0, 0.0));
        assert!(approx(l.p, Vec2::new(1.0, 0.0)));
        // Direction is vertical
        assert!(l.dir.x.abs() < 1e-6);
    }

    #[test]
    fn lbp_passes_through() {
        let l = crease_lbp(Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0));
        assert!(l.signed_dist(Vec2::new(2.0, 2.0)).abs() < 1e-6);
    }

    #[test]
    fn l2s_perp() {
        let l1 = Line::from_two_points(Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0));
        let p1 = Vec2::new(3.0, 5.0);
        let l = crease_l2s(&l1, p1);
        assert!(approx(l.p, p1));
        assert!(l.dir.x.abs() < 1e-6); // vertical
    }

    #[test]
    fn l2l_parallel_midline() {
        // Two horizontal lines y=0 and y=1 → midline y=0.5
        let l1 = Line::from_two_points(Vec2::new(1.0, 1.0), Vec2::new(0.0, 1.0));
        let l2 = Line::from_two_points(Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0));
        let m = crease_l2l(&l1, &l2);
        // m should pass through y=0.5
        assert!((m.signed_dist(Vec2::new(0.5, 0.5))).abs() < 1e-6);
        assert!((m.signed_dist(Vec2::new(7.0, 0.5))).abs() < 1e-6);
    }

    #[test]
    fn intersect_basic() {
        let a = Line::from_two_points(Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0));
        let b = Line::from_two_points(Vec2::new(1.0, -1.0), Vec2::new(1.0, 1.0));
        let p = intersect_lines(&a, &b).unwrap();
        assert!(approx(p, Vec2::new(1.0, 0.0)));
    }

    #[test]
    fn rodrigues_pi_around_x_axis() {
        // rotating (0, 1, 0) by π around x-axis should give (0, -1, 0)
        let v = Vec3::new(0.0, 1.0, 0.0);
        let k = Vec3::new(1.0, 0.0, 0.0);
        let r = rodrigues(v, k, std::f64::consts::PI);
        assert!((r - Vec3::new(0.0, -1.0, 0.0)).norm() < 1e-6);
    }

    #[test]
    fn rodrigues_half_around_x_axis() {
        // rotating (0, 1, 0) by π/2 around x-axis should give (0, 0, 1)
        let v = Vec3::new(0.0, 1.0, 0.0);
        let k = Vec3::new(1.0, 0.0, 0.0);
        let r = rodrigues(v, k, std::f64::consts::FRAC_PI_2);
        assert!((r - Vec3::new(0.0, 0.0, 1.0)).norm() < 1e-6);
    }

    #[test]
    fn reflect_point() {
        let l = Line::from_two_points(Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0));
        let r = l.reflect(Vec2::new(2.0, 3.0));
        assert!(approx(r, Vec2::new(2.0, -3.0)));
    }
}
