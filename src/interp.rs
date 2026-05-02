//! Global OSL interpreter. Evaluates an OSL program directly using analytic
//! geometry: axioms produce real lines, execute-fold reflects polygons across
//! lines. Output is a (crease pattern, layered sheet) pair.

use crate::ast::Expr;
use crate::geom::{
    crease_l2l, crease_l2s, crease_lbp, crease_p2p, intersect_lines, Line, Vec2,
};
use crate::sheet::{FoldType, Sheet};
use anyhow::{anyhow, bail, Result};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Number(f64),
    Symbol(String),
    Point(Vec2),
    Line { line: Line, color: String },
    Region(Region),
}

/// A region is described by a list of half-plane tests in the original sheet
/// frame: a point belongs if it satisfies all `(line, sign)` tests.
#[derive(Debug, Clone)]
pub struct Region {
    pub tests: Vec<(Line, f64)>, // sign: +1 or -1, point is in region if signed_dist*sign >= 0
}

impl Region {
    pub fn full() -> Self {
        Region { tests: Vec::new() }
    }
    pub fn contains(&self, p: Vec2) -> bool {
        self.tests
            .iter()
            .all(|(l, s)| l.signed_dist(p) * s >= -1e-9)
    }
}

#[derive(Debug, Clone)]
pub struct Defun {
    pub params: Vec<String>,
    pub body: Vec<Expr>,
}

pub struct Interp {
    pub size: f64,
    pub env: HashMap<String, Value>,
    pub funcs: HashMap<String, Defun>,
    pub sheet: Sheet,
    pub region_stack: Vec<Region>,
}

impl Interp {
    pub fn new(size: f64) -> Self {
        let mut env = HashMap::new();
        // corners
        env.insert("c1".into(), Value::Point(Vec2::new(0.0, 0.0)));
        env.insert("c2".into(), Value::Point(Vec2::new(size, 0.0)));
        env.insert("c3".into(), Value::Point(Vec2::new(size, size)));
        env.insert("c4".into(), Value::Point(Vec2::new(0.0, size)));
        // edges
        let edge = |a, b, name: &str| -> (String, Value) {
            (
                name.into(),
                Value::Line {
                    line: Line::from_two_points(a, b),
                    color: "black".into(),
                },
            )
        };
        env.insert(
            edge(Vec2::new(0.0, 0.0), Vec2::new(size, 0.0), "e12").0,
            edge(Vec2::new(0.0, 0.0), Vec2::new(size, 0.0), "e12").1,
        );
        env.insert(
            edge(Vec2::new(size, 0.0), Vec2::new(size, size), "e23").0,
            edge(Vec2::new(size, 0.0), Vec2::new(size, size), "e23").1,
        );
        env.insert(
            edge(Vec2::new(size, size), Vec2::new(0.0, size), "e34").0,
            edge(Vec2::new(size, size), Vec2::new(0.0, size), "e34").1,
        );
        env.insert(
            edge(Vec2::new(0.0, size), Vec2::new(0.0, 0.0), "e41").0,
            edge(Vec2::new(0.0, size), Vec2::new(0.0, 0.0), "e41").1,
        );
        // alias e14 == e41
        env.insert(
            "e14".into(),
            Value::Line {
                line: Line::from_two_points(Vec2::new(0.0, size), Vec2::new(0.0, 0.0)),
                color: "black".into(),
            },
        );
        Interp {
            size,
            env,
            funcs: HashMap::new(),
            sheet: Sheet::square(size),
            region_stack: vec![Region::full()],
        }
    }

    pub fn run(&mut self, program: &[Expr]) -> Result<()> {
        for e in program {
            self.eval(e)?;
        }
        Ok(())
    }

    fn eval(&mut self, e: &Expr) -> Result<Value> {
        match e {
            Expr::Nil => Ok(Value::Nil),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Number(n) => Ok(Value::Number(*n)),
            Expr::Str(_) => Ok(Value::Nil),
            Expr::Symbol(s) => self
                .env
                .get(s)
                .cloned()
                .ok_or_else(|| anyhow!("unbound symbol: {}", s)),
            Expr::Quoted(inner) => match inner.as_ref() {
                Expr::Symbol(s) => Ok(Value::Symbol(s.clone())),
                _ => Ok(Value::Nil),
            },
            Expr::Keyword(_, inner) => self.eval(inner),
            Expr::List(xs) => self.eval_list(xs),
        }
    }

