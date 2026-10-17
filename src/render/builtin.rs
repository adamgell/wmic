use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinKind {
    Mof,
    Htable,
    Xml,
    Hform,
    Rawxml,
    Csv,
    Table,
    List,
}

/// Case-insensitive stylesheet name, with or without the `.xsl` suffix.
pub fn builtin_kind(name: &WideText) -> Option<BuiltinKind> {
    let name = name.to_string_lossy().to_ascii_lowercase();
    let name = name.strip_suffix(".xsl").unwrap_or(&name);
    Some(match name {
        "mof" | "wmiclimofformat" => BuiltinKind::Mof,
        "htable" | "htable-sortby" => BuiltinKind::Htable,
        "xml" => BuiltinKind::Xml,
        "hform" => BuiltinKind::Hform,
        "rawxml" => BuiltinKind::Rawxml,
        "csv" => BuiltinKind::Csv,
        "table"
        | "texttable"
        | "texttablewsys"
        | "wmiclitableformat"
        | "wmiclitableformatnosys" => BuiltinKind::Table,
        "value" | "list" | "textvaluelist" | "wmiclivalueformat" => BuiltinKind::List,
        _ => return None,
    })
}

pub fn builtin_source(name: &WideText) -> Option<WideText> {
    let (function, parameters) = match builtin_kind(name)? {
        BuiltinKind::Mof => ("mof(.)", ""),
        BuiltinKind::Htable => (
            "htable(.,string($sortby),string($datatype))",
            "<xsl:param name=\"sortby\"/><xsl:param name=\"datatype\"/>",
        ),
        BuiltinKind::Xml => ("xml(.)", ""),
        BuiltinKind::Hform => ("hform(.)", ""),
        BuiltinKind::Rawxml => {
            return Some(include_str!("../../assets/formats/rawxml.xsl").into());
        }
        BuiltinKind::Csv => (
            "csv(.,string($norefcomma))",
            "<xsl:param name=\"norefcomma\"/>",
        ),
        BuiltinKind::Table => ("table(.)", ""),
        BuiltinKind::List => ("list(.)", ""),
    };
    Some(format!(r#"<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xmlns:m="urn:schemas-microsoft-com:xslt" xmlns:c="urn:wmic-compat:renderers"><xsl:output method="text" encoding="utf-16" omit-xml-declaration="yes"/>{parameters}<m:script language="JScript" implements-prefix="c"><![CDATA[{}]]></m:script><xsl:template match="/"><xsl:value-of select="c:{function}"/></xsl:template></xsl:stylesheet>"#,include_str!("../../assets/formats/renderers.js")).into())
}

pub fn logical_format_name(name: &WideText) -> WideText {
    let lower = name.to_string_lossy().to_ascii_lowercase();
    let bare = lower.strip_suffix(".xsl").unwrap_or(&lower);
    let resource = match bare {
        "table"
        | "texttable"
        | "texttablewsys"
        | "wmiclitableformat"
        | "wmiclitableformatnosys" => "texttable.xsl",
        "value" | "list" | "textvaluelist" | "wmiclivalueformat" => "textvaluelist.xsl",
        "rawxml" => "rawxml.xsl",
        "csv" => "csv.xsl",
        "mof" | "wmiclimofformat" => "mof.xsl",
        "xml" => "xml.xsl",
        "hform" => "hform.xsl",
        "htable" | "htable-sortby" => "htable.xsl",
        _ => return name.clone(),
    };
    #[cfg(windows)]
    let directory = std::env::var("SystemRoot")
        .unwrap_or_else(|_| "C:\\WINDOWS".into())
        .to_uppercase();
    #[cfg(not(windows))]
    let directory = "C:\\WINDOWS";
    format!("{directory}\\system32\\wbem\\\\{resource}").into()
}
