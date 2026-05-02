//! Cell-level simulator. Cells are randomly distributed in a unit-square
//! sheet; communication is local (within radius `r`). Gradients are propagated
//! by BFS over the neighbor graph and reflect approximate Euclidean distance.
//!
//! The simulator drives a compiled cell program (see `compile.rs`). Each
//! op in the program is one of:
//!   - axiom-rule: produce a crease line of cells
//!   - intersect-rule: AND two boolean state vars
//!   - create-region: seed a region by bounded-gradient
//!   - within-region/end-region: scope subsequent ops
//!   - execute-fold-rule: physically fold the sheet (delegated to Sheet)

use crate::geom::{rodrigues, Line, Vec2, Vec3};
use crate::sheet::{FoldType, Sheet};
use rand::SeedableRng;
use rand::Rng;
use rand_chacha::ChaCha8Rng;
use std::collections::HashMap;

/// Per-layer z offset in the 3D view, in sheet units. Each fold pushes the
/// moving side this far above (apical) or below (basal) the stationary side.
pub const STACK_EPS: f64 = 0.012;

#[derive(Debug, Clone)]
pub struct Cell {
    /// Birthplace in the original flat sheet. Never changes. The cell itself
    /// has no access to this — it's substrate bookkeeping for the renderer
    /// and for the fold operator (which needs to reflect points across a
    /// line). The cell program must never read it; that would be a global
    /// coordinate, which the thesis forbids.
    pub pos: Vec2,

    /// The cell's bag of named booleans — its only way to answer
    /// "am I part of the thing called X?" Geometric objects don't exist as
    /// equations anywhere; a line/region/point exists ONLY as the set of
    /// cells whose corresponding flag is true. Examples:
    ///   - `c1..c4`, `e12..e41` — set at birth from sheet position
    ///   - `d1`, `front`, `p1`  — set by axiom/region/intersect rules
    ///   - `current-region`     — narrows participation inside `within-region`
    /// Every rule's logic reduces to lookups in this map plus gradient reads.
    pub state: HashMap<String, bool>,

    /// Hop-count distance to the source of each named gradient currently
    /// active at this cell. Missing key = gradient hasn't reached me yet.
    /// Sources keep their own at 0; everyone else relaxes to
    /// `min(neighbour) + 1` each tick. This is how cells approximate
    /// distance to a named feature without coordinates.
    pub gradients: HashMap<String, f64>,

    /// Whether this cell's apical/basal orientation has been flipped by a
    /// fold. Cells in the moving half-plane of a fold invert; the rest
    /// don't. Read by `execute-fold-rule` so successive folds know which
    /// surface is currently up.
    pub polarity_flipped: bool,

    /// Which layer of the folded paper stack the cell currently sits in.
    /// Substrate bookkeeping (not visible to the cell) — the fold operator
    /// bumps this when stacking, the renderer uses it to draw layer order.
    pub layer: usize,

    /// Where this cell currently *appears* on the flat 2D view, after all
    /// folds applied so far. Substrate-only. At birth equals `pos`; each
    /// fold reflects it across the crease line.
    pub display_pos: Vec2,

    /// 3D rendering position. Used by both the 3D view and the fold
    /// animation. At rest equals `(display_pos.x, display_pos.y,
    /// layer * STACK_EPS * fold_sign)`. Substrate-only.
    pub pos_3d: Vec3,
}

#[derive(Debug, Clone)]
pub struct CellSheet {
    pub size: f64,
    pub r: f64,
    pub cells: Vec<Cell>,
    /// Adjacency list: index -> neighbor indices (cells within r).
    pub neighbors: Vec<Vec<usize>>,
    pub sheet: Sheet,
    /// Most recent gradient field for visualization, keyed by gradient name.
    pub last_gradient: Option<(String, Vec<Option<f64>>)>,
}

