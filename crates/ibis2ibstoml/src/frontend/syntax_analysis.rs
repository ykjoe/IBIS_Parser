//! Syntax analysis — the syntax stage of the frontend pipeline.
//!
//! This stage recognizes the document structure of an IBIS file: it produces
//! the flat [`ParsedBlock`] list consumed by
//! [`ast_builder`](crate::frontend::ast_builder) to build the abstract syntax
//! tree — both from the primary pest path and from the fault-tolerant fallback.
//!
//! The stage's functional primitives are organized as submodules:
//!
//! - [`line_type`] — classify the type of a raw line (e.g. `|`-prefixed
//!   continuation/comment lines). Line-role recognition over raw text is a
//!   syntax-level concern, so it lives here and is shared by the fallback
//!   path.
//! - [`block_grouping`] — fold input into a flat `ParsedBlock` list: the primary
//!   path folds pest pairs ([`group_pairs_to_blocks`]); the fallback path
//!   parses raw lines line by line ([`recover_blocks`]).
//!
//! The re-exports [`group_pairs_to_blocks`] and [`recover_blocks`] expose the
//! primitives consumed by the rest of the pipeline.

pub(crate) use block_grouping::group_pairs_to_blocks;
pub(crate) use block_grouping::recover_blocks;

/// Line-type classification — classify the role of a raw line.
mod line_type {
    /// Whether a line is a continuation line.
    ///
    /// Takes a raw `line`; returns `true` when the trimmed line starts with `|`.
    pub(crate) fn is_continuation_line(line: &str) -> bool {
        line.trim().starts_with('|')
    }

    /// Extract content after the `|` continuation marker.
    ///
    /// Takes a raw `line`; returns the trimmed text after `|`, or `None` when
    /// the line is not a continuation or the content is empty.
    pub(crate) fn parse_continuation_content(line: &str) -> Option<String> {
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

/// Keyword-block grouping — fold input into a flat block list.
mod block_grouping {
    use crate::frontend::Rule;
    use crate::frontend::ast_builder::ParsedBlock;
    use crate::frontend::lexical_analysis::parser;
    use crate::frontend::lexical_analysis::{extract_keyword_name, extract_line_content};

    use super::line_type::is_continuation_line;

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
            // extraction primitive.
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

    /// Parse IBIS content line by line into flat keyword blocks
    /// (fault-tolerant fallback used when the full pest parse fails).
    ///
    /// A `[Keyword]` header line starts a new block; the following
    /// non-comment, non-blank lines accumulate as content.
    /// Continuation/comment lines (starting with `|`) are skipped.
    ///
    /// # Parameters
    ///
    /// * `content` — A string containing the full text of an IBIS file.
    ///
    /// # Returns
    ///
    /// A flat `Vec<ParsedBlock>` in file order, ready for tree construction.
    pub fn recover_blocks(content: &str) -> Vec<ParsedBlock> {
        let mut blocks: Vec<ParsedBlock> = Vec::new();
        let mut current_keyword: Option<String> = None;
        let mut current_rule: Option<Rule> = None;
        let mut accumulated_content: Vec<String> = Vec::new();

        for raw_line in content.lines() {
            let trimmed_line = raw_line.trim();

            // Skip blank lines and comment/continuation lines (starting with `|`).
            if trimmed_line.is_empty() || is_continuation_line(trimmed_line) {
                continue;
            }

            // A `[Keyword]` header line starts a new block.
            if let Some(keyword_name) = parser::keyword_name(trimmed_line) {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_classification() {
        assert!(line_type::is_continuation_line("| continued text"));
        assert!(!line_type::is_continuation_line("[IBIS ver] 2.1"));
        assert_eq!(
            line_type::parse_continuation_content("| continued text"),
            Some("continued text".into())
        );
        assert_eq!(line_type::parse_continuation_content("[Component]"), None);
        assert_eq!(line_type::parse_continuation_content("|"), None);
    }

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

    #[test]
    fn test_recover_blocks_skips_comment_lines() {
        let ibis_content = "\
| This is a comment
[IBIS ver] 2.1
| Another comment
[File name] test.ibs
";
        let blocks = recover_blocks(ibis_content);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].keyword, "IBIS ver");
        assert_eq!(blocks[1].keyword, "File name");
    }
}
