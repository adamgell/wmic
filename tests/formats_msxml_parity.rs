//! The portable renderers must agree with the MSXML pipeline on the same input.
#![cfg(windows)]
use wmic_compat::{
    model::*,
    render::{native::transform_builtin, transform},
};

fn wide(text: &str) -> WideText {
    WideText(text.encode_utf16().collect())
}

fn spec(stages: &[(&str, &[(&str, &str)])]) -> FormatSpec {
    FormatSpec {
        transforms: stages
            .iter()
            .map(|(name, parameters)| TransformSpec {
                name: wide(name),
                parameters: parameters.iter().map(|(n, v)| (wide(n), wide(v))).collect(),
            })
            .collect(),
        before: vec![],
        after: vec![],
    }
}

fn outcome(
    result: Result<wmic_compat::render::RenderedOutput, wmic_compat::render::RenderError>,
) -> Result<String, (u32, String)> {
    result
        .map(|o| o.stdout.to_string_lossy())
        .map_err(|e| (e.code, e.message.to_string_lossy()))
}

thread_local! {
    static CONTEXT: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    static MISMATCHES: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(vec![]) };
}

fn clip(text: &str, at: usize) -> String {
    let start = at.saturating_sub(40);
    text.chars()
        .skip(start)
        .take(100)
        .collect::<String>()
        .escape_debug()
        .to_string()
}

fn same(xml: &str, spec: &FormatSpec) {
    let reference = outcome(transform(&wide(xml), spec));
    let portable = outcome(transform_builtin(&wide(xml), spec));
    if portable == reference {
        return;
    }
    let stages: Vec<_> = spec
        .transforms
        .iter()
        .map(|t| t.name.to_string_lossy())
        .collect();
    let detail = match (&portable, &reference) {
        (Ok(a), Ok(b)) => {
            let at = a
                .chars()
                .zip(b.chars())
                .position(|(x, y)| x != y)
                .unwrap_or(a.chars().count().min(b.chars().count()));
            format!(
                "portable ...{}\n      msxml    ...{}",
                clip(a, at),
                clip(b, at)
            )
        }
        _ => format!("portable {portable:?}\n      msxml    {reference:?}"),
    };
    let context = CONTEXT.with(|c| c.borrow().clone());
    MISMATCHES.with(|m| {
        m.borrow_mut().push(format!(
            "{stages:?} before={} after={} context {context}
      {detail}",
            spec.before.len(),
            spec.after.len(),
        ))
    });
}

fn finish() {
    let all = MISMATCHES.with(|m| std::mem::take(&mut *m.borrow_mut()));
    if !all.is_empty() {
        let shown: Vec<_> = all.iter().take(40).cloned().collect();
        panic!("{} mismatches\n{}", all.len(), shown.join("\n"));
    }
}

const BUILTINS: [&str; 8] = [
    "mof", "htable", "xml", "hform", "rawxml", "csv", "table", "list",
];

fn all_builtins(xml: &str) {
    for name in BUILTINS {
        same(xml, &spec(&[(name, &[])]));
    }
}

fn instance(properties: &str) -> String {
    format!(
        "<COMMAND SEQUENCENUM=\"1\"><REQUEST><COMMANDLINE> x </COMMANDLINE></REQUEST><RESULTS NODE=\"N&amp;1\"><CIM><INSTANCE CLASSNAME=\"C&lt;1\">{properties}</INSTANCE></CIM></RESULTS></COMMAND>"
    )
}