impl CellSheet {
    pub fn new(size: f64, n_cells: usize, r: f64, seed: u64) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let mut cells: Vec<Cell> = Vec::with_capacity(n_cells);
        let min_dist = r * 0.18; // light packing constraint
        let min_d2 = min_dist * min_dist;
        let mut tries = 0;
        while cells.len() < n_cells && tries < n_cells * 200 {
            tries += 1;
            let p = Vec2::new(rng.gen_range(0.0..size), rng.gen_range(0.0..size));
            let too_close = cells
                .iter()
                .any(|c| (c.pos - p).dot(c.pos - p) < min_d2);
            if !too_close {
                let mut state = HashMap::new();
                // Edge tags: cells whose distance to a sheet boundary is < r.
                if p.y < r {
                    state.insert("e12".into(), true);
                }
                if p.x > size - r {
                    state.insert("e23".into(), true);
                }
                if p.y > size - r {
                    state.insert("e34".into(), true);
                }
                if p.x < r {
                    state.insert("e41".into(), true);
                }
                // Corners: cells within r of each corner.
                let corners = [
                    ("c1", Vec2::new(0.0, 0.0)),
                    ("c2", Vec2::new(size, 0.0)),
                    ("c3", Vec2::new(size, size)),
                    ("c4", Vec2::new(0.0, size)),
                ];
                for (name, cp) in corners {
                    if (p - cp).norm() < r {
                        state.insert(name.into(), true);
                    }
                }
                state.insert("current-region".into(), true);
                cells.push(Cell {
                    pos: p,
                    display_pos: p,
                    pos_3d: Vec3::from_xy(p, 0.0),
                    state,
                    gradients: HashMap::new(),
                    polarity_flipped: false,
                    layer: 0,
                });
            }
        }
        let neighbors = build_neighbors(&cells, r);
        CellSheet {
            size,
            r,
            cells,
            neighbors,
            sheet: Sheet::square(size),
            last_gradient: None,
        }
    }

    pub fn cells_with(&self, var: &str) -> Vec<usize> {
        self.cells
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                if *c.state.get(var).unwrap_or(&false) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Propagate a gradient from `source_var`. Each cell whose `source_var` is
    /// true seeds value 0; the value increases by the average inter-cell
    /// distance per BFS hop. A None entry means "never reached" (only happens
    /// for disconnected components or when bounded by `barrier_var`).
    pub fn propagate_gradient(
        &self,
        source_var: &str,
        barrier_var: Option<&str>,
    ) -> Vec<Option<f64>> {
        let n = self.cells.len();
        let mut dist: Vec<Option<f64>> = vec![None; n];
        let hop = self.r * 0.6; // average inter-neighbor distance for spatial estimate
        let mut frontier: Vec<usize> = Vec::new();
        for (i, c) in self.cells.iter().enumerate() {
            if *c.state.get(source_var).unwrap_or(&false) {
                dist[i] = Some(0.0);
                frontier.push(i);
            }
        }
        let mut depth = 0usize;
        while !frontier.is_empty() {
            let mut next = Vec::new();
            for &i in &frontier {
                for &j in &self.neighbors[i] {
                    if dist[j].is_some() {
                        continue;
                    }
                    if let Some(bar) = barrier_var {
                        if *self.cells[j].state.get(bar).unwrap_or(&false) {
                            continue;
                        }
                    }
                    dist[j] = Some((depth + 1) as f64 * hop);
                    next.push(j);
                }
            }
            frontier = next;
            depth += 1;
        }
        dist
    }

    /// Run axiom-2 / axiom-3 style rule: cells where |g1-g2| < threshold belong
    /// to the bisector crease. For axiom-3 the source is a line of cells.
    /// Stores the crease in `crease_var` and returns indices.
    pub fn run_axiom_bisector(
        &mut self,
        p1_var: &str,
        p2_var: &str,
        crease_var: &str,
    ) -> Vec<usize> {
        let g1 = self.propagate_gradient(p1_var, None);
        let g2 = self.propagate_gradient(p2_var, None);
        let threshold = self.r * 1.1;
        let mut indices = Vec::new();
        for (i, c) in self.cells.iter_mut().enumerate() {
            let on = match (g1[i], g2[i]) {
                (Some(a), Some(b)) => (a - b).abs() < threshold,
                _ => false,
            };
            if on {
                c.state.insert(crease_var.to_string(), true);
                indices.push(i);
            }
        }
        self.last_gradient = Some((p1_var.to_string(), g1));
        indices
    }

    /// Axiom-1 / axiom-4: line through two points. Implemented by tropism: the
    /// p1 cell with the lowest g(p2) value starts a crease; we extend along the
    /// gradient toward p2. We approximate by selecting all cells whose
    /// perpendicular distance from the analytic line p1-p2 is small. (Locally
    /// the cell program tropism produces the same locus.)
    pub fn run_axiom_line(
        &mut self,
        p1_var: &str,
        p2_var: &str,
        crease_var: &str,
    ) -> Vec<usize> {
        // Pick a centroid for p1 and p2 cells.
        let p1c = centroid(&self.cells, p1_var);
        let p2c = centroid(&self.cells, p2_var);
        let (p1c, p2c) = match (p1c, p2c) {
            (Some(a), Some(b)) => (a, b),
            _ => return Vec::new(),
        };
        let line = Line::from_two_points(p1c, p2c);
        let mut indices = Vec::new();
        let threshold = self.r * 1.1;
        // limit to segment between p1 and p2
        let dir = (p2c - p1c).normalized();
        let len = (p2c - p1c).norm();
        for (i, c) in self.cells.iter_mut().enumerate() {
            let perp = line.signed_dist(c.pos).abs();
            let along = (c.pos - p1c).dot(dir);
            if perp < threshold && along > -self.r && along < len + self.r {
                c.state.insert(crease_var.to_string(), true);
                indices.push(i);
            }
        }
        indices
    }

    /// Boolean AND: cells with both line vars become point cells for `point_var`.
    pub fn run_intersect(&mut self, l1_var: &str, l2_var: &str, point_var: &str) -> Vec<usize> {
        let mut indices = Vec::new();
        for (i, c) in self.cells.iter_mut().enumerate() {
            let a = *c.state.get(l1_var).unwrap_or(&false);
            let b = *c.state.get(l2_var).unwrap_or(&false);
            if a && b {
                c.state.insert(point_var.to_string(), true);
                indices.push(i);
            }
        }
        indices
    }

    /// Region creation: cells reachable from `seed_var` without crossing
    /// `barrier_var` are part of `region_var`.
    pub fn run_create_region(&mut self, seed_var: &str, barrier_var: &str, region_var: &str) {
        let dist = self.propagate_gradient(seed_var, Some(barrier_var));
        for (i, c) in self.cells.iter_mut().enumerate() {
            if dist[i].is_some() {
                c.state.insert(region_var.to_string(), true);
            }
        }
    }

    /// Execute fold along a crease line. Returns the geometry of the fold
    /// (line + landmark + moving cell indices + post-fold positions) so the
    /// caller can drive an animation. Geometric state (display_pos, layer,
    /// polarity, sheet polygons) is updated synchronously.
    pub fn run_execute_fold(
        &mut self,
        crease_var: &str,
        fold: FoldType,
        landmark_var: &str,
        region_var: Option<&str>,
    ) -> Option<FoldGeom> {
        let (line, landmark) = match (
            best_fit_line(&self.cells, crease_var),
            centroid(&self.cells, landmark_var),
        ) {
            (Some(l), Some(p)) => (l, p),
            _ => return None,
        };
        self.sheet.execute_fold(line, fold, landmark);
        self.sheet.record_crease(line, color_for(crease_var));

        let landmark_sign = line.signed_dist(landmark).signum();
        let mut moving = Vec::new();
        let mut start_3d = Vec::new();
        let mut end_3d = Vec::new();
        let z_sign: f64 = match fold {
            FoldType::Apical => 1.0,
            FoldType::Basal => -1.0,
        };
        for (i, c) in self.cells.iter_mut().enumerate() {
            if let Some(rv) = region_var {
                if !*c.state.get(rv).unwrap_or(&false) {
                    continue;
                }
            }
            let s = line.signed_dist(c.display_pos).signum();
            if s != 0.0 && s != landmark_sign {
                let s0 = c.pos_3d;
                c.display_pos = line.reflect(c.display_pos);
                c.polarity_flipped = !c.polarity_flipped;
                c.layer += 1;
                let new_z = c.layer as f64 * STACK_EPS * z_sign;
                let e0 = Vec3::from_xy(c.display_pos, new_z);
                c.pos_3d = e0;
                moving.push(i);
                start_3d.push(s0);
                end_3d.push(e0);
            }
        }
        Some(FoldGeom {
            line,
            landmark,
            fold,
            moving,
            start_3d,
            end_3d,
        })
    }
}

