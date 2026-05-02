//! bgrun: run an OSL program. By default runs the global geometric interpreter
//! and prints the resulting crease pattern + folded layer count. With `--gui`
//! launches the egui visualiser of the cell-level simulation. With
//! `--cell-sim` prints a text trace of the cell-level simulation steps.
//!
//! Usage:
//!   bgrun <file.osl> [--gui] [--cell-sim] [--cells N] [--radius R] [--seed S]

use anyhow::{anyhow, Result};
use biogami::cellsim::{CellSheet, Runner};
use biogami::compile::{expand_defuns, Compiler};
use biogami::interp::Interp;
use biogami::parser::parse;
use std::fs;

struct Args {
    file: String,
    gui: bool,
    cell_sim: bool,
    cells: usize,
    radius: f64,
    seed: u64,
}

fn parse_args() -> Result<Args> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.is_empty() {
        anyhow::bail!(
            "usage: bgrun <file.osl> [--gui] [--cell-sim] [--cells N] [--radius R] [--seed S]"
        );
    }
    let mut a = Args {
        file: String::new(),
        gui: false,
        cell_sim: false,
        cells: 1500,
        radius: 0.06,
        seed: 42,
    };
    let mut i = 0;
    while i < raw.len() {
        match raw[i].as_str() {
            "--gui" => {
                a.gui = true;
                i += 1;
            }
            "--cell-sim" => {
                a.cell_sim = true;
                i += 1;
            }
            "--cells" => {
                a.cells = raw
                    .get(i + 1)
                    .ok_or_else(|| anyhow!("--cells needs N"))?
                    .parse()?;
                i += 2;
            }
            "--radius" => {
                a.radius = raw
                    .get(i + 1)
                    .ok_or_else(|| anyhow!("--radius needs R"))?
                    .parse()?;
                i += 2;
            }
            "--seed" => {
                a.seed = raw
                    .get(i + 1)
                    .ok_or_else(|| anyhow!("--seed needs S"))?
                    .parse()?;
                i += 2;
            }
            "-h" | "--help" => {
                println!(
                    "usage: bgrun <file.osl> [--gui] [--cell-sim] [--cells N] [--radius R] [--seed S]"
                );
                std::process::exit(0);
            }
            other => {
                if a.file.is_empty() {
                    a.file = other.to_string();
                } else {
                    anyhow::bail!("unexpected arg: {}", other);
                }
                i += 1;
            }
        }
    }
    if a.file.is_empty() {
        anyhow::bail!("no input file");
    }
    Ok(a)
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let src = fs::read_to_string(&args.file)?;

    if args.gui {
        biogami::ui::run_app(&args.file, &src)
            .map_err(|e| anyhow::anyhow!("gui error: {}", e))?;
        return Ok(());
    }

    let prog = parse(&src)?;

    if args.cell_sim {
        let expanded = expand_defuns(&prog)?;
        let mut c = Compiler::new();
        let cp = c.compile(&expanded)?;
        let cells = CellSheet::new(1.0, args.cells, args.radius, args.seed);
        let mut r = Runner::new(cells, cp.ops);
        r.fold_duration = 0.0; // CLI: instantaneous folds
        let mut step = 0;
        while !r.done() {
            let (op_before, phase_before) = (r.pc, r.phase);
            r.step()?;
            step += 1;
            println!(
                "step {:>3} (op {} phase {}): {}",
                step, op_before, phase_before, r.last_message
            );
        }
        println!(
            "done. {} cells, {} layers, {} creases",
            r.cells.cells.len(),
            r.cells.sheet.layers.len(),
            r.cells.sheet.creases.len()
        );
    } else {
        let mut interp = Interp::new(1.0);
        interp.run(&prog)?;
        println!("creases ({}):", interp.sheet.creases.len());
        for c in &interp.sheet.creases {
            println!(
                "  {:>8}  point=({:.3},{:.3})  dir=({:.3},{:.3})",
                c.color, c.line.p.x, c.line.p.y, c.line.dir.x, c.line.dir.y
            );
        }
        println!("layers: {}", interp.sheet.layers.len());
        for (i, l) in interp.sheet.layers.iter().enumerate() {
            println!(
                "  layer {} ({:?}, {} verts)",
                i,
                l.polarity,
                l.poly.len()
            );
        }
    }
    Ok(())
}
