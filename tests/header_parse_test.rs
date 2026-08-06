//! Integration tests for IBIS file header parsing.
//!
//! Parses the file header from a real sample IBIS file and verifies each field.
//! Uses the public [`ibis2ibstoml::frontend::parse`] API (the compat helpers
//! are internal to the frontend and not part of the public interface).
//!
//! Run with:
//!
//! ```sh
//! cargo test --test header_parse_test -- --nocapture
//! ```

// =============================================================================
// Integration test: parse File Header from sample IBIS file → TOML output
//
// Run with:  cargo test --test header_parse_test -- --nocapture
// =============================================================================

use std::fs;
use std::path::Path;

use ibis2ibstoml::frontend::{parse, NodeKind, SectionNode};
use ibis_parser::ibis_parser::ibis_structure::IBIS_FileHeader;

/// Build an [`IBIS_FileHeader`] from the `[File_Header]` virtual node of the
/// frontend AST.
///
/// Walks the root [`SectionNode`] list, finds the `File_Header` node, and maps
/// each child (a file header field) into the strongly-typed header struct.
///
/// # Parameters
///
/// * `tree` — The root-level section tree returned by [`parse`].
///
/// # Returns
///
/// The parsed header struct.
fn header_from_tree(tree: &[SectionNode]) -> IBIS_FileHeader {
    let mut header = IBIS_FileHeader::default();

    let file_header = tree.iter().find(|node| node.keyword == "File_Header");
    let Some(file_header) = file_header else {
        return header;
    };
    assert_eq!(file_header.kind, NodeKind::FileHeader);

    for child in &file_header.children {
        let value = child.content.join("\n");
        match child.keyword.as_str() {
            "IBIS ver" => header.ibis_ver = value,
            "Comment Char" => header.comment_char = value.chars().next(),
            "File name" => header.file_name = value,
            "File Rev" => header.file_rev = value,
            "Date" => header.date = Some(value),
            "Source" => header.source = Some(value),
            "Notes" => header.notes = Some(value),
            "Disclaimer" => header.disclaimer = Some(value),
            "Copyright" => header.copyright = Some(value),
            _ => {}
        }
    }

    header
}

/// Parse the file header section from an IBIS file path via the public API.
///
/// # Parameters
///
/// * `path` — Path to an `.ibs` file. Accepts any type implementing [`AsRef<Path>`].
///
/// # Returns
///
/// A tuple of:
/// * `(IBIS_FileHeader, Vec<String>)` — The parsed header struct and the raw
///   header field keywords as they appear in the AST.
///
/// # Panics
///
/// Panics if the file cannot be read.
fn parse_file_header<P: AsRef<Path>>(path: P) -> (IBIS_FileHeader, Vec<String>) {
    let content = fs::read_to_string(path).expect("Failed to read IBIS file");
    let tree = parse(&content).expect("Failed to parse IBIS file");

    let header = header_from_tree(&tree);

    let file_header = tree.iter().find(|node| node.keyword == "File_Header");
    let raw_lines: Vec<String> = file_header
        .map(|node| {
            node.children
                .iter()
                .map(|child| child.keyword.clone())
                .collect()
        })
        .unwrap_or_default();

    (header, raw_lines)
}

#[test]
fn test_parse_file_header_from_sample() {
    let (header, raw_lines) = parse_file_header("tests/examples/f103c8.ibs");

    println!("========================================");
    println!("Raw header fields (from AST):");
    println!("========================================");
    for line in &raw_lines {
        println!("  {}", line);
    }

    println!("\n========================================");
    println!("Parsed header fields:");
    println!("========================================");
    println!("  ibis_ver:      {:?}", header.ibis_ver);
    println!("  comment_char:  {:?}", header.comment_char);
    println!("  file_name:     {:?}", header.file_name);
    println!("  file_rev:      {:?}", header.file_rev);
    println!("  date:          {:?}", header.date);
    println!("  source:        {:?}", header.source);
    println!("  notes:         {:?}", header.notes);
    println!("  disclaimer:    {:?}", header.disclaimer);
    println!("  copyright:     {:?}", header.copyright);

    // Verify required fields
    assert!(!header.ibis_ver.is_empty(), "IBIS ver should not be empty");
    assert!(
        !header.file_name.is_empty(),
        "File name should not be empty"
    );
    assert!(!header.file_rev.is_empty(), "File rev should not be empty");

    // Sample file specific checks
    assert_eq!(header.ibis_ver, "2.1");
    assert_eq!(header.file_name, "f103c8.ibs");
    assert_eq!(header.file_rev, "1.1");
    assert_eq!(header.date.as_deref(), Some("12-08-2024"));
}
