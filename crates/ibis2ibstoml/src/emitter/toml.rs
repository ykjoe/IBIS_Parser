// =============================================================================
// toml — render the parsed tree into a TOML document
//
// The emitter consumes the backend's typed parsed tree (`ParsedNode`) and writes
// a TOML string. It never touches the frontend AST and performs no semantic work:
// every value is written exactly as it was parsed.
//
// Layout rules:
//
//   - one TOML table per keyword node: `[Path.Keyword]` for a single instance and
//     `[[Path.Keyword]]` for a repeated one (`occurrence == Multiple`, or the same
//     keyword appearing several times among its siblings);
//   - the keyword's own value and its params become fields of that table:
//     `manufacturer = "Acme"`, `r_pkg = [...]`, `"dv/dt_r" = [...]`;
//   - the virtual `[File_Header]` container flattens its entries into plain fields
//     (`ibis_ver = "2.1"`) instead of nested tables;
//   - keys that are not bare TOML keys (`dv/dt_r`, `vinh+`, names with spaces) are
//     quoted exactly as `ibis_schema.toml` spells them.
//
// Value shapes (TOML has no tuples, so a corner becomes a three-element array):
//
//   - `Text`   → `"original text"` (quantities keep their unit: `"1.65V"`);
//   - `Corner` → a one-row table `{ header = ["typ", "min", "max"], data = ["typ", "min", "max"] }`;
//   - `Table`  → `{ header = [...], data = [[...]] }`;
//   - `Lines`  → a multi-line basic string whose closing delimiter sits on its own
//                line (`"""\nline\nline\n"""`), and a plain string for at most one line.
//
// Multi-line arrays inside the inline table are valid TOML 1.1, which keeps wide
// I-V tables readable while staying parseable.
//
// The code is grouped along the two halves of that job: `TomlWriter` walks the
// tree and appends tables/fields, while the `text` sub-module holds the pure
// formatters that turn one value or one key into its TOML spelling.
// =============================================================================

//! TOML rendering of the parsed tree.

use std::fmt::Write as _;

use crate::backend::{Corner, ParsedField, ParsedNode, ParsedTable, ParsedValue};
use crate::schema::Occurrence;

/// The virtual container holding the file header entries.
const FILE_HEADER_CONTAINER: &str = "File_Header";

/// Renders the parsed tree as a TOML document.
///
/// # Parameters
///
/// * `nodes` — Root-level parsed nodes, in document order.
///
/// # Returns
///
/// * `String` — The TOML document (tables, array-of-tables and values).
///
/// # Examples
///
/// ```ignore
/// let parsed = ibis2ibstoml::parse_to_parsed("[IBIS ver] 2.1\n")?;
/// let document = ibis2ibstoml::emitter::serialize_parsed_tree(&parsed);
/// assert!(document.contains("ibis_ver = \"2.1\""));
/// ```
pub fn serialize_parsed_tree(nodes: &[ParsedNode]) -> String {
    let mut writer = TomlWriter { document: String::new() };
    writer.write_siblings(nodes, "");
    writer.document
}

// ---------------------------------------------------------------------------
// Document writer — walks the parsed tree and appends tables, fields and blanks
// ---------------------------------------------------------------------------

/// Accumulates the rendered TOML text while walking the parsed tree.
#[derive(Debug)]
struct TomlWriter {
    document: String,  // Growing TOML document, returned by `serialize_parsed_tree`.
}

impl TomlWriter {
    /// Renders a group of sibling nodes under a shared parent path.
    ///
    /// Takes the `nodes` and their `parent_path`; appends one blank line after
    /// every node so the output stays readable.
    fn write_siblings(&mut self, nodes: &[ParsedNode], parent_path: &str) {
        for node in nodes {
            let as_array = Self::is_array_of_tables(nodes, node);
            self.write_node(node, parent_path, as_array);
            self.document.push('\n');
        }
    }

    /// Whether a node must be written as an array-of-tables.
    ///
    /// Takes the sibling `nodes` and one `node`; returns `true` when the schema marks
    /// the keyword as multiple, or when the same keyword repeats among the siblings
    /// (which keeps the output valid TOML even for a flattened tree).
    fn is_array_of_tables(nodes: &[ParsedNode], node: &ParsedNode) -> bool {
        if node.occurrence == Occurrence::Multiple {
            return true;
        }
        let same_keyword_count = nodes
            .iter()
            .filter(|sibling| sibling.keyword == node.keyword)
            .count();
        same_keyword_count > 1
    }

