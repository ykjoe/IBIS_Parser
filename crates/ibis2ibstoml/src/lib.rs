// =============================================================================
// ibis2ibstoml — IBIS text → TOML conversion pipeline
//
// Stages:
//   1. frontend — `frontend::parse`: IBIS text → `SectionNode` AST tree
//                 (lexical → syntax → AST, with a line-by-line fallback);
//   2. backend  — `backend::semantic_parse`: AST → typed parsed tree
//                 (pre_process → content_parse → validation);
//   3. emitter  — `emitter::serialize_parsed_tree`: parsed tree → TOML document.
//
// Every value stays text-only: quantities keep their original spelling and unit,
// and no numeric normalization happens anywhere in the pipeline.
// =============================================================================

//! IBIS text → TOML, through the frontend / backend / emitter pipeline.
//!
//! [`parse_to_toml`] runs the whole pipeline in strict mode and returns the TOML
//! document; [`parse_to_parsed`] stops after the backend for callers that want the
//! typed parsed tree instead of text.
//!
//! # Examples
//!
//! Basic usage: parse IBIS content into a typed parsed tree.
//!
//! ```rust
//! use ibis2ibstoml::parse_to_parsed;
//!
//! let ibis_content = "\
//! [IBIS ver] 2.1
//! [File name] chip.ibs
//! [File Rev] 1.0
//! [Component] MyChip
//! [Manufacturer] Acme
//! [Package]
//! R_pkg 250.0m 225.0m 275.0m
//! [Pin]
//! PA0 IO8TC
//! [End]
//! ";
//!
//! let parsed = parse_to_parsed(ibis_content).expect("parsing failed");
//! assert_eq!(parsed[0].keyword, "File_Header");
//! ```

pub mod backend;
pub mod emitter;
pub mod frontend;
pub mod schema;

pub use backend::{
    semantic_parse, semantic_parse_lenient, Corner, Diagnostic, ParsedField, ParsedNode,
    ParsedTable, ParsedValue, Rule, Severity, ValidationReport,
};
pub use frontend::SectionNode;

use std::fs;
use std::path::Path;

/// Parses IBIS content into the backend's typed parsed tree.
///
/// Runs the frontend (`frontend::parse`) and then the backend
/// (`backend::semantic_parse`) in strict mode.
///
/// # Parameters
///
/// * `content` — A string containing the full text of an IBIS file.
///
/// # Returns
///
/// * `Ok(Vec<ParsedNode>)` — The parsed tree: typed fields, corner tuples and tables.
/// * `Err(String)` — A human-readable message when the frontend cannot parse the
///   input or the backend rejects its structure.
pub fn parse_to_parsed(content: &str) -> Result<Vec<ParsedNode>, String> {
    let tree = frontend::parse(content)?;
    semantic_parse(&tree).map_err(|error| error.to_string())
}

/// Parses IBIS content into the parsed tree under **lenient** rules.
///
/// Structural problems are collected into a [`ValidationReport`] instead of aborting
/// the conversion.
///
/// # Parameters
///
/// * `content` — A string containing the full text of an IBIS file.
///
/// # Returns
///
/// * `Ok((Vec<ParsedNode>, ValidationReport))` — The parsed tree plus the problems
///   collected on the way.
/// * `Err(String)` — A human-readable message when the frontend cannot parse the input.
pub fn parse_to_parsed_lenient(
    content: &str,
) -> Result<(Vec<ParsedNode>, ValidationReport), String> {
    let tree = frontend::parse(content)?;
    semantic_parse_lenient(&tree).map_err(|error| error.to_string())
}

/// Parses IBIS content and returns the TOML document.
///
/// # Parameters
///
/// * `content` — A string containing the full text of an IBIS file.
///
/// # Returns
///
/// * `Ok(String)` — The TOML document produced by the emitter.
/// * `Err(String)` — A human-readable message when parsing fails.
///
/// # Errors
///
/// Returns `Err` when the frontend cannot parse the input, or when the backend
/// rejects its structure in strict mode.
///
/// # Examples
///
/// ```rust
/// use ibis2ibstoml::parse_to_toml;
///
/// let output =
///     parse_to_toml("[IBIS ver] 2.1\n[File name] chip.ibs\n[File Rev] 1.0\n").expect("parsing failed");
/// assert!(output.contains("ibis_ver = \"2.1\""));
/// ```
pub fn parse_to_toml(content: &str) -> Result<String, String> {
    let parsed = parse_to_parsed(content)?;
    Ok(emitter::serialize_parsed_tree(&parsed))
}

/// Parses IBIS content and returns the TOML document under **lenient** rules.
///
/// # Parameters
///
/// * `content` — A string containing the full text of an IBIS file.
///
/// # Returns
///
/// * `Ok((String, ValidationReport))` — The TOML document plus the collected
///   problems.
/// * `Err(String)` — A human-readable message when parsing fails.
pub fn parse_to_toml_lenient(content: &str) -> Result<(String, ValidationReport), String> {
    let (parsed, report) = parse_to_parsed_lenient(content)?;
    Ok((emitter::serialize_parsed_tree(&parsed), report))
}

