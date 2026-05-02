//! Layered sheet model. The sheet starts as a single square layer; each
//! `execute_fold` reflects the layers on one side of a line across that line,
//! producing a layered (flat) origami. Polarity is per-layer (apical/basal).

use crate::geom::{Line, Vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polarity {
    Apical,
    Basal,
}

impl Polarity {
    pub fn flipped(self) -> Self {
        match self {
            Polarity::Apical => Polarity::Basal,
            Polarity::Basal => Polarity::Apical,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldType {
    /// Apical fold — the apical surface meets itself (valley fold relative to
    /// the apical-up convention).
    Apical,
    /// Basal fold — the basal surface meets itself (mountain fold).
    Basal,
}

/// A polygonal layer of the sheet. Vertices are CCW in the original (un-folded)
/// sheet frame; after folds they may be CW. We track polarity per layer so that
/// after a fold each layer has a consistent apical/basal surface.
#[derive(Debug, Clone)]
pub struct Layer {
    pub poly: Vec<Vec2>,
    pub polarity: Polarity,
}

impl Layer {
    pub fn square(size: f64) -> Self {
        Layer {
            poly: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(size, 0.0),
                Vec2::new(size, size),
                Vec2::new(0.0, size),
            ],
            polarity: Polarity::Apical,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Crease {
    pub line: Line,
    pub color: String,
}

#[derive(Debug, Clone)]
pub struct Sheet {
    pub size: f64,
    pub layers: Vec<Layer>,
    /// Crease pattern recorded in the original (un-folded) sheet frame.
    pub creases: Vec<Crease>,
}

impl Sheet {
    pub fn square(size: f64) -> Self {
        Sheet {
            size,
            layers: vec![Layer::square(size)],
            creases: Vec::new(),
        }
    }

    pub fn record_crease(&mut self, line: Line, color: &str) {
        self.creases.push(Crease {
            line,
            color: color.to_string(),
        });
    }

    /// Fold along `line`. Layers wholly on the side opposite to `landmark` are
    /// reflected and have their polarity inverted; layers wholly on the
    /// landmark side are unchanged. Layers that cross the line are split.
    pub fn execute_fold(&mut self, line: Line, fold: FoldType, landmark: Vec2) {
        let mut new_layers = Vec::new();
        // The "fold side" is the side that moves. The landmark side stays put.
        let landmark_sign = line.signed_dist(landmark).signum();
        // Sign of points to KEEP (landmark side):
        let keep_sign = if landmark_sign == 0.0 {
            1.0
        } else {
            landmark_sign
        };
        for layer in &self.layers {
            let (keep, flip) = split_polygon(&layer.poly, &line, keep_sign);
            if !keep.is_empty() {
                new_layers.push(Layer {
                    poly: keep,
                    polarity: layer.polarity,
                });
            }
            if !flip.is_empty() {
                let reflected: Vec<Vec2> = flip.iter().map(|&p| line.reflect(p)).collect();
                new_layers.push(Layer {
                    poly: reflected,
                    polarity: layer.polarity.flipped(),
                });
            }
        }
        // Layer ordering: when we apical-fold, the moving side flips to land on
        // top of the stationary layers; basal-fold lands beneath. We preserve
        // input order beyond that distinction for a simple visualization.
        match fold {
            FoldType::Apical => {} // moving side is appended after — drawn on top
            FoldType::Basal => new_layers.reverse(),
        }
        self.layers = new_layers;
    }
}

/// Split a CCW polygon by a line into the part with `signed_dist*keep_sign >= 0`
/// (kept) and the rest (flipped). Sutherland-Hodgman style.
fn split_polygon(poly: &[Vec2], line: &Line, keep_sign: f64) -> (Vec<Vec2>, Vec<Vec2>) {
    if poly.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let mut keep = Vec::new();
    let mut flip = Vec::new();
    let n = poly.len();
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        let da = line.signed_dist(a) * keep_sign;
        let db = line.signed_dist(b) * keep_sign;
        let a_keep = da >= -1e-9;
        let b_keep = db >= -1e-9;
        if a_keep {
            keep.push(a);
        } else {
            flip.push(a);
        }
        if (a_keep && !b_keep) || (!a_keep && b_keep) {
            // crossing
            let t = da / (da - db);
            let p = a + (b - a) * t;
            keep.push(p);
            flip.push(p);
        }
    }
    (dedup_close(keep), dedup_close(flip))
}

fn dedup_close(mut v: Vec<Vec2>) -> Vec<Vec2> {
    if v.len() < 2 {
        return v;
    }
    let mut out = Vec::with_capacity(v.len());
    for p in v.drain(..) {
        if let Some(&last) = out.last() {
            if (p - last).norm() < 1e-7 {
                continue;
            }
        }
        out.push(p);
    }
    if out.len() >= 2 {
        if (out[0] - *out.last().unwrap()).norm() < 1e-7 {
            out.pop();
        }
    }
    if out.len() < 3 {
        return Vec::new();
    }
    out
}
