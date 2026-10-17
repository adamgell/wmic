//! Bounded scalar WQL predicates evaluated in memory against records.
use crate::model::{CimValue, Record, WideText};
#[derive(Clone, Debug)]
enum Token {
    Word(WideText),
    Text(WideText),
    Number(i128),
    Real(f64),
    Operator(String),
    Left,
    Right,
}
#[derive(Clone, Debug)]
enum Literal {
    Text(WideText),
    Number(i128),
    Real(f64),
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
                let word = value.to_string_lossy();
                if let Ok(n) = word.parse::<i128>() {
                    tokens.push(Token::Number(n))
                } else if let Some(r) = decimal(&word) {
                    tokens.push(Token::Real(r))
                } else {
                    tokens.push(Token::Word(value))
                }
            }
        }
    }
    Ok(tokens)
}
/// A finite decimal or exponent literal; words such as `inf` and `nan` stay words.
fn decimal(word: &str) -> Option<f64> {
    (word
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'+' | b'-' | b'e' | b'E'))
        && word.bytes().any(|b| b.is_ascii_digit()))
    .then(|| word.parse::<f64>().ok())
    .flatten()
    .filter(|r| r.is_finite())
}
/// A quoted literal compared with a numeric or boolean property is read as that kind of value.
fn coerce(text: &WideText) -> Option<Literal> {
    let text = text.to_string_lossy();
    let text = text.trim();
    if let Ok(n) = text.parse::<i128>() {
        Some(Literal::Number(n))
    } else if let Some(r) = decimal(text) {
        Some(Literal::Real(r))
    } else if text.eq_ignore_ascii_case("true") {
        Some(Literal::Bool(true))
    } else if text.eq_ignore_ascii_case("false") {
        Some(Literal::Bool(false))
    } else {
        None
    }
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
            Some(Token::Real(r)) => Literal::Real(r),
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
                CimValue::String(value)
                | CimValue::DateTime(value)
                | CimValue::Reference(value) => like(value, pattern)? != *not,
                CimValue::Null | CimValue::Empty => false,
                _ => return Err(()),
            },
            Self::Compare(name, op, right) => {
                let left = field(record, name)?;
                let coerced;
                let right = match (left, right) {
                    (
                        CimValue::Null
                        | CimValue::Empty
                        | CimValue::String(_)
                        | CimValue::DateTime(_)
                        | CimValue::Reference(_),
                        _,
                    ) => right,
                    (_, Literal::Text(text)) => {
                        coerced = coerce(text).ok_or(())?;
                        &coerced
                    }
                    _ => right,
                };
                let order = match (left, right) {
                    (CimValue::Null | CimValue::Empty, _) | (_, Literal::Null) => return Ok(false),
                    (
                        CimValue::String(a) | CimValue::DateTime(a) | CimValue::Reference(a),
                        Literal::Text(b),
                    ) => {
                        if a.eq_ordinal_ignore_case(b) {
                            std::cmp::Ordering::Equal
                        } else {
                            a.to_string_lossy()
                                .to_uppercase()
                                .cmp(&b.to_string_lossy().to_uppercase())
                        }
                    }
                    (CimValue::Boolean(a), Literal::Bool(b)) => a.cmp(b),
                    (
                        CimValue::Real32(_) | CimValue::Real64(_),
                        Literal::Number(_) | Literal::Real(_),
                    ) => match real(left).zip(approximate(right)) {
                        Some((a, b)) => match a.partial_cmp(&b) {
                            Some(order) => order,
                            None => return Ok(false),
                        },
                        None => return Err(()),
                    },
                    (left, Literal::Real(b)) => match integer(left) {
                        Some(a) => match (a as f64).partial_cmp(b) {
                            Some(order) => order,
                            None => return Ok(false),
                        },
                        None => return Err(()),
                    },
                    (left, Literal::Number(b)) => match integer(left) {
                        Some(a) => a.cmp(b),
                        None => return Err(()),
                    },
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
fn real(value: &CimValue) -> Option<f64> {
    match value {
        CimValue::Real32(bits) => Some(f32::from_bits(*bits) as f64),
        CimValue::Real64(bits) => Some(f64::from_bits(*bits)),
        _ => None,
    }
}
fn approximate(literal: &Literal) -> Option<f64> {
    match literal {
        Literal::Number(n) => Some(*n as f64),
        Literal::Real(r) => Some(*r),
        _ => None,
    }
}
fn integer(value: &CimValue) -> Option<i128> {
    Some(match value {
        CimValue::Sint8(v) => *v as i128,
        CimValue::Uint8(v) => *v as i128,
        CimValue::Sint16(v) => *v as i128,
        CimValue::Uint16(v) | CimValue::Char16(v) => *v as i128,
        CimValue::Sint32(v) => *v as i128,
        CimValue::Uint32(v) => *v as i128,
        CimValue::Sint64(v) => *v as i128,
        CimValue::Uint64(v) => *v as i128,
        _ => return None,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Property;
    fn record(values: Vec<(&str, CimValue)>) -> Record {
        Record {
            properties: values
                .into_iter()
                .map(|(name, value)| Property {
                    name: name.into(),
                    value,
                    cim_type: 0,
                    system: false,
                    qualifiers: vec![],
                })
                .collect(),
            ..Default::default()
        }
    }
    fn check(expression: &str, record: &Record) -> Result<bool, ()> {
        Predicate::parse(&expression.into())?.matches(record)
    }
    #[test]
    fn every_integer_width_compares_with_integer_literals() {
        let r = record(vec![
            ("A", CimValue::Sint8(-3)),
            ("B", CimValue::Uint8(200)),
            ("C", CimValue::Sint16(-300)),
            ("D", CimValue::Uint16(65535)),
            ("E", CimValue::Sint32(-5)),
            ("F", CimValue::Uint32(4_000_000_000)),
            ("G", CimValue::Sint64(-9_000_000_000)),
            ("H", CimValue::Uint64(u64::MAX)),
            ("I", CimValue::Char16(65)),
        ]);
        for expression in [
            "A = -3",
            "B > 199",
            "C < -299",
            "D = 65535",
            "E <= -5",
            "F >= 4000000000",
            "G < 0",
            "H = 18446744073709551615",
            "I = 65",
            "NOT A = 3 AND B <> 0",
        ] {
            assert_eq!(check(expression, &r), Ok(true), "{expression}");
        }
        for expression in ["A = 3", "B < 200", "H < 1", "I > 65"] {
            assert_eq!(check(expression, &r), Ok(false), "{expression}");
        }
    }
    #[test]
    fn real_values_compare_with_integer_literals() {
        let r = record(vec![
            ("S", CimValue::Real32(1.5f32.to_bits())),
            ("D", CimValue::Real64(2.0f64.to_bits())),
            ("N", CimValue::Real64(f64::NAN.to_bits())),
        ]);
        assert_eq!(check("S > 1", &r), Ok(true));
        assert_eq!(check("S >= 2", &r), Ok(false));
        assert_eq!(check("D = 2", &r), Ok(true));
        assert_eq!(check("D <> 2", &r), Ok(false));
        assert_eq!(check("N = 2", &r), Ok(false));
        assert_eq!(check("N <> 2", &r), Ok(false));
    }
    #[test]
    fn date_time_and_reference_compare_as_text() {
        let r = record(vec![
            ("T", CimValue::DateTime("20260918204038.959360-240".into())),
            ("P", CimValue::Reference("HOST:Win32_X.Id=1".into())),
        ]);
        assert_eq!(check("T = '20260918204038.959360-240'", &r), Ok(true));
        assert_eq!(check("T > '20260101000000.000000-240'", &r), Ok(true));
        assert_eq!(check("T LIKE '202609%'", &r), Ok(true));
        assert_eq!(check("P LIKE '%win32_x%'", &r), Ok(true));
    }
    #[test]
    fn mismatched_types_and_null_keep_their_results() {
        let r = record(vec![
            ("N", CimValue::Null),
            ("I", CimValue::Uint16(1)),
            ("S", CimValue::String("x".into())),
        ]);
        assert_eq!(check("N = 1", &r), Ok(false));
        assert_eq!(check("I = 'x'", &r), Err(()));
        assert_eq!(check("S = 1", &r), Err(()));
        assert_eq!(check("I LIKE '1'", &r), Err(()));
        assert_eq!(check("Missing = 1", &r), Err(()));
    }
    #[test]
    fn quoted_literals_coerce_to_numeric_and_boolean_properties() {
        let r = record(vec![
            ("OSType", CimValue::Uint16(2)),
            ("Load", CimValue::Uint16(7)),
            ("Ratio", CimValue::Real64(2.5f64.to_bits())),
            ("Primary", CimValue::Boolean(true)),
            ("Name", CimValue::String("x".into())),
        ]);
        assert_eq!(check("OSType='2'", &r), Ok(true));
        assert_eq!(check("OSType='3'", &r), Ok(false));
        assert_eq!(check("OSType > '1'", &r), Ok(true));
        assert_eq!(check("Load < '7.5'", &r), Ok(true));
        assert_eq!(check("Ratio = '2.5'", &r), Ok(true));
        assert_eq!(check("Primary='TRUE'", &r), Ok(true));
        assert_eq!(check("Primary='false'", &r), Ok(false));
        assert_eq!(check("Name = 'X'", &r), Ok(true));
        assert_eq!(check("OSType = 'two'", &r), Err(()));
        assert_eq!(check("OSType = ''", &r), Err(()));
        assert_eq!(check("Primary = '2'", &r), Err(()));
        assert_eq!(check("Load = 'TRUE'", &r), Err(()));
    }
    #[test]
    fn decimal_literals_compare_with_integer_and_real_values() {
        let r = record(vec![
            ("LoadPercentage", CimValue::Uint16(2)),
            ("Ratio", CimValue::Real32(1.5f32.to_bits())),
            ("Name", CimValue::String("x".into())),
        ]);
        assert_eq!(check("LoadPercentage > 1.5", &r), Ok(true));
        assert_eq!(check("LoadPercentage < 1.5", &r), Ok(false));
        assert_eq!(check("LoadPercentage = 2.0", &r), Ok(true));
        assert_eq!(check("LoadPercentage > -0.5", &r), Ok(true));
        assert_eq!(check("Ratio = 1.5", &r), Ok(true));
        assert_eq!(check("Ratio >= 1.6", &r), Ok(false));
        assert_eq!(check("Ratio < 1e1", &r), Ok(true));
        assert_eq!(check("Name = 1.5", &r), Err(()));
        assert_eq!(check("Name = inf", &r), Err(()));
        assert_eq!(check("Name = nan", &r), Err(()));
    }
}
