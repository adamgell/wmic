use super::WideText;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatSpec {
    pub before: Vec<TranslationTable>,
    pub after: Vec<TranslationTable>,
    pub transforms: Vec<TransformSpec>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransformSpec {
    pub name: WideText,
    pub parameters: Vec<(WideText, WideText)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderContext {
    #[serde(default)]
    pub components_xml: WideText,
    pub sequence: u32,
    pub every_count: u32,
    pub issued_from: WideText,
    pub started_at: WideText,
    pub command_line: WideText,
    pub nodes: Vec<WideText>,
    pub settings: Vec<(WideText, WideText)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslationTable {
    pub name: WideText,
    pub entries: Vec<(WideText, WideText)>,
}
