use serde::{Deserialize, Serialize};
use std::fmt;

/// Windows text, including code units that cannot be represented by a Rust String.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WideText(pub Vec<u16>);

impl WideText {
    pub fn eq_ignore_case(&self, text: &str) -> bool {
        if text.is_ascii() && self.0.iter().all(|u| *u < 128) {
            return self.0.len() == text.len()
                && self
                    .0
                    .iter()
                    .zip(text.bytes())
                    .all(|(&a, b)| (a as u8).eq_ignore_ascii_case(&b));
        }
        self.eq_ordinal_ignore_case(&WideText::from(text))
    }
    /// Windows identifiers use the OS ordinal uppercase table, without normalization.
    pub fn eq_ordinal_ignore_case(&self, other: &Self) -> bool {
        #[cfg(windows)]
        {
            (unsafe {
                windows::Win32::Globalization::CompareStringOrdinal(&self.0, &other.0, true)
            }) == windows::Win32::Globalization::CSTR_EQUAL
        }
        #[cfg(not(windows))]
        {
            fn upper(value: &[u16]) -> Vec<u16> {
                let mut out = vec![];
                for item in char::decode_utf16(value.iter().copied()) {
                    match item {
                        Ok(ch) => {
                            let mut uppercase = ch.to_uppercase();
                            let first = uppercase.next().unwrap();
                            let ch = if uppercase.next().is_none() {
                                first
                            } else {
                                ch
                            };
                            let mut buffer = [0; 2];
                            out.extend_from_slice(ch.encode_utf16(&mut buffer));
                        }
                        Err(e) => out.push(e.unpaired_surrogate()),
                    }
                }
                out
            }
            upper(&self.0) == upper(&other.0)
        }
    }

    /// Only for human diagnostics; parsing and native calls retain the original units.
    pub fn to_string_lossy(&self) -> String {
        String::from_utf16_lossy(&self.0)
    }

    pub fn trim_ascii(&self) -> Self {
        let start = self
            .0
            .iter()
            .position(|u| !is_space(*u))
            .unwrap_or(self.0.len());
        let end = self
            .0
            .iter()
            .rposition(|u| !is_space(*u))
            .map_or(start, |n| n + 1);
        Self(self.0[start..end].to_vec())
    }
}

pub(crate) fn is_space(unit: u16) -> bool {
    matches!(unit, 9 | 10 | 13 | 32)
}

impl From<&str> for WideText {
    fn from(text: &str) -> Self {
        Self(text.encode_utf16().collect())
    }
}

impl From<String> for WideText {
    fn from(text: String) -> Self {
        Self::from(text.as_str())
    }
}

impl fmt::Display for WideText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_lossy())
    }
}

#[cfg(all(test, windows))]
mod native_tests {
    use super::*;
    #[test]
    #[ignore = "requires Windows ordinal uppercase table"]
    fn identifiers_use_windows_case_comparison_without_losing_surrogates() {
        assert!(WideText::from("Café").eq_ignore_case("CAFÉ"));
        assert!(WideText::from("σ").eq_ignore_case("Σ"));
        assert!(!WideText(vec![0xd800]).eq_ordinal_ignore_case(&WideText::from("�")));
    }
}
