mod support;
use support::wide;
use wmic_compat::model::{EntryMode, Target, Verb};
use wmic_compat::parse::parse_command;

#[test]
fn case_spacing_and_unicode_selectors_keep_the_same_query_contract() {
    // Catches token-boundary, quote-removal, UTF-16 loss, or property-splitting regressions.
    for (alias, get) in [("os", "get"), ("OS", "GET"), ("Os", "GeT")] {
        for separator in [" ", "  ", "\t", " \t "] {
            for selector in [
                "Caption='日本語'",
                "Caption='snowman ☃'",
                "Caption='a,b'",
                "Caption='quoted value'",
                "Version like '10.%'",
                "(Primary=TRUE AND Version IS NOT NULL)",
            ] {
                let line = format!(
                    "\"C:\\Program Files\\wmic.exe\"{separator}{alias}{separator}where{separator}\"{selector}\"{separator}{get}{separator}Caption,Version"
                );
                let parsed = parse_command(&wide(&line), EntryMode::Process).unwrap();
                assert_eq!(parsed.target, Some(Target::Alias(wide(alias))), "{line}");
                assert_eq!(parsed.verb, Verb::Get, "{line}");
                assert_eq!(parsed.selector, Some(wide(selector)), "{line}");
                assert_eq!(
                    parsed.properties,
                    vec![wide("Caption"), wide("Version")],
                    "{line}"
                );
            }
        }
    }
}

#[test]
fn every_unpaired_utf16_surrogate_is_preserved_in_a_quoted_selector() {
    // Catches a conversion through lossy UTF-8 or scalar Unicode during parsing.
    for unit in 0xD800..=0xDFFF {
        let mut line = wide("wmic os where \"").0;
        line.push(unit);
        line.extend(wide("\" get Caption").0);
        let parsed =
            parse_command(&wmic_compat::model::WideText(line), EntryMode::Process).unwrap();
        assert_eq!(parsed.selector.unwrap().0, vec![unit]);
    }
}