/// Result of `run_execute_fold` — geometric truth, plus enough state for the
/// Runner to animate the transition between start and end 3D positions.
#[derive(Debug, Clone)]
pub struct FoldGeom {
    pub line: Line,
    pub landmark: Vec2,
    pub fold: FoldType,
    pub moving: Vec<usize>,
    pub start_3d: Vec<Vec3>,
    pub end_3d: Vec<Vec3>,
}

fn build_neighbors(cells: &[Cell], r: f64) -> Vec<Vec<usize>> {
    let n = cells.len();
    let mut out = vec![Vec::new(); n];
    let r2 = r * r;
    for i in 0..n {
        for j in (i + 1)..n {
            let d = cells[i].pos - cells[j].pos;
            if d.dot(d) < r2 {
                out[i].push(j);
                out[j].push(i);
            }
        }
    }
    out
}

fn centroid(cells: &[Cell], var: &str) -> Option<Vec2> {
    let mut sx = 0.0;
    let mut sy = 0.0;
    let mut n = 0;
    for c in cells {
        if *c.state.get(var).unwrap_or(&false) {
            sx += c.pos.x;
            sy += c.pos.y;
            n += 1;
        }
    }
    if n == 0 {
        None
    } else {
        Some(Vec2::new(sx / n as f64, sy / n as f64))
    }
}

