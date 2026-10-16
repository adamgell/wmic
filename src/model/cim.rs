use super::{Verb, WideText};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CimValue {
    Empty,
    Null,
    String(WideText),
    Boolean(bool),
    Sint8(i8),
    Uint8(u8),
    Sint16(i16),
    Uint16(u16),
    Sint32(i32),
    Uint32(u32),
    Sint64(i64),
    Uint64(u64),
    /// IEEE bits preserve NaNs and signed zero through fixtures.
    Real32(u32),
    Real64(u64),
    Char16(u16),
    DateTime(WideText),
    Reference(WideText),
    Object(Box<Record>),
    Array(Vec<CimValue>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Qualifier {
    pub name: WideText,
    pub value: CimValue,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Property {
    pub name: WideText,
    pub value: CimValue,
    pub cim_type: u32,
    pub system: bool,
    pub qualifiers: Vec<Qualifier>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Record {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_node: Option<WideText>,
    /// Native WMI CIM DTD text retains origin/propagation metadata for presentation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wmi_xml: Option<WideText>,
    pub class: WideText,
    pub path: Option<WideText>,
    pub server: Option<WideText>,
    pub properties: Vec<Property>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropertySchema {
    pub name: WideText,
    pub cim_type: u32,
    pub qualifiers: Vec<Qualifier>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParameterSchema {
    pub name: WideText,
    pub cim_type: u32,
    pub id: Option<u32>,
    pub optional: bool,
    pub default_value: Option<CimValue>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MethodSchema {
    pub name: WideText,
    pub input: Vec<ParameterSchema>,
    pub output: Vec<ParameterSchema>,
    pub is_static: bool,
    pub qualifiers: Vec<Qualifier>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ClassSchema {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wmi_xml: Option<WideText>,
    pub name: WideText,
    pub properties: Vec<PropertySchema>,
    pub methods: Vec<MethodSchema>,
    pub qualifiers: Vec<Qualifier>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ObjectSet {
    pub objects: Vec<Record>,
    pub schema: Option<ClassSchema>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MethodResult {
    pub path: WideText,
    pub return_value: Option<CimValue>,
    pub out_parameters: Vec<Property>,
    /// Optional native MOF text preserves WMI provider serialization.
    #[serde(default)]
    pub text: Option<WideText>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OperationResult {
    pub action: Verb,
    pub paths: Vec<WideText>,
    pub instances: Vec<Record>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ManagementResult {
    Associations(Vec<AssociationGroup>),
    Objects(ObjectSet),
    Schema(ClassSchema),
    Methods(Vec<MethodResult>),
    Operation(OperationResult),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssociationGroup {
    pub source: Record,
    pub associated: Vec<Record>,
}
