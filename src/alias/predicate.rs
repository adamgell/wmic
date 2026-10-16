//! Bounded scalar WQL predicates for the bundled metadata provider.
use crate::model::{CimValue, Record, WideText};
#[derive(Clone, Debug)]
enum Token {
    Word(WideText),
    Text(WideText),
    Number(i128),
    Operator(String),
    Left,
    Right,
}
#[derive(Clone, Debug)]
enum Literal {
    Text(WideText),
    Number(i128),
    Bool(bool),
    Null,
}
#[derive(Clone, Debug)]
enum Expr {
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Not(Box<Expr>),
    Null(WideText, bool),
    Compare(WideText, String, Literal),
    Like(WideText, WideText, bool),
}
pub struct Predicate(Expr);
fn lex(text: &WideText) -> Result<Vec<Token>, ()> {
    let mut tokens = vec![];
    let mut at = 0;
    while at < text.0.len() {
        let u = text.0[at];
        if matches!(u, 9 | 10 | 13 | 32) {
            at += 1;
            continue;
        }
        if tokens.len() >= 2048 {
            return Err(());
        }
        match u {
            40 => {
                tokens.push(Token::Left);
                at += 1
            }
            41 => {
                tokens.push(Token::Right);
                at += 1
            }
            34 | 39 => {
                let quote = u;
                at += 1;
                let mut value = vec![];
                loop {
                    let c = *text.0.get(at).ok_or(())?;
                    at += 1;
                    if c == quote {
                        break;
                    }
                    if c == 92 {
                        value.push(*text.0.get(at).ok_or(())?);
                        at += 1
                    } else {
                        value.push(c)
                    }
                }
                tokens.push(Token::Text(WideText(value)))
            }
            33 | 60 | 61 | 62 => {
                let start = at;
                at += 1;
                if text
                    .0
                    .get(at)
                    .is_some_and(|u| *u == 61 || text.0[start] == 60 && *u == 62)
                {
                    at += 1
                }
                tokens.push(Token::Operator(
                    String::from_utf16(&text.0[start..at]).map_err(|_| ())?,
                ))
            }
            _ => {
                let start = at;
                while at < text.0.len()
                    && !matches!(
                        text.0[at],
                        9 | 10 | 13 | 32 | 40 | 41 | 33 | 60 | 61 | 62 | 34 | 39
                    )
                {
                    at += 1
                }
                if at == start {
                    return Err(());
                }
                let value = WideText(text.0[start..at].to_vec());
                if let Ok(n) = value.to_string_lossy().parse::<i128>() {
                    tokens.push(Token::Number(n))
                } else {
                    tokens.push(Token::Word(value))
                }
            }
        }
    }
    Ok(tokens)
}
struct Parser {
    tokens: Vec<Token>,
    at: usize,
}
impl Parser {
    fn keyword(&mut self, word: &str) -> bool {
        if self
            .tokens
            .get(self.at)
            .is_some_and(|t| matches!(t,Token::Word(w) if w.eq_ignore_case(word)))
        {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn or(&mut self, depth: usize) -> Result<Expr, ()> {
        let mut values = vec![self.and(depth)?];
        while self.keyword("or") {
            values.push(self.and(depth)?)
        }
        Ok(if values.len() == 1 {
            values.remove(0)
        } else {
            Expr::Or(values)
        })
    }
    fn and(&mut self, depth: usize) -> Result<Expr, ()> {
        let mut values = vec![self.atom(depth)?];
        while self.keyword("and") {
            values.push(self.atom(depth)?)
        }
        Ok(if values.len() == 1 {
            values.remove(0)
        } else {
            Expr::And(values)
        })
    }
    fn atom(&mut self, depth: usize) -> Result<Expr, ()> {
        if depth > 32 {
            return Err(());
        }
        if self.keyword("not") {
            return Ok(Expr::Not(Box::new(self.atom(depth + 1)?)));
        }
        if matches!(self.tokens.get(self.at), Some(Token::Left)) {
            self.at += 1;
            let value = self.or(depth + 1)?;
            if !matches!(self.tokens.get(self.at), Some(Token::Right)) {
                return Err(());
            }
            self.at += 1;
            return Ok(value);
        }
        let Some(Token::Word(name)) = self.tokens.get(self.at).cloned() else {
            return Err(());
        };
        self.at += 1;
        if self.keyword("is") {
            let not = self.keyword("not");
            if !self.keyword("null") {
                return Err(());
            }
            return Ok(Expr::Null(name, not));
        }
        let not = self.keyword("not");
        if self.keyword("like") {
            let Some(Token::Text(pattern)) = self.tokens.get(self.at).cloned() else {
                return Err(());
            };
            self.at += 1;
            return Ok(Expr::Like(name, pattern, not));
        }
        if not {
            return Err(());
        }
        let Some(Token::Operator(op)) = self.tokens.get(self.at).cloned() else {
            return Err(());
        };
        self.at += 1;
        if !matches!(op.as_str(), "=" | "!=" | "<>" | "<" | ">" | "<=" | ">=") {
            return Err(());
        }
        let value = match self.tokens.get(self.at).cloned() {
            Some(Token::Text(s)) => Literal::Text(s),
            Some(Token::Number(n)) => Literal::Number(n),
            Some(Token::Word(w)) if w.eq_ignore_case("null") => Literal::Null,
            Some(Token::Word(w)) if w.eq_ignore_case("true") => Literal::Bool(true),
            Some(Token::Word(w)) if w.eq_ignore_case("false") => Literal::Bool(false),
            _ => return Err(()),
        };
        self.at += 1;
        Ok(Expr::Compare(name, op, value))
    }
}
fn field<'a>(record: &'a Record, name: &WideText) -> Result<&'a CimValue, ()> {
    record
        .properties
        .iter()
        .find(|p| p.name.eq_ordinal_ignore_case(name))
        .map(|p| &p.value)
        .ok_or(())
}
impl Predicate {
    pub fn parse(text: &WideText) -> Result<Self, ()> {
        let mut parser = Parser {
            tokens: lex(text)?,
            at: 0,
        };
        let result = parser.or(0)?;
        if parser.at != parser.tokens.len() {
            return Err(());
        }
        Ok(Self(result))
    }
    pub fn matches(&self, record: &Record) -> Result<bool, ()> {
        self.0.matches(record)
    }
}
impl Expr {
    fn matches(&self, record: &Record) -> Result<bool, ()> {
        Ok(match self {
            Self::And(values) => {
                let mut out = true;
                for expr in values {
                    out &= expr.matches(record)?
                }
                out
            }
            Self::Or(values) => {
                let mut out = false;
                for expr in values {
                    out |= expr.matches(record)?
                }
                out
            }
            Self::Not(expr) => !expr.matches(record)?,
            Self::Null(name, not) => {
                matches!(field(record, name)?, CimValue::Empty | CimValue::Null) != *not
            }
            Self::Like(name, pattern, not) => match field(record, name)? {
                CimValue::String(value) => like(value, pattern)? != *not,
                CimValue::Null | CimValue::Empty => false,
                _ => return Err(()),
            },
            Self::Compare(name, op, right) => {
                let left = field(record, name)?;
                let order = match (left, right) {
                    (CimValue::Null | CimValue::Empty, _) | (_, Literal::Null) => return Ok(false),
                    (CimValue::String(a), Literal::Text(b)) => {
                        if a.eq_ordinal_ignore_case(b) {
                            std::cmp::Ordering::Equal
                        } else {
                            a.to_string_lossy()
                                .to_uppercase()
                                .cmp(&b.to_string_lossy().to_uppercase())
                        }
                    }
                    (CimValue::Boolean(a), Literal::Bool(b)) => a.cmp(b),
                    (CimValue::Sint32(a), Literal::Number(b)) => (*a as i128).cmp(b),
                    (CimValue::Uint32(a), Literal::Number(b)) => (*a as i128).cmp(b),
                    _ => return Err(()),
                };
                use std::cmp::Ordering::*;
                match op.as_str() {
                    "=" => order == Equal,
                    "!=" | "<>" => order != Equal,
                    "<" => order == Less,
                    ">" => order == Greater,
                    "<=" => order != Greater,
                    ">=" => order != Less,
                    _ => return Err(()),
                }
            }
        })
    }
}
#[derive(Clone)]
enum Piece {
    Any,
    One,
    Unit(u16),
    Set { not: bool, ranges: Vec<(u16, u16)> },
}
fn like(text: &WideText, pattern: &WideText) -> Result<bool, ()> {
    let mut parts = vec![];
    let mut at = 0;
    while at < pattern.0.len() {
        let u = pattern.0[at];
        at += 1;
        parts.push(match u {
            37 => Piece::Any,
            95 => Piece::One,
            91 => {
                let not = pattern.0.get(at) == Some(&94);
                if not {
                    at += 1
                }
                let mut ranges = vec![];
                loop {
                    let c = *pattern.0.get(at).ok_or(())?;
                    at += 1;
                    if c == 93 {
                        if ranges.is_empty() {
                            return Err(());
                        }
                        break;
                    }
                    if pattern.0.get(at) == Some(&45)
                        && pattern.0.get(at + 1).is_some_and(|u| *u != 93)
                    {
                        at += 1;
                        let end = pattern.0[at];
                        at += 1;
                        ranges.push((c, end))
                    } else {
                        ranges.push((c, c))
                    }
                }
                Piece::Set { not, ranges }
            }
            other => Piece::Unit(other),
        })
    }
    if parts.len() > 2048 || text.0.len() > 65536 {
        return Err(());
    }
    let equal = |a: u16, b: u16| WideText(vec![a]).eq_ordinal_ignore_case(&WideText(vec![b]));
    let upper = |u: u16| if (97..=122).contains(&u) { u - 32 } else { u };
    let mut previous = vec![false; text.0.len() + 1];
    previous[0] = true;
    for part in parts {
        let mut next = vec![false; text.0.len() + 1];
        if matches!(part, Piece::Any) {
            next[0] = previous[0]
        }
        for i in 1..=text.0.len() {
            next[i] = match &part {
                Piece::Any => previous[i] || next[i - 1],
                Piece::One => previous[i - 1],
                Piece::Unit(u) => previous[i - 1] && equal(*u, text.0[i - 1]),
                Piece::Set { not, ranges } => {
                    previous[i - 1]
                        && (ranges.iter().any(|(a, b)| {
                            if a == b {
                                equal(*a, text.0[i - 1])
                            } else {
                                upper(*a) <= upper(text.0[i - 1])
                                    && upper(text.0[i - 1]) <= upper(*b)
                            }
                        }) != *not)
                }
            }
        }
        previous = next;
    }
    Ok(previous[text.0.len()])
}
