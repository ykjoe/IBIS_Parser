//! 语法分析 — 将词法产物（pest pairs）展平为扁平 [`ParsedBlock`] 列表。
//!
//! 职责：识别文档的结构（关键词头 / 内容行），产出供
//! [`ast_builder`](crate::frontend::ast_builder) 构建抽象语法树的扁平 block 序列。

use crate::core::Rule;
use crate::frontend::ast_builder::ParsedBlock;
use crate::frontend::lexical_analysis::extract_keyword_name;

// =============================================================================
// Syntax analysis — pairs → flat blocks
// =============================================================================

/// Walk pest pairs and group into keyword blocks.
pub fn group_pairs_to_blocks(pairs: pest::iterators::Pairs<Rule>) -> Vec<ParsedBlock> {
    let mut blocks: Vec<ParsedBlock> = Vec::new();
    let mut current_keyword: Option<String> = None;
    let mut current_rule: Option<Rule> = None;
    let mut accumulated_content: Vec<String> = Vec::new();

    // The top-level pair is ibis_file; its inner pairs are the matched alternatives.
    for pair in pairs.flatten() {
        let rule = pair.as_rule();

        // Content lines — keep raw text, don't join tokens (preserves original spacing)
        if rule == Rule::content_line {
            let line_content = pair.as_str().trim().to_string();
            if !line_content.is_empty() {
                accumulated_content.push(line_content);
            }
            continue;
        }

        // Keyword headers: grouped rules or generic fallback
        if rule == Rule::first_level_keyword
            || rule == Rule::second_level_keyword
            || rule == Rule::kw_end
            || rule == Rule::keyword
        {
            // Flush previous block
            if let Some(previous_keyword) = current_keyword.take() {
                blocks.push(ParsedBlock {
                    keyword: previous_keyword,
                    rule: current_rule.take().unwrap(),
                    content: accumulated_content.clone(),
                });
                accumulated_content.clear();
            }

            let keyword_name = extract_keyword_name(&pair);
            current_keyword = Some(keyword_name);
            current_rule = Some(rule);
        }
    }

    // Flush the last block
    if let Some(trailing_keyword) = current_keyword.take() {
        blocks.push(ParsedBlock {
            keyword: trailing_keyword,
            rule: current_rule.take().unwrap(),
            content: accumulated_content,
        });
    }

    blocks
}