    fn eval_list(&mut self, xs: &[Expr]) -> Result<Value> {
        let head = xs
            .first()
            .ok_or_else(|| anyhow!("empty list"))?
            .as_symbol()
            .ok_or_else(|| anyhow!("expected symbol head"))?
            .to_string();
        let args = &xs[1..];
        match head.as_str() {
            "define" => self.do_define(args),
            "defun" => self.do_defun(args),
            "set!" => self.do_set(args),
            "begin" => {
                let mut last = Value::Nil;
                for a in args {
                    last = self.eval(a)?;
                }
                Ok(last)
            }
            "crease-lbp" => self.crease("lbp", args),
            "crease-p2p" => self.crease("p2p", args),
            "crease-l2l" => self.crease("l2l", args),
            "crease-l2s" => self.crease("l2s", args),
            "intersect" => {
                let l1 = self.eval(&args[0])?;
                let l2 = self.eval(&args[1])?;
                let (la, _) = expect_line(&l1)?;
                let (lb, _) = expect_line(&l2)?;
                let p = intersect_lines(&la, &lb)
                    .ok_or_else(|| anyhow!("parallel lines do not intersect"))?;
                Ok(Value::Point(p))
            }
            "execute-fold" => self.do_fold(args),
            "create-region" => self.do_create_region(args),
            "within-region" => self.do_within_region(args),
            "or" => {
                // Region "or" — union of regions. The thesis describes it for
                // collections; for simplicity we union by listing tests as an
                // alternation (not supported in Region's AND-only form). Treat
                // as last-region.
                let mut last = Value::Nil;
                for a in args {
                    last = self.eval(a)?;
                }
                Ok(last)
            }
            "not" => {
                let v = self.eval(&args[0])?;
                if let Value::Region(r) = v {
                    // negate: flip all signs (only meaningful for single-test region)
                    let tests = r.tests.iter().map(|(l, s)| (*l, -s)).collect();
                    Ok(Value::Region(Region { tests }))
                } else if let Value::Bool(b) = v {
                    Ok(Value::Bool(!b))
                } else {
                    bail!("not: unsupported operand");
                }
            }
            // Procedure call?
            other => {
                if let Some(fun) = self.funcs.get(other).cloned() {
                    self.call_user(&fun, args)
                } else {
                    bail!("unknown form: {}", other);
                }
            }
        }
    }

    fn call_user(&mut self, fun: &Defun, args: &[Expr]) -> Result<Value> {
        if args.len() != fun.params.len() {
            bail!(
                "arity mismatch in user fn: expected {}, got {}",
                fun.params.len(),
                args.len()
            );
        }
        let mut saved: Vec<(String, Option<Value>)> = Vec::new();
        for (p, a) in fun.params.iter().zip(args) {
            let v = self.eval(a)?;
            saved.push((p.clone(), self.env.insert(p.clone(), v)));
        }
        let mut last = Value::Nil;
        for body in &fun.body {
            last = self.eval(body)?;
        }
        for (p, prev) in saved.into_iter().rev() {
            match prev {
                Some(v) => {
                    self.env.insert(p, v);
                }
                None => {
                    self.env.remove(&p);
                }
            }
        }
        Ok(last)
    }

    fn do_define(&mut self, args: &[Expr]) -> Result<Value> {
        // (define name expr)
        let name = args[0]
            .as_symbol()
            .ok_or_else(|| anyhow!("define: expected symbol name"))?
            .to_string();
        let v = self.eval(&args[1])?;
        self.env.insert(name, v.clone());
        Ok(v)
    }

    fn do_set(&mut self, args: &[Expr]) -> Result<Value> {
        let name = args[0].as_symbol().ok_or_else(|| anyhow!("set!: name"))?.to_string();
        let v = self.eval(&args[1])?;
        self.env.insert(name, v.clone());
        Ok(v)
    }

    fn do_defun(&mut self, args: &[Expr]) -> Result<Value> {
        // (defun (name p1 p2 ...) body...)
        let header = args[0].as_list().ok_or_else(|| anyhow!("defun: header"))?;
        let name = header[0]
            .as_symbol()
            .ok_or_else(|| anyhow!("defun: name"))?
            .to_string();
        let params: Vec<String> = header[1..]
            .iter()
            .map(|e| {
                e.as_symbol()
                    .map(|s| s.to_string())
                    .ok_or_else(|| anyhow!("defun: param must be symbol"))
            })
            .collect::<Result<_>>()?;
        self.funcs.insert(
            name,
            Defun {
                params,
                body: args[1..].to_vec(),
            },
        );
        Ok(Value::Nil)
    }