#[test]
#[ignore = "requires Windows MSXML6"]
fn captured_envelopes_agree() {
    for xml in [
        include_str!("fixtures/reference/os-get-rawxml.stdout.bin"),
        include_str!("fixtures/format-reference/format-rawxml.stdout.bin"),
        include_str!("fixtures/format-reference/format-types-rawxml.stdout.bin"),
        include_str!("fixtures/format-reference/format-rawxml-system.stdout.bin"),
        include_str!("fixtures/format-reference/reference-rawxml.stdout.bin"),
        include_str!("fixtures/format-reference/object-rawxml.stdout.bin"),
    ] {
        all_builtins(xml);
        same(xml, &spec(&[("csv", &[("norefcomma", "true")])]));
        same(xml, &spec(&[("rawxml", &[]), ("csv", &[])]));
        same(xml, &spec(&[("csv", &[]), ("rawxml", &[])]));
        same(xml, &spec(&[("htable", &[("sortby", "Caption")])]));
    }
    finish();
}

#[test]
#[ignore = "requires Windows MSXML6"]
fn value_text_edge_cases_agree() {
    for value in [
        "plain",
        "",
        " ",
        "  lead and trail  ",
        "a\r\nb",
        "a\nb",
        "a\rb",
        "a&#13;b",
        "a&#10;b",
        "a&#9;b",
        "tab\there",
        "a &amp; b &lt; c &gt; d &quot;q&quot; &apos;s&apos;",
        "a > b ]] c",
        "<![CDATA[raw <&> text]]>",
        "<![CDATA[ ]]>",
        "wide \u{5b57}\u{6f22}",
        "astral \u{1f600} end",
        "quote\"s and 'apos'",
        "&#x41;&#66;",
        "  \u{00a0}nbsp  ",
        "x<!-- c -->y",
        "x<?pi data?>y",
        "multi   space\t\tand\nlines",
    ] {
        for kind in ["string", "datetime", "char16", "uint32"] {
            all_builtins(&instance(&format!(
                "<PROPERTY NAME=\"P\" TYPE=\"{kind}\"><VALUE>{value}</VALUE></PROPERTY>"
            )));
        }
    }
    finish();
}

