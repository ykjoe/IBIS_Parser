// =============================================================================
// core — public API for ibis2ibstoml conversion
// =============================================================================

use std::fs;
use std::path::Path;

use crate::ibis2ibstoml::lexical_analy::tokenize;
use crate::ibis2ibstoml::syntax_analy::ibs2toml;

/// Read an IBIS file and produce a `.ibs.toml` representation.
///
/// The function uses a two-phase pipeline:
/// 1. **Lexical analysis** ([`tokenize`]) — pest-based tokenization into keyword blocks.
/// 2. **Syntax analysis** ([`ibs2toml`]) — keyword classification, tree building, TOML output.
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
/// Returns `Err` if the file cannot be read from disk, if the content is not valid
/// IBIS syntax, or if TOML serialisation fails.
///
/// # Panics
///
/// Does not panic under normal operation. Panics only if the internal write
/// buffer cannot accept the serialised output (a programming error).
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

    // Phase 1: lexical analysis (pest-based tokenization)
    let tokens = tokenize(&content);

    // Phase 2: syntax analysis (classification + tree + TOML)
    ibs2toml(tokens)
}
