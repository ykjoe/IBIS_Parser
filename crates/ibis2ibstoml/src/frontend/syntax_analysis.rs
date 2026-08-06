//! Syntax analysis — the syntax stage of the frontend pipeline.
//!
//! This stage recognizes the document structure of an IBIS file: it flattens
//! the lexical output (pest pairs) into a flat [`ParsedBlock`] list, where each
//! keyword header starts a block and the following content lines accumulate
//! into it. That block list is consumed by
//! [`ast_builder`](crate::frontend::ast_builder) to build the abstract syntax
//! tree.
//!
//! The stage's functional capability is:
//!
//! - [`LineClassParser`] — classify a raw line as an ordinary content line or
//!   a `|`-prefixed continuation/comment line. Structure recognition over raw
//!   text is a syntax-level concern, so it lives here as a trait and is shared
//!   by the fault-tolerant recovery path.

use crate::core::Rule;
use crate::frontend::ast_builder::ParsedBlock;
use crate::frontend::lexical_analysis::{extract_keyword_name, extract_line_content};

/// The syntax stage carrier.
///
/// Implements the syntax functional capability: [`LineClassParser`].
pub(crate) struct SyntaxAnalysis;

/// Line-classification capability of the syntax stage.
///
/// Why it exists: raw IBIS text uses `|` both for comments and for
/// continuation lines, and the structure of a block changes depending on
/// whether a line continues the previous one or starts fresh. Classifying
/// lines this way is structure recognition, i.e. a syntax-level concern, so
/// the rule is defined once here and shared by the recovery path.
pub(crate) trait LineClassParser {
    /// Check whether a line is a continuation line.
    ///
    /// A continuation line starts with the `|` marker after leading whitespace
    /// is trimmed.
    ///
    /// # Parameters
    ///
    /// * `line` — A single raw line from an IBIS file.
    ///
    /// # Returns
    ///
    /// * `true` — When the trimmed line starts with `|`.
    /// * `false` — Otherwise.
    fn is_continuation_line(&self, line: &str) -> bool;

    /// Extract the content after the `|` continuation marker, if any.
    ///
    /// # Parameters
    ///
    /// * `line` — A single raw line from an IBIS file.
    ///
    /// # Returns
    ///
    /// * `Some(String)` — The trimmed content after the `|` marker when the
    ///   line is a continuation line and the content is non-empty.
    /// * `None` — When the line is not a continuation line, or the text after
    ///   the marker is empty.
    ///
    /// Retained as part of the line-classification capability: the recovery
    /// path currently only skips continuation lines, but multi-line header
    /// fields and `[Comment Char]` handling will need the extracted content.
    #[allow(dead_code)]
    fn parse_continuation_content(&self, line: &str) -> Option<String>;
}

impl LineClassParser for SyntaxAnalysis {
    fn is_continuation_line(&self, line: &str) -> bool {
        line.trim().starts_with('|')
    }

    fn parse_continuation_content(&self, line: &str) -> Option<String> {
        let trimmed = line.trim();
        if trimmed.starts_with('|') {
            let content = trimmed[1..].trim().to_string();
            if content.is_empty() {
                None
            } else {
                Some(content)
            }
        } else {
            None
        }
    }
}

/// Walk pest pairs and group them into keyword blocks.
///
/// Consumes the top-level `ibis_file` pair and folds its inner pairs into a
/// flat list of [`ParsedBlock`]s: each keyword header starts a new block and
/// subsequent `content_line` pairs accumulate into that block.
///
/// # Parameters
///
/// * `pairs` — The pest pairs produced by parsing an IBIS file.
///
/// # Returns
///
/// A flat `Vec<ParsedBlock>` in file order, ready for tree construction.
pub fn group_pairs_to_blocks(pairs: pest::iterators::Pairs<Rule>) -> Vec<ParsedBlock> {
    let mut blocks: Vec<ParsedBlock> = Vec::new();
    let mut current_keyword: Option<String> = None;
    let mut current_rule: Option<Rule> = None;
    let mut accumulated_content: Vec<String> = Vec::new();

    // The top-level pair is `ibis_file`; its inner pairs are the matched alternatives.
    for pair in pairs.flatten() {
        let rule = pair.as_rule();

        // Content lines — collect the normalized text via the lexical
        // ContentParser capability.
        if rule == Rule::content_line {
            let line_content = extract_line_content(&pair);
            if !line_content.is_empty() {
                accumulated_content.push(line_content);
            }
            continue;
        }

        // Keyword headers: grouped rules or generic fallback.
        if rule == Rule::first_level_keyword
            || rule == Rule::second_level_keyword
            || rule == Rule::kw_end
            || rule == Rule::keyword
        {
            // Flush the previous block before starting a new one.
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

    // Flush the last block.
    if let Some(trailing_keyword) = current_keyword.take() {
        blocks.push(ParsedBlock {
            keyword: trailing_keyword,
            rule: current_rule.take().unwrap(),
            content: accumulated_content,
        });
    }

    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_class_parser_capability() {
        let syntax = SyntaxAnalysis;
        assert!(syntax.is_continuation_line("| continued text"));
        assert!(!syntax.is_continuation_line("[IBIS ver] 2.1"));
        assert_eq!(
            syntax.parse_continuation_content("| continued text"),
            Some("continued text".into())
        );
        assert_eq!(syntax.parse_continuation_content("[Component]"), None);
        assert_eq!(syntax.parse_continuation_content("|"), None);
    }
}