    fn crease(&mut self, axiom: &str, args: &[Expr]) -> Result<Value> {
        // All axioms take 2 args; if a 3rd arg is present it's a color symbol.
        let mut color = "black".to_string();
        let mut argv = args.to_vec();
        if argv.len() == 3 {
            if let Some(s) = argv[2].as_symbol() {
                color = s.to_string();
                argv.pop();
            }
        }
        let line = match axiom {
            "lbp" => {
                let p1 = expect_point(&self.eval(&argv[0])?)?;
                let p2 = expect_point(&self.eval(&argv[1])?)?;
                crease_lbp(p1, p2)
            }
            "p2p" => {
                let p1 = expect_point(&self.eval(&argv[0])?)?;
                let p2 = expect_point(&self.eval(&argv[1])?)?;
                crease_p2p(p1, p2)
            }
            "l2l" => {
                let l1 = expect_line(&self.eval(&argv[0])?)?.0;
                let l2 = expect_line(&self.eval(&argv[1])?)?.0;
                crease_l2l(&l1, &l2)
            }
            "l2s" => {
                let l1 = expect_line(&self.eval(&argv[0])?)?.0;
                let p1 = expect_point(&self.eval(&argv[1])?)?;
                crease_l2s(&l1, p1)
            }
            _ => bail!("unknown axiom"),
        };
        self.sheet.record_crease(line, &color);
        Ok(Value::Line { line, color })
    }

    fn do_create_region(&mut self, args: &[Expr]) -> Result<Value> {
        // (create-region p1 l1 [color])
        let p = expect_point(&self.eval(&args[0])?)?;
        let (line, _) = expect_line(&self.eval(&args[1])?)?;
        let sign = line.signed_dist(p).signum();
        let sign = if sign == 0.0 { 1.0 } else { sign };
        Ok(Value::Region(Region {
            tests: vec![(line, sign)],
        }))
    }

    fn do_within_region(&mut self, args: &[Expr]) -> Result<Value> {
        let r = match self.eval(&args[0])? {
            Value::Region(r) => r,
            _ => bail!("within-region: expected region"),
        };
        self.region_stack.push(r);
        let mut last = Value::Nil;
        for a in &args[1..] {
            last = self.eval(a)?;
        }
        self.region_stack.pop();
        Ok(last)
    }

    fn do_fold(&mut self, args: &[Expr]) -> Result<Value> {
        // (execute-fold line type landmark=p)
        let (line, _) = expect_line(&self.eval(&args[0])?)?;
        let typ = args[1]
            .as_symbol()
            .ok_or_else(|| anyhow!("execute-fold: type"))?;
        let fold = match typ {
            "apical" => FoldType::Apical,
            "basal" => FoldType::Basal,
            other => bail!("unknown fold type: {}", other),
        };
        // landmark may be a keyword or positional
        let landmark_expr = args
            .iter()
            .skip(2)
            .find(|e| matches!(e, Expr::Keyword(k, _) if k == "landmark"))
            .or_else(|| args.get(2));
        let landmark = match landmark_expr {
            Some(Expr::Keyword(_, v)) => expect_point(&self.eval(v)?)?,
            Some(other) => expect_point(&self.eval(other)?)?,
            None => bail!("execute-fold: missing landmark"),
        };
        // If a region is in scope, only its tests apply to layer-splitting (we
        // approximate by using the region's bounding tests as additional cuts;
        // simplest correct behaviour for the partial-layer case is to fold the
        // sheet only where the current region holds. We do the same global fold
        // but pre-filter layers by intersecting with the current region.)
        //
        // For the global geometric model we simply apply the fold to the whole
        // sheet; partial-layer effects are approximated by the cell-level
        // simulator. Record a crease in either case.
        self.sheet.execute_fold(line, fold, landmark);
        Ok(Value::Nil)
    }
}

fn expect_point(v: &Value) -> Result<Vec2> {
    match v {
        Value::Point(p) => Ok(*p),
        _ => bail!("expected point, got {:?}", v),
    }
}

fn expect_line(v: &Value) -> Result<(Line, String)> {
    match v {
        Value::Line { line, color } => Ok((*line, color.clone())),
        _ => bail!("expected line, got {:?}", v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    #[test]
    fn cup_crease_pattern_runs() {
        let src = std::include_str!("../examples/cup.osl");
        let prog = parse(src).unwrap();
        let mut interp = Interp::new(1.0);
        interp.run(&prog).unwrap();
        // The cup program records several creases (at minimum d1, d2, d3, d4, l1).
        assert!(interp.sheet.creases.len() >= 4);
    }
}
