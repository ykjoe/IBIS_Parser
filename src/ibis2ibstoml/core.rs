// =============================================================================
// core — public API for ibis2ibstoml conversion
// =============================================================================

use std::fs;
use std::path::Path;

use crate::ibis2ibstoml::frontend::parse_to_toml;

/// Read an IBIS file and produce a `.ibs.toml` representation.
///
/// The function uses a single-pass pipeline:
/// 1. **Frontend parsing** ([`parse_to_toml`]) — pest-based full parsing
///    (lexical + syntax) and direct TOML serialization.
///
/// # Parameters
///
/// * `path` — Path to an `.ibs` file. Accepts any type implementing [`AsRef<Path>`].
///
/// # Returns
///
/// * `Ok(String)` — The TOML representation of the IBIS content.
/// * `Err(String)` — A human-readable error message if the file cannot be read or conversion fails.
///
/// # Errors
///
/// Returns `Err` if the file cannot be read from disk, or if the content cannot be parsed.
///
/// # Panics
///
/// Does not panic under normal operation.
///
/// # Examples
///
/// ```ignore
/// use ibis_parser::ibis2ibstoml::core::ibs2ibstoml;
///
/// let toml_output = ibs2ibstoml("tests/examples/f103c8.ibs")
///     .expect("parsing failed");
/// assert!(toml_output.contains("[File_Header]"));
/// ```

pub fn ibs2ibstoml<P: AsRef<Path>>(path: P) -> Result<String, String> {
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    // Single-pass frontend: pest full parsing → TOML output
    parse_to_toml(&content)
}
