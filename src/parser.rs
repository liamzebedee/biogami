use crate::ast::{Expr, TopForm};
use anyhow::{anyhow, bail, Result};

#[derive(Debug, Clone, PartialEq)]
enum TokKind {
    LParen,
    RParen,
    Quote,
    Atom(String),
    Str(String),
}

#[derive(Debug, Clone, PartialEq)]
struct Tok {
    kind: TokKind,
    line: usize,
}

fn tokenize(src: &str) -> Result<Vec<Tok>> {
    let mut out = Vec::new();
    let mut chars = src.chars().peekable();
    let mut line: usize = 1;
    while let Some(&c) = chars.peek() {
        match c {
            '\n' => {
                chars.next();
                line += 1;
            }
            ' ' | '\t' | '\r' | ',' => {
                chars.next();
            }
            ';' => {
                while let Some(&c) = chars.peek() {
                    if c == '\n' {
                        break;
                    }
                    chars.next();
                }
            }
            '(' => {
                chars.next();
                out.push(Tok {
                    kind: TokKind::LParen,
                    line,
                });
            }
            ')' => {
                chars.next();
                out.push(Tok {
                    kind: TokKind::RParen,
                    line,
                });
            }
            '\'' => {
                chars.next();
                out.push(Tok {
                    kind: TokKind::Quote,
                    line,
                });
            }
            '"' => {
                chars.next();
                let mut s = String::new();
                while let Some(&c) = chars.peek() {
                    chars.next();
                    if c == '"' {
                        break;
                    }
                    if c == '\\' {
                        if let Some(&n) = chars.peek() {
                            chars.next();
                            match n {
                                'n' => s.push('\n'),
                                't' => s.push('\t'),
                                '\\' => s.push('\\'),
                                '"' => s.push('"'),
                                other => s.push(other),
                            }
                        }
                    } else {
                        if c == '\n' {
                            line += 1;
                        }
                        s.push(c);
                    }
                }
                out.push(Tok {
                    kind: TokKind::Str(s),
                    line,
                });
            }
            _ => {
                let start_line = line;
                let mut s = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || c == '(' || c == ')' || c == ';' || c == '\'' {
                        break;
                    }
                    s.push(c);
                    chars.next();
                }
                if s.is_empty() {
                    return Err(anyhow!("unexpected character"));
                }
                out.push(Tok {
                    kind: TokKind::Atom(s),
                    line: start_line,
                });
            }
        }
    }
    Ok(out)
}

fn parse_atom(s: &str) -> Expr {
    if s == "#t" {
        return Expr::Bool(true);
    }
    if s == "#f" {
        return Expr::Bool(false);
    }
    if let Ok(n) = s.parse::<f64>() {
        return Expr::Number(n);
    }
    if let Some((k, v)) = s.split_once('=') {
        if !k.is_empty() && !v.is_empty() && k.chars().all(is_sym_char) {
            return Expr::Keyword(k.to_string(), Box::new(parse_atom(v)));
        }
    }
    Expr::Symbol(s.to_string())
}

fn is_sym_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | '?' | '!' | '*' | '/' | '+' | '<' | '>')
}

fn parse_one(toks: &[Tok], i: &mut usize) -> Result<Expr> {
    let t = toks
        .get(*i)
        .ok_or_else(|| anyhow!("unexpected end of input"))?
        .clone();
    *i += 1;
    match t.kind {
        TokKind::LParen => {
            let mut items = Vec::new();
            while let Some(t) = toks.get(*i) {
                if matches!(t.kind, TokKind::RParen) {
                    *i += 1;
                    return Ok(Expr::List(items));
                }
                items.push(parse_one(toks, i)?);
            }
            bail!("missing closing paren");
        }
        TokKind::RParen => bail!("unexpected ')'"),
        TokKind::Quote => {
            let inner = parse_one(toks, i)?;
            Ok(Expr::Quoted(Box::new(inner)))
        }
        TokKind::Atom(s) => Ok(parse_atom(&s)),
        TokKind::Str(s) => Ok(Expr::Str(s)),
    }
}

/// Parse a complete OSL program. Returns each top-level form along with the
/// source line where it began.
pub fn parse(src: &str) -> Result<Vec<TopForm>> {
    let toks = tokenize(src)?;
    let mut i = 0;
    let mut out = Vec::new();
    while i < toks.len() {
        let line = toks[i].line;
        let expr = parse_one(&toks, &mut i)?;
        out.push(TopForm { line, expr });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_define() {
        let forms = parse("(define d1 (crease-p2p c3 c1))").unwrap();
        assert_eq!(forms.len(), 1);
        assert_eq!(forms[0].expr.head_symbol(), Some("define"));
        assert_eq!(forms[0].line, 1);
    }

    #[test]
    fn parses_keyword_arg() {
        let forms = parse("(execute-fold d1 apical landmark=c3)").unwrap();
        let xs = forms[0].expr.as_list().unwrap();
        assert!(matches!(&xs[3], Expr::Keyword(k, _) if k == "landmark"));
    }

    #[test]
    fn parses_comments_and_bools() {
        let forms = parse("; hi\n(define x #t)").unwrap();
        assert_eq!(forms.len(), 1);
        assert_eq!(forms[0].line, 2);
    }

    #[test]
    fn parses_defun() {
        let src = "(defun (fold-wing a b)\n  (define t (crease-l2l a b))\n  (execute-fold t apical landmark=b))";
        let forms = parse(src).unwrap();
        assert_eq!(forms[0].expr.head_symbol(), Some("defun"));
        assert_eq!(forms[0].line, 1);
    }

    #[test]
    fn lines_track_through_blank_lines() {
        let src = "(define a 1)\n\n\n(define b 2)";
        let forms = parse(src).unwrap();
        assert_eq!(forms[0].line, 1);
        assert_eq!(forms[1].line, 4);
    }
}
