//! Syntax analysis module — keyword classification, tree building, TOML serialisation.
//!
//! This module implements the "generic consumer" pattern: it does not know the
//! specific format of each IBIS keyword's content. Instead, a [`ParserState`]
//! state machine drives a generic content-rendering loop that handles all section
//! types uniformly.
//!
//! # Design constraint
//!
//! - All values are preserved as TOML strings; no `f64` conversion is performed.
//! - No semantic analysis, numerical conversion, or unit scaling.
//! - Keyword classification is based on pure string matching.
//! - All content lines have already been structured by PEST in [`super::lexical_analy`].
//!
//! # Pipeline
//!
//! 1. **Classify** — map raw keyword strings to [`Keyword`] enum variants.
//! 2. **Render** — serialise [`ContentLine`](super::lexical_analy::ContentLine)s into TOML string values.
//! 3. **Build tree** — organise classified blocks into a hierarchical [`Section`] tree.
//! 4. **Serialise** — emit TOML text from the section tree.
//!
//! # Related modules
//!
//! - [`super::lexical_analy`] — produces the [`Token`](super::lexical_analy::Token) list consumed by this module.
//! - [`super::core`] — top-level orchestrator.


use std::fmt::Write as FmtWrite;
use crate::ibis2ibstoml::lexical_analy::{ContentLine, Token};

// =============================================================================
// Parser state — drives the generic consumer
// =============================================================================

/// Tracks the current parsing context for the state machine.
///
/// The state determines how content lines are aggregated and serialised:
/// - `FileHeader`: collects consecutive header fields under `[File_Header]`
/// - `InArrayParent(keyword)`: collecting child sections for array parents
/// - `InSingleton(keyword)`: a standalone section with no children
/// - `Skipping`: currently inside an `[End]` or skipped section
#[derive(Debug, Clone, PartialEq)]
enum ParserState {
    FileHeader,
    InArrayParent(Keyword),
    InSingleton(Keyword),
    Skipping,
}

// =============================================================================
// Keyword type — maps 1:1 to IBIS 7.0 spec section headers
// =============================================================================

/// Canonical IBIS keyword identifiers.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Keyword {
    // ── Array-parent keywords (9 container types per IBIS 7.0 spec) ──
    Component,
    Model,
    Submodel,
    ExternalCircuit,
    TestData,
    TestLoad,
    DefinePackageModel,
    InterconnectModelSet,
    ModelSelector,

    // ── File header ──
    FileHeader,
    FileHeaderField(String),

    // ── End marker ──
    End,

    // ── Catch-all ──
    Other(String),
}

/// Keywords that belong to the IBIS file header section.
const FILE_HEADER_KEYWORDS: &[&str] = &[
    "IBIS ver",
    "Comment Char",
    "File name",
    "File Rev",
    "Date",
    "Source",
    "Notes",
    "Disclaimer",
    "Copyright",
];

impl Keyword {
    /// Classifies a raw keyword string into a [`Keyword`] variant.
    fn classify(raw_keyword: &str) -> Self {
        match raw_keyword {
            "Component" => Keyword::Component,
            "Model" => Keyword::Model,
            "Submodel" => Keyword::Submodel,
            "External Circuit" => Keyword::ExternalCircuit,
            "Test Data" => Keyword::TestData,
            "Test Load" => Keyword::TestLoad,
            "Define Package Model" => Keyword::DefinePackageModel,
            "Interconnect Model Set" => Keyword::InterconnectModelSet,
            "Model Selector" => Keyword::ModelSelector,
            "End" => Keyword::End,
            raw if FILE_HEADER_KEYWORDS.contains(&raw) => {
                Keyword::FileHeaderField(raw.to_string())
            }
            other => Keyword::Other(other.to_string()),
        }
    }