    /// Renders one node: its table header, its fields and then its children.
    ///
    /// Takes the `node`, the `parent_path` of its scope and whether it is written
    /// as `as_array`.
    fn write_node(&mut self, node: &ParsedNode, parent_path: &str, as_array: bool) {
        let segment = text::key_segment(&node.keyword);
        let path = if parent_path.is_empty() {
            segment
        } else {
            format!("{parent_path}.{segment}")
        };

        // ── Virtual container: its entries become plain fields of `[File_Header]` ──
        if node.keyword == FILE_HEADER_CONTAINER {
            self.write_table_header(&path, false);
            for child in &node.children {
                for field in &child.fields {
                    self.write_field(field);
                }
            }
            return;
        }

        self.write_table_header(&path, as_array);
        for field in &node.fields {
            self.write_field(field);
        }
        self.write_siblings(&node.children, &path);
    }

    /// Writes a table header: `[[path]]` when `as_array`, else `[path]`.
    ///
    /// Takes the dotted `path` and whether the node repeats.
    fn write_table_header(&mut self, path: &str, as_array: bool) {
        if as_array {
            let _ = writeln!(self.document, "[[{path}]]");
        } else {
            let _ = writeln!(self.document, "[{path}]");
        }
    }

    /// Renders one `key = value` line.
    ///
    /// Takes the `field`; appends the rendered line.
    fn write_field(&mut self, field: &ParsedField) {
        let key = text::key_segment(&field.key);
        let value = text::render_value(&field.value);
        let _ = writeln!(self.document, "{key} = {value}");
    }
}

// ---------------------------------------------------------------------------
// Text primitives — buffer-free formatters for one value, one array or one key
// ---------------------------------------------------------------------------

/// Pure formatters turning one parsed fragment into its TOML spelling.
///
/// Each function takes the fragment and returns a `String` without writing to a
/// buffer, so [`TomlWriter`](super::TomlWriter) keeps no quoting or escaping detail.
mod text {
    use super::{Corner, ParsedTable, ParsedValue};

    /// Renders one value as TOML.
    ///
    /// Takes the parsed `value`; returns its TOML text.
    pub(super) fn render_value(value: &ParsedValue) -> String {
        match value {
            ParsedValue::Text(text) => quote(text),
            ParsedValue::Corner(corner) => render_corner(corner),
            ParsedValue::Table(table) => render_table(table),
            ParsedValue::Lines(lines) => render_lines(lines),
        }
    }

    /// Renders a corner as a one-row table.
    ///
    /// Takes the `corner`; returns
    /// `{ header = ["typ", "min", "max"], data = ["typ", "min", "max"] }`, which keeps
    /// the corner readable on a single line while using the same shape as any table.
    fn render_corner(corner: &Corner) -> String {
        let header_values = ["typ".to_string(), "min".to_string(), "max".to_string()];
        let header = render_string_array(&header_values);
        let row_values = [corner.0.clone(), corner.1.clone(), corner.2.clone()];
        let data = render_string_array(&row_values);
        format!("{{ header = {header}, data = {data} }}")
    }

    /// Renders a table as an inline table with a multi-line `data` array.
    ///
    /// Takes the `table`; returns `{ header = [...], data = [[...]] }`.
    fn render_table(table: &ParsedTable) -> String {
        let header = render_string_array(&table.header);
        let mut rendered = format!("{{ header = {header}, data = [");
        for row in &table.data {
            rendered.push('\n');
            rendered.push_str("    ");
            rendered.push_str(&render_string_array(row));
            rendered.push(',');
        }
        if !table.data.is_empty() {
            rendered.push('\n');
        }
        rendered.push_str("] }");
        rendered
    }

    /// Renders ordered free-text lines as a TOML string, not a list of bracketed values.
    ///
    /// Takes the `lines`; returns `""` for an empty list, a plain string for a single
    /// line, and a multi-line basic string otherwise:
    ///
    /// ```toml
    /// notes = """
    /// first line
    /// second line
    /// """
    /// ```
    ///
    /// The closing `"""` always sits on a line of its own, below the last text line.
    /// TOML drops the newline right after the opening `"""` and keeps the one before the
    /// closing delimiter, so the value is `lines.join("\n")` plus a trailing newline.
    fn render_lines(lines: &[String]) -> String {
        match lines {
            [] => "\"\"".to_string(),
            [single] => quote(single),
            many => {
                let mut rendered = String::from("\"\"\"\n");
                for line in many {
                    rendered.push_str(&escape_basic(line));
                    rendered.push('\n');
                }
                rendered.push_str("\"\"\"");
                rendered
            }
        }
    }

