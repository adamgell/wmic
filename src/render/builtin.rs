use super::*;
pub fn builtin_source(name: &WideText) -> Option<WideText> {
    let name = name.to_string_lossy().to_ascii_lowercase();
    let name = name.strip_suffix(".xsl").unwrap_or(&name);
    let (function, parameters) = match name {
        "mof" | "wmiclimofformat" => ("mof(.)", ""),
        "htable" | "htable-sortby" => (
            "htable(.,string($sortby),string($datatype))",
            "<xsl:param name=\"sortby\"/><xsl:param name=\"datatype\"/>",
        ),
        "xml" => ("xml(.)", ""),
        "hform" => ("hform(.)", ""),
        "rawxml" => return Some(include_str!("../../assets/formats/rawxml.xsl").into()),
        "csv" => (
            "csv(.,string($norefcomma))",
            "<xsl:param name=\"norefcomma\"/>",
        ),
        "table"
        | "texttable"
        | "texttablewsys"
        | "wmiclitableformat"
        | "wmiclitableformatnosys" => ("table(.)", ""),
        "value" | "list" | "textvaluelist" | "wmiclivalueformat" => ("list(.)", ""),
        _ => return None,
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