    /// Returns the TOML-safe section name for this keyword.
    fn toml_name(&self) -> String {
        match self {
            Keyword::Component => "Component".into(),
            Keyword::Model => "Model".into(),
            Keyword::Submodel => "Submodel".into(),
            Keyword::ExternalCircuit => "External_Circuit".into(),
            Keyword::TestData => "Test_Data".into(),
            Keyword::TestLoad => "Test_Load".into(),
            Keyword::DefinePackageModel => "Define_Package_Model".into(),
            Keyword::InterconnectModelSet => "Interconnect_Model_Set".into(),
            Keyword::ModelSelector => "Model_Selector".into(),
            Keyword::FileHeader => "File_Header".into(),
            Keyword::FileHeaderField(original_name) => toml_section_name(original_name),
            Keyword::End => {
                unreachable!("End keyword is never serialised")
            }
            Keyword::Other(original_name) => toml_section_name(original_name),
        }
    }

    /// Whether this keyword is an array parent.
    fn is_array_parent(&self) -> bool {
        matches!(
            self,
            Keyword::Component
                | Keyword::Model
                | Keyword::Submodel
                | Keyword::ExternalCircuit
                | Keyword::TestData
                | Keyword::TestLoad
                | Keyword::DefinePackageModel
                | Keyword::InterconnectModelSet
                | Keyword::ModelSelector
        )
    }

    /// Whether this keyword is `End`.
    fn is_end(&self) -> bool {
        matches!(self, Keyword::End)
    }

    /// Whether this keyword is a file header field.
    fn is_file_header_field(&self) -> bool {
        matches!(self, Keyword::FileHeaderField(_))
    }
}

// =============================================================================
// Section tree — intermediate representation
// =============================================================================

#[derive(Debug, Clone)]
struct Section {
    kind: Keyword,
    content: Vec<String>,
    children: Vec<Section>,
}

// =============================================================================
// String helpers
// =============================================================================

fn escape_toml_string(raw_value: &str) -> String {
    let escaped_value = raw_value
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!("\"{}\"", escaped_value)
}

fn toml_section_name(keyword: &str) -> String {
    keyword.replace(' ', "_")
}

fn toml_key_name(section_name: &str) -> String {
    section_name.to_lowercase()
}

// =============================================================================
// Generic consumer — ParserState-driven TOML assembly
// =============================================================================

/// Render a block of [`ContentLine`]s into TOML string values for a given section.
///
/// This is the core of the "Generic Consumer" pattern: it does NOT know which
/// specific IBIS keyword it is processing.  It simply serialises all content
/// lines as either single-value, array, or per-line entries, depending on the
/// section's container type (array-parent vs singleton).
///
/// # Parameters
///
/// * `keyword` — The classified [`Keyword`] for this section. Determines
///   whether content is rendered as a single value (file-header fields) or
///   as a multi-line array (all other sections).
/// * `content_lines` — The structured content lines from PEST, provided by
///   the lexical analysis phase.
///
/// # Returns
///
/// A [`Vec<String>`] of pre-rendered TOML string values for this section's content.
/// File-header fields return a single-element vector; all other sections return
/// one element per content line.
fn render_content(
    keyword: &Keyword,
    content_lines: &[ContentLine],
) -> Vec<String> {
    let mut rendered_values: Vec<String> = Vec::new();

    // File header fields are single-value
    if keyword.is_file_header_field() {
        if let Some(first_line) = content_lines.first() {
            let raw_value = match first_line {
                ContentLine::KeyValue { values, .. } => {
                    values.join(" ")
                }
                ContentLine::TableRecord { columns } => {
                    columns.join(" ")
                }
            };
            rendered_values.push(raw_value);
        }
        return rendered_values;
    }

    // For all other sections, each content line becomes one TOML value
    for content_line in content_lines {
        let rendered_line = match content_line {
            ContentLine::KeyValue { key, values } => {
                // Key-value lines: rejoin as "key value1 value2 ..."
                let mut reconstructed = key.clone();
                if !values.is_empty() {
                    reconstructed.push(' ');
                    reconstructed.push_str(&values.join(" "));
                }
                reconstructed
            }
            ContentLine::TableRecord { columns } => {
                columns.join(" ")
            }
        };
        rendered_values.push(rendered_line);
    }

    rendered_values
}

// =============================================================================
// TOML serialization
// =============================================================================

