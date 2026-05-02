//! Compiler: OSL program → cell program. Each OSL operation becomes a call to
//! the corresponding cell-program rule (axiom1-rule, axiom2-rule, intersect-rule,
//! create-region-rule, execute-fold-rule). Gradient names are allocated per op
//! per the thesis (3 gradients per axiom; reusable after a fold via `flush`).
//!
//! The cell program is itself a Lisp-flavoured s-expression, ready to be run
//! by the cell simulator (see `cellsim.rs` driver in `interp.rs::run_cell`).

use crate::ast::Expr;
use anyhow::{anyhow, bail, Result};

pub struct Compiler {
    next_gradient: usize,
    next_temp: usize,
}

#[derive(Debug, Clone)]
pub struct CellProgram {
    /// Local boolean state vars (from defines). The cell already has e12-e41,
    /// c1-c4 implicitly.
    pub state_vars: Vec<String>,
    /// Lowered s-expressions, executed sequentially.
    pub ops: Vec<Expr>,
}

impl CellProgram {
    pub fn to_source(&self) -> String {
        let mut out = String::new();
        out.push_str(";; CELL PROGRAM\n");
        out.push_str(";; initial state: e12-e41, c1-c4, current-region=#t\n");
        for v in &self.state_vars {
            out.push_str(&format!("(define {} #f)\n", v));
        }
        out.push_str("(set! local-delay (calibrate))\n");
        for op in &self.ops {
            out.push_str(&format!("{}\n", op));
        }
        out
    }
}

impl Compiler {
    pub fn new() -> Self {
        Compiler {
            next_gradient: 1,
            next_temp: 1,
        }
    }

    fn fresh_grad(&mut self) -> String {
        let n = self.next_gradient;
        self.next_gradient += 1;
        format!("g{}", n)
    }

    fn fresh_temp(&mut self) -> String {
        let n = self.next_temp;
        self.next_temp += 1;
        format!("t{}", n)
    }

    pub fn compile(&mut self, program: &[Expr]) -> Result<CellProgram> {
        let mut state_vars = Vec::new();
        let mut ops = Vec::new();
        for e in program {
            self.compile_top(e, &mut state_vars, &mut ops)?;
        }
        Ok(CellProgram { state_vars, ops })
    }

    fn compile_top(
        &mut self,
        e: &Expr,
        state: &mut Vec<String>,
        ops: &mut Vec<Expr>,
    ) -> Result<()> {
        let xs = e
            .as_list()
            .ok_or_else(|| anyhow!("compiler: top-level must be a list"))?;
        let head = xs[0]
            .as_symbol()
            .ok_or_else(|| anyhow!("compiler: head must be a symbol"))?;
        match head {
            "define" => {
                let name = xs[1]
                    .as_symbol()
                    .ok_or_else(|| anyhow!("compiler: define name"))?
                    .to_string();
                state.push(name.clone());
                let rhs = self.compile_op(&xs[2], &name)?;
                ops.push(set_call(&name, rhs));
                Ok(())
            }
            "defun" => {
                // Inline expansion at call sites — we don't emit cell-level fns.
                // Store the body for later expansion.
                bail!("compiler: defun must be expanded prior to compile (not yet supported)");
            }
            "execute-fold" => {
                let op = self.compile_execute_fold(xs)?;
                ops.push(op);
                Ok(())
            }
            "within-region" => {
                let region = xs[1]
                    .as_symbol()
                    .ok_or_else(|| anyhow!("compiler: within-region region must be a symbol"))?
                    .to_string();
                ops.push(set_call("current-region", Expr::sym(&region)));
                for body in &xs[2..] {
                    self.compile_top(body, state, ops)?;
                }
                ops.push(set_call("current-region", Expr::Bool(true)));
                Ok(())
            }
            other => bail!("compiler: unsupported top-level form: {}", other),
        }
    }

