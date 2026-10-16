use crate::model::WideText;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSettings {
    pub namespace: WideText,
    pub role: WideText,
    pub nodes: Vec<WideText>,
    pub explicit_namespace: bool,
    pub explicit_role: bool,
}

impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            namespace: r"root\cimv2".into(),
            role: r"root\cli".into(),
            nodes: vec![".".into()],
            explicit_namespace: false,
            explicit_role: false,
        }
    }
}