fn serialize_section(section: &Section, parent_path: &str, output_buffer: &mut String) {
    let section_name = section.kind.toml_name();
    let full_path = if parent_path.is_empty() {
        section_name.clone()
    } else {
        format!("{}.{}", parent_path, section_name)
    };

    let toml_key = toml_key_name(&section_name);

    // ── Emit section header ──
    if section.kind.is_array_parent() {
        let _ = writeln!(output_buffer, "[[{}]]", full_path);
    } else {
        let _ = writeln!(output_buffer, "[{}]", full_path);
    }

    // ── Emit content lines ──
    match section.content.len() {
        0 => {}
        1 => {
            let _ = writeln!(
                output_buffer,
                "{} = {}",
                toml_key,
                escape_toml_string(&section.content[0])
            );
        }
        multiple_lines => {
            let _ = writeln!(output_buffer, "{} = [", toml_key);
            for (line_index, line) in section.content.iter().enumerate() {
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

    let _ = writeln!(output_buffer);

    // ── Recursively serialise children ──
    for child_section in &section.children {
        serialize_section(child_section, &full_path, output_buffer);
    }
}

// =============================================================================
// Public API — TOML conversion
// =============================================================================

/// Convert a list of [`Token`]s into a TOML string.
///
/// The conversion uses a ParserState-driven pipeline:
/// 1. Classify each token's keyword.
/// 2. Render content lines into TOML strings using the generic consumer.
/// 3. Build a hierarchical section tree (file header group → array parent children).
/// 4. Serialise the tree to TOML.
///
/// # Parameters
///
/// * `tokens` — A list of [`Token`]s from lexical analysis, in file order.
///
/// # Returns
///
/// * `Ok(String)` — The TOML representation of the IBIS content.
/// * `Err(String)` — A human-readable error message if conversion fails.
///
/// # Errors
///
/// Returns `Err` if the token list is malformed (e.g., contains contradictory
/// nesting that cannot be represented in the section tree).
///
/// # Panics
///
/// Does not panic under normal operation. Panics only if the internal
/// [`write!`](std::write!) macro fails on the output buffer (a programming error).
///
/// # Examples
///
/// ```rust
/// use ibis_parser::ibis2ibstoml::lexical_analy::tokenize;
/// use ibis_parser::ibis2ibstoml::syntax_analy::ibs2toml;
///
/// let tokens = tokenize("[IBIS ver] 2.1\n[Component] MyChip\n[End]\n");
/// let toml_output = ibs2toml(tokens).expect("conversion failed");
/// assert!(toml_output.contains("ibis_ver"));
/// ```
pub fn ibs2toml(tokens: Vec<Token>) -> Result<String, String> {
    // ── Phase 1: classify + render content ──
    let mut classified_blocks: Vec<(Keyword, Vec<String>)> = Vec::new();

    for token in tokens.into_iter() {
        let classified_keyword = Keyword::classify(&token.keyword);
        let rendered_content =
            render_content(&classified_keyword, &token.content);
        classified_blocks.push((classified_keyword, rendered_content));
    }

    // ── Phase 2: build hierarchical section tree ──
    let section_tree = build_tree(classified_blocks);

    // ── Phase 3: serialise to TOML ──
    let mut output_buffer = String::new();
    for root_section in &section_tree {
        serialize_section(root_section, "", &mut output_buffer);
    }

    Ok(output_buffer)
}

// =============================================================================
// Tree construction
// =============================================================================

fn collect_child_sections(
    blocks: &[(Keyword, Vec<String>)],
    start_index: usize,
) -> (Vec<Section>, usize) {
    let mut collected_children = Vec::new();
    let mut current_index = start_index;

    while current_index < blocks.len() {
        let (child_keyword, child_lines) = &blocks[current_index];

        if child_keyword.is_array_parent() || child_keyword.is_end() {
            break;
        }

        collected_children.push(Section {
            kind: child_keyword.clone(),
            content: child_lines.clone(),
            children: Vec::new(),
        });

        current_index += 1;
    }

    (collected_children, current_index)
}

fn build_tree(blocks: Vec<(Keyword, Vec<String>)>) -> Vec<Section> {
    // ── Phase A: ParserState machine for file header grouping ──
    let mut parser_state = ParserState::FileHeader;
    let mut root_sections: Vec<Section> = Vec::new();
    let mut current_index = 0;

    // File header: collect consecutive FileHeaderField blocks
    if !blocks.is_empty() && blocks[0].0.is_file_header_field() {
        let mut file_header_children = Vec::new();
        while current_index < blocks.len() {
            if !blocks[current_index].0.is_file_header_field() {
                break;
            }
            let (keyword, lines) = &blocks[current_index];
            file_header_children.push(Section {
                kind: keyword.clone(),
                content: lines.clone(),
                children: Vec::new(),
            });
            current_index += 1;
        }
        root_sections.push(Section {
            kind: Keyword::FileHeader,
            content: Vec::new(),
            children: file_header_children,
        });
        parser_state = ParserState::FileHeader;
    }

    // ── Phase B: process remaining blocks ──
    while current_index < blocks.len() {
        let (keyword, content_lines) = &blocks[current_index];

        match keyword {
            kw if kw.is_array_parent() => {
                parser_state =
                    ParserState::InArrayParent(kw.clone());
                current_index += 1;

                let (children, next_index) =
                    collect_child_sections(&blocks, current_index);

                root_sections.push(Section {
                    kind: kw.clone(),
                    content: content_lines.clone(),
                    children,
                });

                current_index = next_index;
            }

            kw if kw.is_end() => {
                parser_state = ParserState::Skipping;
                current_index += 1;
            }

            singleton_keyword => {
                parser_state =
                    ParserState::InSingleton(singleton_keyword.clone());
                root_sections.push(Section {
                    kind: singleton_keyword.clone(),
                    content: content_lines.clone(),
                    children: Vec::new(),
                });
                current_index += 1;
            }
        }
    }

    root_sections
}

// =============================================================================
// Compat API — legacy function wrappers for integration tests
// =============================================================================

/// Legacy wrapper: parse a single header line (pure string match).
pub fn parse_header_line(line: &str) -> Option<(&'static str, String)> {
    let trimmed_line = line.trim();

    if trimmed_line.is_empty() || trimmed_line.starts_with('|') {
        return None;
    }

    let closing_bracket_position = trimmed_line.find(']')?;
    let keyword_part = &trimmed_line[1..closing_bracket_position];
    let value_part =
        trimmed_line[closing_bracket_position + 1..].trim().to_string();

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

/// Check whether a line is a continuation line (starts with `|`).
///
/// # Parameters
///
/// * `line` — A single line of IBIS content.
///
/// # Returns
///
/// `true` if the trimmed line starts with `|`, `false` otherwise.
///
/// # Examples
///
/// ```rust
/// use ibis_parser::ibis2ibstoml::syntax_analy::is_continuation_line;
///
/// assert!(is_continuation_line("| continued text"));
/// assert!(!is_continuation_line("[IBIS ver] 2.1"));
/// ```
pub fn is_continuation_line(line: &str) -> bool {
    line.trim().starts_with('|')
}

/// Extract the text after the `|` continuation marker.
///
/// # Parameters
///
/// * `line` — A single line that may start with `|`.
///
/// # Returns
///
/// * `Some(content)` — The trimmed text after `|` if the line is a continuation.
/// * `None` — If the line is not a continuation, or contains only whitespace after `|`.
///
/// # Examples
///
/// ```rust
/// use ibis_parser::ibis2ibstoml::syntax_analy::parse_continuation_content;
///
/// let content = parse_continuation_content("| more notes").unwrap();
/// assert_eq!(content, "more notes");
/// ```
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

/// Extract the section keyword name from a line containing `[Keyword]`.
///
/// # Parameters
///
/// * `line` — A line that may contain a keyword in square brackets.
///
/// # Returns
///
/// * `Some(keyword)` — The trimmed keyword name if brackets are found.
/// * `None` — If no closing `]` is found, or the content between brackets is empty.
///
/// # Examples
///
/// ```rust
/// use ibis_parser::ibis2ibstoml::syntax_analy::identify_section_keyword;
///
/// assert_eq!(identify_section_keyword("[Component]").unwrap(), "Component");
/// assert_eq!(identify_section_keyword("[Model]").unwrap(), "Model");
/// ```
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
    use crate::ibis2ibstoml::lexical_analy::ContentLine;

    #[test]
    fn test_keyword_classify_known() {
        assert_eq!(Keyword::classify("Component"), Keyword::Component);
        assert_eq!(Keyword::classify("Model"), Keyword::Model);
        assert_eq!(Keyword::classify("End"), Keyword::End);
        assert_eq!(Keyword::classify("Submodel"), Keyword::Submodel);
        assert_eq!(
            Keyword::classify("External Circuit"),
            Keyword::ExternalCircuit
        );
        assert_eq!(Keyword::classify("Test Data"), Keyword::TestData);
        assert_eq!(Keyword::classify("Test Load"), Keyword::TestLoad);
        assert_eq!(
            Keyword::classify("Define Package Model"),
            Keyword::DefinePackageModel
        );
        assert_eq!(
            Keyword::classify("Interconnect Model Set"),
            Keyword::InterconnectModelSet
        );
        assert_eq!(
            Keyword::classify("Model Selector"),
            Keyword::ModelSelector
        );
        assert_eq!(
            Keyword::classify("IBIS ver"),
            Keyword::FileHeaderField("IBIS ver".into())
        );
        assert_eq!(
            Keyword::classify("File name"),
            Keyword::FileHeaderField("File name".into())
        );
    }

    #[test]
    fn test_keyword_classify_unknown() {
        assert_eq!(
            Keyword::classify("CustomKeyword"),
            Keyword::Other("CustomKeyword".into())
        );
    }

    #[test]
    fn test_array_parent_keywords() {
        assert!(Keyword::Component.is_array_parent());
        assert!(Keyword::Model.is_array_parent());
        assert!(Keyword::Submodel.is_array_parent());
        assert!(Keyword::ExternalCircuit.is_array_parent());
        assert!(Keyword::TestData.is_array_parent());
        assert!(Keyword::TestLoad.is_array_parent());
        assert!(Keyword::DefinePackageModel.is_array_parent());
        assert!(Keyword::InterconnectModelSet.is_array_parent());
        assert!(Keyword::ModelSelector.is_array_parent());
        assert!(!Keyword::End.is_array_parent());
        assert!(!Keyword::FileHeader.is_array_parent());
        assert!(!Keyword::FileHeaderField("IBIS ver".into()).is_array_parent());
        assert!(!Keyword::Other("Pin".into()).is_array_parent());
    }

    #[test]
    fn test_toml_name_known() {
        assert_eq!(Keyword::Component.toml_name(), "Component");
        assert_eq!(Keyword::ExternalCircuit.toml_name(), "External_Circuit");
        assert_eq!(
            Keyword::InterconnectModelSet.toml_name(),
            "Interconnect_Model_Set"
        );
        assert_eq!(Keyword::ModelSelector.toml_name(), "Model_Selector");
        assert_eq!(Keyword::FileHeader.toml_name(), "File_Header");
        assert_eq!(
            Keyword::FileHeaderField("IBIS ver".into()).toml_name(),
            "IBIS_ver"
        );
    }

    #[test]
    fn test_toml_name_other() {
        assert_eq!(
            Keyword::Other("Pin Mapping".into()).toml_name(),
            "Pin_Mapping"
        );
    }

    #[test]
    fn test_render_content_file_header_field() {
        let lines = vec![ContentLine::TableRecord {
            columns: vec!["2.1".into()],
        }];
        let result =
            render_content(&Keyword::FileHeaderField("IBIS ver".into()), &lines);
        assert_eq!(result, vec!["2.1"]);
    }

    #[test]
    fn test_render_content_kv_line() {
        let lines = vec![ContentLine::KeyValue {
            key: "R_pkg".into(),
            values: vec!["250.0m".into(), "225.0m".into(), "275.0m".into()],
        }];
        let result =
            render_content(&Keyword::Other("Package".into()), &lines);
        assert_eq!(result, vec!["R_pkg 250.0m 225.0m 275.0m"]);
    }

    #[test]
    fn test_render_content_table_record() {
        let lines = vec![
            ContentLine::TableRecord {
                columns: vec![
                    "1".into(),
                    "RAS0#".into(),
                    "Buffer1".into(),
                    "200.0m".into(),
                    "5.0nH".into(),
                    "2.0pF".into(),
                ],
            },
        ];
        let result =
            render_content(&Keyword::Other("Pin".into()), &lines);
        assert_eq!(
            result,
            vec!["1 RAS0# Buffer1 200.0m 5.0nH 2.0pF"]
        );
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
    fn test_build_tree_flat_sections() {
        let blocks = vec![
            (Keyword::Other("Manufacturer".into()), vec!["STMicro".into()]),
            (Keyword::Other("Package".into()), vec!["R_pkg 0.1".into()]),
        ];
        let tree = build_tree(blocks);
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].kind, Keyword::Other("Manufacturer".into()));
        assert_eq!(tree[1].kind, Keyword::Other("Package".into()));
        assert!(tree[0].children.is_empty());
    }

    #[test]
    fn test_build_tree_array_section_with_children() {
        let blocks = vec![
            (Keyword::Component, vec!["MyComp".into()]),
            (Keyword::Other("Manufacturer".into()), vec!["STMicro".into()]),
            (Keyword::Other("Package".into()), vec!["R_pkg 0.1".into()]),
            (Keyword::Model, vec!["MyModel".into()]),
        ];
        let tree = build_tree(blocks);
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].kind, Keyword::Component);
        assert_eq!(tree[0].children.len(), 2);
        assert_eq!(
            tree[0].children[0].kind,
            Keyword::Other("Manufacturer".into())
        );
        assert_eq!(
            tree[0].children[1].kind,
            Keyword::Other("Package".into())
        );
        assert_eq!(tree[1].kind, Keyword::Model);
        assert!(tree[1].children.is_empty());
    }

    #[test]
    fn test_build_tree_skips_end() {
        let blocks = vec![
            (Keyword::Component, vec!["MyComp".into()]),
            (Keyword::End, vec![]),
            (Keyword::Other("Header".into()), vec!["val".into()]),
        ];
        let tree = build_tree(blocks);
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].kind, Keyword::Component);
        assert_eq!(tree[1].kind, Keyword::Other("Header".into()));
    }

    #[test]
    fn test_ibs2toml_simple() {
        let tokens = vec![
            Token {
                keyword: "IBIS ver".into(),
                content: vec![ContentLine::TableRecord {
                    columns: vec!["2.1".into()],
                }],
            },
            Token {
                keyword: "Component".into(),
                content: vec![ContentLine::TableRecord {
                    columns: vec!["STM32F103".into()],
                }],
            },
            Token {
                keyword: "Manufacturer".into(),
                content: vec![ContentLine::TableRecord {
                    columns: vec!["STMicro".into()],
                }],
            },
            Token {
                keyword: "Package".into(),
                content: vec![
                    ContentLine::KeyValue {
                        key: "R_pkg".into(),
                        values: vec!["0.1".into()],
                    },
                    ContentLine::KeyValue {
                        key: "L_pkg".into(),
                        values: vec!["1nH".into()],
                    },
                ],
            },
        ];
        let result = ibs2toml(tokens).unwrap();
        assert!(result.contains("[File_Header]"));
        assert!(result.contains("[File_Header.IBIS_ver]"));
        assert!(result.contains("ibis_ver = \"2.1\""));
        assert!(result.contains("[[Component]]"));
        assert!(result.contains("component = \"STM32F103\""));
        assert!(result.contains("[Component.Manufacturer]"));
        assert!(result.contains("manufacturer = \"STMicro\""));
        assert!(result.contains("[Component.Package]"));
        assert!(result.contains("package = ["));
    }

    #[test]
    fn test_ibs2toml_multiple_models() {
        let tokens = vec![
            Token {
                keyword: "Model".into(),
                content: vec![ContentLine::TableRecord {
                    columns: vec!["ModelA".into()],
                }],
            },
            Token {
                keyword: "Model".into(),
                content: vec![ContentLine::TableRecord {
                    columns: vec!["ModelB".into()],
                }],
            },
        ];
        let result = ibs2toml(tokens).unwrap();
        assert_eq!(result.matches("[[Model]]").count(), 2);
        assert!(result.contains("model = \"ModelA\""));
        assert!(result.contains("model = \"ModelB\""));
    }

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
}
