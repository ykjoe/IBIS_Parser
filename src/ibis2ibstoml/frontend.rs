//! Frontend module — pest-based full parsing + TOML serialization.
//!
//! This module implements the v5 multi-level AST tree architecture:
//!
//! 1. **Parse** — Use [`IbisParser::parse`] with the [`ibis_file`](Rule::ibis_file) rule.
//! 2. **Group** — Walk the pest pair tree into flat [`ParsedBlock`] entries.
//! 3. **Build tree** — Recursively build a multi-level [`SectionNode`] tree.
//! 4. **Serialize** — Recursively emit TOML from the section tree.
//!
//! # Design constraints
//!
//! - All values are preserved as TOML strings; no `f64` conversion is performed.
//! - No semantic analysis, numerical conversion, or unit scaling.
//! - Keyword classification is done by pest rule matching (no Rust string match).
//! - The generic [`keyword`](Rule::keyword) rule serves as fallback for unrecognized keywords.

use std::fmt::Write as FmtWrite;

use pest::Parser;

// =============================================================================
// Parser type
// =============================================================================

#[derive(pest_derive::Parser)]
#[grammar = "ibis2ibstoml/ibis.pest"]
pub struct IbisParser;

// =============================================================================
// File header field detection
// =============================================================================

/// Known file header keyword names, used to group them under `[File_Header]`.
const FILE_HEADER_FIELD_NAMES: &[&str] = &[
    "IBIS ver", "Comment Char", "File name", "File Rev",
    "Date", "Source", "Notes", "Disclaimer", "Copyright",
];

/// Check whether a parsed block is a file header field.
fn is_file_header_field(block: &ParsedBlock) -> bool {
    FILE_HEADER_FIELD_NAMES.contains(&block.keyword.as_str())
}

// =============================================================================
// Keyword name extraction
// =============================================================================

/// Extract the keyword name from a keyword-header pair.
///
/// Works for both specific `kw_*` rules and the generic [`keyword`](Rule::keyword) rule.
/// The pair's string representation is `[KeywordName]`; brackets are stripped.
fn extract_keyword_name(pair: &pest::iterators::Pair<Rule>) -> String {
    let pair_text = pair.as_str();
    let start_position = pair_text.find('[').map(|pos| pos + 1).unwrap_or(0);
    let end_position = pair_text.find(']').unwrap_or(pair_text.len());
    pair_text[start_position..end_position].trim().to_string()
}

// =============================================================================
// TOML string helpers
// =============================================================================

/// Escape and wrap a raw string value for TOML output.
fn escape_toml_string(raw_value: &str) -> String {
    let escaped_value = raw_value
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!("\"{}\"", escaped_value)
}

/// Convert a keyword name to a TOML-safe section name (spaces → underscores).
fn toml_section_name(keyword: &str) -> String {
    keyword.replace(' ', "_")
}

/// Convert a section name to a TOML key name (lowercased last path segment).
fn toml_key_name(section_name: &str) -> String {
    let last_segment = section_name.rsplit('.').next().unwrap_or(section_name);
    last_segment.to_lowercase()
}

// =============================================================================
// Content extraction helpers
// =============================================================================

/// Extract all inner tokens from a content_line pair as a single string.
fn extract_line_content(pair: &pest::iterators::Pair<Rule>) -> String {
    let mut parts: Vec<String> = Vec::new();
    for inner_pair in pair.clone().into_inner() {
        let token_str = inner_pair.as_str().trim().to_string();
        if !token_str.is_empty() {
            parts.push(token_str);
        }
    }
    parts.join(" ")
}

// =============================================================================
// Multi-level AST tree types
// =============================================================================

/// Role of a section node in the TOML output.
///
/// The frontend does NOT distinguish array-of-tables (`[[...]]`)
/// from regular tables (`[...]`); that is a backend concern.
#[derive(Debug, Clone, PartialEq)]
enum NodeKind {
    /// `[File_Header]` — virtual container for file header fields.
    FileHeader,
    /// `[Section]` or `[Parent.Child]` — regular section.
    Regular,
}

/// A node in the hierarchical IBIS section tree.
#[derive(Debug, Clone)]
struct SectionNode {
    /// Keyword name (e.g., "Component", "IBIS ver", "Pin").
    keyword: String,
    /// Role determining TOML output format.
    kind: NodeKind,
    /// Content lines belonging directly to this section.
    content: Vec<String>,
    /// Child sections nested under this node.
    children: Vec<SectionNode>,
}

// =============================================================================
// Intermediate representation — flat blocks from pest pairs
// =============================================================================

