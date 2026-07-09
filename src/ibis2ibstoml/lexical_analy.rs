//! Lexical analysis module — Tokenize raw IBIS text into structured [`Token`]s.
//!
//! Uses PEST's `ibis_file` top-level rule to parse the entire file into a
//! structured pair tree. The grammar handles three layers:
//!
//! 1. Basic tokenisation (`ident`, `si_number`)
//! 2. Line-level structure (`kw_header`, `generic_kv_line`, `table_record_line`)
//! 3. File topology (grouping into an ordered sequence)
//!
//! This module extracts keyword blocks and groups content lines under them,
//! producing a [`Vec<Token>`] for downstream syntax analysis.
//!
//! # Related modules
//!
//! - [`super::syntax_analy`] — consumes the token list to build the section tree
//! - [`super::core`] — top-level orchestration, entry point

use pest::iterators::Pairs;
use pest::Parser;

// =============================================================================
// Parser type
// =============================================================================

/// Pest-generated parser from [`ibis.pest`](ibis.pest).
///
/// The grammar performs full-structure recognition at the file level via the
/// [`ibis_file`](Rule::ibis_file) rule.  No specific keyword names are defined
/// in the grammar; all keyword classification happens in [`syntax_analy`](super::syntax_analy).

#[derive(pest_derive::Parser)]
#[grammar = "ibis2ibstoml/ibis.pest"]
pub struct IbisParser;

// =============================================================================
// Content line — structured output from PEST
// =============================================================================

/// A single content line, already structured by PEST into key-value or table form.
///
/// This eliminates all manual string splitting in the Rust consumer code.
#[derive(Debug, Clone, PartialEq)]
pub enum ContentLine {
    /// A key-value line parsed by [`generic_kv_line`](Rule::generic_kv_line).
    ///
    /// Examples:
    /// - `R_pkg 250.0m 225.0m 275.0m` → key="R_pkg", values=["250.0m","225.0m","275.0m"]
    /// - `Vinh = 2.0V` → key="Vinh", values=["2.0V"]
    KeyValue {
        key: String,
        values: Vec<String>,
    },
    /// A table data line parsed by [`table_record_line`](Rule::table_record_line).
    ///
    /// Examples:
    /// - `1    RAS0#    Buffer1` → ["1", "RAS0#", "Buffer1"]
    /// - `-3.3000    -2.0000mA` → ["-3.3000", "-2.0000mA"]
    TableRecord {
        columns: Vec<String>,
    },
}

// =============================================================================
// Token — the output of lexical analysis
// =============================================================================

/// A lexical token representing one IBIS keyword block.
///
/// Each [`Token`] corresponds to a `[Keyword]` header plus all content lines
/// that follow it (until the next keyword or end-of-file).
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub keyword: String,
    pub content: Vec<ContentLine>,
}

// =============================================================================
// Internal helpers — PEST pair extraction
// =============================================================================

/// Extract the keyword name from a `keyword` pair.
///
/// `keyword` is atomic (`@{ ... }`), so the entire match is `[KeywordName]`.
/// We strip the brackets to get the name.
fn extract_keyword_from_pair(pair: pest::iterators::Pair<Rule>) -> String {
    let pair_text = pair.as_str();
    let start_position = pair_text.find('[').map(|pos| pos + 1).unwrap_or(0);
    let end_position = pair_text.find(']').unwrap_or(pair_text.len());
    pair_text[start_position..end_position].trim().to_string()
}

/// Extract structured content from a `generic_kv_line` pair.
fn extract_kv_from_pair(pair: pest::iterators::Pair<Rule>) -> ContentLine {
    let mut extracted_key: Option<String> = None;
    let mut extracted_values: Vec<String> = Vec::new();

    for inner_pair in pair.into_inner() {
        if inner_pair.as_rule() == Rule::ident && extracted_key.is_none() {
            // The first `ident` child is the key (tagged as `key` in the grammar)
            extracted_key = Some(inner_pair.as_str().to_string());
        } else {
            let token_str = inner_pair.as_str().trim().to_string();
            if !token_str.is_empty() {
                extracted_values.push(token_str);
            }
        }
    }

    let key = extracted_key.unwrap_or_default();
    ContentLine::KeyValue {
        key,
        values: extracted_values,
    }
}

