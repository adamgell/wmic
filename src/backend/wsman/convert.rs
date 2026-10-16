//! Text to CIM value conversion and the datetime and interval forms of the wire and of CIM.
use crate::{
    backend::{AutomationValue, convert_value},
    model::CimValue,
};

fn digits(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Microseconds from fraction digits: left-justified, zero padded, truncated beyond six digits.
fn fraction_micros(fraction: &str) -> Option<u32> {
    if fraction.is_empty() {
        return Some(0);
    }
    digits(fraction.get(..fraction.len().min(6))?)?;
    let mut padded: String = fraction.chars().take(6).collect();
    while padded.len() < 6 {
        padded.push('0');
    }
    padded.parse().ok()
}

/// Signed minutes from `+hh:mm`, `-hh:mm`, `Z` or an absent offset.
fn offset_minutes(text: &str) -> Option<i32> {
    if text.is_empty() || text == "Z" {
        return Some(0);
    }
    let sign = match text.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let (hours, minutes) = text[1..].split_once(':')?;
    Some(sign * (digits(hours)? as i32 * 60 + digits(minutes)? as i32))
}

fn offset_suffix(offset: i32) -> String {
    format!("{}{:03}", if offset < 0 { '-' } else { '+' }, offset.abs())
}

/// Splits `a-b-c` into three fixed-width digit groups.
fn date_fields(text: &str) -> Option<[&str; 3]> {
    let mut parts = text.split('-');
    let fields = [parts.next()?, parts.next()?, parts.next()?];
    let widths = [4, 2, 2];
    let fits = fields
        .iter()
        .zip(widths)
        .all(|(f, w)| f.len() == w && digits(f).is_some());
    (parts.next().is_none() && fits).then_some(fields)
}

/// Splits `hh:mm:ss` into three two-digit groups.
fn clock_fields(text: &str) -> Option<[&str; 3]> {
    let mut parts = text.split(':');
    let fields = [parts.next()?, parts.next()?, parts.next()?];
    let fits = fields.iter().all(|f| f.len() == 2 && digits(f).is_some());
    (parts.next().is_none() && fits).then_some(fields)
}

/// Splits a time with optional fraction and zone into clock, fraction and zone text.
fn split_time(time: &str) -> (&str, &str, &str) {
    let zone_at = time.find(['Z', '+', '-']).unwrap_or(time.len());
    let (clock, zone) = time.split_at(zone_at);
    let (clock, fraction) = clock.split_once('.').unwrap_or((clock, ""));
    (clock, fraction, zone)
}

/// `xs:dateTime` to the CIM form `yyyymmddHHMMSS.ffffff+UUU`.
/// The wall-clock fields and the offset are kept; nothing is converted to UTC.
pub fn xs_datetime_to_cim(text: &str) -> Option<String> {
    let (date, time) = text.trim().split_once('T')?;
    let [year, month, day] = date_fields(date)?;
    let (clock, fraction, zone) = split_time(time);
    let [hour, minute, second] = clock_fields(clock)?;
    Some(format!(
        "{year}{month}{day}{hour}{minute}{second}.{:06}{}",
        fraction_micros(fraction)?,
        offset_suffix(offset_minutes(zone)?)
    ))
}

/// `xs:duration` of days and time fields to the CIM interval `ddddddddHHMMSS.ffffff:000`.
/// Years and months have no fixed length and are rejected.
pub fn xs_duration_to_cim(text: &str) -> Option<String> {
    let rest = text.trim().strip_prefix('P')?;
    let (date, time) = rest.split_once('T').unwrap_or((rest, ""));
    if date.is_empty() && time.is_empty() {
        return None;
    }
    let mut days = 0u64;
    if !date.is_empty() {
        days = u64::from(digits(date.strip_suffix('D')?)?);
    }
    let (mut hours, mut minutes, mut seconds, mut micros) = (0u64, 0u64, 0u64, 0u64);
    let mut remaining = time;
    while !remaining.is_empty() {
        let end = remaining.find(['H', 'M', 'S'])?;
        let (value, unit) = (&remaining[..end], remaining.as_bytes()[end]);
        remaining = &remaining[end + 1..];
        match unit {
            b'H' => hours = u64::from(digits(value)?),
            b'M' => minutes = u64::from(digits(value)?),
            _ => {
                let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
                seconds = u64::from(digits(whole)?);
                micros = u64::from(fraction_micros(fraction)?);
            }
        }
    }
    let total = ((days * 24 + hours) * 60 + minutes) * 60 + seconds;
    let (days, rest) = (total / 86_400, total % 86_400);
    if days > 99_999_999 {
        return None;
    }
    Some(format!(
        "{days:08}{:02}{:02}{:02}.{micros:06}:000",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    ))
}

/// True when a text has the 25-character CIM timestamp or interval layout.
pub fn is_cim_datetime(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 25 && b[14] == b'.' && matches!(b[21], b'+' | b'-' | b':')
}

/// CIM timestamp to `xs:dateTime`; `None` for intervals and for values with wildcards.
pub fn cim_to_xs_datetime(text: &str) -> Option<String> {
    if !is_cim_datetime(text) || text.as_bytes()[21] == b':' || !text.is_ascii() {
        return None;
    }
    let digit_groups = [(0, 14), (15, 21), (22, 25)];
    if digit_groups
        .iter()
        .any(|&(a, b)| !text[a..b].bytes().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    let offset = digits(&text[22..25])?;
    Some(format!(
        "{}-{}-{}T{}:{}:{}.{}{}{:02}:{:02}",
        &text[0..4],
        &text[4..6],
        &text[6..8],
        &text[8..10],
        &text[10..12],
        &text[12..14],
        &text[15..21],
        &text[21..22],
        offset / 60,
        offset % 60
    ))
}

/// CIM interval to `xs:duration`; `None` for timestamps and for values with wildcards.
pub fn cim_to_xs_duration(text: &str) -> Option<String> {
    if !is_cim_datetime(text) || text.as_bytes()[21] != b':' || !text.is_ascii() {
        return None;
    }
    let days = digits(&text[0..8])?;
    let (hours, minutes, seconds) = (
        digits(&text[8..10])?,
        digits(&text[10..12])?,
        digits(&text[12..14])?,
    );
    digits(&text[15..21])?;
    Some(format!(
        "P{days}DT{hours}H{minutes}M{seconds}.{}S",
        &text[15..21]
    ))
}

/// `xs:date` as a CIM timestamp with a wildcard time part.
pub fn xs_date_to_cim(text: &str) -> Option<String> {
    let text = text.trim();
    let zone_at = text
        .get(10..)
        .and_then(|z| z.find(['Z', '+', '-']))
        .map(|at| at + 10);
    let (date, zone) = text.split_at(zone_at.unwrap_or(text.len()));
    let [year, month, day] = date_fields(date)?;
    Some(format!(
        "{year}{month}{day}******.******{}",
        offset_suffix(offset_minutes(zone)?)
    ))
}

/// `xs:time` as a CIM timestamp with a wildcard date part.
pub fn xs_time_to_cim(text: &str) -> Option<String> {
    let (clock, fraction, zone) = split_time(text.trim());
    let [hour, minute, second] = clock_fields(clock)?;
    Some(format!(
        "********{hour}{minute}{second}.{:06}{}",
        fraction_micros(fraction)?,
        offset_suffix(offset_minutes(zone)?)
    ))
}

fn real_text(text: &str) -> String {
    match text.trim() {
        "INF" | "+INF" => "inf".into(),
        "-INF" => "-inf".into(),
        other => other.to_owned(),
    }
}

/// A scalar of one CIM base type from its wire text; `None` when the text does not fit the type.
pub fn text_value(text: &str, base_type: u32) -> Option<CimValue> {
    Some(match base_type {
        8 => CimValue::String(text.into()),
        101 => CimValue::DateTime(text.into()),
        102 => CimValue::Reference(text.into()),
        103 => {
            let units: Vec<u16> = text.encode_utf16().collect();
            match units.as_slice() {
                [unit] => CimValue::Char16(*unit),
                _ => return None,
            }
        }
        11 => match text.trim() {
            t if t.eq_ignore_ascii_case("true") || t == "1" => CimValue::Boolean(true),
            t if t.eq_ignore_ascii_case("false") || t == "0" => CimValue::Boolean(false),
            _ => return None,
        },
        4 => CimValue::Real32(real_text(text).parse::<f32>().ok()?.to_bits()),
        5 => CimValue::Real64(real_text(text).parse::<f64>().ok()?.to_bits()),
        2 | 3 | 16 | 17 | 18 | 19 | 20 | 21 => {
            convert_value(AutomationValue::String(text.trim().into()), base_type).ok()?
        }
        _ => return None,
    })
}

fn real_to_text(value: f64) -> String {
    if value.is_nan() {
        "NaN".into()
    } else if value.is_infinite() {
        if value < 0.0 { "-INF" } else { "INF" }.into()
    } else {
        value.to_string()
    }
}

/// Wire text of a scalar value; `None` for null and for values with no text form.
pub fn value_text(value: &CimValue) -> Option<String> {
    Some(match value {
        CimValue::String(s) | CimValue::DateTime(s) | CimValue::Reference(s) => s.to_string_lossy(),
        CimValue::Boolean(b) => b.to_string(),
        CimValue::Sint8(v) => v.to_string(),
        CimValue::Uint8(v) => v.to_string(),
        CimValue::Sint16(v) => v.to_string(),
        CimValue::Uint16(v) => v.to_string(),
        CimValue::Sint32(v) => v.to_string(),
        CimValue::Uint32(v) => v.to_string(),
        CimValue::Sint64(v) => v.to_string(),
        CimValue::Uint64(v) => v.to_string(),
        CimValue::Real32(bits) => real_to_text(f64::from(f32::from_bits(*bits))),
        CimValue::Real64(bits) => real_to_text(f64::from_bits(*bits)),
        CimValue::Char16(unit) => String::from_utf16_lossy(&[*unit]),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datetime_keeps_wall_clock_and_offset() {
        let cases = [
            (
                "2026-10-13T15:47:08.097097-04:00",
                "20261013154708.097097-240",
            ),
            (
                "2026-10-13T15:47:11.87422-04:00",
                "20261013154711.874220-240",
            ),
            ("2025-11-22T22:37:53-05:00", "20251122223753.000000-300"),
            ("2026-01-29T00:00:00Z", "20260129000000.000000+000"),
            ("2026-03-01T08:30:00.351+05:30", "20260301083000.351000+330"),
            (
                "2026-03-01T08:30:00.1234567+00:00",
                "20260301083000.123456+000",
            ),
        ];
        for (wire, cim) in cases {
            assert_eq!(xs_datetime_to_cim(wire).as_deref(), Some(cim), "{wire}");
        }
        assert_eq!(xs_datetime_to_cim("not a date"), None);
        assert_eq!(xs_datetime_to_cim("2026-13-0xT00:00:00Z"), None);
    }

    #[test]
    fn cim_datetime_round_trips_to_the_wire_form() {
        for cim in [
            "20261013154708.097097-240",
            "20260301083000.351000+330",
            "20260129000000.000000+000",
        ] {
            let xs = cim_to_xs_datetime(cim).unwrap();
            assert_eq!(xs_datetime_to_cim(&xs).as_deref(), Some(cim));
        }
        assert_eq!(cim_to_xs_datetime("20260101******.******+000"), None);
        assert_eq!(cim_to_xs_datetime("00000001020304.500000:000"), None);
    }

    #[test]
    fn durations_become_cim_intervals() {
        assert_eq!(
            xs_duration_to_cim("P1DT2H3M4.5S").as_deref(),
            Some("00000001020304.500000:000")
        );
        assert_eq!(
            xs_duration_to_cim("PT36H").as_deref(),
            Some("00000001120000.000000:000")
        );
        assert_eq!(
            xs_duration_to_cim("PT0.25S").as_deref(),
            Some("00000000000000.250000:000")
        );
        assert_eq!(xs_duration_to_cim("P1Y"), None);
        assert_eq!(xs_duration_to_cim("P1M"), None);
        assert_eq!(xs_duration_to_cim("-PT1S"), None);
        assert_eq!(xs_duration_to_cim("P"), None);
        let back = cim_to_xs_duration("00000001020304.500000:000").unwrap();
        assert_eq!(
            xs_duration_to_cim(&back).as_deref(),
            Some("00000001020304.500000:000")
        );
    }

    #[test]
    fn date_and_time_children_use_wildcards() {
        assert_eq!(
            xs_date_to_cim("2026-10-13").as_deref(),
            Some("20261013******.******+000")
        );
        assert_eq!(
            xs_time_to_cim("15:47:08.5-04:00").as_deref(),
            Some("********154708.500000-240")
        );
    }

    #[test]
    fn scalars_follow_the_cim_type() {
        assert_eq!(text_value("42", 21), Some(CimValue::Uint64(42)));
        assert_eq!(
            text_value("18446744073709551615", 21),
            Some(CimValue::Uint64(u64::MAX))
        );
        assert_eq!(text_value("-240", 2), Some(CimValue::Sint16(-240)));
        assert_eq!(text_value("256", 17), None);
        assert_eq!(text_value("true", 11), Some(CimValue::Boolean(true)));
        assert_eq!(text_value("0", 11), Some(CimValue::Boolean(false)));
        assert_eq!(text_value("maybe", 11), None);
        assert_eq!(text_value("x", 103), Some(CimValue::Char16(120)));
        assert_eq!(text_value("xy", 103), None);
        assert_eq!(
            text_value("1.5", 4),
            Some(CimValue::Real32(1.5f32.to_bits()))
        );
        assert_eq!(
            text_value("-2.25E2", 5),
            Some(CimValue::Real64((-225.0f64).to_bits()))
        );
        assert_eq!(
            text_value("INF", 5),
            Some(CimValue::Real64(f64::INFINITY.to_bits()))
        );
        assert_eq!(
            text_value("-INF", 4),
            Some(CimValue::Real32(f32::NEG_INFINITY.to_bits()))
        );
        assert!(matches!(
            text_value("NaN", 5),
            Some(CimValue::Real64(bits)) if f64::from_bits(bits).is_nan()
        ));
    }

    #[test]
    fn reals_print_in_the_schema_forms() {
        assert_eq!(
            value_text(&CimValue::Real32(f32::INFINITY.to_bits())).as_deref(),
            Some("INF")
        );
        assert_eq!(
            value_text(&CimValue::Real64(1.5f64.to_bits())).as_deref(),
            Some("1.5")
        );
        assert_eq!(value_text(&CimValue::Null), None);
    }
}
