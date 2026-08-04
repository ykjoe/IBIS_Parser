//! 容错回退解析 — 当 pest 完整解析失败时，逐行解析为扁平 [`ParsedBlock`] 列表。
//!
//! 该模块只负责容错恢复出扁平 block，建树与序列化由
//! [`frontend::parse`](crate::frontend::parse) / [`emitter`](crate::emitter) 完成。

use crate::core::Rule;
use crate::frontend::ast_builder::ParsedBlock;

/// 逐行解析 IBIS 内容为扁平 keyword blocks（容错回退）。
///
/// # Parameters
///
/// * `content` — A string containing the full text of an IBIS file.
///
/// # Returns
///
/// 扁平 [`ParsedBlock`] 列表，供 [`frontend::parse`](crate::frontend::parse) 构建抽象语法树。
pub(crate) fn recover_blocks(content: &str) -> Vec<ParsedBlock> {
    let mut blocks: Vec<ParsedBlock> = Vec::new();
    let mut current_keyword: Option<String> = None;
    let mut current_rule: Option<Rule> = None;
    let mut accumulated_content: Vec<String> = Vec::new();

    for raw_line in content.lines() {
        let trimmed_line = raw_line.trim();

        if trimmed_line.is_empty() || trimmed_line.starts_with('|') {
            continue;
        }

        if let Some(closing_bracket) = trimmed_line.find(']') {
            if trimmed_line.starts_with('[') {
                if let Some(previous_keyword) = current_keyword.take() {
                    blocks.push(ParsedBlock {
                        keyword: previous_keyword,
                        rule: current_rule.take().unwrap_or(Rule::keyword),
                        content: accumulated_content.clone(),
                    });
                    accumulated_content.clear();
                }

                let keyword_name = trimmed_line[1..closing_bracket].trim().to_string();
                current_keyword = Some(keyword_name);
                current_rule = Some(Rule::keyword);

                let text_after_bracket = trimmed_line[closing_bracket + 1..].trim().to_string();
                if !text_after_bracket.is_empty() {
                    accumulated_content.push(text_after_bracket);
                }
                continue;
            }
        }

        accumulated_content.push(trimmed_line.to_string());
    }

    if let Some(trailing_keyword) = current_keyword.take() {
        blocks.push(ParsedBlock {
            keyword: trailing_keyword,
            rule: current_rule.take().unwrap_or(Rule::keyword),
            content: accumulated_content,
        });
    }

    blocks
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recover_blocks_simple() {
        let ibis_content = "\
[IBIS ver] 2.1
[Component] STM32F103
[Manufacturer] STMicro
";
        let blocks = recover_blocks(ibis_content);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].keyword, "IBIS ver");
        assert_eq!(blocks[0].content, vec!["2.1".to_string()]);
        assert_eq!(blocks[1].keyword, "Component");
        assert_eq!(blocks[1].content, vec!["STM32F103".to_string()]);
        assert_eq!(blocks[2].keyword, "Manufacturer");
        assert_eq!(blocks[2].content, vec!["STMicro".to_string()]);
    }
}