/// Extract structured content from a `table_record_line` pair.
fn extract_table_from_pair(pair: pest::iterators::Pair<Rule>) -> ContentLine {
    let mut extracted_columns: Vec<String> = Vec::new();

    for inner_pair in pair.into_inner() {
        let column_value = inner_pair.as_str().trim().to_string();
        if !column_value.is_empty() {
            extracted_columns.push(column_value);
        }
    }

    ContentLine::TableRecord {
        columns: extracted_columns,
    }
}

// =============================================================================
// Public API
// =============================================================================

/// Tokenise raw IBIS content into a list of [`Token`]s.
///
/// The tokenization pipeline uses PEST's `ibis_file` rule for full-structure
/// recognition:
/// 1. Parse the entire file into PEST pairs via [`IbisParser::parse`].
/// 2. Walk pairs sequentially, grouping content lines under `kw_header` pairs.
/// 3. For each content pair (`generic_kv_line` / `table_record_line`), extract
///    structured data (key-value or table columns).
///
/// If the full PEST parse fails, the function falls back to a line-by-line
/// tokenizer ([`fallback_tokenize`]) to handle non-standard or malformed input.
///
/// # Parameters
///
/// * `content` — A string containing the full text of an IBIS file.
///
/// # Returns
///
/// A [`Vec<Token>`] where each token represents one keyword block, in file order,
/// with content already structured into [`ContentLine::KeyValue`] or
/// [`ContentLine::TableRecord`] form.
///
/// # Errors
///
/// Does not return `Err`. If PEST parsing fails, the function silently falls
/// back to the line-based tokenizer and returns a best-effort result.
///
/// # Panics
///
/// Does not panic under normal operation.
///
/// # Examples
///
/// ```rust
/// use ibis_parser::ibis2ibstoml::lexical_analy::tokenize;
///
/// let tokens = tokenize("[IBIS ver] 2.1\n[Component] MyChip\n");
/// assert_eq!(tokens.len(), 2);
/// assert_eq!(tokens[0].keyword, "IBIS ver");
/// ```
pub fn tokenize(content: &str) -> Vec<Token> {
    // ── Phase 1: full PEST parse ──
    let parsed_pairs = match IbisParser::parse(Rule::ibis_file, content) {
        Ok(pairs) => pairs,
        Err(_parse_error) => {
            // If the full file parse fails, fall back to the line-based approach
            return fallback_tokenize(content);
        }
    };

    // ── Phase 2: walk pairs and group into tokens ──
    let mut tokens: Vec<Token> = Vec::new();
    let mut current_keyword: Option<String> = None;
    let mut accumulated_content: Vec<ContentLine> = Vec::new();

    for pair in parsed_pairs {
        match pair.as_rule() {
            Rule::keyword => {
                // Flush previous token
                if let Some(previous_keyword) = current_keyword.take() {
                    tokens.push(Token {
                        keyword: previous_keyword,
                        content: accumulated_content.clone(),
                    });
                    accumulated_content.clear();
                }

                let keyword_name = extract_keyword_from_pair(pair);
                current_keyword = Some(keyword_name);
            }

            Rule::kv_line => {
                let structured_line = extract_kv_from_pair(pair);
                accumulated_content.push(structured_line);
            }

            Rule::data_line => {
                let structured_line = extract_table_from_pair(pair);
                accumulated_content.push(structured_line);
            }

            // line_comment is silent (_{}), so it never appears here
            _ => {
                // Unknown rules are silently skipped
            }
        }
    }

    // Flush the last token
    if let Some(trailing_keyword) = current_keyword.take() {
        tokens.push(Token {
            keyword: trailing_keyword,
            content: accumulated_content,
        });
    }

    tokens
}