#[test]
#[ignore = "requires Windows MSXML6"]
fn property_shapes_agree() {
    for properties in [
        "",
        "<PROPERTY NAME=\"A\" TYPE=\"string\"></PROPERTY>",
        "<PROPERTY NAME=\"A\" TYPE=\"string\"/>",
        "<PROPERTY NAME=\"A\" TYPE=\"string\"><VALUE/></PROPERTY>",
        "<PROPERTY NAME=\"A\" TYPE=\"string\"><VALUE></VALUE></PROPERTY>",
        "<PROPERTY.ARRAY NAME=\"A\" TYPE=\"string\"></PROPERTY.ARRAY>",
        "<PROPERTY.ARRAY NAME=\"A\" TYPE=\"string\"><VALUE.ARRAY></VALUE.ARRAY></PROPERTY.ARRAY>",
        "<PROPERTY.ARRAY NAME=\"A\" TYPE=\"string\"><VALUE.ARRAY><VALUE>x</VALUE><VALUE>y &amp; z</VALUE><VALUE/></VALUE.ARRAY></PROPERTY.ARRAY>",
        "<PROPERTY.ARRAY NAME=\"A\" TYPE=\"char16\"><VALUE.ARRAY><VALUE>x</VALUE><VALUE>y</VALUE></VALUE.ARRAY></PROPERTY.ARRAY>",
        "<PROPERTY.ARRAY NAME=\"A\" TYPE=\"uint32\"><VALUE.ARRAY><VALUE>1</VALUE><VALUE>2</VALUE></VALUE.ARRAY></PROPERTY.ARRAY>",
        "<PROPERTY.ARRAY NAME=\"A\" TYPE=\"datetime\"><VALUE.ARRAY><VALUE>20260101000000.000000+000</VALUE></VALUE.ARRAY></PROPERTY.ARRAY>",
        "<PROPERTY NAME=\"B\" TYPE=\"boolean\"><VALUE>TRUE</VALUE></PROPERTY><PROPERTY.ARRAY NAME=\"A\" TYPE=\"string\"><VALUE.ARRAY><VALUE>x</VALUE></VALUE.ARRAY></PROPERTY.ARRAY><PROPERTY NAME=\"C\" TYPE=\"string\"><VALUE>z</VALUE></PROPERTY>",
        "<PROPERTY.REFERENCE NAME=\"R\"></PROPERTY.REFERENCE>",
        "<PROPERTY.REFERENCE NAME=\"R\"><VALUE.REFERENCE><INSTANCEPATH><NAMESPACEPATH><HOST>H</HOST><LOCALNAMESPACEPATH><NAMESPACE NAME=\"root\"/><NAMESPACE NAME=\"cimv2\"/></LOCALNAMESPACEPATH></NAMESPACEPATH><INSTANCENAME CLASSNAME=\"K\"><KEYBINDING NAME=\"a\"><KEYVALUE VALUETYPE=\"string\">1</KEYVALUE></KEYBINDING><KEYBINDING NAME=\"b\"><KEYVALUE>2</KEYVALUE></KEYBINDING></INSTANCENAME></INSTANCEPATH></VALUE.REFERENCE></PROPERTY.REFERENCE>",
        "<PROPERTY.REFERENCE NAME=\"R\"><VALUE.REFERENCE><INSTANCENAME CLASSNAME=\"K\"/></VALUE.REFERENCE></PROPERTY.REFERENCE>",
        "<PROPERTY.REFERENCE NAME=\"R\"><VALUE.REFERENCE><INSTANCENAME CLASSNAME=\"K\"><KEYBINDING NAME=\"a\"/></INSTANCENAME></VALUE.REFERENCE></PROPERTY.REFERENCE>",
        "<PROPERTY.OBJECT NAME=\"O\"><VALUE.OBJECT><INSTANCE CLASSNAME=\"I\"><PROPERTY NAME=\"X\" TYPE=\"string\"><VALUE>in</VALUE></PROPERTY></INSTANCE></VALUE.OBJECT></PROPERTY.OBJECT><PROPERTY NAME=\"Y\" TYPE=\"string\"><VALUE>out</VALUE></PROPERTY>",
        "<PROPERTY NAME=\"A\" TYPE=\"string\" CLASSORIGIN=\"c\" PROPAGATED=\"true\"><VALUE>v</VALUE></PROPERTY>",
        "<PROPERTY NAME=\"A\" TYPE=\"string\"><!-- c --><VALUE>v</VALUE></PROPERTY>",
        "<PROPERTY NAME=\"a&quot;&amp;&lt;&gt;b\" TYPE=\"string\"><VALUE>v</VALUE></PROPERTY>",
    ] {
        all_builtins(&instance(properties));
    }
    finish();
}

