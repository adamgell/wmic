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

/// Built-in CLI aliases use WMIC's case- and width-insensitive comparison.
/// Names with an extension are stylesheet filenames, resolved by the XSL engine.
pub fn builtin_kind(name: &WideText) -> Option<BuiltinKind> {
    [
        (BuiltinKind::Mof, &["mof", "wmiclimofformat"][..]),
        (BuiltinKind::Htable, &["htable", "htable-sortby"][..]),
        (BuiltinKind::Xml, &["xml"][..]),
        (BuiltinKind::Hform, &["hform"][..]),
        (BuiltinKind::Rawxml, &["rawxml"][..]),
        (BuiltinKind::Csv, &["csv"][..]),
        (
            BuiltinKind::Table,
            &[
                "table",
                "texttable",
                "texttablewsys",
                "wmiclitableformat",
                "wmiclitableformatnosys",
            ][..],
        ),
        (
            BuiltinKind::List,
            &["value", "list", "textvaluelist", "wmiclivalueformat"][..],
        ),
    ]
    .into_iter()
    .find_map(|(kind, aliases)| {
        aliases
            .iter()
            .any(|alias| name.eq_cli(alias))
            .then_some(kind)
    })
}

pub fn builtin_source(name: &WideText) -> Option<WideText> {
    // Resource consumers use literal filenames; CLI resolution checks builtin_kind first.
    let kind = builtin_kind(name).or_else(|| {
        let suffix = name.0.len().checked_sub(4)?;
        WideText(name.0[suffix..].to_vec())
            .eq_cli(".xsl")
            .then(|| builtin_kind(&WideText(name.0[..suffix].to_vec())))
            .flatten()
    })?;
    let (function, parameters) = match kind {
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
    let resource = match builtin_kind(name) {
        Some(BuiltinKind::Table) => "texttable.xsl",
        Some(BuiltinKind::List) => "textvaluelist.xsl",
        Some(BuiltinKind::Rawxml) => "rawxml.xsl",
        Some(BuiltinKind::Csv) => "csv.xsl",
        Some(BuiltinKind::Mof) => "mof.xsl",
        Some(BuiltinKind::Xml) => "xml.xsl",
        Some(BuiltinKind::Hform) => "hform.xsl",
        Some(BuiltinKind::Htable) => "htable.xsl",
        None => return name.clone(),
    };
    #[cfg(windows)]
    let directory = std::env::var("SystemRoot")
        .unwrap_or_else(|_| "C:\\WINDOWS".into())
        .to_uppercase();
    #[cfg(not(windows))]
    let directory = "C:\\WINDOWS";
    format!("{directory}\\system32\\wbem\\\\{resource}").into()
}
