use crate::model::*;
fn invalid() -> ParseError {
    ParseError::new(0xABE5, "Invalid command.", None)
}
fn atom(units: &[u16], at: &mut usize) -> Result<(WideText, bool), ParseError> {
    while units.get(*at) == Some(&32) {
        *at += 1
    }
    if let Some(&q) = units.get(*at).filter(|u| matches!(u, 34 | 39)) {
        *at += 1;
        let start = *at;
        while let Some(&u) = units.get(*at) {
            if u == q {
                let value = WideText(units[start..*at].to_vec());
                *at += 1;
                return Ok((value, true));
            }
            *at += 1
        }
        return Err(invalid());
    }
    let start = *at;
    while units.get(*at).is_some_and(|u| !matches!(u, 44 | 58)) {
        *at += 1
    }
    let value = WideText(units[start..*at].to_vec()).trim_ascii();
    if value.0.is_empty() {
        return Err(invalid());
    }
    if value.0.iter().any(|u| {
        !(*u >= 128
            || (65..=90).contains(u)
            || (97..=122).contains(u)
            || (48..=57).contains(u)
            || matches!(*u, 95 | 46 | 92 | 61 | 40 | 41))
    }) {
        return Err(ParseError::new(0xACB4, "Invalid GET switch.", None));
    }
    Ok((value, false))
}
pub fn parse_format_spec(value: &WideText) -> Result<FormatSpec, ParseError> {
    let mut at = 0;
    let mut transforms = vec![];
    loop {
        let (mut name, quoted) = atom(&value.0, &mut at)?;
        if quoted && value.0.get(at) == Some(&40) {
            let start = at;
            while value.0.get(at).is_some_and(|u| !matches!(u, 44 | 58)) {
                at += 1
            }
            name.0.extend(&value.0[start..at]);
        }
        let mut parameters = vec![];
        while value.0.get(at) == Some(&58) {
            at += 1;
            // The legacy lexer requires each parameter token to be quoted.
            if !value.0.get(at).is_some_and(|u| matches!(u, 34 | 39)) {
                return Err(invalid());
            }
            let (token, _) = atom(&value.0, &mut at)?;
            let pieces: Vec<_> = token.0.split(|u| *u == 61).collect();
            if pieces.len() < 2 {
                return Err(invalid());
            }
            parameters.push((WideText(pieces[0].to_vec()), WideText(pieces[1].to_vec())));
        }
        transforms.push(TransformSpec { name, parameters });
        if at == value.0.len() {
            break;
        }
        if value.0[at] != 44 {
            return Err(invalid());
        }
        at += 1;
    }
    Ok(FormatSpec {
        transforms,
        before: vec![],
        after: vec![],
    })
}
