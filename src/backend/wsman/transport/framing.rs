//! `multipart/encrypted` message framing (MS-WSMV 2.2.9.1.1).

use super::super::TransportError;
use super::ntlm::{SIGNATURE_LEN, Session};

pub const BOUNDARY: &str = "Encrypted Boundary";
pub const CONTENT_TYPE: &str = "multipart/encrypted;protocol=\"application/HTTP-SPNEGO-session-encrypted\";boundary=\"Encrypted Boundary\"";

fn protocol(message: &str) -> TransportError {
    TransportError::Protocol(format!("encrypted message: {message}"))
}

/// Seals `plain` and wraps it in the two-part layout the service expects.
pub fn encrypt(session: &mut Session, plain: &[u8]) -> Vec<u8> {
    let (signature, sealed) = session.seal(plain);
    let mut out = format!(
        "--{BOUNDARY}\r\n\tContent-Type: application/HTTP-SPNEGO-session-encrypted\r\n\tOriginalContent: type=application/soap+xml;charset=UTF-8;Length={}\r\n--{BOUNDARY}\r\n\tContent-Type: application/octet-stream\r\n",
        plain.len()
    )
    .into_bytes();
    out.extend_from_slice(&(SIGNATURE_LEN as u32).to_le_bytes());
    out.extend_from_slice(&signature);
    out.extend_from_slice(&sealed);
    out.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    out
}

/// Extracts the `boundary` parameter of a multipart `Content-Type` value.
pub fn boundary_of(content_type: &str) -> Option<String> {
    let lower = content_type.to_ascii_lowercase();
    let at = lower.find("boundary=")? + "boundary=".len();
    let rest = &content_type[at..];
    let value = match rest.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next()?,
        None => rest.split(';').next()?.trim(),
    };
    (!value.is_empty()).then(|| value.to_owned())
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn skip_blank(body: &[u8], mut at: usize) -> usize {
    while body.get(at).is_some_and(|b| b.is_ascii_whitespace()) {
        at += 1;
    }
    at
}

/// Skips spaces and tabs, then one line break.
fn skip_line_end(body: &[u8], mut at: usize) -> Result<usize, TransportError> {
    while matches!(body.get(at), Some(b' ' | b'\t')) {
        at += 1;
    }
    match body.get(at) {
        Some(b'\r') if body.get(at + 1) == Some(&b'\n') => Ok(at + 2),
        Some(b'\n') => Ok(at + 1),
        _ => Err(protocol("boundary line is not terminated")),
    }
}

/// A header line is printable ASCII ending in a line break. Binary payload never matches.
fn header_line(body: &[u8], at: usize) -> Option<(&str, usize)> {
    let end = at + body.get(at..)?.iter().position(|&b| b == b'\n')?;
    let line = &body[at..end];
    let printable = line
        .iter()
        .all(|&b| b == b'\t' || b == b'\r' || (0x20..0x7f).contains(&b));
    if !printable {
        return None;
    }
    let text = std::str::from_utf8(line).ok()?;
    Some((text.trim_end_matches('\r'), end + 1))
}

fn original_length(metadata: &[u8]) -> Result<usize, TransportError> {
    let text = String::from_utf8_lossy(metadata);
    for line in text.lines() {
        let lower = line.trim().to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("originalcontent:") {
            let digits: String = rest
                .split_once("length=")
                .map(|(_, tail)| tail.chars().take_while(char::is_ascii_digit).collect())
                .unwrap_or_default();
            return digits
                .parse()
                .map_err(|_| protocol("OriginalContent has no Length"));
        }
    }
    Err(protocol("metadata part has no OriginalContent"))
}

/// Verifies and unseals every part of an encrypted response body and concatenates the plaintext.
/// Accepts header lines with or without leading tabs and boundary lines with trailing blanks.
pub fn decrypt(
    session: &mut Session,
    body: &[u8],
    boundary: &str,
) -> Result<Vec<u8>, TransportError> {
    let marker = format!("--{boundary}").into_bytes();
    let mut out = Vec::new();
    let mut parts = 0usize;
    let mut pos = 0usize;
    loop {
        let found = find(&body[pos..], &marker).ok_or_else(|| protocol("boundary not found"))?;
        if skip_blank(body, pos) != pos + found {
            return Err(protocol("unexpected bytes before boundary"));
        }
        pos += found + marker.len();
        if body[pos..].starts_with(b"--") {
            break;
        }
        pos = skip_line_end(body, pos)?;

        let next =
            find(&body[pos..], &marker).ok_or_else(|| protocol("metadata part is not closed"))?;
        let length = original_length(&body[pos..pos + next])?;
        pos += next + marker.len();
        pos = skip_line_end(body, pos)?;

        let mut octet_stream = false;
        while let Some((line, after)) = header_line(body, pos) {
            let line = line.trim();
            pos = after;
            if line.is_empty() {
                break;
            }
            let lower = line.to_ascii_lowercase();
            octet_stream |=
                lower.starts_with("content-type:") && lower.contains("application/octet-stream");
        }
        if !octet_stream {
            return Err(protocol("payload part is not application/octet-stream"));
        }

        let header = body
            .get(pos..pos + 4)
            .ok_or_else(|| protocol("payload is truncated"))?;
        let signature_len =
            u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
        if signature_len != SIGNATURE_LEN {
            return Err(protocol("signature length is not 16"));
        }
        let signature_at = pos + 4;
        let data_at = signature_at + SIGNATURE_LEN;
        let end = data_at
            .checked_add(length)
            .filter(|&end| end <= body.len())
            .ok_or_else(|| protocol("payload is shorter than OriginalContent Length"))?;
        let plain = session
            .unseal(&body[signature_at..data_at], &body[data_at..end])
            .map_err(|e| TransportError::Protocol(e.to_string()))?;
        out.extend_from_slice(&plain);
        parts += 1;
        pos = end;
    }
    if parts == 0 {
        return Err(protocol("no encrypted part"));
    }
    Ok(out)
}
