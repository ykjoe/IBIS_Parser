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
//! 2. **backend** — Semantic layer: [`backend::semantic_parse`] maps the
//!    [`SectionNode`] tree into a strongly-typed [`IBIS_File`]
//!    ([`backend::ibis_structure`]) using declarative mapping tables, then
//!    validates it.
//! 3. **emitter** — [`emitter::toml::serialize_ibis_file`] serializes the
//!    strongly-typed [`IBIS_File`] into a TOML string.
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
//! let toml_output = parse_to_toml(
//!     "[IBIS ver] 2.1\n[File name] chip.ibs\n[File Rev] 1.0\n[Component] MyChip\n[Manufacturer] Acme\n[End]\n",
//! )
//! .expect("parsing failed");
//! assert!(toml_output.contains("ibis_ver"));
//! ```

pub mod backend;
pub mod emitter;
pub mod frontend;
pub mod schema;

pub use backend::{Issue, ValidationReport};

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
/// let toml_output = parse_to_toml(
///     "[IBIS ver] 2.1\n[File name] chip.ibs\n[File Rev] 1.0\n[Component] MyChip\n[Manufacturer] Acme\n[End]\n",
/// )
/// .expect("parsing failed");
/// assert!(toml_output.contains("ibis_ver"));
/// ```
pub fn parse_to_toml(content: &str) -> Result<String, String> {
    // Phase 1: frontend parsing → AST tree.
    println!("(1) Parsing all the keywords & sections");
    let tree = frontend::parse(content)?;
    std::fs::write("ast_debug.txt", format!("{:#?}", tree)).ok();

    // Phase 2: backend semantic analysis → strongly-typed IBIS_File.
    println!("(2) Analyzing content semantic");
    let file = backend::semantic_parse(&tree).map_err(|e| format!("{e}"))?;

    // Phase 3: emitter serialization → TOML.
    println!("(3) Serializing content semantic");
    Ok(emitter::toml::serialize_ibis_file(&file))
}

/// Parse IBIS content and produce TOML under **lenient** validation.
///
/// Identical to [`parse_to_toml`], but the backend collects validation issues
/// into a [`ValidationReport`] (returned alongside the TOML string) instead of
/// aborting the conversion on the first problem.
///
/// # Parameters
///
/// * `content` — A string containing the full text of an IBIS file.
///
/// # Returns
///
/// * `Ok((String, ValidationReport))` — The TOML representation plus the
///   collected validation issues (errors + warnings).
/// * `Err(String)` — A human-readable error message if parsing fails.
pub fn parse_to_toml_lenient(content: &str) -> Result<(String, ValidationReport), String> {
    let tree = frontend::parse(content)?;
    let (file, report) = backend::semantic_parse_lenient(&tree).map_err(|e| format!("{e}"))?;
    Ok((emitter::toml::serialize_ibis_file(&file), report))
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
    println!("Reading & Parsing ibs file...");
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
[File name] test.ibs
[File Rev] 1.0
[Component] STM32F103
[Manufacturer] STMicro
[Package]
R_pkg 0.1
L_pkg 1nH
";
        let result = parse_to_toml(ibis_content).unwrap();
        assert!(result.contains("[File_Header]"), "Missing [File_Header]");
        assert!(result.contains("ibis_ver = \"2.1\""), "Missing ibis_ver");
        assert!(result.contains("[[Component]]"), "Missing [[Component]]");
        assert!(result.contains("component = \"STM32F103\""), "Missing component");
        assert!(result.contains("manufacturer = \"STMicro\""), "Missing manufacturer");
        assert!(result.contains("[Component.Package]"), "Missing Component.Package");
        assert!(result.contains("r_pkg = { col_header = [\"typ\", \"min\", \"max\"], data = [\"0.1\"] }"), "Missing r_pkg corner (table)");
    }

    #[test]
    fn test_parse_to_toml_handles_comments() {
        let ibis_content = "\
| This is a comment
[IBIS ver] 2.1
| Another comment
[File name] test.ibs
[File Rev] 1.0
";
        let result = parse_to_toml(ibis_content).unwrap();
        assert!(result.contains("[File_Header]"));
        assert!(result.contains("ibis_ver = \"2.1\""));
        assert!(result.contains("file_name = \"test.ibs\""));
    }

    #[test]
    fn test_parse_to_toml_multiple_models() {
        let ibis_content = "\
[IBIS ver] 2.1
[File name] test.ibs
[File Rev] 1.0
[Model] ModelA
Model_type I/O
[Model] ModelB
Model_type I/O
";
        let result = parse_to_toml(ibis_content).unwrap();
        assert_eq!(result.matches("[[Model]]").count(), 2);
        assert!(result.contains("model = \"ModelA\""));
        assert!(result.contains("model = \"ModelB\""));
    }

    #[test]
    fn test_parse_to_toml_end_is_skipped() {
        let ibis_content = "\
[IBIS ver] 2.1
[File name] test.ibs
[File Rev] 1.0
[Component] MyComp
[Manufacturer] Acme
[End]
";
        let result = parse_to_toml(ibis_content).unwrap();
        assert!(result.contains("[[Component]]"));
        assert!(!result.contains("End"), "[End] should not appear in output");
    }

    #[test]
    fn test_parse_to_toml_lenient_collects_report() {
        let ibis_content = "\
[IBIS ver] 2.1
[File name] test.ibs
[File Rev] 1.0
[Model] ModelA
";
        let (toml_output, report) = parse_to_toml_lenient(ibis_content).unwrap();
        assert!(toml_output.contains("[[Model]]"));
        // 宽松模式：缺少 model_type 被 validator 收集而非阻断。
        assert!(
            report.errors.iter().any(|issue| issue.message.contains("model_type")),
            "expected a model_type issue in {:?}",
            report.errors
        );
    }
}
