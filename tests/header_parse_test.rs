//! Integration tests for IBIS file header parsing.
//!
//! Parses the file header from a real sample IBIS file and verifies each field,
//! exercising both public layers:
//!
//! - `ibis2ibstoml::frontend::parse` — the raw AST (`[File_Header]` virtual node);
//! - `ibis2ibstoml::parse_to_parsed` — the backend's typed parsed tree.
//!
//! Run with:
//!
//! ```sh
//! cargo test --test header_parse_test -- --nocapture
//! ```

// =============================================================================
// Integration test: parse the File Header from a sample IBIS file
//
// Run with:  cargo test --test header_parse_test -- --nocapture
// =============================================================================

use std::fs;
use std::path::Path;

use ibis2ibstoml::frontend::{parse, NodeKind, SectionNode};
use ibis2ibstoml::{parse_to_parsed, ParsedNode, ParsedValue};

/// Reads one file header entry value out of the parsed tree.
///
/// Takes the parsed `[File_Header]` node, the canonical entry keyword (e.g.
/// `"IBIS_Ver"`) and the entry's own field key (e.g. `"ibis_ver"`); returns the
/// entry text, joining multi-line entries with newlines.
fn header_entry(header: &ParsedNode, keyword: &str, key: &str) -> Option<String> {
    let entry = header.children.iter().find(|child| child.keyword == keyword)?;
    let field = entry.fields.iter().find(|field| field.key == key)?;
    match &field.value {
        ParsedValue::Text(text) => Some(text.clone()),
        ParsedValue::Lines(lines) => Some(lines.join("\n")),
        _ => None,
    }
}

/// Parses the file header from an IBIS file path through both public layers.
///
/// Takes a `path` to an `.ibs` file; returns the raw AST keywords of the header
/// fields together with the parsed `[File_Header]` node.
///
/// # Panics
///
/// Panics when the file cannot be read, when either layer fails, or when the AST
/// carries no `[File_Header]` node.
fn parse_file_header<P: AsRef<Path>>(path: P) -> (Vec<String>, ParsedNode) {
    let content = fs::read_to_string(path).expect("failed to read IBIS file");

    let tree: Vec<SectionNode> = parse(&content).expect("frontend parse failed");
    let header_node = tree
        .iter()
        .find(|node| node.keyword == "File_Header")
        .expect("missing [File_Header] node");
    assert_eq!(header_node.kind, NodeKind::FileHeader);
    let raw_keywords: Vec<String> = header_node
        .children
        .iter()
        .map(|child| child.keyword.clone())
        .collect();

    let parsed = parse_to_parsed(&content).expect("backend parse failed");
    let parsed_header = parsed
        .into_iter()
        .find(|node| node.keyword == "File_Header")
        .expect("missing parsed [File_Header] node");

    (raw_keywords, parsed_header)
}

#[test]
fn test_parse_file_header_from_sample() {
    let (raw_keywords, header) = parse_file_header("tests/examples/f103c8.ibs");

    println!("========================================");
    println!("Raw header fields (from AST):");
    println!("========================================");
    for keyword in &raw_keywords {
        println!("  {keyword}");
    }

    let ibis_ver = header_entry(&header, "IBIS_Ver", "ibis_ver");
    let comment_char = header_entry(&header, "Comment_Char", "comment_char");
    let file_name = header_entry(&header, "File_Name", "file_name");
    let file_rev = header_entry(&header, "File_Rev", "file_rev");
    let date = header_entry(&header, "Date", "date");
    let source = header_entry(&header, "Source", "source");
    let notes = header_entry(&header, "Notes", "notes");

    println!("\n========================================");
    println!("Parsed header fields:");
    println!("========================================");
    println!("  ibis_ver:      {ibis_ver:?}");
    println!("  comment_char:  {comment_char:?}");
    println!("  file_name:     {file_name:?}");
    println!("  file_rev:      {file_rev:?}");
    println!("  date:          {date:?}");
    println!("  source:        {source:?}");
    println!("  notes:         {notes:?}");

    // Required fields must be present and non-empty.
    assert!(!ibis_ver.clone().unwrap_or_default().is_empty(), "IBIS ver is empty");
    assert!(!file_name.clone().unwrap_or_default().is_empty(), "File name is empty");
    assert!(!file_rev.clone().unwrap_or_default().is_empty(), "File Rev is empty");

    // Sample-specific values.
    assert_eq!(ibis_ver.as_deref(), Some("2.1"));
    assert_eq!(file_name.as_deref(), Some("f103c8.ibs"));
    assert_eq!(file_rev.as_deref(), Some("1.1"));
    assert_eq!(date.as_deref(), Some("12-08-2024"));
}