#[test]
#[ignore = "requires Windows MSXML6"]
fn document_shapes_agree() {
    for xml in [
        "<COMMAND/>",
        "<COMMAND></COMMAND>",
        "<OTHER><INSTANCE CLASSNAME=\"X\"><PROPERTY NAME=\"A\" TYPE=\"string\"><VALUE>1</VALUE></PROPERTY></INSTANCE></OTHER>",
        "<?xml version=\"1.0\"?><COMMAND/>",
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><COMMAND a=\"1\"><b/></COMMAND>",
        "<?xml version=\"1.0\" encoding=\"utf-16\" standalone=\"yes\"?><COMMAND a=\"1\"><b/></COMMAND>",
        "<!-- lead --><COMMAND/><!-- trail -->",
        "<?pi data?><COMMAND><?inner a=\"b\"?></COMMAND>",
        "<COMMAND><RESULTS NODE=\"A\"><CIM/></RESULTS><RESULTS NODE=\"B\"><CIM><INSTANCE CLASSNAME=\"X\"/></CIM></RESULTS></COMMAND>",
        "<COMMAND><RESULTS NODE=\"A\"><CIM><INSTANCE CLASSNAME=\"X\"><PROPERTY NAME=\"A\" TYPE=\"string\"><VALUE>1</VALUE></PROPERTY></INSTANCE></CIM></RESULTS><RESULTS NODE=\"B\"><CIM><INSTANCE CLASSNAME=\"X\"><PROPERTY NAME=\"A\" TYPE=\"string\"><VALUE>2</VALUE></PROPERTY><PROPERTY NAME=\"B\" TYPE=\"string\"><VALUE>3</VALUE></PROPERTY></INSTANCE></CIM></RESULTS></COMMAND>",
        "<COMMAND><RESULTS NODE=\"A\"><CIM><INSTANCE CLASSNAME=\"X\"><PROPERTY NAME=\"A\" TYPE=\"string\"><VALUE>1</VALUE></PROPERTY><PROPERTY NAME=\"B\" TYPE=\"string\"><VALUE>2</VALUE></PROPERTY></INSTANCE><INSTANCE CLASSNAME=\"Y\"><PROPERTY NAME=\"A\" TYPE=\"string\"><VALUE>3</VALUE></PROPERTY></INSTANCE></CIM></RESULTS></COMMAND>",
        "<COMMAND xml:space=\"preserve\">\n  <RESULTS NODE=\"A\">  <CIM> <INSTANCE CLASSNAME=\"X\"> <PROPERTY NAME=\"A\" TYPE=\"string\"> <VALUE>  </VALUE> </PROPERTY> </INSTANCE> </CIM> </RESULTS>\n</COMMAND>",
        "<COMMAND>\n  <RESULTS NODE=\"A\">  <CIM> <INSTANCE CLASSNAME=\"X\"> <PROPERTY NAME=\"A\" TYPE=\"string\"> <VALUE>  </VALUE> </PROPERTY> </INSTANCE> </CIM> </RESULTS>\n</COMMAND>",
        "<COMMAND>mixed <b>content</b> here</COMMAND>",
        "<COMMAND>  <b>x</b>  y  </COMMAND>",
        "<COMMAND a='single' b=\"dou'ble\" c='dou\"ble'/>",
        "<COMMAND a=\"line1\nline2\ttab\"/>",
        "<COMMAND a=\"x&#10;y&#9;z&#13;w\"/>",
        "<COMMAND a=\"1 &lt; 2 &amp;&gt; 3\"/>",
        "<ns:COMMAND xmlns:ns=\"urn:x\" ns:a=\"1\"><ns:b/></ns:COMMAND>",
        "<COMMAND xmlns=\"urn:x\"><b/></COMMAND>",
        "<COMMAND><![CDATA[a < b]]></COMMAND>",
        "<COMMAND>a&#x1F600;b</COMMAND>",
        "\u{feff}<COMMAND/>",
        "<COMMAND/>   ",
        "   <COMMAND/>",
        "\r\n<COMMAND/>\r\n",
    ] {
        all_builtins(xml);
    }
    finish();
}

#[test]
#[ignore = "requires Windows MSXML6"]
fn rejected_documents_agree() {
    for xml in [
        "",
        " ",
        "plain",
        "<a>",
        "<a></b>",
        "<a/><b/>",
        "<a b=1/>",
        "<a b=\"1\" b=\"2\"/>",
        "<!DOCTYPE a><a/>",
        "<!DOCTYPE a [<!ENTITY e \"x\">]><a>&e;</a>",
        "<a>&unknown;</a>",
        "<a>\u{1}</a>",
        "<a>\u{ffff}</a>",
        "<a><!-- -- --></a>",
        "<a><!-- bad ---></a>",
        "text<a/>",
        "<a/>text",
        " <?xml version=\"1.0\"?><a/>",
        "<a>]]></a>",
        "<a b=\"<\"/>",
        "<a:b/>",
        "<1a/>",
        "<a b/>",
        "<a ><b></a></b>",
        "<?xml version=\"1.0\"?><?xml version=\"1.0\"?><a/>",
        "<?xml encoding=\"UTF-8\"?><a/>",
        "<?pi?><a/>",
        "<a>&#0;</a>",
        "<a>&#xD800;</a>",
        "<a>&#x110000;</a>",
        "<a>&#;</a>",
        "<a>&amp</a>",
    ] {
        for name in ["rawxml", "csv"] {
            same(xml, &spec(&[(name, &[])]));
        }
    }
    finish();
}

