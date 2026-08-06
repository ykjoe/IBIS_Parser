//! Lexical analysis — the lexical stage of the frontend pipeline.
//!
//! This stage is responsible for the two lowest-level reading operations on
//! IBIS text:
//!
//! - Parse IBIS text into pest pairs via [`IbisParser`]
//!   (`#[derive(pest_derive::Parser)]`), which binds the pest grammar
//!   [`ibis.pest`](frontend/ibis.pest) to Rust.
//! - Expose its reading capabilities as functional traits
//!   ([`KeywordNameParser`], [`ContentParser`]) so that every other stage can
//!   read the same primitives without re-implementing them.
//!
//! The stage's functional capabilities are:
//!
//! - [`KeywordNameParser`] — read a section keyword out of a `[Keyword]` header
//!   token. Downstream stages (syntax grouping, recovery) must know which
//!   keyword a block belongs to, so this primitive lives at the lexical level.
//! - [`ContentParser`] — read the normalized text of a content line. Downstream
//!   stages accumulate content lines into blocks, so the trimmed form is fixed
//!   here at the lexical level.
//!
//! The stage carrier [`LexicalAnalysis`] implements these capabilities. The
//! interface functions [`extract_keyword_name`] and [`extract_line_content`]
//! organize the capabilities for pest-pair consumers; the recovery path
//! consumes the traits directly on raw lines.

pub use grammar::{IbisParser, Rule};
pub use extraction::{extract_keyword_name, extract_line_content};

/// Pest grammar binding — exposes the generated [`Rule`] enum and the parser
/// entry point.
mod grammar {
    #[derive(pest_derive::Parser)]
    #[grammar = "frontend/ibis.pest"]
    pub struct IbisParser;
}

/// The lexical stage carrier.
///
/// Implements the lexical functional capabilities: [`KeywordNameParser`] and
/// [`ContentParser`].
pub(crate) struct LexicalAnalysis;

/// Keyword-name reading capability of the lexical stage.
///
/// Why it exists: every keyword block in an IBIS file is introduced by a
/// `[Keyword]` header token. To know what a block is about, downstream stages
/// need the keyword name that lives inside the brackets. Providing it as a
/// trait at the lexical level lets the pest path and the recovery path share
/// one implementation.
pub(crate) trait KeywordNameParser {
    /// Read the keyword name out of a `[Keyword]` header token.
    ///
    /// Returns the trimmed content inside the brackets with its original case
    /// preserved (e.g. `"Component"`). Returns `None` when the token has no
    /// closing `]` or the bracket content is empty.
    fn keyword_name(&self, token: &str) -> Option<String>;
}

/// Content-line reading capability of the lexical stage.
///
/// Why it exists: the data of a keyword block is a sequence of content lines.
/// Downstream stages collect these lines verbatim, so the exact trimmed form
/// is fixed once here at the lexical level instead of being re-decided by
/// every consumer.
pub(crate) trait ContentParser {
    /// Read the normalized text of a content line.
    ///
    /// Returns the line with leading and trailing whitespace removed.
    fn parse_content_line(&self, line: &str) -> String;
}

impl KeywordNameParser for LexicalAnalysis {
    fn keyword_name(&self, token: &str) -> Option<String> {
        let trimmed = token.trim();
        let closing = trimmed.find(']')?;
        let inside = &trimmed[..=closing];
        let content = &inside[1..inside.len() - 1];
        if content.is_empty() {
            None
        } else {
            Some(content.trim().to_string())
        }
    }
}

impl ContentParser for LexicalAnalysis {
    fn parse_content_line(&self, line: &str) -> String {
        line.trim().to_string()
    }
}

/// Token extraction — organizes the lexical capabilities for pest pairs.
///
/// These interface functions adapt the lexical traits to the pest-pair world:
/// a pair carries both a rule and its matched text, and the functions forward
/// the matched text to the corresponding capability.
mod extraction {
    use super::grammar::Rule;
    use super::{ContentParser, KeywordNameParser, LexicalAnalysis};

    /// Interface function: read the keyword name from a keyword-header pair.
    ///
    /// Works for both specific `kw_*` rules and the generic
    /// [`keyword`](Rule::keyword) rule. The pair's matched text is
    /// `[KeywordName]`; the brackets are stripped through [`KeywordNameParser`].
    ///
    /// # Returns
    ///
    /// The keyword name inside the brackets (e.g. `"Component"`), or an empty
    /// string when no keyword name can be read.
    pub fn extract_keyword_name(pair: &pest::iterators::Pair<Rule>) -> String {
        LexicalAnalysis.keyword_name(pair.as_str()).unwrap_or_default()
    }

    /// Interface function: read the normalized text from a `content_line` pair.
    ///
    /// Forwards the pair's matched text through [`ContentParser`] so syntax
    /// grouping collects content lines through a single entry point.
    ///
    /// # Returns
    ///
    /// The content line with leading and trailing whitespace removed.
    pub fn extract_line_content(pair: &pest::iterators::Pair<Rule>) -> String {
        LexicalAnalysis.parse_content_line(pair.as_str())
    }
}

#[cfg(test)]
mod tests {
    use pest::Parser;

    use super::*;

    #[test]
    fn test_parse_to_toml_content_line() {
        let line = "R_pkg 250.0m 225.0m 275.0m";
        let pairs = IbisParser::parse(Rule::content_line, line).unwrap();
        for pair in pairs {
            let extracted = extraction::extract_line_content(&pair);
            assert_eq!(extracted, "R_pkg 250.0m 225.0m 275.0m");
        }
    }

    #[test]
    fn test_parse_to_toml_data_line_parse() {
        let line = "1 RAS0# Buffer1 200.0m 5.0nH 2.0pF";
        let pairs = IbisParser::parse(Rule::content_line, line).unwrap();
        for pair in pairs {
            let extracted = extraction::extract_line_content(&pair);
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
    fn test_keyword_name_parser_capability() {
        let lexical = LexicalAnalysis;
        assert_eq!(lexical.keyword_name("[Component]"), Some("Component".into()));
        assert_eq!(lexical.keyword_name("[Model]"), Some("Model".into()));
        assert_eq!(lexical.keyword_name("[End]"), Some("End".into()));
        assert_eq!(lexical.keyword_name("plain content"), None);
        assert_eq!(lexical.keyword_name("[]"), None);
    }

    #[test]
    fn test_content_parser_capability() {
        let lexical = LexicalAnalysis;
        assert_eq!(lexical.parse_content_line("  R_pkg 0.1  "), "R_pkg 0.1");
    }

    #[test]
    fn debug_pest_ibis_file() {
        let content = "[IBIS ver] 2.1\n[Component] MyChip\n";
        let pairs = IbisParser::parse(Rule::ibis_file, content).unwrap();
        for pair in pairs.flatten() {
            println!(
                "DEBUG pair: rule={:?} text='{}'",
                pair.as_rule(),
                pair.as_str().escape_default()
            );
        }
    }
}
