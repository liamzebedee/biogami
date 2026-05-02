//! egui-based GUI for biogami: a four-pane time-travel debugger.
//!
//! Layout:
//!
//!   ┌─────────────┬───────────────────────────────────────────────┐
//!   │             │   OSL source        │   Compiled cell program │
//!   │  controls   ├─────────────────────┼─────────────────────────┤
//!   │             │   2D top-down       │   3D animated           │
//!   └─────────────┴─────────────────────┴─────────────────────────┘
//!
//! The currently-executing line is highlighted in *both* the OSL and the
//! cell-program panes. One OSL line maps to one or more cell-program lines,
//! so the OSL highlight typically stays put while the cell-program highlight
//! moves down. Step Back unwinds via the Runner's snapshot history.

use crate::cellsim::{CellSheet, Runner};
use crate::compile::{expand_defuns, CellProgram, Compiler};
use crate::geom::{Vec2, Vec3};
use crate::parser::parse;
use crate::sheet::Polarity;
use anyhow::Result;
use eframe::egui::{
    self, Align, Color32, Painter, Pos2, Rect, Response, Sense, Stroke, Vec2 as EVec2,
};

pub struct App {
    title: String,
    /// OSL source — editable in the UI.
    source: String,

    runner: Option<Runner>,
    /// Cell-program text and per-op line mapping — recomputed on Reset.
    cell_text: String,
    cell_op_line: Vec<usize>,
    /// Names of state variables to draw labels for (corners + edges + every
    /// user-defined point/line/region from the compiled cell program).
    label_names: Vec<String>,
    show_labels: bool,

    auto: bool,
    speed_steps_per_frame: usize,
    n_cells: usize,
    radius: f64,
    seed: u64,
    fold_duration: f32,
    /// Per-fold z offset in the 3D view (sheet units). 0 = physically
    /// flat-folded (layers coincide), small positive = visible stack.
    layer_spread: f64,

    error: Option<String>,
    show_gradient: bool,
    last_frame: Option<std::time::Instant>,

    yaw: f32,
    pitch: f32,
}

