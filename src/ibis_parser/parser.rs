// =============================================================================
// IBIS Parser — pest-based parser for IBIS files
// =============================================================================

use pest::iterators::Pairs;
use pest::Parser;
use serde::Serialize;

use crate::ibis_parser::ibis_structure::IBIS_FileHeader;

/// Pest-generated parser from ibis.pest
#[derive(pest_derive::Parser)]
#[grammar = "ibis_parser/ibis.pest"]
pub struct IbisParser;

// ---------------------------------------------------------------------------
// File Header parsing
// ---------------------------------------------------------------------------

/// Parse a single header line and extract (keyword, raw_value).
pub fn parse_header_line(line: &str) -> Option<(&'static str, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('|') {
        return None;
    }

    // Try each header rule
    macro_rules! try_parse {
        ($rule:ident, $name:expr) => {
            if let Ok(pairs) = IbisParser::parse(Rule::$rule, trimmed) {
                let value = extract_value_after_keyword(pairs)?;
                return Some(($name, value));
            }
        };
    }

    try_parse!(header_ibis_ver, "ibis_ver");
    try_parse!(header_comment_char, "comment_char");
    try_parse!(header_file_name, "file_name");
    try_parse!(header_file_rev, "file_rev");
    try_parse!(header_date, "date");
    try_parse!(header_source, "source");
    try_parse!(header_notes_line, "notes");
    try_parse!(header_disclaimer_line, "disclaimer");
    try_parse!(header_copyright_line, "copyright");

    None
}

/// Extract the text after the keyword brackets from parsed pairs.
fn extract_value_after_keyword(pairs: Pairs<Rule>) -> Option<String> {
    // The pairs structure for a header field is:
    //   header_* → [kw_* → "[Keyword]", value_rule → "value"]
    // We want to skip the kw_* inner pairs and collect the rest
    for pair in pairs {
        for inner in pair.into_inner() {
            match inner.as_rule() {
                Rule::kw_ibis_ver
                | Rule::kw_comment_char
                | Rule::kw_file_name
                | Rule::kw_file_rev
                | Rule::kw_date
                | Rule::kw_source
                | Rule::kw_notes
                | Rule::kw_disclaimer
                | Rule::kw_copyright => {
                    // Skip the keyword part
                }
                _ => {
                    let val = inner.as_str().trim().to_string();
                    if !val.is_empty() {
                        return Some(val);
                    }
                }
            }
        }
    }
    None
}

/// Check if a line is a continuation of a multi-line header field (starts with |)
pub fn is_continuation_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|')
}

/// Parse the continuation content after the | marker
pub fn parse_continuation_content(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.starts_with('|') {
        let content = trimmed[1..].trim().to_string();
        if content.is_empty() { None } else { Some(content) }
    } else {
        None
    }
}

/// Serialize the IBIS file header to TOML string for verification.
pub fn header_to_toml(header: &IBIS_FileHeader) -> Result<String, toml::ser::Error> {
    #[derive(Serialize)]
    struct HeaderOutput {
        ibis_ver: String,
        comment_char: Option<String>,
        file_name: String,
        file_rev: String,
        date: Option<String>,
        source: Option<String>,
        notes: Option<String>,
        disclaimer: Option<String>,
        copyright: Option<String>,
    }

    let output = HeaderOutput {
        ibis_ver: header.ibis_ver.clone(),
        comment_char: header.comment_char.map(|c| c.to_string()),
        file_name: header.file_name.clone(),
        file_rev: header.file_rev.clone(),
        date: header.date.clone(),
        source: header.source.clone(),
        notes: header.notes.clone(),
        disclaimer: header.disclaimer.clone(),
        copyright: header.copyright.clone(),
    };

    toml::to_string(&output)
}

/// Parse a raw line to identify the section keyword (e.g. "[Component]" -> "Component")
pub fn identify_section_keyword(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if let Ok(pairs) = IbisParser::parse(Rule::keyword_header, trimmed) {
        for pair in pairs {
            for inner in pair.into_inner() {
                if inner.as_rule() == Rule::keyword_body {
                    return Some(inner.as_str().trim().to_string());
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibis_parser::ibis_structure::IBIS_FileHeader;

    #[test]
    fn test_parse_ibis_ver() {
        let (key, val) = parse_header_line("[IBIS ver] 2.1").unwrap();
        assert_eq!(key, "ibis_ver");
        assert_eq!(val, "2.1");
    }

    #[test]
    fn test_parse_file_name() {
        let (key, val) = parse_header_line("[File name] f103c8.ibs").unwrap();
        assert_eq!(key, "file_name");
        assert_eq!(val, "f103c8.ibs");
    }

    #[test]
    fn test_parse_file_rev() {
        let (key, val) = parse_header_line("[File Rev] 1.1").unwrap();
        assert_eq!(key, "file_rev");
        assert_eq!(val, "1.1");
    }

    #[test]
    fn test_parse_date() {
        let (key, val) = parse_header_line("[Date] 12-08-2024").unwrap();
        assert_eq!(key, "date");
        assert_eq!(val, "12-08-2024");
    }

    #[test]
    fn test_parse_source() {
        let (key, val) = parse_header_line("[Source] STMicroelectronics").unwrap();
        assert_eq!(key, "source");
        assert_eq!(val, "STMicroelectronics");
    }

    #[test]
    fn test_identify_section() {
        assert_eq!(identify_section_keyword("[Component]").unwrap(), "Component");
        assert_eq!(identify_section_keyword("[Model]").unwrap(), "Model");
        assert_eq!(identify_section_keyword("[End]").unwrap(), "End");
    }

    #[test]
    fn test_comment_line_not_parsed() {
        assert!(parse_header_line("| This is a comment").is_none());
    }

    #[test]
    fn test_continuation_line() {
        assert!(is_continuation_line("| continued text"));
        assert!(!is_continuation_line("[IBIS ver] 2.1"));
    }

    #[test]
    fn test_header_to_toml() {
        let header = IBIS_FileHeader {
            ibis_ver: "2.1".to_string(),
            comment_char: Some('|'),
            file_name: "test.ibs".to_string(),
            file_rev: "1.0".to_string(),
            date: Some("01-01-2024".to_string()),
            source: Some("Test".to_string()),
            notes: None,
            disclaimer: None,
            copyright: None,
        };
        let toml = header_to_toml(&header).unwrap();
        assert!(toml.contains("ibis_ver = \"2.1\""));
        assert!(toml.contains("file_name = \"test.ibs\""));
    }
}
