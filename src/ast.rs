use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Nil,
    Bool(bool),
    Number(f64),
    Str(String),
    Symbol(String),
    Keyword(String, Box<Expr>),
    List(Vec<Expr>),
    Quoted(Box<Expr>),
}

impl Expr {
    pub fn sym(s: &str) -> Self {
        Expr::Symbol(s.to_string())
    }

    pub fn list<I: IntoIterator<Item = Expr>>(items: I) -> Self {
        Expr::List(items.into_iter().collect())
    }

    pub fn as_symbol(&self) -> Option<&str> {
        match self {
            Expr::Symbol(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Expr]> {
        match self {
            Expr::List(xs) => Some(xs),
            _ => None,
        }
    }

    pub fn head_symbol(&self) -> Option<&str> {
        self.as_list().and_then(|xs| xs.first()?.as_symbol())
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Nil => write!(f, "()"),
            Expr::Bool(b) => write!(f, "{}", if *b { "#t" } else { "#f" }),
            Expr::Number(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    write!(f, "{}", *n as i64)
                } else {
                    write!(f, "{}", n)
                }
            }
            Expr::Str(s) => write!(f, "\"{}\"", s.replace('"', "\\\"")),
            Expr::Symbol(s) => write!(f, "{}", s),
            Expr::Keyword(k, v) => write!(f, "{}={}", k, v),
            Expr::Quoted(e) => write!(f, "'{}", e),
            Expr::List(xs) => {
                write!(f, "(")?;
                for (i, x) in xs.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", x)?;
                }
                write!(f, ")")
            }
        }
    }
}