impl App {
    pub fn new(title: String, source: String) -> Self {
        let mut s = App {
            title,
            source,
            runner: None,
            cell_text: String::new(),
            cell_op_line: Vec::new(),
            label_names: Vec::new(),
            show_labels: true,
            auto: false,
            speed_steps_per_frame: 1,
            n_cells: 1500,
            radius: 0.06,
            seed: 42,
            fold_duration: 0.3,
            layer_spread: 0.02,
            error: None,
            show_gradient: true,
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
        self.cell_text.clear();
        self.cell_op_line.clear();
        self.label_names.clear();
        match self.build_runner_and_program() {
            Ok((r, cp)) => {
                let (text, op_line) = cp.render();
                self.cell_text = text;
                self.cell_op_line = op_line;
                // Always-present substrate names + every user-defined state var.
                self.label_names.extend([
                    "c1", "c2", "c3", "c4", "e12", "e23", "e34", "e41",
                ].iter().map(|s| s.to_string()));
                for v in &cp.state_vars {
                    self.label_names.push(v.clone());
                }
                self.runner = Some(r);
            }
            Err(e) => self.error = Some(format!("{}", e)),
        }
        self.last_frame = None;
    }

    fn build_runner_and_program(&self) -> Result<(Runner, CellProgram)> {
        let prog = parse(&self.source)?;
        let expanded = expand_defuns(&prog)?;
        let mut c = Compiler::new();
        let cp = c.compile(&expanded)?;
        let cells = CellSheet::new(1.0, self.n_cells, self.radius, self.seed);
        let mut r = Runner::new(cells, cp.ops.clone());
        r.fold_duration = self.fold_duration;
        r.stack_eps = self.layer_spread;
        r.history_enabled = true;
        Ok((r, cp))
    }

    /// Active OSL line, if a program is running and not exhausted.
    fn active_osl_line(&self) -> Option<usize> {
        self.runner.as_ref().and_then(|r| r.current_global_line())
    }

    /// Active cell-program text line, if a program is running and not
    /// exhausted.
    fn active_cell_line(&self) -> Option<usize> {
        let r = self.runner.as_ref()?;
        if r.pc >= r.ops.len() {
            return None;
        }
        self.cell_op_line.get(r.pc).copied()
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

        // Drive the runner.
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

        self.show_controls(ctx);
        self.show_main(ctx);

        if needs_repaint {
            ctx.request_repaint();
        }
    }
}

impl App {
    fn show_controls(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("controls")
            .resizable(true)
            .default_width(260.0)
            .show(ctx, |ui| {
                ui.heading(&self.title);
                ui.separator();
                ui.label("Sheet");
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
                ui.separator();
                ui.label("Playback");
                ui.horizontal(|ui| {
                    ui.label("Fold dur (s)");
                    ui.add(egui::Slider::new(&mut self.fold_duration, 0.0..=2.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Steps/frame");
                    ui.add(egui::Slider::new(&mut self.speed_steps_per_frame, 1..=10));
                });
                let resp = ui.horizontal(|ui| {
                    ui.label("Layer spread");
                    ui.add(
                        egui::Slider::new(&mut self.layer_spread, 0.0..=0.08)
                            .fixed_decimals(3),
                    )
                });
                if resp.inner.changed() {
                    if let Some(r) = self.runner.as_mut() {
                        r.set_stack_eps(self.layer_spread);
                    }
                }
                ui.checkbox(&mut self.show_gradient, "Show gradient field");
                ui.checkbox(&mut self.show_labels, "Label named entities");

                ui.separator();
                self.show_legend(ui);

                if let Some(err) = &self.error {
                    ui.separator();
                    ui.colored_label(Color32::from_rgb(220, 90, 90), err);
                }
            });
    }

    fn show_legend(&self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Legend").strong());
        let row = |ui: &mut egui::Ui, color: Color32, label: &str, hint: &str| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(EVec2::new(14.0, 14.0), Sense::hover());
                ui.painter().rect_filled(rect, 2.0, color);
                ui.label(egui::RichText::new(label).strong());
                ui.label(
                    egui::RichText::new(hint)
                        .small()
                        .color(Color32::from_rgb(150, 150, 160)),
                );
            });
        };
        row(ui, Color32::YELLOW, "corner", "c1 – c4");
        row(ui, Color32::from_rgb(80, 100, 220), "edge h", "e12 (bottom) / e34 (top)");
        row(ui, Color32::from_rgb(220, 80, 80), "edge v", "e23 (right) / e41 (left)");
        row(ui, Color32::from_rgb(0, 220, 0), "crease", "d1 / t2");
        row(ui, Color32::from_rgb(0, 220, 220), "crease", "d2 / t3");
        row(
            ui,
            Color32::from_rgb(220, 0, 220),
            "crease",
            "d3 / d4 / t4",
        );
        row(ui, Color32::from_rgb(220, 220, 0), "crease", "l1 / cntr");
        row(
            ui,
            Color32::from_rgb(180, 180, 185),
            "blank",
            "no special role",
        );
        row(
            ui,
            Color32::from_rgb(40, 40, 50),
            "polarity flipped",
            "back of folded layer",
        );
        row(
            ui,
            Color32::from_rgb(60, 60, 60),
            "outside region",
            "masked by within-region",
        );
        ui.label(
            egui::RichText::new(
                "When `show gradient` is on, cells display\n\
                 the BFS hop count of the active gradient as\n\
                 a warm/cool spectrum, overriding the colors\n\
                 above.",
            )
            .small()
            .color(Color32::from_rgb(150, 150, 160)),
        );
    }

