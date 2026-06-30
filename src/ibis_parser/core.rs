// =============================================================================
// IBIS Parser Core API
// =============================================================================

use std::fs;
use std::path::Path;

use crate::ibis_parser::converter::ibs2toml;

/// Parse raw IBIS file content into the AST representation.
pub fn ibis_file_parse(_content: &str) -> Result<(), String> {
    todo!("implement IBIS file parser");
}

/// Read an IBIS file and produce a `.ibs.toml` representation.
///
/// The function uses pest to find all keyword headers, splits the file into
/// section blocks, and serializes them to TOML.
pub fn ibs2ibstoml<P: AsRef<Path>>(path: P) -> Result<String, String> {
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read file: {}", e))?;
    ibs2toml(&content)
}