/// A parsed keyword block with its content lines.
#[derive(Debug, Clone)]
struct ParsedBlock {
    /// Raw keyword name (e.g., "Component", "IBIS ver", "Package").
    keyword: String,
    /// Pest rule variant that matched this keyword header.
    rule: Rule,
    /// Content lines belonging to this block.
    content: Vec<String>,
}

// =============================================================================
// Public API — parse IBIS content and produce TOML
// =============================================================================

/// Parse IBIS content and produce TOML output in a single pass.
///
/// # Pipeline
///
/// 1. **Parse** — Use [`IbisParser::parse`] with the [`ibis_file`](Rule::ibis_file) rule.
/// 2. **Group** — Walk the pest pair tree into flat [`ParsedBlock`] entries.
/// 3. **Build tree** — Recursively build a multi-level [`SectionNode`] tree.
/// 4. **Serialize** — Recursively emit TOML from the section tree.
///
/// If the full PEST parse fails, a line-by-line fallback is used.
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
/// If pest parsing fails and the fallback also fails, an error message is returned.
///
/// # Examples
///
/// ```rust
/// use ibis_parser::ibis2ibstoml::frontend::parse_to_toml;
///
/// let toml_output = parse_to_toml("[IBIS ver] 2.1\n[Component] MyChip\n[End]\n")
///     .expect("parsing failed");
/// assert!(toml_output.contains("ibis_ver"));
/// ```
pub fn parse_to_toml(content: &str) -> Result<String, String> {
    // ── Phase 1: parse with pest ──
    let parsed_pairs = match IbisParser::parse(Rule::ibis_file, content) {
        Ok(pairs) => pairs,
        Err(_parse_error) => {
            return Ok(fallback_parse_to_toml(content));
        }
    };

    // ── Phase 2: group pairs into flat keyword blocks ──
    let blocks = group_pairs_to_blocks(parsed_pairs);

    // ── Phase 3: build hierarchical section tree ──
    // Multiple root-level groups (FileHeader + first-level containers + singletons)
    let mut tree: Vec<SectionNode> = Vec::new();
    let mut block_index = 0;
    while block_index < blocks.len() {
        let (mut nodes, next_index) = build_section_tree(&blocks, block_index, &[]);
        tree.append(&mut nodes);
        block_index = next_index;
    }

    // ── Phase 4: serialize tree to TOML ──
    let mut output_buffer = String::new();
    serialize_tree(&tree, "", &mut output_buffer);

    Ok(output_buffer)
}

// =============================================================================
// Phase 2: Pair tree → flat blocks
// =============================================================================

/// Walk pest pairs and group into keyword blocks.
fn group_pairs_to_blocks(pairs: pest::iterators::Pairs<Rule>) -> Vec<ParsedBlock> {
    let mut blocks: Vec<ParsedBlock> = Vec::new();
    let mut current_keyword: Option<String> = None;
    let mut current_rule: Option<Rule> = None;
    let mut accumulated_content: Vec<String> = Vec::new();

    // The top-level pair is ibis_file; its inner pairs are the matched alternatives.
    for pair in pairs.flatten() {
        let rule = pair.as_rule();

        // Content lines — keep raw text, don't join tokens (preserves original spacing)
        if rule == Rule::content_line {
            let line_content = pair.as_str().trim().to_string();
            if !line_content.is_empty() {
                accumulated_content.push(line_content);
            }
            continue;
        }

        // Keyword headers: grouped rules or generic fallback
        if rule == Rule::first_level_keyword
            || rule == Rule::second_level_keyword
            || rule == Rule::kw_end
            || rule == Rule::keyword
        {
            // Flush previous block
            if let Some(previous_keyword) = current_keyword.take() {
                blocks.push(ParsedBlock {
                    keyword: previous_keyword,
                    rule: current_rule.take().unwrap(),
                    content: accumulated_content.clone(),
                });
                accumulated_content.clear();
            }

            let keyword_name = extract_keyword_name(&pair);
            current_keyword = Some(keyword_name);
            current_rule = Some(rule);
        }
    }

    // Flush the last block
    if let Some(trailing_keyword) = current_keyword.take() {
        blocks.push(ParsedBlock {
            keyword: trailing_keyword,
            rule: current_rule.take().unwrap(),
            content: accumulated_content,
        });
    }

    blocks
}

// =============================================================================
// Phase 3: Flat blocks → hierarchical tree
// =============================================================================

