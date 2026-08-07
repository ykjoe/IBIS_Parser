//! Lexical analysis — the lexical stage of the frontend pipeline.
//!
//! This stage is responsible for the two lowest-level reading operations on
//! IBIS text:
//!
//! - Parse IBIS text into pest pairs via [`IbisParser`]
//!   (`#[derive(pest_derive::Parser)]`), which binds the pest grammar
//!   [`ibis.pest`](frontend/ibis.pest) to Rust.
//! - Expose its reading primitives as plain functions in the [`parser`] module
//!   so that every other stage can read the same primitives without
//!   re-implementing them.
//!
//! The stage's reading primitives are:
//!
//! - [`parser::keyword_name`] — read a section keyword out of a `[Keyword]`
//!   header token. Downstream stages (syntax grouping, recovery) must know
//!   which keyword a block belongs to, so this primitive lives at the lexical
//!   level.
//! - [`parser::parse_content_line`] — read the normalized text of a content
//!   line. Downstream stages accumulate content lines into blocks, so the
//!   trimmed form is fixed here at the lexical level.
//!
//! The [`extraction`] module adapts these primitives for pest-pair consumers;
//! the recovery path calls the [`parser`] functions directly on raw lines.


pub use grammar::{IbisParser, Rule};
pub use extraction::{extract_keyword_name, extract_line_content};



/// Pest grammar binding — exposes the generated [`Rule`] enum and the parser
/// entry point.
mod grammar {
    #[derive(pest_derive::Parser)]
    #[grammar = "frontend/ibis.pest"]
    pub struct IbisParser;
}


/// Reading primitives of the lexical stage — plain functions shared by the
/// pest path and the recovery path.
pub(crate) mod parser {
    /// Read the keyword name out of a `[Keyword]` header token.
    pub(crate) fn keyword_name(token: &str) -> Option<String> {
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

    /// Read the normalized text of a content line.
    pub(crate) fn parse_content_line(line: &str) -> String {
        line.trim().to_string()
    }
}


/// Token extraction — adapts the lexical primitives to pest pairs.
///
/// These interface functions forward the matched text of a pair to the
/// corresponding [`parser`] primitive.
mod extraction {
    use super::grammar::Rule;
    use super::parser;

    /// Interface function: extract the keyword name from a keyword-header pair.
    pub fn extract_keyword_name(pair: &pest::iterators::Pair<Rule>) -> String {
        parser::keyword_name(pair.as_str()).unwrap_or_default()
    }

    /// Interface function: extract the normalized text from a `content_line` pair.
    pub fn extract_line_content(pair: &pest::iterators::Pair<Rule>) -> String {
        parser::parse_content_line(pair.as_str())
    }
}



/// Test function - test the extraction functions in mod [`extraction`] primitive.
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
    fn test_keyword_name() {
        assert_eq!(parser::keyword_name("[Component]"), Some("Component".into()));
        assert_eq!(parser::keyword_name("[Model]"), Some("Model".into()));
        assert_eq!(parser::keyword_name("[End]"), Some("End".into()));
        assert_eq!(parser::keyword_name("plain content"), None);
        assert_eq!(parser::keyword_name("[]"), None);
    }

    #[test]
    fn test_parse_content_line() {
        assert_eq!(parser::parse_content_line("  R_pkg 0.1  "), "R_pkg 0.1");
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
