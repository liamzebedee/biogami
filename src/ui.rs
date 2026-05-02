//! egui-based GUI for visualising the cell sheet, gradients, creases, and
//! folds. Loads an .osl program, compiles it, and steps a `Runner`. Shows
//! both a top-down 2D view and a 3D view; folds are animated in both.

use crate::cellsim::{CellSheet, Runner};
use crate::compile::{expand_defuns, Compiler};
use crate::geom::{Vec2, Vec3};
use crate::parser::parse;
use crate::sheet::Polarity;
use anyhow::Result;
use eframe::egui::{self, Color32, Painter, Pos2, Rect, Sense, Stroke, Vec2 as EVec2};

pub struct App {
    title: String,
    source: String,
    runner: Option<Runner>,
    auto: bool,
    speed_steps_per_frame: usize,
    n_cells: usize,
    radius: f64,
    seed: u64,
    error: Option<String>,
    show_gradient: bool,
    fold_duration: f32,
    last_frame: Option<std::time::Instant>,
    // 3D camera
    yaw: f32,
    pitch: f32,
}

impl App {
    pub fn new(title: String, source: String) -> Self {
        let mut s = App {
            title,
            source,
            runner: None,
            auto: false,
            speed_steps_per_frame: 1,
            n_cells: 1500,
            radius: 0.06,
            seed: 42,
            error: None,
            show_gradient: true,
            fold_duration: 0.3,
            last_frame: None,
            yaw: 0.6,
            pitch: 1.05,
        };
        s.reset();
        s
    }

    fn reset(&mut self) {
        self.runner = None;
        self.error = None;
        match self.build_runner() {
            Ok(r) => self.runner = Some(r),
            Err(e) => self.error = Some(format!("{}", e)),
        }
        self.last_frame = None;
    }