#[test]
#[ignore = "requires Windows MSXML6"]
fn sort_parameters_agree() {
    let rows = |values: &[&str]| {
        let body: String = values
            .iter()
            .map(|v| {
                format!(
                    "<INSTANCE CLASSNAME=\"C\"><PROPERTY NAME=\"K\" TYPE=\"string\"><VALUE>{v}</VALUE></PROPERTY><PROPERTY NAME=\"Q\" TYPE=\"string\"><VALUE>{v}!</VALUE></PROPERTY></INSTANCE>"
                )
            })
            .collect();
        format!("<COMMAND><RESULTS NODE=\"N\"><CIM>{body}</CIM></RESULTS></COMMAND>")
    };
    let inputs = [
        rows(&["b", "a", "c", "B", "A", ""]),
        rows(&[
            "10", "9", "100", "-1", "1e2", "0x10", " 5 ", "abc", "", "1.5", ".5", "5.",
        ]),
        rows(&[
            "Infinity",
            "-Infinity",
            "5",
            "NaN",
            "+3",
            "1,5",
            "0b11",
            "0o7",
        ]),
        rows(&["\u{1f600}", "\u{ffe0}", "z", "\u{e9}", "Z"]),
        rows(&["3", "3", "3", "2", "2", "1"]),
    ];
    for xml in &inputs {
        for (sortby, datatype) in [
            ("", ""),
            ("K", ""),
            ("K", "number"),
            ("K", "string"),
            ("K", "NUMBER"),
            ("Q", ""),
            ("Q", "number"),
            ("k", ""),
            ("Missing", "number"),
            ("", "number"),
        ] {
            same(
                xml,
                &spec(&[("htable", &[("sortby", sortby), ("datatype", datatype)])]),
            );
        }
    }
    finish();
}

#[test]
#[ignore = "requires Windows MSXML6"]
fn parameter_forms_agree() {
    let xml = instance(
        "<PROPERTY.REFERENCE NAME=\"R\"><VALUE.REFERENCE><INSTANCEPATH><INSTANCENAME CLASSNAME=\"K\"><KEYBINDING NAME=\"a\"><KEYVALUE>1</KEYVALUE></KEYBINDING><KEYBINDING NAME=\"b\"><KEYVALUE>2</KEYVALUE></KEYBINDING></INSTANCENAME></INSTANCEPATH></VALUE.REFERENCE></PROPERTY.REFERENCE>",
    );
    for parameters in [
        &[][..],
        &[("norefcomma", "true")][..],
        &[("norefcomma", "TRUE")][..],
        &[("norefcomma", "false")][..],
        &[("norefcomma", "")][..],
        &[("norefcomma", "true"), ("norefcomma", "false")][..],
        &[("norefcomma", "false"), ("norefcomma", "true")][..],
        &[("unused", "x")][..],
    ] {
        same(&xml, &spec(&[("csv", parameters)]));
    }
    finish();
}

#[test]
#[ignore = "requires Windows MSXML6"]
fn limits_agree() {
    let rows = |n: usize| {
        format!(
            "<COMMAND><RESULTS NODE=\"H\"><CIM>{}</CIM></RESULTS></COMMAND>",
            "<INSTANCE CLASSNAME=\"C\"><PROPERTY NAME=\"P\" TYPE=\"string\"><VALUE>v</VALUE></PROPERTY></INSTANCE>"
                .repeat(n)
        )
    };
    let columns = |n: usize| {
        format!(
            "<COMMAND><RESULTS NODE=\"H\"><CIM><INSTANCE CLASSNAME=\"C\">{}</INSTANCE></CIM></RESULTS></COMMAND>",
            "<PROPERTY NAME=\"P\" TYPE=\"string\"><VALUE>v</VALUE></PROPERTY>".repeat(n)
        )
    };
    for xml in [
        rows(2048),
        rows(2049),
        rows(2050),
        columns(128),
        columns(129),
        columns(130),
    ] {
        for name in ["table", "csv"] {
            same(&xml, &spec(&[(name, &[])]));
        }
    }
    finish();
}

