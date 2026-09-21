//! Syntax analysis — the syntax stage of the frontend pipeline.
//!
//! This stage recognizes the document structure of an IBIS file: it folds the
//! raw text into the flat [`ParsedBlock`] list consumed by
//! [`ast_builder`](crate::frontend::ast_builder) to build the abstract syntax
//! tree. All values stay raw strings, and the nesting level of a keyword is read
//! from [`ibis_schema.toml`](crate::schema) — never from a grammar or a
//! hard-coded list.
//!
//! The stage's functional primitives are organized as submodules:
//!
//! - [`line_type`] — classify the type of a raw line (e.g. `|`-prefixed
//!   continuation/comment lines). Line-role recognition over raw text is a
//!   syntax-level concern, so it lives here.
//! - [`block_grouping`] — fold input into a flat `ParsedBlock` list
//!   ([`group_lines_to_blocks`]) and give every block its level
//!   ([`classify_block_level`]).
//!
//! The re-export [`group_lines_to_blocks`] exposes the primitive consumed by
//! the rest of the pipeline.

// =============================================================================
// syntax_analysis — line classification and block grouping
//
// Design constraints:
//   - The level of a keyword is its depth in `ibis_schema.toml` (`find_level`),
//     so adding a section means editing the schema only;
//   - Grouping is total: it never fails, because every non-header line is
//     either a comment/continuation line or content;
//   - Content lines keep their trimmed text as-is; no tokenizing happens here.
// =============================================================================

pub(crate) use block_grouping::group_lines_to_blocks;

/// Line-type classification — classify the role of a raw line.
mod line_type {
    /// Whether a line is a continuation line.
    ///
    /// Takes a raw `line`; returns `true` when the trimmed line starts with `|`.
    pub(crate) fn is_continuation_line(line: &str) -> bool {
        line.trim().starts_with('|')
    }
}

/// Keyword-block grouping — fold input into a flat block list.
mod block_grouping {
    use crate::frontend::ast_builder::ParsedBlock;
    use crate::frontend::lexical_analysis::parser;
    use crate::schema::{find_level, keyword_level, normalize_keyword, TERMINATOR_KEYWORD};

    use super::line_type::is_continuation_line;