    fn build_runner(&self) -> Result<Runner> {
        let prog = parse(&self.source)?;
        let expanded = expand_defuns(&prog)?;
        let mut c = Compiler::new();
        let cp = c.compile(&expanded)?;
        let cells = CellSheet::new(1.0, self.n_cells, self.radius, self.seed);
        let mut r = Runner::new(cells, cp.ops);
        r.fold_duration = self.fold_duration;
        Ok(r)
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Frame timing
        let now = std::time::Instant::now();
        let dt = match self.last_frame {
            Some(t) => (now - t).as_secs_f32().min(0.1),
            None => 1.0 / 60.0,
        };
        self.last_frame = Some(now);

        // Drive the runner: tick anim first, then optionally step.
        let mut needs_repaint = false;
        if let Some(r) = self.runner.as_mut() {
            r.fold_duration = self.fold_duration;
            if r.in_transition() {
                r.tick(dt);
                needs_repaint = true;
            } else if self.auto {
                for _ in 0..self.speed_steps_per_frame {
                    if r.done() {
                        self.auto = false;
                        break;
                    }
                    if r.in_transition() {
                        break;
                    }
                    if let Err(e) = r.step() {
                        self.error = Some(format!("{}", e));
                        self.auto = false;
                        break;
                    }
                }
                needs_repaint = true;
            }
        }

        egui::SidePanel::left("controls")
            .resizable(true)
            .default_width(360.0)
            .show(ctx, |ui| {
                ui.heading(&self.title);
                ui.separator();
                ui.label("Source:");
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.source)
                            .code_editor()
                            .desired_width(f32::INFINITY)
                            .desired_rows(12),
                    );
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Cells");
                    ui.add(egui::Slider::new(&mut self.n_cells, 200..=6000));
                });
                ui.horizontal(|ui| {
                    ui.label("Radius");
                    ui.add(egui::Slider::new(&mut self.radius, 0.02..=0.15));
                });
                ui.horizontal(|ui| {
                    ui.label("Seed");
                    ui.add(egui::DragValue::new(&mut self.seed));
                });
                ui.horizontal(|ui| {
                    ui.label("Fold dur (s)");
                    ui.add(egui::Slider::new(&mut self.fold_duration, 0.0..=2.0));
                });
                ui.horizontal(|ui| {
                    if ui.button("Reset / Recompile").clicked() {
                        self.reset();
                    }
                    if let Some(r) = self.runner.as_mut() {
                        let busy = r.in_transition();
                        ui.add_enabled_ui(!busy, |ui| {
                            if ui.button("Step").clicked() {
                                if let Err(e) = r.step() {
                                    self.error = Some(format!("{}", e));
                                }
                            }
                            let label = if self.auto { "Pause" } else { "Run" };
                            if ui.button(label).clicked() {
                                self.auto = !self.auto;
                            }
                        });
                        if busy {
                            ui.label("(folding…)");
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Steps/frame");
                    ui.add(egui::Slider::new(&mut self.speed_steps_per_frame, 1..=10));
                });
                ui.checkbox(&mut self.show_gradient, "Show gradient field");

                if let Some(r) = &self.runner {
                    ui.separator();
                    ui.label(format!(
                        "PC: {}/{}    Cells: {}",
                        r.pc,
                        r.ops.len(),
                        r.cells.cells.len()
                    ));
                    ui.label(format!("Last: {}", r.last_message));
                    if let Some(reg) = &r.current_region {
                        ui.label(format!("Active region: {}", reg));
                    }
                    if r.done() && !r.in_transition() {
                        ui.label("(done)");
                    }
                }

                if let Some(err) = &self.error {
                    ui.colored_label(egui::Color32::RED, err);
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            let avail = ui.available_size();
            let half_w = (avail.x - 8.0) * 0.5;
            let side = half_w.min(avail.y - 4.0);
            ui.horizontal(|ui| {
                let (rect2d, _) =
                    ui.allocate_exact_size(EVec2::new(side, side), Sense::hover());
                let painter2d = ui.painter_at(rect2d);
                self.draw_2d(&painter2d, rect2d);

                let (rect3d, resp3d) = ui.allocate_exact_size(
                    EVec2::new(side, side),
                    Sense::click_and_drag(),
                );
                if resp3d.dragged() {
                    let drag = resp3d.drag_delta();
                    self.yaw -= drag.x * 0.01;
                    self.pitch = (self.pitch + drag.y * 0.01).clamp(0.05, 1.55);
                }
                if resp3d.double_clicked() {
                    self.yaw = 0.6;
                    self.pitch = 1.05;
                }
                let painter3d = ui.painter_at(rect3d);
                self.draw_3d(&painter3d, rect3d);
            });
        });

        if needs_repaint {
            ctx.request_repaint();
        }
    }
}

impl App {
    fn draw_2d(&self, painter: &Painter, rect: Rect) {
        painter.rect_filled(rect, 0.0, Color32::from_rgb(28, 30, 34));
        painter.text(
            Pos2 { x: rect.left() + 6.0, y: rect.top() + 4.0 },
            egui::Align2::LEFT_TOP,
            "2D (top-down)",
            egui::FontId::proportional(12.0),
            Color32::from_rgb(160, 160, 160),
        );

        let r = match &self.runner {
            Some(r) => r,
            None => return,
        };
        let size = r.cells.size as f32;
        let inset = 18.0;
        let to_screen = |p: Vec2| -> Pos2 {
            let x = (p.x as f32) / size;
            let y = (p.y as f32) / size;
            Pos2 {
                x: rect.left() + inset + x * (rect.width() - 2.0 * inset),
                y: rect.bottom() - inset - y * (rect.height() - 2.0 * inset),
            }
        };

        let cell_radius = 1.6_f32.max(rect.width() / 260.0);
        for (i, c) in r.cells.cells.iter().enumerate() {
            let color = self.cell_color(c, r, i);
            painter.circle_filled(to_screen(c.display_pos), cell_radius, color);
        }

        // Layer outlines
        for layer in &r.cells.sheet.layers {
            let stroke = match layer.polarity {
                Polarity::Apical => Stroke::new(1.5, Color32::from_rgb(180, 180, 220)),
                Polarity::Basal => Stroke::new(1.5, Color32::from_rgb(220, 180, 180)),
            };
            for w in layer.poly.windows(2) {
                painter.line_segment([to_screen(w[0]), to_screen(w[1])], stroke);
            }
            if layer.poly.len() >= 2 {
                painter.line_segment(
                    [
                        to_screen(*layer.poly.last().unwrap()),
                        to_screen(layer.poly[0]),
                    ],
                    stroke,
                );
            }
        }
    }

