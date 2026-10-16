use super::WideText;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryMode {
    Process,
    Interactive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verb {
    Get,
    List,
    Assoc,
    Call,
    Create,
    Delete,
    Set,
    Help,
    Context,
    Exit,
    Interactive,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Target {
    Alias(WideText),
    Path(WideText),
    Class(WideText),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModifierScope {
    Global,
    Verb,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwitchValue {
    Missing,
    Reset,
    Value(WideText),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modifier {
    #[serde(default)]
    pub raw_value: Option<WideText>,
    pub name: WideText,
    pub value: SwitchValue,
    pub scope: ModifierScope,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedCommand {
    /// Original post-executable spacing, with credential values redacted for presentation.
    #[serde(default)]
    pub command_line: WideText,
    pub raw: WideText,
    pub target: Option<Target>,
    pub selector: Option<WideText>,
    pub verb: Verb,
    pub verb_explicit: bool,
    pub help: bool,
    pub properties: Vec<WideText>,
    pub view: Option<WideText>,
    pub method: Option<WideText>,
    pub arguments: Vec<WideText>,
    #[serde(default)]
    pub raw_arguments: Vec<WideText>,
    pub positionals: Vec<WideText>,
    pub modifiers: Vec<Modifier>,
}

impl ParsedCommand {
    pub(crate) fn empty(raw: WideText) -> Self {
        Self {
            command_line: raw.clone(),
            raw,
            target: None,
            selector: None,
            verb: Verb::Interactive,
            verb_explicit: false,
            help: false,
            properties: vec![],
            view: None,
            method: None,
            arguments: vec![],
            raw_arguments: vec![],
            positionals: vec![],
            modifiers: vec![],
        }
    }
}