    /// Fold IBIS content into flat keyword blocks, line by line.
    ///
    /// A `[Keyword]` header line starts a new block, given its level by
    /// [`classify_block_level`]; the following non-comment, non-blank lines
    /// accumulate as content. Continuation/comment lines (starting with `|`) and
    /// blank lines are skipped. Text sitting after the closing bracket on a
    /// header line already belongs to the block that line opens.
    ///
    /// # Parameters
    ///
    /// * `content` — A string containing the full text of an IBIS file.
    ///
    /// # Returns
    ///
    /// A flat `Vec<ParsedBlock>` in file order, ready for tree construction.
    pub(crate) fn group_lines_to_blocks(content: &str) -> Vec<ParsedBlock> {
        let mut blocks: Vec<ParsedBlock> = Vec::new();
        let mut current_keyword: Option<String> = None;
        let mut current_level: Option<usize> = None;
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
                        level: current_level.take().unwrap_or(keyword_level::SECOND_LEVEL),
                        content: accumulated_content.clone(),
                    });
                    accumulated_content.clear();
                }

                current_level = Some(classify_block_level(&keyword_name));
                current_keyword = Some(keyword_name);

                // Text after the bracket on the same line belongs to this block.
                if let Some(closing_bracket) = trimmed_line.find(']') {
                    let text_after_bracket =
                        parser::parse_content_line(&trimmed_line[closing_bracket + 1..]);
                    if !text_after_bracket.is_empty() {
                        accumulated_content.push(text_after_bracket);
                    }
                }
                continue;
            }

            // Ordinary content line.
            accumulated_content.push(parser::parse_content_line(trimmed_line));
        }

        // Flush the last block.
        if let Some(trailing_keyword) = current_keyword.take() {
            blocks.push(ParsedBlock {
                keyword: trailing_keyword,
                level: current_level.take().unwrap_or(keyword_level::SECOND_LEVEL),
                content: accumulated_content,
            });
        }

        blocks
    }

    /// Nesting level of a keyword, read from the schema.
    ///
    /// Takes a keyword; returns [`keyword_level::TERMINATOR`] for the `[End]`
    /// terminator ([`TERMINATOR_KEYWORD`]), the depth [`find_level`] reports when
    /// the schema tree contains the keyword (top-level sections answer
    /// `keyword_level::ROOT`), and [`keyword_level::SECOND_LEVEL`] otherwise — a
    /// keyword the schema tree does not contain sits directly under the top-level
    /// section it appears in, just like `Component.Manufacturer`.
    fn classify_block_level(keyword: &str) -> usize {
        if normalize_keyword(keyword) == TERMINATOR_KEYWORD {
            return keyword_level::TERMINATOR;
        }
        find_level(keyword).unwrap_or(keyword_level::SECOND_LEVEL)
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::keyword_level;

    use super::*;

    #[test]
    fn test_line_classification() {
        assert!(line_type::is_continuation_line("| continued text"));
        assert!(!line_type::is_continuation_line("[IBIS ver] 2.1"));
    }

    #[test]
    fn test_group_lines_to_blocks_simple() {
        let ibis_content = "\
[IBIS ver] 2.1
[Component] STM32F103
[Manufacturer] STMicro
";
        let blocks = group_lines_to_blocks(ibis_content);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].keyword, "IBIS ver");
        assert_eq!(blocks[0].content, vec!["2.1".to_string()]);
        assert_eq!(blocks[1].keyword, "Component");
        assert_eq!(blocks[1].content, vec!["STM32F103".to_string()]);
        assert_eq!(blocks[2].keyword, "Manufacturer");
        assert_eq!(blocks[2].content, vec!["STMicro".to_string()]);
    }

    #[test]
    fn test_group_lines_to_blocks_reads_the_level_from_the_schema() {
        let ibis_content = "\
[Component] STM32F103
[Manufacturer] STMicro
[End]
";
        let blocks = group_lines_to_blocks(ibis_content);
        assert_eq!(blocks[0].level, keyword_level::ROOT);
        assert_eq!(blocks[1].level, keyword_level::SECOND_LEVEL);
        assert_eq!(blocks[2].level, keyword_level::TERMINATOR);
    }

    #[test]
    fn test_group_lines_to_blocks_counts_deep_levels() {
        let ibis_content = "\
[Define Package Model] PKG
[Model Data] R1
[Resistance Matrix] R1 R2
";
        let blocks = group_lines_to_blocks(ibis_content);
        assert_eq!(blocks[0].level, keyword_level::ROOT);
        assert_eq!(blocks[1].level, keyword_level::SECOND_LEVEL);
        assert_eq!(blocks[2].level, 3);
    }

    #[test]
    fn test_group_lines_to_blocks_treats_a_keyword_absent_from_the_schema_as_a_child() {
        // `[R Series]` appears in virtex5.ibs but has no place in the schema tree;
        // it sits directly under `[Component]`, so it is a secondary section.
        let ibis_content = "\
[Component] CHIP
[R Series] 1 2 3
";
        let blocks = group_lines_to_blocks(ibis_content);
        assert_eq!(blocks[1].keyword, "R Series");
        assert_eq!(blocks[1].level, keyword_level::SECOND_LEVEL);
    }

    #[test]
    fn test_group_lines_to_blocks_keeps_bracketed_content_lines_as_content() {
        let ibis_content = "\
[Notes]
ODT modeled with [Submodel] support
";
        let blocks = group_lines_to_blocks(ibis_content);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].keyword, "Notes");
        assert_eq!(blocks[0].content, vec!["ODT modeled with [Submodel] support".to_string()]);
    }

    #[test]
    fn test_group_lines_to_blocks_skips_comment_lines() {
        let ibis_content = "\
| This is a comment
[IBIS ver] 2.1
| Another comment
[File name] test.ibs
";
        let blocks = group_lines_to_blocks(ibis_content);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].keyword, "IBIS ver");
        assert_eq!(blocks[1].keyword, "File name");
    }
}