    fn draw_3d(&self, painter: &Painter, rect: Rect) {
        painter.rect_filled(rect, 0.0, Color32::from_rgb(20, 22, 26));
        painter.text(
            Pos2 { x: rect.left() + 6.0, y: rect.top() + 4.0 },
            egui::Align2::LEFT_TOP,
            "3D (drag to rotate, double-click to reset)",
            egui::FontId::proportional(12.0),
            Color32::from_rgb(160, 160, 160),
        );

        let r = match &self.runner {
            Some(r) => r,
            None => return,
        };
        if r.cells.cells.is_empty() {
            return;
        }

        // Camera: tilt = self.pitch (0 = horizontal, π/2 = top-down). The
        // projection is yaw-around-z then tilt-around-camera-x, exaggerating
        // z so each fold's stack height is clearly visible.
        let cy_yaw = self.yaw.cos();
        let sy_yaw = self.yaw.sin();
        let ct = self.pitch.cos();
        let st = self.pitch.sin();
        let z_exaggerate: f32 = 40.0;

        // Centroid (world) of current cell positions, so the camera always
        // looks at where the action is.
        let n = r.cells.cells.len() as f64;
        let mut cx_w = 0.0;
        let mut cy_w = 0.0;
        let mut cz_w = 0.0;
        for c in &r.cells.cells {
            cx_w += c.pos_3d.x;
            cy_w += c.pos_3d.y;
            cz_w += c.pos_3d.z;
        }
        cx_w /= n;
        cy_w /= n;
        cz_w /= n;

        // Project a point into camera space (no scale/translate yet).
        let project_cam = |p: Vec3| -> (f32, f32, f32) {
            let dx = (p.x - cx_w) as f32;
            let dy = (p.y - cy_w) as f32;
            let dz = (p.z - cz_w) as f32 * z_exaggerate;
            // Yaw around world z
            let x1 = dx * cy_yaw - dy * sy_yaw;
            let y1 = dx * sy_yaw + dy * cy_yaw;
            let z1 = dz;
            // Tilt around camera x (z-up sweeps into screen-up as tilt → 0)
            let x2 = x1;
            let y2 = y1 * ct + z1 * st;
            let z2 = -y1 * st + z1 * ct;
            (x2, y2, z2)
        };

        // First pass: project all cells to find the screen-space bbox.
        let projected: Vec<(f32, f32, f32)> =
            r.cells.cells.iter().map(|c| project_cam(c.pos_3d)).collect();
        let (mut mn_x, mut mx_x) = (f32::INFINITY, f32::NEG_INFINITY);
        let (mut mn_y, mut mx_y) = (f32::INFINITY, f32::NEG_INFINITY);
        for &(x, y, _) in &projected {
            mn_x = mn_x.min(x);
            mx_x = mx_x.max(x);
            mn_y = mn_y.min(y);
            mx_y = mx_y.max(y);
        }
        let bw = (mx_x - mn_x).max(1e-3);
        let bh = (mx_y - mn_y).max(1e-3);
        let bcx = (mn_x + mx_x) * 0.5;
        let bcy = (mn_y + mx_y) * 0.5;

        let margin = 28.0;
        let avail_w = (rect.width() - 2.0 * margin).max(1.0);
        let avail_h = (rect.height() - 2.0 * margin).max(1.0);
        // Auto-fit with a small headroom (0.92) so the sheet doesn't kiss the
        // edges. We don't shrink/grow more than ~25%/frame to keep folds
        // visually stable.
        let target_scale = ((avail_w / bw).min(avail_h / bh)) * 0.92;

        let center = rect.center();
        let to_screen = |c: (f32, f32, f32)| -> Pos2 {
            Pos2 {
                x: center.x + (c.0 - bcx) * target_scale,
                y: center.y - (c.1 - bcy) * target_scale,
            }
        };

        // Sort by depth (camera z2: smaller = farther). Paint farther first.
        let mut order: Vec<usize> = (0..projected.len()).collect();
        order.sort_by(|&a, &b| projected[a].2.partial_cmp(&projected[b].2).unwrap());

        let cell_radius = 1.6_f32.max(rect.width() / 260.0);
        // Depth range for brightness modulation
        let mut zmin = f32::INFINITY;
        let mut zmax = f32::NEG_INFINITY;
        for &(_, _, z) in &projected {
            zmin = zmin.min(z);
            zmax = zmax.max(z);
        }
        let zspan = (zmax - zmin).max(1e-3);

        for &i in &order {
            let c = &r.cells.cells[i];
            let pcam = projected[i];
            let p = to_screen(pcam);
            let depth_t = (pcam.2 - zmin) / zspan; // 0 = far, 1 = near
            let brightness = 0.65 + 0.35 * depth_t;
            let color = self.cell_color(c, r, i);
            let color = Color32::from_rgb(
                (color.r() as f32 * brightness).min(255.0) as u8,
                (color.g() as f32 * brightness).min(255.0) as u8,
                (color.b() as f32 * brightness).min(255.0) as u8,
            );
            painter.circle_filled(p, cell_radius, color);
        }
    }