/// Reads an IBIS file and returns its single-string rendering.
///
/// # Parameters
///
/// * `path` — Path to an `.ibs` file; accepts anything implementing [`AsRef<Path>`].
///
/// # Returns
///
/// * `Ok(String)` — The rendering produced by [`parse_to_toml`].
/// * `Err(String)` — A human-readable message when the file cannot be read or the
///   conversion fails.
///
/// # Errors
///
/// Returns `Err` when the file cannot be read, or when parsing fails.
pub fn ibs2ibstoml<P: AsRef<Path>>(path: P) -> Result<String, String> {
    let content =
        fs::read_to_string(&path).map_err(|error| format!("failed to read file: {error}"))?;
    parse_to_toml(&content)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Finds the child node carrying the given canonical keyword.
    fn child_with_keyword<'a>(node: &'a ParsedNode, keyword: &str) -> Option<&'a ParsedNode> {
        node.children.iter().find(|child| child.keyword == keyword)
    }

    #[test]
    fn test_parse_to_parsed_simple_component() {
        let ibis_content = "\
[IBIS ver] 2.1
[File name] test.ibs
[File Rev] 1.0
[Component] STM32F103
[Manufacturer] STMicro
[Package]
R_pkg 250.0m 225.0m 275.0m
[Pin]
PA0 IO8TC
";
        let parsed = parse_to_parsed(ibis_content).expect("parsing failed");

        let header = &parsed[0];
        assert_eq!(header.keyword, "File_Header");
        let ibis_ver = child_with_keyword(header, "IBIS_Ver").expect("IBIS ver entry");
        assert_eq!(
            ibis_ver.field("ibis_ver").map(|field| field.value.clone()),
            Some(ParsedValue::Text("2.1".into()))
        );

        let component = parsed
            .iter()
            .find(|node| node.keyword == "Component")
            .expect("Component section");
        assert_eq!(
            component.field("component").map(|field| field.value.clone()),
            Some(ParsedValue::Text("STM32F103".into()))
        );

        let package = child_with_keyword(component, "Package").expect("Package section");
        assert_eq!(
            package.field("r_pkg").map(|field| field.value.clone()),
            Some(ParsedValue::Corner(Corner(
                "250.0m".into(),
                "225.0m".into(),
                "275.0m".into()
            )))
        );
    }

    #[test]
    fn test_parse_to_parsed_multiple_models() {
        let ibis_content = "\
[IBIS ver] 2.1
[File name] test.ibs
[File Rev] 1.0
[Model] ModelA
Model_type I/O
[Model] ModelB
Model_type I/O
";
        let parsed = parse_to_parsed(ibis_content).expect("parsing failed");
        let models: Vec<&ParsedNode> = parsed
            .iter()
            .filter(|node| node.keyword == "Model")
            .collect();

        assert_eq!(models.len(), 2);
        assert_eq!(
            models[0].field("model").map(|field| field.value.clone()),
            Some(ParsedValue::Text("ModelA".into()))
        );
        assert_eq!(
            models[1].field("model").map(|field| field.value.clone()),
            Some(ParsedValue::Text("ModelB".into()))
        );
    }

    #[test]
    fn test_parse_to_parsed_lenient_collects_report() {
        let ibis_content = "\
[IBIS ver] 2.1
[File name] test.ibs
[File Rev] 1.0
[Mystery Section] unknown content
";
        let (parsed, report) = parse_to_parsed_lenient(ibis_content).expect("lenient mode parses");

        assert_eq!(parsed.len(), 2);
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors[0].message.contains("Mystery Section"));
        assert_eq!(report.errors[0].scope, "(file)");
    }

    #[test]
    fn test_parse_to_toml_renders_toml() {
        let ibis_content = "[IBIS ver] 2.1\n[File name] test.ibs\n[File Rev] 1.0\n";
        let output = parse_to_toml(ibis_content).expect("parsing failed");

        assert!(output.contains("[File_Header]"), "{output}");
        assert!(output.contains("ibis_ver = \"2.1\""), "{output}");
        assert!(output.contains("file_name = \"test.ibs\""), "{output}");

        let document = toml::from_str::<toml::Value>(&output).expect("output must be valid TOML");
        assert!(document.get("File_Header").is_some());
    }

    #[test]
    fn test_parse_to_toml_writes_arrays_of_tables() {
        let ibis_content = "\
[IBIS ver] 2.1
[File name] test.ibs
[File Rev] 1.0
[Model] ModelA
Model_type I/O
[Model] ModelB
Model_type I/O
";
        let output = parse_to_toml(ibis_content).expect("parsing failed");

        assert_eq!(output.matches("[[Model]]").count(), 2, "{output}");
        assert!(output.contains("model = \"ModelA\""), "{output}");

        let document = toml::from_str::<toml::Value>(&output).expect("output must be valid TOML");
        let models = document.get("Model").and_then(|value| value.as_array());
        assert_eq!(models.map(Vec::len), Some(2));
    }
}
