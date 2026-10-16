#![allow(dead_code)]
use wmic_compat::model::WideText;

pub fn wide(text: &str) -> WideText {
    WideText(text.encode_utf16().collect())
}

/// Resolves WMIC command text through the stock alias store into the backend request it produces.
pub fn request(text: &str) -> wmic_compat::backend::ResolvedRequest {
    use wmic_compat::{
        alias::{AliasStore, ResolvedCommand},
        model::EntryMode,
        parse::parse_command,
        session::SessionSettings,
    };
    let parsed = parse_command(&wide(text), EntryMode::Interactive).unwrap();
    let ResolvedCommand::Management(request) = AliasStore::stock()
        .unwrap()
        .resolve(&parsed, &SessionSettings::default())
        .unwrap()
    else {
        panic!("{text} is not a management request")
    };
    *request
}