    fn show_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("⟲ Reset").clicked() {
                self.reset();
            }
            let busy = self
                .runner
                .as_ref()
                .map(|r| r.in_transition())
                .unwrap_or(false);
            let history_empty = self
                .runner
                .as_ref()
                .map(|r| r.history.is_empty())
                .unwrap_or(true);
            ui.add_enabled_ui(!busy && !history_empty, |ui| {
                if ui.button("◀ Back").clicked() {
                    if let Some(r) = self.runner.as_mut() {
                        r.step_back();
                    }
                }
            });
            ui.add_enabled_ui(!busy, |ui| {
                if ui.button("Step ▶").clicked() {
                    if let Some(r) = self.runner.as_mut() {
                        if let Err(e) = r.step() {
                            self.error = Some(format!("{}", e));
                        }
                    }
                }
                let label = if self.auto { "⏸ Pause" } else { "▶ Run" };
                if ui.button(label).clicked() {
                    self.auto = !self.auto;
                }
            });
            ui.separator();
            if let Some(r) = &self.runner {
                ui.label(format!("PC {}/{}", r.pc, r.ops.len()));
                ui.label(format!("hist {}", r.history.len()));
                if r.in_transition() {
                    ui.colored_label(Color32::from_rgb(255, 200, 80), "(folding…)");
                }
                if r.done() && !r.in_transition() {
                    ui.colored_label(Color32::from_rgb(120, 220, 120), "✔ done");
                }
                if let Some(reg) = &r.current_region {
                    ui.label(
                        egui::RichText::new(format!("region: {}", reg))
                            .color(Color32::from_rgb(180, 200, 230)),
                    );
                }
            }
            ui.separator();
            if let Some(r) = &self.runner {
                if !r.last_message.is_empty() {
                    ui.label(
                        egui::RichText::new(&r.last_message)
                            .small()
                            .color(Color32::from_rgb(170, 170, 180)),
                    );
                }
            }
        });
    }

    fn show_main(&mut self, ctx: &egui::Context) {
        let active_osl = self.active_osl_line();
        let active_cell = self.active_cell_line();
        let runner_done = self.runner.as_ref().map(|r| r.done()).unwrap_or(true);

        egui::CentralPanel::default().show(ctx, |ui| {
            self.show_toolbar(ui);
            ui.separator();

            let total_h = ui.available_height();
            egui::TopBottomPanel::top("source_panel")
                .resizable(true)
                .default_height(total_h * 0.5)
                .min_height(160.0)
                .show_inside(ui, |ui| {
                    ui.columns(2, |cols| {
                        cols[0].push_id("osl_pane", |ui| {
                            self.show_osl_pane(ui, active_osl, runner_done);
                        });
                        cols[1].push_id("cell_pane", |ui| {
                            self.show_source_pane(
                                ui,
                                "Cell program  (local)",
                                &self.cell_text.clone(),
                                active_cell,
                                runner_done,
                            );
                        });
                    });
                });

            ui.columns(2, |cols| {
                cols[0].push_id("view2d", |ui| {
                    self.draw_2d_pane(ui);
                });
                cols[1].push_id("view3d", |ui| {
                    self.draw_3d_pane(ui);
                });
            });
        });
    }

    fn show_osl_pane(
        &mut self,
        ui: &mut egui::Ui,
        active_line: Option<usize>,
        completed: bool,
    ) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("OSL  (global)")
                    .strong()
                    .color(Color32::from_rgb(180, 180, 200)),
            );
            ui.label(
                egui::RichText::new("editable — hit Reset to recompile")
                    .small()
                    .color(Color32::from_rgb(140, 140, 150)),
            );
        });
        ui.separator();
        let avail = ui.available_size();
        let editor = egui::TextEdit::multiline(&mut self.source)
            .code_editor()
            .font(egui::FontId::monospace(13.0))
            .desired_width(f32::INFINITY)
            .desired_rows(20);
        let resp = ui.add_sized(avail, editor);
        if !completed {
            if let Some(line) = active_line {
                // TextEdit pads ~4px at the top; rows are ~16px at this font.
                let row_h = 16.0;
                let y_top = resp.rect.top() + 4.0 + (line as f32 - 1.0) * row_h;
                let hl = Rect::from_min_size(
                    Pos2::new(resp.rect.left() + 2.0, y_top),
                    EVec2::new(resp.rect.width() - 4.0, row_h),
                );
                ui.painter().rect_filled(
                    hl,
                    2.0,
                    Color32::from_rgba_unmultiplied(255, 215, 90, 50),
                );
                ui.painter().text(
                    Pos2::new(resp.rect.left() - 4.0, y_top),
                    egui::Align2::RIGHT_TOP,
                    "▶",
                    egui::FontId::monospace(13.0),
                    Color32::from_rgb(255, 200, 80),
                );
            }
        }
    }

    fn draw_2d_pane(&self, ui: &mut egui::Ui) {
        let avail = ui.available_size();
        let side = avail.x.min(avail.y);
        let (rect, resp) =
            ui.allocate_exact_size(EVec2::new(side, side), Sense::hover());
        let painter = ui.painter_at(rect);
        self.draw_2d(&painter, rect);
        self.handle_hover_2d(ui, &resp, rect);
    }

    fn draw_3d_pane(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_size();
        let side = avail.x.min(avail.y);
        let (rect, resp) =
            ui.allocate_exact_size(EVec2::new(side, side), Sense::click_and_drag());
        if resp.dragged() {
            let drag = resp.drag_delta();
            self.yaw -= drag.x * 0.01;
            self.pitch = (self.pitch + drag.y * 0.01).clamp(0.05, 1.55);
        }
        if resp.double_clicked() {
            self.yaw = 0.6;
            self.pitch = 1.05;
        }
        let painter = ui.painter_at(rect);
        self.draw_3d(&painter, rect);
        self.handle_hover_3d(ui, &resp, rect);
    }

    fn handle_hover_2d(&self, ui: &egui::Ui, resp: &Response, rect: Rect) {
        let r = match &self.runner {
            Some(r) => r,
            None => return,
        };
        let Some(hover) = resp.hover_pos() else { return };
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
        let pick_radius_sq = 18.0_f32 * 18.0;
        let mut best: Option<(usize, f32)> = None;
        for (i, c) in r.cells.cells.iter().enumerate() {
            let p = to_screen(c.display_pos);
            let dx = p.x - hover.x;
            let dy = p.y - hover.y;
            let d2 = dx * dx + dy * dy;
            if d2 < pick_radius_sq && best.map_or(true, |(_, bd)| d2 < bd) {
                best = Some((i, d2));
            }
        }
        if let Some((idx, _)) = best {
            egui::show_tooltip_at_pointer(
                ui.ctx(),
                egui::Id::new("cell_tooltip_2d"),
                |ui| self.render_cell_tooltip(ui, idx),
            );
        }
    }

    fn handle_hover_3d(&self, ui: &egui::Ui, resp: &Response, rect: Rect) {
        let r = match &self.runner {
            Some(r) => r,
            None => return,
        };
        let Some(hover) = resp.hover_pos() else { return };
        if r.cells.cells.is_empty() {
            return;
        }

        // Replicate draw_3d's projection so hover hits match what's drawn.
        let cy_yaw = self.yaw.cos();
        let sy_yaw = self.yaw.sin();
        let ct = self.pitch.cos();
        let st = self.pitch.sin();
        let z_exaggerate: f32 = 1.0;
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
        let project_cam = |p: Vec3| -> (f32, f32, f32) {
            let dx = (p.x - cx_w) as f32;
            let dy = (p.y - cy_w) as f32;
            let dz = (p.z - cz_w) as f32 * z_exaggerate;
            let x1 = dx * cy_yaw - dy * sy_yaw;
            let y1 = dx * sy_yaw + dy * cy_yaw;
            let z1 = dz;
            let y2 = y1 * ct + z1 * st;
            let z2 = -y1 * st + z1 * ct;
            (x1, y2, z2)
        };
        let projected: Vec<(f32, f32, f32)> = r
            .cells
            .cells
            .iter()
            .map(|c| project_cam(c.pos_3d))
            .collect();
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
        let target_scale = ((avail_w / bw).min(avail_h / bh)) * 0.92;
        let center = rect.center();
        let to_screen = |c: (f32, f32, f32)| -> Pos2 {
            Pos2 {
                x: center.x + (c.0 - bcx) * target_scale,
                y: center.y - (c.1 - bcy) * target_scale,
            }
        };

        let pick_radius_sq = 18.0_f32 * 18.0;
        let mut best: Option<(usize, f32, f32)> = None;
        for (i, &p) in projected.iter().enumerate() {
            let s = to_screen(p);
            let dx = s.x - hover.x;
            let dy = s.y - hover.y;
            let d2 = dx * dx + dy * dy;
            // Prefer closer (smaller pixel distance) and frontmost (larger z2).
            if d2 < pick_radius_sq {
                let (_x, _y, z2) = p;
                let key = d2 - z2 * 0.5; // bias to front cells
                if best.map_or(true, |(_, bk, _)| key < bk) {
                    best = Some((i, key, d2));
                }
            }
        }
        if let Some((idx, _, _)) = best {
            egui::show_tooltip_at_pointer(
                ui.ctx(),
                egui::Id::new("cell_tooltip_3d"),
                |ui| self.render_cell_tooltip(ui, idx),
            );
        }
    }

    fn render_cell_tooltip(&self, ui: &mut egui::Ui, idx: usize) {
        let Some(r) = self.runner.as_ref() else {
            return;
        };
        let c = &r.cells.cells[idx];
        ui.style_mut().override_text_style = Some(egui::TextStyle::Monospace);
        ui.label(
            egui::RichText::new(format!("Cell #{}", idx))
                .strong()
                .color(Color32::from_rgb(255, 230, 160)),
        );
        ui.label(format!(
            "pos    ({:.3}, {:.3})",
            c.display_pos.x, c.display_pos.y
        ));
        ui.label(format!(
            "layer  {}    polarity_flipped {}",
            c.layer, c.polarity_flipped
        ));

        let mut true_states: Vec<&str> = c
            .state
            .iter()
            .filter_map(|(k, v)| if *v { Some(k.as_str()) } else { None })
            .collect();
        true_states.sort();
        if true_states.is_empty() {
            ui.label(
                egui::RichText::new("state: (none true)")
                    .color(Color32::from_rgb(140, 140, 150)),
            );
        } else {
            ui.label(egui::RichText::new("state (true):").strong());
            // group into rows of three for a compact table
            let chunked: Vec<&[&str]> = true_states.chunks(3).collect();
            for chunk in chunked {
                let row = chunk
                    .iter()
                    .map(|s| format!("{:<10}", s))
                    .collect::<String>();
                ui.label(format!("  {}", row));
            }
        }
        if let Some((name, g)) = &r.cells.last_gradient {
            if let Some(v) = g.get(idx).copied().flatten() {
                ui.separator();
                ui.label(format!("active gradient {}: {:.3}", name, v));
            } else {
                ui.label(
                    egui::RichText::new(format!("active gradient {}: —", name))
                        .color(Color32::from_rgb(140, 140, 150)),
                );
            }
        }
    }

    fn show_source_pane(
        &self,
        ui: &mut egui::Ui,
        title: &str,
        source: &str,
        active_line: Option<usize>,
        completed: bool,
    ) {
        ui.label(
            egui::RichText::new(title)
                .strong()
                .color(Color32::from_rgb(180, 180, 200)),
        );
        ui.separator();
        let line_count = source.lines().count().max(1);
        let gutter_w = line_count.to_string().len();
        let row_h = 16.0;
        let font = egui::FontId::monospace(13.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (i, line) in source.lines().enumerate() {
                    let line_no = i + 1;
                    let active = !completed && active_line == Some(line_no);
                    let avail_w = ui.available_width();
                    let resp = ui.allocate_response(
                        EVec2::new(avail_w, row_h),
                        Sense::hover(),
                    );
                    let painter = ui.painter();
                    let rect = resp.rect;
                    let bg = if active {
                        Color32::from_rgb(85, 65, 20)
                    } else {
                        Color32::TRANSPARENT
                    };
                    if active {
                        painter.rect_filled(rect, 2.0, bg);
                    }
                    let prefix_color = if active {
                        Color32::from_rgb(255, 210, 100)
                    } else {
                        Color32::from_rgb(90, 90, 100)
                    };
                    let gutter_color = if active {
                        Color32::from_rgb(255, 230, 160)
                    } else {
                        Color32::from_rgb(110, 110, 120)
                    };
                    let body_color = if active {
                        Color32::from_rgb(255, 245, 200)
                    } else {
                        Color32::from_rgb(200, 200, 205)
                    };
                    // Render in three pieces: marker, gutter line number, body.
                    let mut x = rect.left() + 4.0;
                    painter.text(
                        Pos2 { x, y: rect.top() },
                        egui::Align2::LEFT_TOP,
                        if active { "▶" } else { " " },
                        font.clone(),
                        prefix_color,
                    );
                    x += 12.0;
                    let gutter_text = format!("{:>w$}", line_no, w = gutter_w);
                    painter.text(
                        Pos2 { x, y: rect.top() },
                        egui::Align2::LEFT_TOP,
                        &gutter_text,
                        font.clone(),
                        gutter_color,
                    );
                    x += (gutter_w as f32) * 8.0 + 10.0;
                    painter.text(
                        Pos2 { x, y: rect.top() },
                        egui::Align2::LEFT_TOP,
                        line,
                        font.clone(),
                        body_color,
                    );
                    if active {
                        resp.scroll_to_me(Some(Align::Center));
                    }
                }
            });
    }

    fn draw_2d(&self, painter: &Painter, rect: Rect) {
        painter.rect_filled(rect, 0.0, Color32::from_rgb(28, 30, 34));
        painter.text(
            Pos2 {
                x: rect.left() + 6.0,
                y: rect.top() + 4.0,
            },
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

        if self.show_labels {
            let labels = self.name_centroids();
            for (name, _count, centroid) in labels {
                let p = to_screen(centroid);
                let color = Self::name_color(&name);
                Self::draw_label(painter, p, &name, color);
            }
        }
    }

    /// Pill-shaped label with a colored leading dot. Used by both views.
    fn draw_label(painter: &Painter, anchor: Pos2, text: &str, color: Color32) {
        let font = egui::FontId::proportional(11.0);
        // Approximate width: char count * 6 + padding for the dot.
        let pad_x = 6.0;
        let dot_r = 3.5;
        let approx_w = text.chars().count() as f32 * 6.5 + dot_r * 2.0 + pad_x * 2.0;
        let h = 16.0;
        let rect = Rect::from_min_size(
            Pos2::new(anchor.x - approx_w * 0.5, anchor.y - h * 0.5),
            EVec2::new(approx_w, h),
        );
        painter.rect_filled(
            rect,
            8.0,
            Color32::from_rgba_unmultiplied(15, 15, 20, 215),
        );
        painter.rect_stroke(
            rect,
            8.0,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 200)),
        );
        painter.circle_filled(
            Pos2::new(rect.left() + pad_x + dot_r, rect.center().y),
            dot_r,
            color,
        );
        painter.text(
            Pos2::new(rect.left() + pad_x + dot_r * 2.0 + 4.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            font,
            Color32::from_rgb(240, 240, 245),
        );
    }

    fn draw_3d(&self, painter: &Painter, rect: Rect) {
        painter.rect_filled(rect, 0.0, Color32::from_rgb(20, 22, 26));
        painter.text(
            Pos2 {
                x: rect.left() + 6.0,
                y: rect.top() + 4.0,
            },
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

        let cy_yaw = self.yaw.cos();
        let sy_yaw = self.yaw.sin();
        let ct = self.pitch.cos();
        let st = self.pitch.sin();
        let z_exaggerate: f32 = 1.0;

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

        let project_cam = |p: Vec3| -> (f32, f32, f32) {
            let dx = (p.x - cx_w) as f32;
            let dy = (p.y - cy_w) as f32;
            let dz = (p.z - cz_w) as f32 * z_exaggerate;
            let x1 = dx * cy_yaw - dy * sy_yaw;
            let y1 = dx * sy_yaw + dy * cy_yaw;
            let z1 = dz;
            let x2 = x1;
            let y2 = y1 * ct + z1 * st;
            let z2 = -y1 * st + z1 * ct;
            (x2, y2, z2)
        };

        let projected: Vec<(f32, f32, f32)> = r
            .cells
            .cells
            .iter()
            .map(|c| project_cam(c.pos_3d))
            .collect();
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
        let target_scale = ((avail_w / bw).min(avail_h / bh)) * 0.92;

        let center = rect.center();
        let to_screen = |c: (f32, f32, f32)| -> Pos2 {
            Pos2 {
                x: center.x + (c.0 - bcx) * target_scale,
                y: center.y - (c.1 - bcy) * target_scale,
            }
        };

        let mut order: Vec<usize> = (0..projected.len()).collect();
        order.sort_by(|&a, &b| projected[a].2.partial_cmp(&projected[b].2).unwrap());

        let cell_radius = 1.6_f32.max(rect.width() / 260.0);
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
            let depth_t = (pcam.2 - zmin) / zspan;
            let brightness = 0.65 + 0.35 * depth_t;
            let color = self.cell_color(c, r, i);
            let color = Color32::from_rgb(
                (color.r() as f32 * brightness).min(255.0) as u8,
                (color.g() as f32 * brightness).min(255.0) as u8,
                (color.b() as f32 * brightness).min(255.0) as u8,
            );
            painter.circle_filled(p, cell_radius, color);
        }

        if self.show_labels {
            // Project each named entity's 3D centroid through the same camera
            // and draw the label on top of the cells.
            let labels = self.name_centroids_3d();
            // Sort by depth so labels in front overdraw labels behind.
            let mut placed: Vec<(f32, Pos2, String, Color32)> = Vec::new();
            for (name, _count, centroid) in labels {
                let cam = project_cam(centroid);
                let p = to_screen(cam);
                placed.push((cam.2, p, name.clone(), Self::name_color(&name)));
            }
            placed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            for (_z, p, name, color) in placed {
                Self::draw_label(painter, p, &name, color);
            }
        }
    }

    /// Match `cell_color`'s priority for a single named entity. Used so
    /// label swatches/text agree with the cell rendering.
    fn name_color(name: &str) -> Color32 {
        match name {
            "c1" | "c2" | "c3" | "c4" => Color32::YELLOW,
            "e12" | "e34" => Color32::from_rgb(80, 100, 220),
            "e23" | "e41" | "e14" => Color32::from_rgb(220, 80, 80),
            "d1" | "t2" => Color32::from_rgb(0, 220, 0),
            "d2" | "t3" => Color32::from_rgb(0, 220, 220),
            "d3" | "d4" | "t4" => Color32::from_rgb(220, 0, 220),
            "l1" | "cntr-line" => Color32::from_rgb(220, 220, 0),
            "front" => Color32::from_rgb(180, 140, 220),
            "back" => Color32::from_rgb(140, 180, 220),
            "rside" => Color32::from_rgb(180, 220, 140),
            "lside" => Color32::from_rgb(220, 180, 140),
            "p1" | "p2" | "p3" => Color32::from_rgb(255, 200, 100),
            _ => Color32::from_rgb(220, 220, 220),
        }
    }

    /// For each named entity, compute the centroid of true-state cells.
    /// Filters out names with too few cells (noise) and the meta var
    /// `current-region`. Returns `(name, count, centroid_2d)`.
    fn name_centroids(&self) -> Vec<(String, usize, Vec2)> {
        let mut out = Vec::new();
        let r = match &self.runner {
            Some(r) => r,
            None => return out,
        };
        for name in &self.label_names {
            if name == "current-region" {
                continue;
            }
            let mut sx = 0.0;
            let mut sy = 0.0;
            let mut n = 0usize;
            for c in &r.cells.cells {
                if *c.state.get(name).unwrap_or(&false) {
                    sx += c.display_pos.x;
                    sy += c.display_pos.y;
                    n += 1;
                }
            }
            if n >= 2 {
                out.push((name.clone(), n, Vec2::new(sx / n as f64, sy / n as f64)));
            }
        }
        out
    }

    /// 3D version of `name_centroids` — uses `pos_3d` so labels follow folded
    /// cells through the stack.
    fn name_centroids_3d(&self) -> Vec<(String, usize, Vec3)> {
        let mut out = Vec::new();
        let r = match &self.runner {
            Some(r) => r,
            None => return out,
        };
        for name in &self.label_names {
            if name == "current-region" {
                continue;
            }
            let (mut sx, mut sy, mut sz) = (0.0, 0.0, 0.0);
            let mut n = 0usize;
            for c in &r.cells.cells {
                if *c.state.get(name).unwrap_or(&false) {
                    sx += c.pos_3d.x;
                    sy += c.pos_3d.y;
                    sz += c.pos_3d.z;
                    n += 1;
                }
            }
            if n >= 2 {
                out.push((
                    name.clone(),
                    n,
                    Vec3::new(sx / n as f64, sy / n as f64, sz / n as f64),
                ));
            }
        }
        out
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
        viewport: egui::ViewportBuilder::default().with_inner_size([1480.0, 900.0]),
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
