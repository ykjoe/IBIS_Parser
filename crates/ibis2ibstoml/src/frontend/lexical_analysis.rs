//! 词法分析 — pest 规则导出与关键词/内容 token 提取。
//!
//! 职责：
//! 1. 通过 [`IbisParser`]（`#[derive(pest_derive::Parser)]`）将 IBIS 文本完整解析为 pest pairs；
//! 2. 提供关键词名提取（[`extract_keyword_name`]）与内容行提取（[`extract_line_content`]）辅助函数。

// =============================================================================
// Parser type — pest 规则导出
// =============================================================================

#[derive(pest_derive::Parser)]
#[grammar = "frontend/ibis.pest"]
pub struct IbisParser;

// =============================================================================
// Keyword name extraction
// =============================================================================

/// Extract the keyword name from a keyword-header pair.
///
/// Works for both specific `kw_*` rules and the generic [`keyword`](Rule::keyword) rule.
/// The pair's string representation is `[KeywordName]`; brackets are stripped.
pub fn extract_keyword_name(pair: &pest::iterators::Pair<Rule>) -> String {
    let pair_text = pair.as_str();
    let start_position = pair_text.find('[').map(|pos| pos + 1).unwrap_or(0);
    let end_position = pair_text.find(']').unwrap_or(pair_text.len());
    pair_text[start_position..end_position].trim().to_string()
}

// =============================================================================
// Content extraction helpers
// =============================================================================

/// Extract all inner tokens from a content_line pair as a single string.
///
/// 测试辅助函数：用于验证 `content_line` 的 token 提取。
/// 生产代码为保留原始空格，直接使用 `pair.as_str().trim()`，不调用本函数。
#[cfg(test)]
fn extract_line_content(pair: &pest::iterators::Pair<Rule>) -> String {
    let mut parts: Vec<String> = Vec::new();
    for inner_pair in pair.clone().into_inner() {
        let token_str = inner_pair.as_str().trim().to_string();
        if !token_str.is_empty() {
            parts.push(token_str);
        }
    }
    parts.join(" ")
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use pest::Parser;
    use super::*;

    #[test]
    fn test_parse_to_toml_content_line() {
        let line = "R_pkg 250.0m 225.0m 275.0m";
        let pairs = IbisParser::parse(Rule::content_line, line).unwrap();
        for pair in pairs {
            let extracted = extract_line_content(&pair);
            assert_eq!(extracted, "R_pkg 250.0m 225.0m 275.0m");
        }
    }

    #[test]
    fn test_parse_to_toml_data_line_parse() {
        let line = "1 RAS0# Buffer1 200.0m 5.0nH 2.0pF";
        let pairs = IbisParser::parse(Rule::content_line, line).unwrap();
        for pair in pairs {
            let extracted = extract_line_content(&pair);
            assert!(!extracted.is_empty());
            assert!(extracted.contains("1"));
        }
    }

    #[test]
    fn test_extract_keyword_name_specific() {
        let pairs = IbisParser::parse(Rule::kw_component, "[Component]").unwrap();
        for pair in pairs {
            assert_eq!(extract_keyword_name(&pair), "Component");
        }
    }

    #[test]
    fn test_extract_keyword_name_generic() {
        let pairs = IbisParser::parse(Rule::keyword, "[CustomSection]").unwrap();
        for pair in pairs {
            assert_eq!(extract_keyword_name(&pair), "CustomSection");
        }
    }

    #[test]
    fn debug_pest_ibis_file() {
        let content = "[IBIS ver] 2.1\n[Component] MyChip\n";
        let pairs = IbisParser::parse(Rule::ibis_file, content).unwrap();
        for pair in pairs.flatten() {
            println!("DEBUG pair: rule={:?} text='{}'", pair.as_rule(), pair.as_str().escape_default());
        }
    }
}