    /// Renders string values as a single-line TOML array.
    ///
    /// Takes the `values`; returns `["a", "b"]` (or `[]` when empty).
    fn render_string_array(values: &[String]) -> String {
        let quoted: Vec<String> = values.iter().map(|value| quote(value)).collect();
        format!("[{}]", quoted.join(", "))
    }

    /// Renders a key or path segment, quoting it when it is not a bare TOML key.
    ///
    /// Takes the raw `raw` text; returns the bare key when it is alphanumeric with
    /// `_`/`-`, and a quoted key otherwise (`"dv/dt_r"`, `"vinh+"`).
    pub(super) fn key_segment(raw: &str) -> String {
        let is_bare = !raw.is_empty()
            && raw
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-');
        if is_bare {
            raw.to_string()
        } else {
            quote(raw)
        }
    }

    /// Escapes the body of a TOML basic string (single- or multi-line).
    ///
    /// Takes the raw `text`; returns it with backslashes and double quotes escaped,
    /// which is valid in both `"…"` and `"""…"""` forms.
    fn escape_basic(text: &str) -> String {
        text.replace('\\', "\\\\").replace('"', "\\\"")
    }

    /// Wraps text in a TOML basic string, escaping backslashes and quotes.
    ///
    /// Takes the raw `text`; returns the quoted form.
    fn quote(text: &str) -> String {
        format!("\"{}\"", escape_basic(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Corner;

    /// Builds a parsed field for tests.
    fn field(key: &str, value: ParsedValue) -> ParsedField {
        ParsedField { key: key.to_string(), value }
    }

    /// Builds a parsed node for tests.
    fn node(
        keyword: &str,
        occurrence: Occurrence,
        fields: Vec<ParsedField>,
        children: Vec<ParsedNode>,
    ) -> ParsedNode {
        ParsedNode { keyword: keyword.to_string(), occurrence, fields, children }
    }

    /// Parses the rendered document back to make sure it is valid TOML.
    fn assert_valid_toml(document: &str) {
        let parsed = toml::from_str::<toml::Value>(document);
        assert!(parsed.is_ok(), "rendered document is not valid TOML: {parsed:?}\n{document}");
    }

    #[test]
    fn test_renders_file_header_entries_as_flat_fields() {
        let header = node(
            "File_Header",
            Occurrence::Once,
            Vec::new(),
            vec![
                node(
                    "IBIS_Ver",
                    Occurrence::Once,
                    vec![field("ibis_ver", ParsedValue::Text("2.1".into()))],
                    Vec::new(),
                ),
                node(
                    "Notes",
                    Occurrence::Once,
                    vec![field(
                        "notes",
                        ParsedValue::Lines(vec!["first line".into(), "second line".into()]),
                    )],
                    Vec::new(),
                ),
            ],
        );

        let document = serialize_parsed_tree(&[header]);

        assert!(document.starts_with("[File_Header]\n"), "{document}");
        assert!(document.contains("ibis_ver = \"2.1\""), "{document}");
        assert!(document.contains("notes = \"\"\"\nfirst line\nsecond line\n\"\"\""), "{document}");
        assert_valid_toml(&document);
    }

    #[test]
    fn test_renders_sections_as_tables_and_arrays_of_tables() {
        let component = node(
            "Component",
            Occurrence::Multiple,
            vec![field("component", ParsedValue::Text("MyChip".into()))],
            vec![
                node(
                    "Package",
                    Occurrence::Once,
                    vec![field(
                        "r_pkg",
                        ParsedValue::Corner(Corner("250.0m".into(), "225.0m".into(), "275.0m".into())),
                    )],
                    Vec::new(),
                ),
                node(
                    "Pin",
                    Occurrence::Once,
                    vec![field(
                        "pin",
                        ParsedValue::Table(ParsedTable {
                            header: vec!["signal_name".into(), "model_name".into()],
                            data: vec![vec!["PA0".into(), "IO8TC".into()]],
                        }),
                    )],
                    Vec::new(),
                ),
            ],
        );

        let document = serialize_parsed_tree(&[component]);

        assert!(document.contains("[[Component]]"), "{document}");
        assert!(document.contains("component = \"MyChip\""), "{document}");
        assert!(document.contains("[Component.Package]"), "{document}");
        assert!(
            document.contains(
                "r_pkg = { header = [\"typ\", \"min\", \"max\"], data = [\"250.0m\", \"225.0m\", \"275.0m\"] }"
            ),
            "{document}"
        );
        assert!(document.contains("[Component.Pin]"), "{document}");
        assert!(
            document.contains("pin = { header = [\"signal_name\", \"model_name\"], data = ["),
            "{document}"
        );
        assert!(document.contains("[\"PA0\", \"IO8TC\"],"), "{document}");
        assert_valid_toml(&document);
    }

    #[test]
    fn test_repeated_keyword_without_schema_occurrence_still_becomes_an_array() {
        let first = node(
            "Manufacturer",
            Occurrence::Once,
            vec![field("manufacturer", ParsedValue::Text("Acme".into()))],
            Vec::new(),
        );
        let second = node(
            "Manufacturer",
            Occurrence::Once,
            vec![field("manufacturer", ParsedValue::Text("Bravo".into()))],
            Vec::new(),
        );

        let document = serialize_parsed_tree(&[first, second]);

        assert_eq!(document.matches("[[Manufacturer]]").count(), 2, "{document}");
        assert_valid_toml(&document);
    }

    #[test]
    fn test_symbolic_keys_are_quoted_with_the_schema_spelling() {
        let ramp = node(
            "Ramp",
            Occurrence::Once,
            vec![
                field(
                    "dv/dt_r",
                    ParsedValue::Corner(Corner("1.9".into(), "1.1".into(), "2.0".into())),
                ),
                field("vinh+", ParsedValue::Text("3.3".into())),
                field("r_load", ParsedValue::Text("1k".into())),
            ],
            Vec::new(),
        );

        let document = serialize_parsed_tree(&[ramp]);

        assert!(
            document.contains(
                "\"dv/dt_r\" = { header = [\"typ\", \"min\", \"max\"], data = [\"1.9\", \"1.1\", \"2.0\"] }"
            ),
            "{document}"
        );
        assert!(document.contains("\"vinh+\" = \"3.3\""), "{document}");
        assert!(document.contains("r_load = \"1k\""), "{document}");
        assert_valid_toml(&document);
    }

    #[test]
    fn test_empty_table_keeps_a_plain_header() {
        let section = node("Node_Declarations", Occurrence::Once, Vec::new(), Vec::new());

        let document = serialize_parsed_tree(&[section]);

        assert_eq!(document.trim(), "[Node_Declarations]");
        assert_valid_toml(&document);
    }

    #[test]
    fn test_lines_become_a_multi_line_string_with_the_closing_delimiter_below() {
        let notes = node(
            "Notes",
            Occurrence::Once,
            vec![field(
                "notes",
                ParsedValue::Lines(vec!["he said \"hi\"".into(), "path C:\\tmp".into()]),
            )],
            Vec::new(),
        );

        let document = serialize_parsed_tree(&[notes]);

        // The closing `"""` owns its line instead of trailing the last text line.
        assert_eq!(
            document.trim(),
            "[Notes]\nnotes = \"\"\"\nhe said \\\"hi\\\"\npath C:\\\\tmp\n\"\"\""
        );
        assert_valid_toml(&document);

        let reparsed: toml::Value = toml::from_str(&document).expect("valid TOML");
        let value = reparsed.get("Notes").and_then(|table| table.get("notes"));
        assert_eq!(
            value.and_then(|value| value.as_str()),
            Some("he said \"hi\"\npath C:\\tmp\n")
        );
    }

    #[test]
    fn test_a_single_line_stays_a_plain_string() {
        let source = node(
            "Source",
            Occurrence::Once,
            vec![field("source", ParsedValue::Lines(vec!["Acme Corp".into()]))],
            Vec::new(),
        );

        let document = serialize_parsed_tree(&[source]);

        assert_eq!(document.trim(), "[Source]\nsource = \"Acme Corp\"");
        assert_valid_toml(&document);
    }
}