#[test]
#[ignore = "requires Windows MSXML6"]
fn chains_agree() {
    let xml = include_str!("fixtures/reference/os-get-rawxml.stdout.bin");
    for chain in [
        vec!["rawxml", "csv"],
        vec!["rawxml", "rawxml", "table"],
        vec!["xml", "rawxml"],
        vec!["xml", "csv"],
        vec!["mof", "rawxml"],
        vec!["mof", "csv"],
        vec!["htable", "csv"],
        vec!["hform", "csv"],
        vec!["hform", "rawxml"],
        vec!["hform", "xml"],
        vec!["htable", "xml"],
        vec!["list", "csv"],
        vec!["table", "table"],
        vec!["csv", "WmicCompatMissing"],
        vec!["WmicCompatMissing", "csv"],
        vec!["rawxml", "WmicCompatMissing"],
    ] {
        let stages: Vec<(&str, &[(&str, &str)])> = chain.iter().map(|n| (*n, &[][..])).collect();
        same(xml, &spec(&stages));
    }
    finish();
}

fn table(entries: &[(&str, &str)]) -> TranslationTable {
    TranslationTable {
        name: wide("T"),
        entries: entries.iter().map(|(a, b)| (wide(a), wide(b))).collect(),
    }
}

#[test]
#[ignore = "requires Windows MSXML6"]
fn translation_agrees() {
    let inputs = [
        "<COMMAND><REQUEST><COMMANDLINE>a,b&lt;tag&gt;</COMMANDLINE></REQUEST><RESULTS NODE=\"HOST\"><CIM><INSTANCE CLASSNAME=\"Sample\"><PROPERTY NAME=\"Value\" TYPE=\"string\"><VALUE>a,b&amp; &lt;tag&gt;</VALUE></PROPERTY></INSTANCE></CIM></RESULTS></COMMAND>",
        "<COMMAND><RESULTS NODE=\"a\"><CIM><INSTANCE CLASSNAME=\"a\"><PROPERTY NAME=\"a\" TYPE=\"string\"><VALUE>a a\r\na</VALUE></PROPERTY><PROPERTY.REFERENCE NAME=\"R\"><VALUE.REFERENCE><INSTANCENAME CLASSNAME=\"K\"><KEYBINDING NAME=\"a\"><KEYVALUE>aaa</KEYVALUE></KEYBINDING></INSTANCENAME></VALUE.REFERENCE></PROPERTY.REFERENCE><PROPERTY.ARRAY NAME=\"A\" TYPE=\"string\"><VALUE.ARRAY><VALUE>a1</VALUE><VALUE>a2</VALUE></VALUE.ARRAY></PROPERTY.ARRAY></INSTANCE></CIM></RESULTS></COMMAND>",
        "<COMMAND><RESULTS NODE=\"a\"><CIM><INSTANCE CLASSNAME=\"a\"><PROPERTY NAME=\"a\" TYPE=\"string\"><VALUE>   </VALUE></PROPERTY><PROPERTY NAME=\"b\" TYPE=\"string\"><VALUE/></PROPERTY></INSTANCE></CIM></RESULTS></COMMAND>",
        "<COMMAND><RESULTS NODE=\"a\"><CIM><INSTANCE CLASSNAME=\"a\"><PROPERTY NAME=\"a\" TYPE=\"string\"><VALUE>a, b</VALUE></PROPERTY></INSTANCE></CIM></RESULTS></COMMAND>",
        "<COMMAND/>",
        "plain",
    ];
    let sets: [Vec<TranslationTable>; 7] = [
        vec![table(&[("&lt;", "<"), ("&gt;", ">")])],
        vec![table(&[(",", " ")])],
        vec![table(&[("a", "")])],
        vec![table(&[("a", "<&>\"'")])],
        vec![table(&[("a", "b")]), table(&[("b", "c")])],
        vec![table(&[(" ", "")])],
        vec![table(&[("a", "\r"), ("b", "\n"), ("c", "\t")])],
    ];
    for xml in inputs {
        for set in &sets {
            for name in ["csv", "rawxml", "list", "table"] {
                let mut s = spec(&[(name, &[])]);
                s.before = set.clone();
                same(xml, &s);
                let mut s = spec(&[(name, &[])]);
                s.after = set.clone();
                same(xml, &s);
            }
        }
    }
    finish();
}

