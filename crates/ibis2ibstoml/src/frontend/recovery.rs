//! Fault-tolerant recovery parsing — when the full pest parse fails, parse
//! line by line into a flat [`ParsedBlock`] list.
//!
//! This stage does not invent its own parsing logic; it is the fallback
//! consumer of the functional capabilities owned by the other stages:
//!
//! - [`KeywordNameParser`] (lexical) — recognize `[Keyword]` header lines.
//! - [`LineClassParser`] (syntax) — skip `|` continuation/comment lines.
//!
//! The caller ([`frontend::parse`](crate::frontend::parse)) chooses when to
//! take this path: the pest grammar is tried first, and recovery only runs
//! when that full parse fails. It produces the same flat block list that the
//! AST builder expects, so the rest of the pipeline is unchanged.

use crate::core::Rule;
use crate::frontend::ast_builder::ParsedBlock;
use crate::frontend::lexical_analysis::KeywordNameParser;
use crate::frontend::syntax_analysis::LineClassParser;

/// Parse IBIS content line by line into flat keyword blocks (fault-tolerant
/// fallback).
///
/// Used when the full pest parse fails. A `[Keyword]` header line (detected
/// via [`KeywordNameParser`]) starts a new block; the following non-comment,
/// non-blank lines accumulate as content. Continuation/comment lines (starting
/// with `|`) are skipped via [`LineClassParser`].
///
/// # Parameters
///
/// * `content` — A string containing the full text of an IBIS file.
/// * `keyword_parser` — The lexical keyword-name capability.
/// * `line_class` — The syntax line-classification capability.
///
/// # Returns
///
/// A flat [`ParsedBlock`] list, consumed by
/// [`frontend::parse`](crate::frontend::parse) to build the abstract syntax tree.
pub(crate) fn recover_blocks(
    content: &str,
    keyword_parser: &impl KeywordNameParser,
    line_class: &impl LineClassParser,
) -> Vec<ParsedBlock> {
    let mut blocks: Vec<ParsedBlock> = Vec::new();
    let mut current_keyword: Option<String> = None;
    let mut current_rule: Option<Rule> = None;
    let mut accumulated_content: Vec<String> = Vec::new();

    for raw_line in content.lines() {
        let trimmed_line = raw_line.trim();

        // Skip blank lines and comment/continuation lines (starting with `|`).
        if trimmed_line.is_empty() || line_class.is_continuation_line(trimmed_line) {
            continue;
        }

        // A `[Keyword]` header line starts a new block.
        if let Some(keyword_name) = keyword_parser.keyword_name(trimmed_line) {
            // Flush the previous block before starting a new one.
            if let Some(previous_keyword) = current_keyword.take() {
                blocks.push(ParsedBlock {
                    keyword: previous_keyword,
                    rule: current_rule.take().unwrap_or(Rule::keyword),
                    content: accumulated_content.clone(),
                });
                accumulated_content.clear();
            }

            current_keyword = Some(keyword_name);
            current_rule = Some(Rule::keyword);

            // Text after the bracket on the same line belongs to this block.
            if let Some(closing_bracket) = trimmed_line.find(']') {
                let text_after_bracket = trimmed_line[closing_bracket + 1..].trim().to_string();
                if !text_after_bracket.is_empty() {
                    accumulated_content.push(text_after_bracket);
                }
            }
            continue;
        }

        // Ordinary content line.
        accumulated_content.push(trimmed_line.to_string());
    }

    // Flush the last block.
    if let Some(trailing_keyword) = current_keyword.take() {
        blocks.push(ParsedBlock {
            keyword: trailing_keyword,
            rule: current_rule.take().unwrap_or(Rule::keyword),
            content: accumulated_content,
        });
    }

    blocks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::lexical_analysis::LexicalAnalysis;
    use crate::frontend::syntax_analysis::SyntaxAnalysis;

    #[test]
    fn test_recover_blocks_simple() {
        let ibis_content = "\
[IBIS ver] 2.1
[Component] STM32F103
[Manufacturer] STMicro
";
        let blocks = recover_blocks(ibis_content, &LexicalAnalysis, &SyntaxAnalysis);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].keyword, "IBIS ver");
        assert_eq!(blocks[0].content, vec!["2.1".to_string()]);
        assert_eq!(blocks[1].keyword, "Component");
        assert_eq!(blocks[1].content, vec!["STM32F103".to_string()]);
        assert_eq!(blocks[2].keyword, "Manufacturer");
        assert_eq!(blocks[2].content, vec!["STMicro".to_string()]);
    }

    #[test]
    fn test_recover_blocks_skips_comment_lines() {
        let ibis_content = "\
| This is a comment
[IBIS ver] 2.1
| Another comment
[File name] test.ibs
";
        let blocks = recover_blocks(ibis_content, &LexicalAnalysis, &SyntaxAnalysis);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].keyword, "IBIS ver");
        assert_eq!(blocks[1].keyword, "File name");
    }
}