/// Best-fit line via principal component analysis on cells where `var` is true.
fn best_fit_line(cells: &[Cell], var: &str) -> Option<Line> {
    let pts: Vec<Vec2> = cells
        .iter()
        .filter(|c| *c.state.get(var).unwrap_or(&false))
        .map(|c| c.pos)
        .collect();
    if pts.len() < 2 {
        return None;
    }
    let n = pts.len() as f64;
    let mean = pts.iter().fold(Vec2::new(0.0, 0.0), |a, &p| a + p) * (1.0 / n);
    let (mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
    for p in &pts {
        let d = *p - mean;
        sxx += d.x * d.x;
        sxy += d.x * d.y;
        syy += d.y * d.y;
    }
    sxx /= n;
    sxy /= n;
    syy /= n;
    // dominant eigenvector of [[sxx,sxy],[sxy,syy]]
    let tr = sxx + syy;
    let det = sxx * syy - sxy * sxy;
    let disc = (tr * tr * 0.25 - det).max(0.0).sqrt();
    let lambda = tr * 0.5 + disc;
    let dir = if sxy.abs() > 1e-9 {
        Vec2::new(lambda - syy, sxy).normalized()
    } else if sxx >= syy {
        Vec2::new(1.0, 0.0)
    } else {
        Vec2::new(0.0, 1.0)
    };
    Some(Line { p: mean, dir })
}

fn color_for(name: &str) -> &'static str {
    match name {
        s if s.contains("d1") => "green",
        s if s.contains("d2") => "cyan",
        s if s.contains("d3") => "magenta",
        s if s.contains("d4") => "magenta",
        s if s.contains("l1") => "yellow",
        _ => "gray",
    }
}

// ---------- Cell program runner ----------

use crate::ast::Expr;
use anyhow::{anyhow, bail, Result};

/// Drives a CellSheet from the compiled cell program. Multi-phase: each
/// op advances through several phases (e.g. axiom2 = propagate g1 → propagate
/// g2 → emit crease) so the visualiser shows gradients forming before the
/// crease appears.
pub struct Runner {
    pub cells: CellSheet,
    pub ops: Vec<Expr>,
    pub pc: usize,
    pub phase: usize,
    pub current_region: Option<String>,
    pub last_message: String,
    g1_buf: Option<Vec<Option<f64>>>,
    g2_buf: Option<Vec<Option<f64>>>,
    pub anim: Option<FoldAnim>,
    /// Default fold animation duration in seconds. Set by the UI; the CLI sets
    /// this to 0 to disable animation.
    pub fold_duration: f32,
}

/// Active fold animation. The Runner holds Some(_) of these from the moment a
/// fold begins until tick() observes t≥1.
#[derive(Debug, Clone)]
pub struct FoldAnim {
    pub line: Line,
    pub fold: FoldType,
    pub moving: Vec<usize>,
    pub start_3d: Vec<Vec3>,
    pub end_3d: Vec<Vec3>,
    pub axis_p: Vec3,
    pub axis_d: Vec3,
    pub t: f32,
    pub duration: f32,
}

