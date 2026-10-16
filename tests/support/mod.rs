#![allow(dead_code)]
use wmic_compat::model::WideText;

pub fn wide(text: &str) -> WideText {
    WideText(text.encode_utf16().collect())
}
