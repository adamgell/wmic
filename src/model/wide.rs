use serde::{Deserialize, Serialize};
use std::fmt;

/// Windows text, including code units that cannot be represented by a Rust String.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WideText(pub Vec<u16>);

impl WideText {
    pub fn eq_ascii_ignore_case(&self, text: &str) -> bool {
        text.is_ascii()
            && self.0.len() == text.len()
            && self
                .0
                .iter()
                .zip(text.bytes())
                .all(|(&unit, byte)| unit < 128 && (unit as u8).eq_ignore_ascii_case(&byte))
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