impl Runner {
    pub fn new(cells: CellSheet, ops: Vec<Expr>) -> Self {
        Runner {
            cells,
            ops,
            pc: 0,
            phase: 0,
            current_region: None,
            last_message: String::new(),
            g1_buf: None,
            g2_buf: None,
            anim: None,
            fold_duration: 0.3,
        }
    }

    pub fn in_transition(&self) -> bool {
        self.anim.is_some()
    }

    /// Advance an active fold animation by `dt` seconds. Returns true if the
    /// animation just completed this tick. tick() only updates visual cell
    /// positions (pos_3d, display_pos); pc/phase advancement is handled
    /// synchronously by `step()` when it starts the animation.
    pub fn tick(&mut self, dt: f32) -> bool {
        let mut just_finished = false;
        if let Some(a) = self.anim.as_mut() {
            a.t = (a.t + dt / a.duration.max(1e-3)).min(1.0);
            let theta = a.t as f64 * std::f64::consts::PI;
            for (k, &i) in a.moving.iter().enumerate() {
                let s = a.start_3d[k];
                let e = a.end_3d[k];
                let rotated = a.axis_p + rodrigues(s - a.axis_p, a.axis_d, theta);
                // Linearly ramp toward the final stacked z so the cell ends at
                // its layer offset rather than at z=0 after a flat-fold rotation.
                let z = rotated.z + (e.z - 0.0) * a.t as f64;
                let p = Vec3::new(rotated.x, rotated.y, z);
                self.cells.cells[i].pos_3d = p;
                self.cells.cells[i].display_pos = Vec2::new(p.x, p.y);
            }
            if a.t >= 1.0 {
                for (k, &i) in a.moving.iter().enumerate() {
                    self.cells.cells[i].pos_3d = a.end_3d[k];
                    self.cells.cells[i].display_pos =
                        Vec2::new(a.end_3d[k].x, a.end_3d[k].y);
                }
                just_finished = true;
            }
        }
        if just_finished {
            self.anim = None;
        }
        just_finished
    }

    pub fn done(&self) -> bool {
        self.pc >= self.ops.len()
    }

    /// Advance one micro-step. While a fold animation is in progress this is
    /// a no-op — call `tick(dt)` to advance the animation. Returns Ok(())
    /// regardless of whether the op completed; the caller should keep
    /// stepping until `done()`.
    pub fn step(&mut self) -> Result<()> {
        if self.done() || self.in_transition() {
            return Ok(());
        }
        let op = self.ops[self.pc].clone();
        let advanced = self.exec_phase(&op)?;
        if advanced {
            self.pc += 1;
            self.phase = 0;
            self.g1_buf = None;
            self.g2_buf = None;
        } else {
            self.phase += 1;
        }
        Ok(())
    }

    /// Execute the current phase of the current op. Returns true when the op
    /// has fully finished (caller advances pc); false when there are more
    /// phases to run for this op.
    fn exec_phase(&mut self, op: &Expr) -> Result<bool> {
        let xs = op
            .as_list()
            .ok_or_else(|| anyhow!("runner: expected list, got {}", op))?;
        let head = xs[0].as_symbol().ok_or_else(|| anyhow!("runner: head"))?;
        match head {
            "set!" => self.exec_set(xs),
            "execute-fold-rule" => self.exec_fold(xs),
            "define" => Ok(true), // no-op, present only in to_source
            other => bail!("runner: unknown op: {}", other),
        }
    }

