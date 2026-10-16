//! Typed reads over untyped Core Foundation property lists.
use super::iokit::nul_terminated;
use core_foundation::{
    array::CFArray,
    base::{CFType, TCFType},
    data::CFData,
    date::CFDate,
    dictionary::CFDictionary,
    number::CFNumber,
    string::CFString,
};

pub type Dictionary = CFDictionary<CFString, CFType>;

/// Seconds between the Unix epoch and the Core Foundation epoch (2001-01-01).
const ABSOLUTE_TIME_TO_UNIX: i64 = 978_307_200;

pub fn dictionary(value: &CFType) -> Option<Dictionary> {
    let untyped = value.downcast::<CFDictionary>()?;
    // The get rule retains the shared reference; the untyped handle releases its own.
    Some(unsafe { Dictionary::wrap_under_get_rule(untyped.as_concrete_TypeRef()) })
}

pub fn array(value: &CFType) -> Option<CFArray<CFType>> {
    let untyped = value.downcast::<CFArray>()?;
    // The get rule retains the shared reference; the untyped handle releases its own.
    Some(unsafe { CFArray::<CFType>::wrap_under_get_rule(untyped.as_concrete_TypeRef()) })
}

pub fn field(dictionary: &Dictionary, key: &str) -> Option<CFType> {
    dictionary
        .find(CFString::new(key))
        .map(|value| (*value).clone())
}

/// Text of a string value; byte-array values are read up to the first NUL.
pub fn text_of(value: &CFType) -> Option<String> {
    if let Some(text) = value.downcast::<CFString>() {
        return Some(text.to_string());
    }
    Some(nul_terminated(value.downcast::<CFData>()?.bytes()))
}

pub fn text(dictionary: &Dictionary, key: &str) -> Option<String> {
    text_of(&field(dictionary, key)?)
}

pub fn number(dictionary: &Dictionary, key: &str) -> Option<i64> {
    field(dictionary, key)?.downcast::<CFNumber>()?.to_i64()
}

pub fn bytes(dictionary: &Dictionary, key: &str) -> Option<Vec<u8>> {
    Some(
        field(dictionary, key)?
            .downcast::<CFData>()?
            .bytes()
            .to_vec(),
    )
}

/// Text elements of an array value; other elements are skipped.
pub fn strings(dictionary: &Dictionary, key: &str) -> Vec<String> {
    let Some(list) = field(dictionary, key).and_then(|v| array(&v)) else {
        return vec![];
    };
    list.iter()
        .filter_map(|item| item.downcast::<CFString>().map(|s| s.to_string()))
        .collect()
}

/// Unix time of a date value.
pub fn unix_time(dictionary: &Dictionary, key: &str) -> Option<i64> {
    let date = field(dictionary, key)?.downcast::<CFDate>()?;
    Some(date.abs_time().floor() as i64 + ABSOLUTE_TIME_TO_UNIX)
}
