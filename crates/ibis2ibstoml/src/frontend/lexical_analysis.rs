//! Lexical analysis — the lexical stage of the frontend pipeline.
//!
//! This stage provides the two lowest-level reading operations on IBIS text:
//!
//! - [`parser::keyword_name`] — read a section keyword out of a `[Keyword]`
//!   header token;
//! - [`parser::parse_content_line`] — read the normalized text of a content
//!   line.
//!
//! Both are plain functions over `&str`, so every other stage reads the same
//! primitives instead of re-implementing them. There is no grammar engine: the
//! IBIS structure comes from [`ibis_schema.toml`](crate::schema), which the
//! syntax and AST stages read.
//!
//! # Input / Output
//!
//! | Input | Output |
//! |-------|--------|
//! | A raw header token (`"[Component]"`) | `Option<String>` |
//! | A raw content line (`"  R_pkg 0.1  "`) | `String` |
//!
//! # Related modules
//!
//! - [`super::syntax_analysis`] — folds the input into a flat block list using
//!   these primitives;
//! - [`super::ast_builder`] — builds the section tree out of those blocks.

// =============================================================================
// lexical_analysis — reading primitives of the frontend's first stage
//
// Design constraints:
//   - Purely textual: no grammar engine, no token stream and no pair tree;
//   - The IBIS structure is NOT described here: it lives in
//     `schema/ibis_schema.toml` and is read by the syntax and AST stages;
//   - All values are preserved as raw strings; no numeric conversion happens
//     here.
// =============================================================================

/// Reading primitives of the lexical stage — plain functions over raw text.
pub(crate) mod parser {
    /// Read the keyword name out of a `[Keyword]` header token.
    ///
    /// Returns `None` unless the token opens with `[`, so that a content line
    /// merely mentioning a bracket (`ODT modeled with [Submodel]`) is never
    /// read as a keyword header.
    pub(crate) fn keyword_name(token: &str) -> Option<String> {
        let header = token.trim().strip_prefix('[')?;
        let closing = header.find(']')?;
        let name = header[..closing].trim();
        if name.is_empty() || name.contains('[') {
            return None;
        }
        Some(name.to_string())
    }

    /// Read the normalized text of a content line.
    pub(crate) fn parse_content_line(line: &str) -> String {
        line.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyword_name() {
        assert_eq!(parser::keyword_name("[Component]"), Some("Component".into()));
        assert_eq!(parser::keyword_name("[Model]"), Some("Model".into()));
        assert_eq!(parser::keyword_name("[End]"), Some("End".into()));
        assert_eq!(parser::keyword_name("plain content"), None);
        assert_eq!(parser::keyword_name("[]"), None);
    }

    #[test]
    fn test_keyword_name_requires_a_leading_bracket() {
        assert_eq!(parser::keyword_name("ODT modeled with [Submodel]"), None);
        assert_eq!(parser::keyword_name("Added new [Receiver Thresholds]"), None);
        assert_eq!(parser::keyword_name("["), None);
        assert_eq!(parser::keyword_name("[Model Spec"), None);
    }

    #[test]
    fn test_parse_content_line() {
        assert_eq!(parser::parse_content_line("  R_pkg 0.1  "), "R_pkg 0.1");
    }
}