/// Build a hierarchical section tree from flat parsed blocks.
///
/// The construction logic:
/// 1. File header fields (matched by [`is_file_header_field`]) are grouped under
///    a virtual `[File_Header]` parent node.
/// 2. First-level containers ([`first_level_keyword`](Rule::first_level_keyword)) emit
///    as `[[array]]` and recursively collect subsequent non-first-level blocks as children.
/// 3. End markers are skipped.
/// 4. All other blocks emit as child/singleton nodes.
///
/// # Parameters
///
/// * `blocks` — Flat list of parsed keyword blocks in file order.
/// * `start` — Starting index for this recursion level.
/// * `stop_rules` — List of rule variants that should stop child collection.
///
/// # Returns
///
/// A tuple of `(Vec<SectionNode>, usize)` — the built nodes and the next index to process.
fn build_section_tree(
    blocks: &[ParsedBlock],
    start: usize,
    stop_rules: &[Rule],
) -> (Vec<SectionNode>, usize) {
    let mut nodes: Vec<SectionNode> = Vec::new();
    let mut block_index = start;

    // ── Phase A: file header grouping ──
    if block_index < blocks.len() && is_file_header_field(&blocks[block_index]) {
        let mut children: Vec<SectionNode> = Vec::new();
        while block_index < blocks.len() && is_file_header_field(&blocks[block_index]) {
            children.push(SectionNode {
                keyword: blocks[block_index].keyword.clone(),
                kind: NodeKind::Regular,
                content: blocks[block_index].content.clone(),
                children: Vec::new(),
            });
            block_index += 1;
        }
        nodes.push(SectionNode {
            keyword: "File_Header".into(),
            kind: NodeKind::FileHeader,
            content: Vec::new(),
            children,
        });
        return (nodes, block_index);
    }

    // ── Phase B: process remaining blocks ──
    while block_index < blocks.len() {
        let block = &blocks[block_index];

        // Check stop rules
        if stop_rules.contains(&block.rule) {
            break;
        }

        if block.rule == Rule::kw_end {
            // Skip [End] marker
            block_index += 1;
            continue;
        }

        if block.rule == Rule::first_level_keyword {
            // First-level container: create parent node, recursively collect children
            let keyword_name = block.keyword.clone();
            let content = block.content.clone();
            block_index += 1;

            // Recursively collect child blocks
            let (children, next_index) = build_section_tree(
                blocks,
                block_index,
                &[Rule::first_level_keyword, Rule::kw_end],
            );
            block_index = next_index;

            nodes.push(SectionNode {
                keyword: keyword_name,
                kind: NodeKind::Regular,
                content,
                children,
            });
        } else {
            // Second-level or generic keyword → child or singleton node
            nodes.push(SectionNode {
                keyword: block.keyword.clone(),
                kind: NodeKind::Regular,
                content: block.content.clone(),
                children: Vec::new(),
            });
            block_index += 1;
        }
    }

    (nodes, block_index)
}

// =============================================================================
// Phase 4: Tree → TOML serialization
// =============================================================================

/// Recursively serialize a section tree to TOML.
///
/// # Parameters
///
/// * `nodes` — The section nodes to serialize.
/// * `parent_path` — Dot-separated TOML path of the parent (e.g., `"Component"`).
/// * `output_buffer` — The mutable TOML output string being built.
fn serialize_tree(
    nodes: &[SectionNode],
    parent_path: &str,
    output_buffer: &mut String,
) {
    for node in nodes {
        let section_name = toml_section_name(&node.keyword);
        let full_path = if parent_path.is_empty() {
            section_name.clone()
        } else {
            format!("{}.{}", parent_path, section_name)
        };

        match node.kind {
            NodeKind::Regular => {
                // [Section] or [Parent.Child] — regular section
                let _ = writeln!(output_buffer, "[{}]", full_path);
                emit_content(output_buffer, &section_name, &node.content);
                let _ = writeln!(output_buffer);
                serialize_tree(&node.children, &full_path, output_buffer);
            }

            NodeKind::FileHeader => {
                // [File_Header] — emit section header then children
                let _ = writeln!(output_buffer, "[{}]", full_path);
                let _ = writeln!(output_buffer);
                if !node.children.is_empty() {
                    serialize_tree(&node.children, &full_path, output_buffer);
                }
            }
        }
    }
}

/// Emit content lines as TOML key-value pairs.
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

// =============================================================================
// Fallback parser — line-by-line when pest full parse fails
// =============================================================================

