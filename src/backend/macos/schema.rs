//! Class definitions come from an embedded dump of the Windows schema; only supported classes are present.
use crate::model::{
    CimValue, ClassSchema, MethodSchema, ParameterSchema, PropertySchema, Qualifier, WideText,
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    sync::OnceLock,
};

const SCHEMA: &str = include_str!("../../../assets/macos-schema.json");

#[derive(Deserialize)]
struct WireClass {
    name: String,
    #[serde(rename = "super")]
    superclass: Option<String>,
    #[serde(default)]
    qualifiers: HashMap<String, serde_json::Value>,
    properties: Vec<WireProperty>,
    methods: Vec<WireMethod>,
}

#[derive(Deserialize)]
struct WireProperty {
    name: String,
    #[serde(rename = "type")]
    cim_type: u32,
    #[serde(default)]
    qualifiers: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct WireMethod {
    name: String,
    #[serde(rename = "static", default)]
    is_static: bool,
    return_type: Option<String>,
    #[serde(rename = "in", default)]
    input: Vec<WireParameter>,
    #[serde(rename = "out", default)]
    output: Vec<WireParameter>,
}

#[derive(Deserialize)]
struct WireParameter {
    name: String,
    #[serde(rename = "type")]
    cim_type: u32,
    id: Option<u32>,
    #[serde(default)]
    optional: bool,
    cimtype_name: Option<String>,
    #[serde(default)]
    qualifiers: HashMap<String, serde_json::Value>,
}

/// Name, CIM type and key flag of one schema property.
#[derive(Clone, Debug)]
pub struct PropertyInfo {
    pub name: String,
    pub cim_type: u32,
    pub key: bool,
}

/// One supported class: the public schema plus the lookups providers and the query path need.
#[derive(Clone, Debug)]
pub struct ClassInfo {
    pub name: String,
    pub superclass: Option<String>,
    /// Ancestors from the direct superclass to the root.
    pub derivation: Vec<String>,
    pub dynasty: String,
    pub singleton: bool,
    /// Schema order.
    pub properties: Vec<PropertyInfo>,
    pub schema: ClassSchema,
}

impl ClassInfo {
    /// Index and definition of a property, matched ignoring ASCII case.
    pub fn property(&self, name: &str) -> Option<(usize, &PropertyInfo)> {
        self.properties
            .iter()
            .enumerate()
            .find(|(_, p)| p.name.eq_ignore_ascii_case(name))
    }

    /// Key properties in the alphabetical order WMI uses for object paths.
    pub fn keys(&self) -> Vec<usize> {
        let mut keys: Vec<usize> = (0..self.properties.len())
            .filter(|i| self.properties[*i].key)
            .collect();
        keys.sort_by_key(|i| self.properties[*i].name.to_ascii_lowercase());
        keys
    }
}

fn parent(class: &str) -> Option<&'static str> {
    Some(match class {
        "CIM_OperatingSystem" | "CIM_System" | "CIM_Service" => "CIM_EnabledLogicalElement",
        "CIM_EnabledLogicalElement"
        | "CIM_LogicalDevice"
        | "CIM_SoftwareElement"
        | "CIM_SystemResource"
        | "CIM_Process"
        | "Win32_Session"
        | "Win32_Account" => "CIM_LogicalElement",
        "CIM_LogicalElement" | "CIM_PhysicalElement" => "CIM_ManagedSystemElement",
        "CIM_Processor"
        | "CIM_MediaAccessDevice"
        | "CIM_NetworkAdapter"
        | "CIM_StorageExtent"
        | "CIM_Sensor" => "CIM_LogicalDevice",
        "CIM_UnitaryComputerSystem" => "CIM_ComputerSystem",
        "CIM_ComputerSystem" => "CIM_System",
        "CIM_BIOSElement" => "CIM_SoftwareElement",
        "CIM_Card" | "CIM_PhysicalFrame" => "CIM_PhysicalPackage",
        "CIM_PhysicalPackage" | "CIM_PhysicalComponent" => "CIM_PhysicalElement",
        "CIM_Chassis" => "CIM_PhysicalFrame",
        "CIM_PhysicalMemory" => "CIM_Chip",
        "CIM_Chip" => "CIM_PhysicalComponent",
        "CIM_TemperatureSensor" | "CIM_VoltageSensor" => "CIM_NumericSensor",
        "CIM_NumericSensor" => "CIM_Sensor",
        "CIM_CDROMDrive" | "CIM_TapeDrive" | "CIM_DiskDrive" => "CIM_MediaAccessDevice",
        "Win32_BaseService" => "CIM_Service",
        "CIM_LogicalDisk" | "CIM_StorageVolume" | "CIM_GenericDiskPartition" => "CIM_StorageExtent",
        "CIM_DiskPartition" => "CIM_GenericDiskPartition",
        _ => return None,
    })
}

