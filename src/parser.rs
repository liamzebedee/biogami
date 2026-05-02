use crate::ast::Expr;
use anyhow::{anyhow, bail, Result};

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    LParen,
    RParen,
    Quote,
    Atom(String),
    Str(String),
}

fn tokenize(src: &str) -> Result<Vec<Tok>> {
    let mut out = Vec::new();
    let mut chars = src.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' | '\n' | '\r' | ',' => {
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
                out.push(Tok::LParen);
            }
            ')' => {
                chars.next();
                out.push(Tok::RParen);
            }
            '\'' => {
                chars.next();
                out.push(Tok::Quote);
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
                        s.push(c);
                    }
                }
                out.push(Tok::Str(s));
            }
            _ => {
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
                out.push(Tok::Atom(s));
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
    match t {
        Tok::LParen => {
            let mut items = Vec::new();
            while let Some(t) = toks.get(*i) {
                if matches!(t, Tok::RParen) {
                    *i += 1;
                    return Ok(Expr::List(items));
                }
                items.push(parse_one(toks, i)?);
            }
            bail!("missing closing paren");
        }
        Tok::RParen => bail!("unexpected ')'"),
        Tok::Quote => {
            let inner = parse_one(toks, i)?;
            Ok(Expr::Quoted(Box::new(inner)))
        }
        Tok::Atom(s) => Ok(parse_atom(&s)),
        Tok::Str(s) => Ok(Expr::Str(s)),
    }
}

pub fn parse(src: &str) -> Result<Vec<Expr>> {
    let toks = tokenize(src)?;
    let mut i = 0;
    let mut out = Vec::new();
    while i < toks.len() {
        out.push(parse_one(&toks, &mut i)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_define() {
        let exprs = parse("(define d1 (crease-p2p c3 c1))").unwrap();
        assert_eq!(exprs.len(), 1);
        assert_eq!(exprs[0].head_symbol(), Some("define"));
    }

    #[test]
    fn parses_keyword_arg() {
        let exprs = parse("(execute-fold d1 apical landmark=c3)").unwrap();
        let xs = exprs[0].as_list().unwrap();
        assert!(matches!(&xs[3], Expr::Keyword(k, _) if k == "landmark"));
    }

    #[test]
    fn parses_comments_and_bools() {
        let exprs = parse("; hi\n(define x #t)").unwrap();
        assert_eq!(exprs.len(), 1);
    }

    #[test]
    fn parses_defun() {
        let src = "(defun (fold-wing a b)\n  (define t (crease-l2l a b))\n  (execute-fold t apical landmark=b))";
        let exprs = parse(src).unwrap();
        assert_eq!(exprs[0].head_symbol(), Some("defun"));
    }
}