    fn exec_set(&mut self, xs: &[Expr]) -> Result<bool> {
        let name = xs[1]
            .as_symbol()
            .ok_or_else(|| anyhow!("set!: name"))?
            .to_string();
        let rhs = &xs[2];
        // Trivial RHS values complete in a single phase.
        if let Expr::Bool(b) = rhs {
            if name == "current-region" {
                if *b {
                    self.current_region = None;
                    self.last_message = "leaving region".into();
                }
            } else {
                for c in &mut self.cells.cells {
                    c.state.insert(name.clone(), *b);
                }
            }
            return Ok(true);
        }
        if let Expr::Symbol(s) = rhs {
            if name == "current-region" {
                self.current_region = Some(s.clone());
                self.last_message = format!("entering region {}", s);
            } else {
                let src = s.clone();
                for c in &mut self.cells.cells {
                    let v = *c.state.get(&src).unwrap_or(&false);
                    c.state.insert(name.clone(), v);
                }
            }
            return Ok(true);
        }
        // Rule call.
        let rxs = rhs.as_list().ok_or_else(|| anyhow!("set!: rhs"))?;
        let rhead = rxs[0].as_symbol().ok_or_else(|| anyhow!("rhs head"))?;
        match rhead {
            "axiom1-rule" => self.phase_axiom_line(&name, rxs),
            "axiom2-rule" => self.phase_axiom_bisector(&name, rxs),
            "intersect-rule" => self.phase_intersect(&name, rxs),
            "create-region-rule" => self.phase_create_region(&name, rxs),
            other => bail!("runner: unsupported rule call: {}", other),
        }
    }

    /// axiom1: line between two points. Phases:
    ///   0 — propagate gradient from p2 (visible)
    ///   1 — propagate gradient from p1 (visible)
    ///   2 — emit the crease cells along p1→p2
    fn phase_axiom_line(&mut self, name: &str, rxs: &[Expr]) -> Result<bool> {
        let p1 = rxs[1].as_symbol().unwrap();
        let p2 = rxs[2].as_symbol().unwrap();
        match self.phase {
            0 => {
                let g = self.cells.propagate_gradient(p2, None);
                self.cells.last_gradient = Some((format!("g({})", p2), g));
                self.last_message =
                    format!("axiom1 [{}]: gradient from {}", name, p2);
                Ok(false)
            }
            1 => {
                let g = self.cells.propagate_gradient(p1, None);
                self.cells.last_gradient = Some((format!("g({})", p1), g));
                self.last_message =
                    format!("axiom1 [{}]: gradient from {} (tropism)", name, p1);
                Ok(false)
            }
            _ => {
                self.cells.run_axiom_line(p1, p2, name);
                self.cells.last_gradient = None;
                self.last_message = format!("axiom1 [{}]: crease from {}→{}", name, p1, p2);
                Ok(true)
            }
        }
    }

    /// axiom2/3: bisector between two sources. Phases:
    ///   0 — propagate g1 from p1 (visible)
    ///   1 — propagate g2 from p2 (visible)
    ///   2 — select cells where |g1-g2|<r as crease
    fn phase_axiom_bisector(&mut self, name: &str, rxs: &[Expr]) -> Result<bool> {
        let p1 = rxs[1].as_symbol().unwrap();
        let p2 = rxs[2].as_symbol().unwrap();
        match self.phase {
            0 => {
                let g = self.cells.propagate_gradient(p1, None);
                self.cells.last_gradient = Some((format!("g({})", p1), g.clone()));
                self.g1_buf = Some(g);
                self.last_message =
                    format!("axiom2/3 [{}]: gradient from {}", name, p1);
                Ok(false)
            }
            1 => {
                let g = self.cells.propagate_gradient(p2, None);
                self.cells.last_gradient = Some((format!("g({})", p2), g.clone()));
                self.g2_buf = Some(g);
                self.last_message =
                    format!("axiom2/3 [{}]: gradient from {}", name, p2);
                Ok(false)
            }
            _ => {
                let g1 = self.g1_buf.take().unwrap_or_default();
                let g2 = self.g2_buf.take().unwrap_or_default();
                let threshold = self.cells.r * 1.1;
                for (i, c) in self.cells.cells.iter_mut().enumerate() {
                    let on = match (g1.get(i).copied().flatten(), g2.get(i).copied().flatten()) {
                        (Some(a), Some(b)) => (a - b).abs() < threshold,
                        _ => false,
                    };
                    if on {
                        c.state.insert(name.to_string(), true);
                    }
                }
                self.cells.last_gradient = None;
                self.last_message =
                    format!("axiom2/3 [{}]: bisector cells marked", name);
                Ok(true)
            }
        }
    }

