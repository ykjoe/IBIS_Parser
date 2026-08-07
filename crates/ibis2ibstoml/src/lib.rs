//! ibis2ibstoml — First-pass IBIS-to-TOML conversion pipeline.
//!
//! This crate converts raw IBIS text into its TOML representation using a
//! three-stage pipeline:
//!
//! 1. **frontend** — The only public interface [`frontend::parse`]: IBIS text
//!    → abstract syntax tree. Internally it runs lexical analysis
//!    ([`frontend::lexical_analysis`]) → syntax analysis
//!    ([`frontend::syntax_analysis`]) → AST building
//!    ([`frontend::ast_builder`]), with a fault-tolerant fallback
//!    ([`frontend::recovery`]) when the pest parse fails.
//! 2. **backend** — Reserved: backend semantic processing / data-conversion
//!    interface.
//! 3. **emitter** — Serializes the
//!    [`SectionNode`](frontend::ast_builder::SectionNode) tree into TOML.
//!
//! All values are preserved as raw strings; no numeric conversion or unit
//! scaling is performed.
//!
//! The crate entry point exposes [`parse_to_toml`] and [`ibs2ibstoml`] as the
//! top-level public API.
//!
//! # Examples
//!
//! ```rust
//! use ibis2ibstoml::parse_to_toml;
//!
//! let toml_output = parse_to_toml("[IBIS ver] 2.1\n[Component] MyChip\n[End]\n")
//!     .expect("parsing failed");
//! assert!(toml_output.contains("ibis_ver"));
//! ```

pub mod backend;
pub mod emitter;
pub mod frontend;

use std::fs;
use std::path::Path;

/// Parse IBIS content and produce TOML output in a single pass.
///
/// Orchestrates the complete pipeline: the frontend parses the IBIS text into
/// a [`SectionNode`](frontend::ast_builder::SectionNode) tree, then the emitter
/// recursively serializes that tree into a TOML string.
///
/// # Pipeline
///
/// 1. **Frontend** — [`frontend::parse`] parses the IBIS text into a
///    [`SectionNode`](frontend::ast_builder::SectionNode) tree.
/// 2. **Emit** — [`emitter::toml::serialize_tree_to_string`] recursively
///    serializes the tree to TOML.
///
/// # Parameters
///
/// * `content` — A string containing the full text of an IBIS file.
///
/// # Returns
///
/// * `Ok(String)` — The TOML representation of the IBIS content.
/// * `Err(String)` — A human-readable error message if parsing fails.
///
/// # Errors
///
/// If pest parsing fails and the fallback also fails, an error message is
/// returned.
///
/// # Panics
///
/// Does not panic under normal operation.
///
/// # Examples
///
/// ```rust
/// use ibis2ibstoml::parse_to_toml;
///
/// let toml_output = parse_to_toml("[IBIS ver] 2.1\n[Component] MyChip\n[End]\n")
///     .expect("parsing failed");
/// assert!(toml_output.contains("ibis_ver"));
/// ```
pub fn parse_to_toml(content: &str) -> Result<String, String> {
    // Phase 1: frontend parsing → AST tree.
    let tree = frontend::parse(content)?;

    // Phase 2: emitter serialization → TOML.
    Ok(emitter::toml::serialize_tree_to_string(&tree))
}

/// Read an IBIS file and produce a `.ibs.toml` representation.
///
/// Reads the file from disk and delegates conversion to [`parse_to_toml`],
/// which runs the pest-based full parsing (lexical + syntax) followed by
/// direct TOML serialization.
///
/// # Parameters
///
/// * `path` — Path to an `.ibs` file. Accepts any type implementing
///   [`AsRef<Path>`].
///
/// # Returns
///
/// * `Ok(String)` — The TOML representation of the IBIS content.
/// * `Err(String)` — A human-readable error message if the file cannot be read
///   or conversion fails.
///
/// # Errors
///
/// Returns `Err` if the file cannot be read from disk, or if the content
/// cannot be parsed.
///
/// # Panics
///
/// Does not panic under normal operation.
pub fn ibs2ibstoml<P: AsRef<Path>>(path: P) -> Result<String, String> {
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    parse_to_toml(&content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_to_toml_simple() {
        let ibis_content = "\
[IBIS ver] 2.1
[Component] STM32F103
[Manufacturer] STMicro
[Package]
R_pkg 0.1
L_pkg 1nH
";
        let result = parse_to_toml(ibis_content).unwrap();
        assert!(result.contains("[File_Header.IBIS_ver]"), "Missing File_Header.IBIS_ver");
        assert!(result.contains("ibis_ver = \"2.1\""), "Missing ibis_ver");
        assert!(result.contains("[Component]"), "Missing [Component]");
        assert!(result.contains("component = \"STM32F103\""), "Missing component");
        assert!(result.contains("[Component.Manufacturer]"), "Missing Component.Manufacturer");
        assert!(result.contains("[Component.Package]"), "Missing Component.Package");
    }

    #[test]
    fn test_parse_to_toml_handles_comments() {
        let ibis_content = "\
| This is a comment
[IBIS ver] 2.1
| Another comment
[File name] test.ibs
";
        let result = parse_to_toml(ibis_content).unwrap();
        assert!(result.contains("[File_Header.IBIS_ver]"));
        assert!(result.contains("[File_Header.File_name]"));
    }

    #[test]
    fn test_parse_to_toml_multiple_models() {
        let ibis_content = "[Model] ModelA\n[Model] ModelB\n";
        let result = parse_to_toml(ibis_content).unwrap();
        assert_eq!(result.matches("[Model]").count(), 2);
        assert!(result.contains("model = \"ModelA\""));
        assert!(result.contains("model = \"ModelB\""));
    }

    #[test]
    fn test_parse_to_toml_end_is_skipped() {
        let ibis_content = "\
[Component] MyComp
[End]
[Other] val
";
        let result = parse_to_toml(ibis_content).unwrap();
        assert!(result.contains("[Component]"));
        assert!(result.contains("[Other]"));
        assert!(!result.contains("End"), "[End] should not appear in output");
    }
}