    fn compile_op(&mut self, e: &Expr, dst: &str) -> Result<Expr> {
        let xs = e.as_list().ok_or_else(|| anyhow!("compiler: op must be a list"))?;
        let head = xs[0]
            .as_symbol()
            .ok_or_else(|| anyhow!("compiler: op head"))?;
        match head {
            "crease-lbp" | "crease-l2s" => {
                // axiom1-rule p1 p2 gstart gend
                let p1 = symbol_or_err(&xs[1])?;
                let p2 = symbol_or_err(&xs[2])?;
                let g1 = self.fresh_grad();
                let g2 = self.fresh_grad();
                Ok(Expr::list(vec![
                    Expr::sym("axiom1-rule"),
                    Expr::sym(&p1),
                    Expr::sym(&p2),
                    Expr::sym(&g1),
                    Expr::sym(&g2),
                ]))
            }
            "crease-p2p" | "crease-l2l" => {
                // axiom2-rule p1 p2 g1 g2 gend
                let p1 = symbol_or_err(&xs[1])?;
                let p2 = symbol_or_err(&xs[2])?;
                let g1 = self.fresh_grad();
                let g2 = self.fresh_grad();
                let gend = self.fresh_grad();
                Ok(Expr::list(vec![
                    Expr::sym("axiom2-rule"),
                    Expr::sym(&p1),
                    Expr::sym(&p2),
                    Expr::sym(&g1),
                    Expr::sym(&g2),
                    Expr::sym(&gend),
                ]))
            }
            "intersect" => {
                let l1 = symbol_or_err(&xs[1])?;
                let l2 = symbol_or_err(&xs[2])?;
                Ok(Expr::list(vec![
                    Expr::sym("intersect-rule"),
                    Expr::sym(&l1),
                    Expr::sym(&l2),
                ]))
            }
            "create-region" => {
                let p1 = symbol_or_err(&xs[1])?;
                let l1 = symbol_or_err(&xs[2])?;
                let gbound = self.fresh_grad();
                let gend = self.fresh_grad();
                let _ = dst;
                Ok(Expr::list(vec![
                    Expr::sym("create-region-rule"),
                    Expr::sym(&p1),
                    Expr::sym(&l1),
                    Expr::sym(&gbound),
                    Expr::sym(&gend),
                ]))
            }
            other => bail!("compiler: unsupported rhs op: {}", other),
        }
    }

    fn compile_execute_fold(&mut self, xs: &[Expr]) -> Result<Expr> {
        // (execute-fold line type landmark=p)
        let line = symbol_or_err(&xs[1])?;
        let typ = symbol_or_err(&xs[2])?;
        // landmark
        let landmark = xs
            .iter()
            .skip(3)
            .find_map(|e| match e {
                Expr::Keyword(k, v) if k == "landmark" => v.as_symbol().map(|s| s.to_string()),
                Expr::Symbol(s) => Some(s.clone()),
                _ => None,
            })
            .ok_or_else(|| anyhow!("compiler: execute-fold missing landmark"))?;
        let gbounded = self.fresh_grad();
        let gend = self.fresh_grad();
        let _ = self.fresh_temp();
        Ok(Expr::list(vec![
            Expr::sym("execute-fold-rule"),
            Expr::sym(&line),
            Expr::sym(&typ),
            Expr::sym(&landmark),
            Expr::sym(&gbounded),
            Expr::sym(&gend),
        ]))
    }
}

fn set_call(name: &str, rhs: Expr) -> Expr {
    Expr::list(vec![Expr::sym("set!"), Expr::sym(name), rhs])
}

fn symbol_or_err(e: &Expr) -> Result<String> {
    e.as_symbol()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("compiler: expected symbol"))
}

