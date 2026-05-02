//! bgcompile: compile an OSL program to a cell program (a Lisp s-expression
//! sequence of axiomN-rule / intersect-rule / create-region-rule /
//! execute-fold-rule calls). Output is human-readable text.
//!
//! Usage:
//!   bgcompile <file.osl> [-o <out.cell>]
//!   bgcompile -                  (read from stdin)

use anyhow::{anyhow, Result};
use biogami::compile::{expand_defuns, Compiler};
use biogami::parser::parse;
use std::fs;
use std::io::{self, Read, Write};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: bgcompile <file.osl|-> [-o <out.cell>]");
        std::process::exit(2);
    }
    let mut input = None;
    let mut output: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                output = Some(
                    args.get(i + 1)
                        .cloned()
                        .ok_or_else(|| anyhow!("-o needs an argument"))?,
                );
                i += 2;
            }
            "-h" | "--help" => {
                println!("usage: bgcompile <file.osl|-> [-o <out.cell>]");
                return Ok(());
            }
            other => {
                input = Some(other.to_string());
                i += 1;
            }
        }
    }
    let src = match input.as_deref() {
        Some("-") | None => {
            let mut s = String::new();
            io::stdin().read_to_string(&mut s)?;
            s
        }
        Some(path) => fs::read_to_string(path)?,
    };

    let prog = parse(&src)?;
    let expanded = expand_defuns(&prog)?;
    let mut c = Compiler::new();
    let cp = c.compile(&expanded)?;
    let text = cp.to_source();

    match output {
        Some(path) => {
            fs::write(&path, &text)?;
            eprintln!("wrote {} ({} bytes)", path, text.len());
        }
        None => {
            io::stdout().write_all(text.as_bytes())?;
        }
    }
    Ok(())
}