    fn phase_intersect(&mut self, name: &str, rxs: &[Expr]) -> Result<bool> {
        let l1 = rxs[1].as_symbol().unwrap();
        let l2 = rxs[2].as_symbol().unwrap();
        self.cells.run_intersect(l1, l2, name);
        self.last_message = format!("intersect: point {} = {} ∩ {}", name, l1, l2);
        Ok(true)
    }

    /// create-region: phase 0 = propagate bounded gradient (visible), phase 1
    /// = mark region cells.
    fn phase_create_region(&mut self, name: &str, rxs: &[Expr]) -> Result<bool> {
        let seed = rxs[1].as_symbol().unwrap();
        let bar = rxs[2].as_symbol().unwrap();
        match self.phase {
            0 => {
                let g = self.cells.propagate_gradient(seed, Some(bar));
                self.cells.last_gradient = Some((format!("region({})", name), g.clone()));
                self.g1_buf = Some(g);
                self.last_message =
                    format!("region [{}]: bounded gradient from {} barrier {}", name, seed, bar);
                Ok(false)
            }
            _ => {
                let g = self.g1_buf.take().unwrap_or_default();
                for (i, c) in self.cells.cells.iter_mut().enumerate() {
                    if g.get(i).copied().flatten().is_some() {
                        c.state.insert(name.to_string(), true);
                    }
                }
                self.cells.last_gradient = None;
                self.last_message = format!("region [{}]: cells marked", name);
                Ok(true)
            }
        }
    }

    /// execute-fold: phase 0 = highlight landmark region, phase 1 = start the
    /// fold animation. tick() then drives the rotation; on completion it
    /// auto-advances pc.
    fn exec_fold(&mut self, xs: &[Expr]) -> Result<bool> {
        let line = xs[1].as_symbol().unwrap().to_string();
        let typ = xs[2].as_symbol().unwrap().to_string();
        let landmark = xs[3].as_symbol().unwrap().to_string();
        let fold = match typ.as_str() {
            "apical" => crate::sheet::FoldType::Apical,
            "basal" => crate::sheet::FoldType::Basal,
            other => bail!("runner: unknown fold type {}", other),
        };
        match self.phase {
            0 => {
                let g = self.cells.propagate_gradient(&landmark, Some(&line));
                self.cells.last_gradient = Some((format!("landmark({})", landmark), g));
                self.last_message = format!(
                    "execute-fold {} ({}): landmark region from {}",
                    line, typ, landmark
                );
                Ok(false)
            }
            _ => {
                self.last_message = format!(
                    "execute-fold {} ({}) landmark={}{}",
                    line,
                    typ,
                    landmark,
                    self.current_region
                        .as_ref()
                        .map(|r| format!(" within {}", r))
                        .unwrap_or_default()
                );
                let region = self.current_region.clone();
                let geom = self
                    .cells
                    .run_execute_fold(&line, fold, &landmark, region.as_deref());
                self.cells.last_gradient = None;
                if let Some(g) = geom {
                    if !g.moving.is_empty() {
                        // Build the 3D rotation axis: the fold line in the
                        // xy-plane, oriented so the moving side rotates toward
                        // +z (apical) or -z (basal) at θ=π/2.
                        let axis_p = Vec3::from_xy(g.line.p, 0.0);
                        let axis_d_xy = Vec3::new(g.line.dir.x, g.line.dir.y, 0.0);
                        let rep = g.start_3d[0];
                        let r2 = rep.xy() - g.line.p;
                        let cross_z = g.line.dir.x * r2.y - g.line.dir.y * r2.x;
                        let mut axis_d = if cross_z > 0.0 {
                            axis_d_xy
                        } else {
                            axis_d_xy * -1.0
                        };
                        if matches!(fold, FoldType::Basal) {
                            axis_d = axis_d * -1.0;
                        }
                        let duration = self.fold_duration.max(0.0);
                        self.anim = Some(FoldAnim {
                            line: g.line,
                            fold: g.fold,
                            moving: g.moving,
                            start_3d: g.start_3d,
                            end_3d: g.end_3d,
                            axis_p,
                            axis_d,
                            t: 0.0,
                            duration,
                        });
                        // If duration is zero (CLI), drain immediately so the
                        // caller sees the final state without having to tick.
                        if duration <= 0.0 {
                            self.tick(1.0);
                        }
                    }
                }
                Ok(true)
            }
        }
    }
}