fn fallback_parse_to_toml(content: &str) -> String {
    let mut blocks: Vec<ParsedBlock> = Vec::new();
    let mut current_keyword: Option<String> = None;
    let mut current_rule: Option<Rule> = None;
    let mut accumulated_content: Vec<String> = Vec::new();

    for raw_line in content.lines() {
        let trimmed_line = raw_line.trim();

        if trimmed_line.is_empty() || trimmed_line.starts_with('|') {
            continue;
        }

        if let Some(closing_bracket) = trimmed_line.find(']') {
            if trimmed_line.starts_with('[') {
                if let Some(previous_keyword) = current_keyword.take() {
                    blocks.push(ParsedBlock {
                        keyword: previous_keyword,
                        rule: current_rule.take().unwrap_or(Rule::keyword),
                        content: accumulated_content.clone(),
                    });
                    accumulated_content.clear();
                }

                let keyword_name = trimmed_line[1..closing_bracket].trim().to_string();
                current_keyword = Some(keyword_name);
                current_rule = Some(Rule::keyword);

                let text_after_bracket = trimmed_line[closing_bracket + 1..].trim().to_string();
                if !text_after_bracket.is_empty() {
                    accumulated_content.push(text_after_bracket);
                }
                continue;
            }
        }

        accumulated_content.push(trimmed_line.to_string());
    }

    if let Some(trailing_keyword) = current_keyword.take() {
        blocks.push(ParsedBlock {
            keyword: trailing_keyword,
            rule: current_rule.take().unwrap_or(Rule::keyword),
            content: accumulated_content,
        });
    }

    // Build tree: loop to handle multiple root-level groups
    let mut tree: Vec<SectionNode> = Vec::new();
    let mut block_index = 0;
    while block_index < blocks.len() {
        let (mut nodes, next_index) = build_section_tree(&blocks, block_index, &[]);
        tree.append(&mut nodes);
        block_index = next_index;
    }

    let mut output_buffer = String::new();
    serialize_tree(&tree, "", &mut output_buffer);
    output_buffer
}

// =============================================================================
// Compat API — legacy function wrappers for integration tests
// =============================================================================

pub fn parse_header_line(line: &str) -> Option<(&'static str, String)> {
    let trimmed_line = line.trim();

    if trimmed_line.is_empty() || trimmed_line.starts_with('|') {
        return None;
    }

    let closing_bracket_position = trimmed_line.find(']')?;
    let keyword_part = &trimmed_line[1..closing_bracket_position];
    let value_part = trimmed_line[closing_bracket_position + 1..].trim().to_string();

    let toml_key = match keyword_part {
        "IBIS ver" => "ibis_ver",
        "Comment Char" => "comment_char",
        "File name" => "file_name",
        "File Rev" => "file_rev",
        "Date" => "date",
        "Source" => "source",
        "Notes" => "notes",
        "Disclaimer" => "disclaimer",
        "Copyright" => "copyright",
        _ => return None,
    };

    Some((toml_key, value_part))
}

pub fn is_continuation_line(line: &str) -> bool {
    line.trim().starts_with('|')
}

pub fn parse_continuation_content(line: &str) -> Option<String> {
    let trimmed_line = line.trim();
    if trimmed_line.starts_with('|') {
        let content_after_marker = trimmed_line[1..].trim().to_string();
        if content_after_marker.is_empty() {
            None
        } else {
            Some(content_after_marker)
        }
    } else {
        None
    }
}

