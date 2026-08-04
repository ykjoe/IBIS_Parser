//! 兼容 API — 为既有集成测试保留的逐行解析辅助函数。
//!
//! 这些函数由旧实现直接搬移而来，保持签名不变，
//! 以便 `tests/header_parse_test.rs` 等既有测试继续使用。
//! 该模块**不属于**前端 Pipeline 公共接口，仅为遗留兼容工具。

/// Parse a file header line into `(toml_key, value)`.
///
/// Recognises the standard IBIS file header keywords and maps them to
/// lowercased TOML keys. Returns `None` for comments, blank lines, and
/// non-header keywords.
pub fn parse_header_line(line: &str) -> Option<(&'static str, String)> {
    let trimmed_line = line.trim();

    if trimmed_line.is_empty() || trimmed_line.starts_with('|') {
        return None;
    }

    let closing_bracket_position = trimmed_line.find(']')?;
    let keyword_part = &trimmed_line[1..closing_bracket_position];
    let value_part = trimmed_line[closing_bracket_position + 1..].trim().to_string();

    let toml_key = match keyword_part {
        "IBIS ver" => "ibis_ver",
        "Comment Char" => "comment_char",
        "File name" => "file_name",
        "File Rev" => "file_rev",
        "Date" => "date",
        "Source" => "source",
        "Notes" => "notes",
        "Disclaimer" => "disclaimer",
        "Copyright" => "copyright",
        _ => return None,
    };

    Some((toml_key, value_part))
}

/// Check whether a line is a continuation line (starts with `|`).
pub fn is_continuation_line(line: &str) -> bool {
    line.trim().starts_with('|')
}

/// Extract content after the `|` continuation marker, if any.
pub fn parse_continuation_content(line: &str) -> Option<String> {
    let trimmed_line = line.trim();
    if trimmed_line.starts_with('|') {
        let content_after_marker = trimmed_line[1..].trim().to_string();
        if content_after_marker.is_empty() {
            None
        } else {
            Some(content_after_marker)
        }
    } else {
        None
    }
}

/// Identify the section keyword from a `[Keyword]` header line.
pub fn identify_section_keyword(line: &str) -> Option<String> {
    let trimmed_line = line.trim();
    let closing_bracket_position = trimmed_line.find(']')?;
    let keyword_part = &trimmed_line[..=closing_bracket_position];
    let content = &keyword_part[1..keyword_part.len() - 1];
    if content.is_empty() {
        None
    } else {
        Some(content.trim().to_string())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_header_line_ibis_ver() {
        let (key, value) = parse_header_line("[IBIS ver] 2.1").unwrap();
        assert_eq!(key, "ibis_ver");
        assert_eq!(value, "2.1");
    }

    #[test]
    fn test_parse_header_line_file_name() {
        let (key, value) = parse_header_line("[File name] f103c8.ibs").unwrap();
        assert_eq!(key, "file_name");
        assert_eq!(value, "f103c8.ibs");
    }

    #[test]
    fn test_parse_header_line_date() {
        let (key, value) = parse_header_line("[Date] 12-08-2024").unwrap();
        assert_eq!(key, "date");
        assert_eq!(value, "12-08-2024");
    }

    #[test]
    fn test_parse_header_line_comment() {
        assert!(parse_header_line("| This is a comment").is_none());
    }

    #[test]
    fn test_identify_section_keyword() {
        assert_eq!(
            identify_section_keyword("[Component]").unwrap(),
            "Component"
        );
        assert_eq!(identify_section_keyword("[Model]").unwrap(), "Model");
        assert_eq!(identify_section_keyword("[End]").unwrap(), "End");
    }

    #[test]
    fn test_continuation_line() {
        assert!(is_continuation_line("| continued text"));
        assert!(!is_continuation_line("[IBIS ver] 2.1"));
    }
}