/// Fallback tokenizer for when the full PEST parse fails.
///
/// Uses a simpler line-by-line approach.  This ensures robustness when
/// encountering non-standard or malformed IBIS files.
fn fallback_tokenize(content: &str) -> Vec<Token> {
    let mut tokens: Vec<Token> = Vec::new();
    let mut current_keyword: Option<String> = None;
    let mut accumulated_content: Vec<ContentLine> = Vec::new();

    for raw_line in content.lines() {
        let trimmed_line = raw_line.trim();

        // Skip empty lines and comment lines
        if trimmed_line.is_empty() || trimmed_line.starts_with('|') {
            continue;
        }

        // Check for keyword header using simple bracket detection
        if let Some(closing_bracket) = trimmed_line.find(']') {
            if trimmed_line.starts_with('[') {
                // Flush previous token
                if let Some(previous_keyword) = current_keyword.take() {
                    tokens.push(Token {
                        keyword: previous_keyword,
                        content: accumulated_content.clone(),
                    });
                    accumulated_content.clear();
                }

                let keyword_name = trimmed_line[1..closing_bracket].trim().to_string();
                current_keyword = Some(keyword_name);

                // Capture text after bracket as content
                let text_after_bracket = trimmed_line[closing_bracket + 1..].trim().to_string();
                if !text_after_bracket.is_empty() {
                    accumulated_content.push(ContentLine::TableRecord {
                        columns: vec![text_after_bracket],
                    });
                }
                continue;
            }
        }

        // Not a keyword line → add as table record
        accumulated_content.push(ContentLine::TableRecord {
            columns: vec![trimmed_line.to_string()],
        });
    }

    // Flush the last token
    if let Some(trailing_keyword) = current_keyword.take() {
        tokens.push(Token {
            keyword: trailing_keyword,
            content: accumulated_content,
        });
    }

    tokens
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use pest::Parser;

    #[test]
    fn test_tokenize_simple() {
        let ibis_content = "\
[IBIS ver] 2.1
[Component] STM32F103
[Manufacturer] STMicro
[Package]
R_pkg 0.1
L_pkg 1nH
";
        let tokens = tokenize(ibis_content);
        assert_eq!(tokens.len(), 4);

        assert_eq!(tokens[0].keyword, "IBIS ver");
        assert_eq!(tokens[0].content.len(), 1);
        if let ContentLine::TableRecord { ref columns } = tokens[0].content[0] {
            assert!(columns[0].contains("2.1"));
        } else {
            panic!("Expected TableRecord for IBIS ver content");
        }

        assert_eq!(tokens[1].keyword, "Component");
        assert_eq!(tokens[1].content.len(), 1);

        assert_eq!(tokens[2].keyword, "Manufacturer");

        assert_eq!(tokens[3].keyword, "Package");
        assert_eq!(tokens[3].content.len(), 2);
    }

    #[test]
    fn test_tokenize_handles_comments() {
        let ibis_content = "\
| This is a comment
[IBIS ver] 2.1
| Another comment
[File name] test.ibs
";
        let tokens = tokenize(ibis_content);
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].keyword, "IBIS ver");
        assert_eq!(tokens[1].keyword, "File name");
    }

    #[test]
    fn test_tokenize_multiple_models() {
        let ibis_content = "[Model] ModelA\n[Model] ModelB\n";
        let tokens = tokenize(ibis_content);
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].content.len(), 1);
        assert_eq!(tokens[1].content.len(), 1);
    }

    #[test]
    fn test_kv_line_parse() {
        // Use PEST directly to test a kv line
        let line = "R_pkg 250.0m 225.0m 275.0m";
        let pairs = IbisParser::parse(Rule::kv_line, line).unwrap();
        for pair in pairs {
            let extracted = extract_kv_from_pair(pair);
            if let ContentLine::KeyValue { ref key, ref values } = extracted {
                assert_eq!(key, "R_pkg");
                assert_eq!(values.len(), 3);
                assert_eq!(values[0], "250.0m");
            } else {
                panic!("Expected KeyValue");
            }
        }
    }

    #[test]
    fn test_table_record_parse() {
        let line = "1 RAS0# Buffer1 200.0m 5.0nH 2.0pF";
        let pairs = IbisParser::parse(Rule::data_line, line).unwrap();
        for pair in pairs {
            let extracted = extract_table_from_pair(pair);
            if let ContentLine::TableRecord { ref columns } = extracted {
                assert!(columns.len() >= 3);
                assert_eq!(columns[0], "1");
            } else {
                panic!("Expected TableRecord");
            }
        }
    }
}