pub fn identify_section_keyword(line: &str) -> Option<String> {
    let trimmed_line = line.trim();
    let closing_bracket_position = trimmed_line.find(']')?;
    let keyword_part = &trimmed_line[..=closing_bracket_position];
    let content = &keyword_part[1..keyword_part.len() - 1];
    if content.is_empty() {
        None
    } else {
        Some(content.trim().to_string())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_to_toml tests ──

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

    #[test]
    fn test_parse_to_toml_content_line() {
        let line = "R_pkg 250.0m 225.0m 275.0m";
        let pairs = IbisParser::parse(Rule::content_line, line).unwrap();
        for pair in pairs {
            let extracted = extract_line_content(&pair);
            assert_eq!(extracted, "R_pkg 250.0m 225.0m 275.0m");
        }
    }

    #[test]
    fn test_parse_to_toml_data_line_parse() {
        let line = "1 RAS0# Buffer1 200.0m 5.0nH 2.0pF";
        let pairs = IbisParser::parse(Rule::content_line, line).unwrap();
        for pair in pairs {
            let extracted = extract_line_content(&pair);
            assert!(!extracted.is_empty());
            assert!(extracted.contains("1"));
        }
    }

    // ── fallback tests ──

    #[test]
    fn test_fallback_parse_simple() {
        let ibis_content = "\
[IBIS ver] 2.1
[Component] STM32F103
[Manufacturer] STMicro
";
        let result = fallback_parse_to_toml(ibis_content);
        assert!(result.contains("[File_Header.IBIS_ver]"), "Missing File_Header.IBIS_ver in fallback");
        assert!(result.contains("[Component]"), "Missing [Component] in fallback");
    }

    // ── section tree tests ──

    #[test]
    fn test_build_section_tree_file_header() {
        let blocks = vec![
            ParsedBlock { keyword: "IBIS ver".into(), rule: Rule::second_level_keyword, content: vec!["2.1".into()] },
            ParsedBlock { keyword: "File name".into(), rule: Rule::second_level_keyword, content: vec!["test.ibs".into()] },
        ];
        let (tree, _) = build_section_tree(&blocks, 0, &[]);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].keyword, "File_Header");
        assert_eq!(tree[0].children.len(), 2);
        assert_eq!(tree[0].children[0].keyword, "IBIS ver");
    }

    #[test]
    fn test_build_section_tree_component_with_children() {
        let blocks = vec![
            ParsedBlock { keyword: "Component".into(), rule: Rule::first_level_keyword, content: vec!["MyComp".into()] },
            ParsedBlock { keyword: "Manufacturer".into(), rule: Rule::second_level_keyword, content: vec!["Acme".into()] },
            ParsedBlock { keyword: "Package".into(), rule: Rule::second_level_keyword, content: vec!["R_pkg 0.1".into()] },
        ];
        let (tree, _) = build_section_tree(&blocks, 0, &[]);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].kind, NodeKind::Regular);
        assert_eq!(tree[0].children.len(), 2);
    }

    // ── compat API tests ──

    #[test]
    fn test_parse_header_line_ibis_ver() {
        let (key, value) = parse_header_line("[IBIS ver] 2.1").unwrap();
        assert_eq!(key, "ibis_ver");
        assert_eq!(value, "2.1");
    }

    #[test]
    fn test_parse_header_line_file_name() {
        let (key, value) = parse_header_line("[File name] f103c8.ibs").unwrap();
        assert_eq!(key, "file_name");
        assert_eq!(value, "f103c8.ibs");
    }

    #[test]
    fn test_parse_header_line_date() {
        let (key, value) = parse_header_line("[Date] 12-08-2024").unwrap();
        assert_eq!(key, "date");
        assert_eq!(value, "12-08-2024");
    }

    #[test]
    fn test_parse_header_line_comment() {
        assert!(parse_header_line("| This is a comment").is_none());
    }

    #[test]
    fn test_identify_section_keyword() {
        assert_eq!(
            identify_section_keyword("[Component]").unwrap(),
            "Component"
        );
        assert_eq!(identify_section_keyword("[Model]").unwrap(), "Model");
        assert_eq!(identify_section_keyword("[End]").unwrap(), "End");
    }

    #[test]
    fn test_continuation_line() {
        assert!(is_continuation_line("| continued text"));
        assert!(!is_continuation_line("[IBIS ver] 2.1"));
    }

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
    fn test_extract_keyword_name_specific() {
        let pairs = IbisParser::parse(Rule::kw_component, "[Component]").unwrap();
        for pair in pairs {
            assert_eq!(extract_keyword_name(&pair), "Component");
        }
    }

    #[test]
    fn test_extract_keyword_name_generic() {
        let pairs = IbisParser::parse(Rule::keyword, "[CustomSection]").unwrap();
        for pair in pairs {
            assert_eq!(extract_keyword_name(&pair), "CustomSection");
        }
    }

    #[test]
    fn test_is_file_header_field() {
        let header_block = ParsedBlock {
            keyword: "IBIS ver".into(),
            rule: Rule::second_level_keyword,
            content: vec![],
        };
        assert!(is_file_header_field(&header_block));

        let non_header_block = ParsedBlock {
            keyword: "Pin".into(),
            rule: Rule::second_level_keyword,
            content: vec![],
        };
        assert!(!is_file_header_field(&non_header_block));
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

    #[test]
    fn debug_pest_ibis_file() {
        let content = "[IBIS ver] 2.1\n[Component] MyChip\n";
        let pairs = IbisParser::parse(Rule::ibis_file, content).unwrap();
        for pair in pairs.flatten() {
            println!("DEBUG pair: rule={:?} text='{}'", pair.as_rule(), pair.as_str().escape_default());
        }
    }
}
