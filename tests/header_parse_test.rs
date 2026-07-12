//! Integration tests for IBIS file header parsing.
//!
//! Parses the file header from a real sample IBIS file and verifies each field.
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

use ibis_parser::ibis2ibstoml::frontend::{
    identify_section_keyword, is_continuation_line, parse_continuation_content,
    parse_header_line,
};
use ibis_parser::ibis_parser::ibis_structure::IBIS_FileHeader;

/// Parse the file header section from an IBIS file path.
///
/// Reads the file, extracts recognised header fields (IBIS ver, File name, etc.),
/// and returns both the structured header and the raw header lines.
///
/// # Parameters
///
/// * `path` — Path to an `.ibs` file. Accepts any type implementing [`AsRef<Path>`].
///
/// # Returns
///
/// A tuple of:
/// * `(IBIS_FileHeader, Vec<String>)` — The parsed header struct and the raw
///   header lines as they appear in the file.
///
/// # Panics
///
/// Panics if the file cannot be read.
fn parse_file_header<P: AsRef<Path>>(path: P) -> (IBIS_FileHeader, Vec<String>) {
    let content = fs::read_to_string(path).expect("Failed to read IBIS file");
    let lines: Vec<&str> = content.lines().collect();

    let mut header = IBIS_FileHeader::default();
    let mut raw_lines: Vec<String> = Vec::new();
    let mut in_header = true;
    let mut multi_line_field: Option<&str> = None;

    for &line in &lines {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        // Detect end of header: a non-header keyword
        if in_header && !trimmed.starts_with('|') {
            if let Some(keyword) = identify_section_keyword(trimmed) {
                match keyword.as_str() {
                    "IBIS ver" | "Comment Char" | "File name" | "File Rev"
                    | "Date" | "Source" | "Notes" | "Disclaimer" | "Copyright" => {}
                    _ => {
                        in_header = false;
                        continue;
                    }
                }
            }
        }

        if !in_header {
            break;
        }

        // Handle continuation lines for multi-line fields (Notes/Disclaimer/Copyright)
        if is_continuation_line(trimmed) {
            if let Some(field) = multi_line_field {
                if let Some(content) = parse_continuation_content(trimmed) {
                    let target = match field {
                        "notes" => &mut header.notes,
                        "disclaimer" => &mut header.disclaimer,
                        "copyright" => &mut header.copyright,
                        _ => continue,
                    };
                    let current = target.clone().unwrap_or_default();
                    *target = Some(if current.is_empty() {
                        content
                    } else {
                        format!("{}\n{}", current, content)
                    });
                }
            }
            raw_lines.push(trimmed.to_string());
            continue;
        }

        // Parse header line
        if let Some((key, value)) = parse_header_line(trimmed) {
            match key {
                "ibis_ver" => header.ibis_ver = value,
                "comment_char" => header.comment_char = value.chars().next(),
                "file_name" => header.file_name = value,
                "file_rev" => header.file_rev = value,
                "date" => header.date = Some(value),
                "source" => header.source = Some(value),
                "notes" => {
                    header.notes = Some(value);
                    multi_line_field = Some("notes");
                }
                "disclaimer" => {
                    header.disclaimer = Some(value);
                    multi_line_field = Some("disclaimer");
                }
                "copyright" => {
                    header.copyright = Some(value);
                    multi_line_field = Some("copyright");
                }
                _ => {}
            }
            raw_lines.push(trimmed.to_string());
        }
    }

    (header, raw_lines)
}

#[test]
fn test_parse_file_header_from_sample() {
    let (header, raw_lines) = parse_file_header("tests/examples/f103c8.ibs");

    println!("========================================");
    println!("Raw header lines:");
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