    fn cell_color(&self, c: &crate::cellsim::Cell, runner: &Runner, idx: usize) -> Color32 {
        if let Some(reg) = &runner.current_region {
            if !*c.state.get(reg).unwrap_or(&false) {
                return Color32::from_rgb(60, 60, 60);
            }
        }
        if self.show_gradient {
            if let Some((_, g)) = &runner.cells.last_gradient {
                if let Some(v) = g.get(idx).copied().flatten() {
                    let t = ((v / runner.cells.size) * 8.0).fract() as f32;
                    return Color32::from_rgb(
                        (255.0 * t) as u8,
                        (255.0 * (1.0 - t)) as u8,
                        80,
                    );
                }
            }
        }
        for (var, color) in &[
            ("d1", Color32::from_rgb(0, 220, 0)),
            ("d2", Color32::from_rgb(0, 220, 220)),
            ("d3", Color32::from_rgb(220, 0, 220)),
            ("d4", Color32::from_rgb(220, 100, 220)),
            ("l1", Color32::from_rgb(220, 220, 0)),
            ("t2", Color32::from_rgb(0, 220, 0)),
            ("t3", Color32::from_rgb(0, 220, 220)),
            ("t4", Color32::from_rgb(220, 0, 220)),
            ("cntr-line", Color32::from_rgb(255, 220, 0)),
        ] {
            if *c.state.get(*var).unwrap_or(&false) {
                return *color;
            }
        }
        if *c.state.get("c1").unwrap_or(&false)
            || *c.state.get("c2").unwrap_or(&false)
            || *c.state.get("c3").unwrap_or(&false)
            || *c.state.get("c4").unwrap_or(&false)
        {
            return Color32::YELLOW;
        }
        if *c.state.get("e12").unwrap_or(&false) || *c.state.get("e34").unwrap_or(&false) {
            return Color32::from_rgb(80, 100, 220);
        }
        if *c.state.get("e23").unwrap_or(&false) || *c.state.get("e41").unwrap_or(&false) {
            return Color32::from_rgb(220, 80, 80);
        }
        if c.polarity_flipped {
            Color32::from_rgb(40, 40, 50)
        } else {
            Color32::from_rgb(180, 180, 185)
        }
    }
}

pub fn run_app(title: &str, source: &str) -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };
    let title = title.to_string();
    let source = source.to_string();
    eframe::run_native(
        "biogami",
        opts,
        Box::new(move |_| Box::new(App::new(title, source))),
    )
}