/// Expand `(defun (name args...) body...)` calls inline so the compiler sees a
/// flat sequence of OSL operations. Returns the expanded program.
pub fn expand_defuns(program: &[Expr]) -> Result<Vec<Expr>> {
    use std::collections::HashMap;
    let mut funcs: HashMap<String, (Vec<String>, Vec<Expr>)> = HashMap::new();
    let mut out = Vec::new();
    for e in program {
        if let Some("defun") = e.head_symbol() {
            let xs = e.as_list().unwrap();
            let header = xs[1]
                .as_list()
                .ok_or_else(|| anyhow!("defun: header"))?;
            let name = header[0]
                .as_symbol()
                .ok_or_else(|| anyhow!("defun: name"))?
                .to_string();
            let params: Vec<String> = header[1..]
                .iter()
                .map(|p| {
                    p.as_symbol()
                        .map(|s| s.to_string())
                        .ok_or_else(|| anyhow!("defun: param"))
                })
                .collect::<Result<_>>()?;
            let body = xs[2..].to_vec();
            funcs.insert(name, (params, body));
            continue;
        }
        out.push(expand_calls(e, &funcs)?);
    }
    // Defuns introduce nested calls; flatten any block that ended up as a list
    // of statements (we wrap inlined bodies in `begin`).
    Ok(flatten_begin(out))
}

fn expand_calls(
    e: &Expr,
    funcs: &std::collections::HashMap<String, (Vec<String>, Vec<Expr>)>,
) -> Result<Expr> {
    if let Expr::List(xs) = e {
        if let Some(head) = xs.first().and_then(|e| e.as_symbol()) {
            if let Some((params, body)) = funcs.get(head) {
                if xs.len() - 1 != params.len() {
                    bail!(
                        "arity mismatch in {}: expected {}, got {}",
                        head,
                        params.len(),
                        xs.len() - 1
                    );
                }
                let bindings: std::collections::HashMap<String, Expr> = params
                    .iter()
                    .cloned()
                    .zip(xs[1..].iter().cloned())
                    .collect();
                let mut inlined: Vec<Expr> = Vec::new();
                for stmt in body {
                    inlined.push(expand_calls(&substitute(stmt, &bindings), funcs)?);
                }
                let mut block = vec![Expr::sym("begin")];
                block.extend(inlined);
                return Ok(Expr::List(block));
            }
        }
        let mut new = Vec::with_capacity(xs.len());
        for c in xs {
            new.push(expand_calls(c, funcs)?);
        }
        return Ok(Expr::List(new));
    }
    Ok(e.clone())
}

fn substitute(e: &Expr, bindings: &std::collections::HashMap<String, Expr>) -> Expr {
    match e {
        Expr::Symbol(s) => bindings.get(s).cloned().unwrap_or_else(|| e.clone()),
        Expr::Keyword(k, v) => Expr::Keyword(k.clone(), Box::new(substitute(v, bindings))),
        Expr::List(xs) => Expr::List(xs.iter().map(|c| substitute(c, bindings)).collect()),
        Expr::Quoted(inner) => Expr::Quoted(Box::new(substitute(inner, bindings))),
        _ => e.clone(),
    }
}

fn flatten_begin(stmts: Vec<Expr>) -> Vec<Expr> {
    let mut out = Vec::new();
    for s in stmts {
        if let Expr::List(xs) = &s {
            if xs.first().and_then(|e| e.as_symbol()) == Some("begin") {
                for c in xs.iter().skip(1).cloned() {
                    out.extend(flatten_begin(vec![c]));
                }
                continue;
            }
        }
        out.push(s);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    #[test]
    fn cup_compiles() {
        let src = include_str!("../examples/cup.osl");
        let prog = parse(src).unwrap();
        let expanded = expand_defuns(&prog).unwrap();
        let mut c = Compiler::new();
        let cp = c.compile(&expanded).unwrap();
        let s = cp.to_source();
        assert!(s.contains("axiom2-rule"));
        assert!(s.contains("execute-fold-rule"));
        assert!(s.contains("create-region-rule"));
    }

    #[test]
    fn airplane_inlines_defun() {
        let src = include_str!("../examples/airplane.osl");
        let prog = parse(src).unwrap();
        let expanded = expand_defuns(&prog).unwrap();
        // After expansion there should be no `defun` left.
        assert!(expanded.iter().all(|e| e.head_symbol() != Some("defun")));
    }
}
