use crate::model::{Modifier, Verb, WideText};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionOptions {
    pub node: WideText,
    pub namespace: WideText,
    pub locale: Option<WideText>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedRequest {
    pub connection: ConnectionOptions,
    pub class_name: WideText,
    pub instance_path: Option<WideText>,
    pub class_mode: bool,
    pub query: Option<WideText>,
    pub selector: Option<WideText>,
    pub verb: Verb,
    pub properties: Vec<WideText>,
    pub method: Option<WideText>,
    pub arguments: Vec<WideText>,
    pub modifiers: Vec<Modifier>,
    pub view: Option<WideText>,
    pub display: Vec<(WideText, WideText)>,
    pub format: Option<WideText>,
}
