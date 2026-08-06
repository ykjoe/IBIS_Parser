//! TOML output — generate a TOML string from a [`SectionNode`] tree.
//!
//! Serialization rules:
//! - All sections use `[Section]` or `[Parent.Child]` format;
//! - `[[array-of-tables]]` is the backend's responsibility; the output layer
//!   does not distinguish it;
//! - Single-line content emits `key = "value"`, multi-line content emits a TOML array.

use std::fmt::Write as FmtWrite;

use crate::frontend::{NodeKind, SectionNode};

/// Escape and wrap a raw string value for TOML output.
///
/// # Parameters
///
/// * `raw_value` — The raw string to escape.
///
/// # Returns
///
/// The escaped value wrapped in double quotes.
fn escape_toml_string(raw_value: &str) -> String {
    let escaped_value = raw_value
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!("\"{}\"", escaped_value)
}

/// Convert a keyword name to a TOML-safe section name (spaces → underscores).
///
/// # Parameters
///
/// * `keyword` — The raw keyword name, e.g. `"Pin Mapping"`.
///
/// # Returns
///
/// The section name with spaces replaced by underscores, e.g. `"Pin_Mapping"`.
fn toml_section_name(keyword: &str) -> String {
    keyword.replace(' ', "_")
}

/// Convert a section name to a TOML key name (lowercased last path segment).
///
/// # Parameters
///
/// * `section_name` — A dot-separated TOML section path.
///
/// # Returns
///
/// The lowercased last path segment used as the key.
fn toml_key_name(section_name: &str) -> String {
    let last_segment = section_name.rsplit('.').next().unwrap_or(section_name);
    last_segment.to_lowercase()
}

/// Recursively serialize a section tree to TOML.
///
/// # Parameters
///
/// * `nodes` — The section nodes to serialize.
/// * `parent_path` — Dot-separated TOML path of the parent (e.g., `"Component"`).
/// * `output_buffer` — The mutable TOML output string being built.
///
/// # Returns
///
/// Does not return a value; output is appended directly to `output_buffer`.
pub fn serialize_tree(nodes: &[SectionNode], parent_path: &str, output_buffer: &mut String) {
    for node in nodes {
        let section_name = toml_section_name(&node.keyword);
        let full_path = if parent_path.is_empty() {
            section_name.clone()
        } else {
            format!("{}.{}", parent_path, section_name)
        };

        match node.kind {
            NodeKind::Regular => {
                // [Section] or [Parent.Child] — regular section.
                let _ = writeln!(output_buffer, "[{}]", full_path);
                emit_content(output_buffer, &section_name, &node.content);
                let _ = writeln!(output_buffer);
                serialize_tree(&node.children, &full_path, output_buffer);
            }

            NodeKind::FileHeader => {
                // [File_Header] — emit the section header, then its children.
                let _ = writeln!(output_buffer, "[{}]", full_path);
                let _ = writeln!(output_buffer);
                if !node.children.is_empty() {
                    serialize_tree(&node.children, &full_path, output_buffer);
                }
            }
        }
    }
}

/// Serialize a section tree to a TOML string.
///
/// # Parameters
///
/// * `tree` — The root-level section nodes to serialize.
///
/// # Returns
///
/// The TOML representation of the section tree.
pub fn serialize_tree_to_string(tree: &[SectionNode]) -> String {
    let mut output_buffer = String::new();
    serialize_tree(tree, "", &mut output_buffer);
    output_buffer
}

/// Emit content lines as TOML key-value pairs.
///
/// # Parameters
///
/// * `output_buffer` — The mutable TOML output string being built.
/// * `section_name` — The section name used to derive the TOML key.
/// * `content` — The content lines to emit.
fn emit_content(output_buffer: &mut String, section_name: &str, content: &[String]) {
    let toml_key = toml_key_name(section_name);

    match content.len() {
        0 => {}
        1 => {
            let _ = writeln!(
                output_buffer,
                "{} = {}",
                toml_key,
                escape_toml_string(&content[0])
            );
        }
        multiple_lines => {
            let _ = writeln!(output_buffer, "{} = [", toml_key);
            for (line_index, line) in content.iter().enumerate() {
                let separator = if line_index < multiple_lines - 1 {
                    ","
                } else {
                    ""
                };
                let _ = writeln!(
                    output_buffer,
                    "    {}{}",
                    escape_toml_string(line),
                    separator
                );
            }
            let _ = writeln!(output_buffer, "]");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::{NodeKind, SectionNode};

    #[test]
    fn test_escape_toml_string_wraps_in_quotes() {
        assert_eq!(escape_toml_string("hello"), "\"hello\"");
    }

    #[test]
    fn test_escape_toml_string_escapes_backslashes() {
        assert_eq!(escape_toml_string("a\\b"), "\"a\\\\b\"");
    }

    #[test]
    fn test_escape_toml_string_escapes_double_quotes() {
        assert_eq!(escape_toml_string("say \"hi\""), "\"say \\\"hi\\\"\"");
    }

    #[test]
    fn test_toml_section_name_replaces_spaces() {
        assert_eq!(toml_section_name("Pin Mapping"), "Pin_Mapping");
        assert_eq!(toml_section_name("GND Clamp"), "GND_Clamp");
        assert_eq!(toml_section_name("NoSpaces"), "NoSpaces");
    }

    #[test]
    fn test_serialize_tree_file_header() {
        let tree = vec![
            SectionNode {
                keyword: "File_Header".into(),
                kind: NodeKind::FileHeader,
                content: vec![],
                children: vec![
                    SectionNode {
                        keyword: "IBIS ver".into(),
                        kind: NodeKind::Regular,
                        content: vec!["2.1".into()],
                        children: vec![],
                    },
                ],
            },
        ];
        let mut output = String::new();
        serialize_tree(&tree, "", &mut output);
        assert!(output.contains("[File_Header.IBIS_ver]"));
        assert!(output.contains("ibis_ver = \"2.1\""));
    }

    #[test]
    fn test_serialize_tree_first_level() {
        let tree = vec![
            SectionNode {
                keyword: "Component".into(),
                kind: NodeKind::Regular,
                content: vec!["MyChip".into()],
                children: vec![
                    SectionNode {
                        keyword: "Manufacturer".into(),
                        kind: NodeKind::Regular,
                        content: vec!["Acme".into()],
                        children: vec![],
                    },
                ],
            },
        ];
        let mut output = String::new();
        serialize_tree(&tree, "", &mut output);
        assert!(output.contains("[Component]"));
        assert!(output.contains("[Component.Manufacturer]"));
    }
}