#[test]
#[ignore = "requires Windows MSXML6"]
fn generated_value_text_agrees() {
    const TOKENS: [&str; 24] = [
        "a",
        "B",
        "9",
        " ",
        "  ",
        "\t",
        "\n",
        "\r",
        "\r\n",
        "&amp;",
        "&lt;",
        "&gt;",
        "&#32;",
        "&#10;",
        "&#13;",
        "&#9;",
        "\"",
        "'",
        "\u{5b57}",
        "\u{1f600}",
        "<![CDATA[x ]]>",
        "<![CDATA[ ]]>",
        "<!--c-->",
        "\u{a0}",
    ];
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut next = move |bound: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % bound as u64) as usize
    };
    for _ in 0..250 {
        let picks: Vec<&str> = (0..1 + next(7))
            .map(|_| TOKENS[next(TOKENS.len())])
            .collect();
        let value: String = picks.concat();
        let kind = ["string", "char16", "uint32"][next(3)];
        CONTEXT.with(|c| *c.borrow_mut() = format!("{kind} {:?}", value));
        let xml = instance(&format!(
            "<PROPERTY NAME=\"P\" TYPE=\"{kind}\"><VALUE>{value}</VALUE></PROPERTY><PROPERTY.ARRAY NAME=\"A\" TYPE=\"{kind}\"><VALUE.ARRAY><VALUE>{value}</VALUE><VALUE/></VALUE.ARRAY></PROPERTY.ARRAY>"
        ));
        all_builtins(&xml);
        let plain: String = picks
            .iter()
            .filter(|token| !token.starts_with("<!"))
            .copied()
            .collect();
        let xml = instance(&format!(
            "<PROPERTY NAME=\"P\" TYPE=\"{kind}\"><VALUE>{plain}</VALUE></PROPERTY>"
        ));
        let mut with_tables = spec(&[("csv", &[])]);
        with_tables.before = vec![table(&[("a", "b"), (" ", "_")])];
        same(&xml, &with_tables);
        with_tables.transforms = spec(&[("rawxml", &[])]).transforms;
        same(&xml, &with_tables);
    }
    finish();
}

#[test]
#[ignore = "requires Windows MSXML6"]
fn translation_trims_values_like_the_text_property() {
    for value in [
        "  pad  ",
        "<![CDATA[ x ]]>",
        " a <![CDATA[ b ]]> c ",
        "&#32;a&#32;",
        "\u{a0}a\u{a0}",
        "a\n\tb\n",
        "<![CDATA[ ]]>",
        "x<!-- c --> y ",
    ] {
        let xml = instance(&format!(
            "<PROPERTY NAME=\"P\" TYPE=\"string\"><VALUE>{value}</VALUE></PROPERTY>"
        ));
        for name in ["csv", "rawxml"] {
            let mut s = spec(&[(name, &[])]);
            s.before = vec![table(&[("zzz", "y")])];
            same(&xml, &s);
        }
    }
    finish();
}