fn derivation(superclass: Option<&str>) -> Vec<String> {
    let mut out = vec![];
    let mut at = superclass.map(str::to_owned);
    while let Some(class) = at {
        at = parent(&class).map(str::to_owned);
        out.push(class);
    }
    out
}

fn base_type(name: &str) -> u32 {
    match name.to_ascii_lowercase().as_str() {
        "boolean" => 11,
        "uint8" => 17,
        "sint8" => 16,
        "uint16" => 18,
        "sint16" => 2,
        "uint32" => 19,
        "sint32" => 3,
        "uint64" => 21,
        "sint64" => 20,
        "real32" => 4,
        "real64" => 5,
        "char16" => 103,
        "datetime" => 101,
        "reference" => 102,
        "instance" => 13,
        _ => 8,
    }
}

fn flag(qualifiers: &HashMap<String, serde_json::Value>, name: &str) -> bool {
    qualifiers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case(name) && v.as_bool() == Some(true))
}

fn qualifier(name: &str, value: bool) -> Qualifier {
    Qualifier {
        name: name.into(),
        value: CimValue::Boolean(value),
    }
}

fn parameter(wire: &WireParameter) -> ParameterSchema {
    let object = wire.cim_type & 0xfff == 13;
    let cim_type_name = if object {
        let embedded = wire
            .qualifiers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("EmbeddedInstance"))
            .and_then(|(_, v)| v.as_str());
        Some(embedded.map_or_else(|| "object".to_owned(), |c| format!("object:{c}")))
    } else {
        wire.cimtype_name.clone()
    };
    ParameterSchema {
        cim_type_name: cim_type_name.map(WideText::from),
        name: wire.name.as_str().into(),
        cim_type: wire.cim_type,
        id: wire.id,
        optional: wire.optional,
        default_value: None,
    }
}

fn method(wire: &WireMethod, implemented: bool) -> MethodSchema {
    let mut output: Vec<ParameterSchema> = wire.output.iter().map(parameter).collect();
    if let Some(ty) = &wire.return_type {
        output.push(ParameterSchema {
            cim_type_name: Some(ty.to_ascii_lowercase().into()),
            name: "ReturnValue".into(),
            cim_type: base_type(ty),
            id: None,
            optional: false,
            default_value: None,
        });
    }
    MethodSchema {
        name: wire.name.as_str().into(),
        input: wire.input.iter().map(parameter).collect(),
        output,
        is_static: wire.is_static,
        qualifiers: vec![qualifier("implemented", implemented)],
    }
}

fn class(wire: &WireClass, implemented: &[&str]) -> ClassInfo {
    let derivation = derivation(wire.superclass.as_deref());
    let properties: Vec<PropertyInfo> = wire
        .properties
        .iter()
        .map(|p| PropertyInfo {
            name: p.name.clone(),
            cim_type: p.cim_type,
            key: flag(&p.qualifiers, "key"),
        })
        .collect();
    let schema = ClassSchema {
        wmi_xml: None,
        name: wire.name.as_str().into(),
        properties: wire
            .properties
            .iter()
            .filter(|p| !p.name.starts_with("__"))
            .map(|p| {
                let mut qualifiers = vec![qualifier("read", true)];
                if flag(&p.qualifiers, "write") {
                    qualifiers.push(qualifier("write", true));
                }
                PropertySchema {
                    name: p.name.as_str().into(),
                    cim_type: p.cim_type,
                    qualifiers,
                }
            })
            .collect(),
        methods: wire
            .methods
            .iter()
            .map(|m| {
                method(
                    m,
                    implemented.iter().any(|n| n.eq_ignore_ascii_case(&m.name)),
                )
            })
            .collect(),
        qualifiers: vec![],
    };
    ClassInfo {
        name: wire.name.clone(),
        superclass: wire.superclass.clone(),
        dynasty: derivation
            .last()
            .cloned()
            .unwrap_or_else(|| wire.name.clone()),
        derivation,
        singleton: flag(&wire.qualifiers, "Singleton"),
        properties,
        schema,
    }
}

fn wire() -> &'static BTreeMap<String, WireClass> {
    static PARSED: OnceLock<BTreeMap<String, WireClass>> = OnceLock::new();
    PARSED.get_or_init(|| serde_json::from_str(SCHEMA).expect("embedded macOS schema"))
}

/// Builds the definition of one class by name; `implemented` lists methods a provider executes.
pub fn load(name: &str, implemented: &[&str]) -> Option<ClassInfo> {
    let (_, found) = wire().iter().find(|(k, _)| k.eq_ignore_ascii_case(name))?;
    Some(class(found, implemented))
}

/// Names of every class in the embedded schema.
pub fn class_names() -> Vec<String> {
    wire().keys().cloned().collect()
}
