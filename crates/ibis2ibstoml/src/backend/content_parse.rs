// =============================================================================
// content_parse — second phase: turn every `content` line into typed fields
//
// The AST keeps the text after a keyword verbatim in `content`. This phase reads
// that text against the schema and produces, for every node:
//
//   - the keyword's own value, keyed by the snake-cased keyword name
//     (`[Manufacturer] STMicro` → `manufacturer = "STMicro"`);
//   - one field per declared `__param__`, typed as text / quantity / corner;
//   - a `{ header, data }` table for row-oriented sections;
//   - an ordered line list for content no declaration accounts for.
//
// Those values are deliberately text-only:
//
//   - text / quantity → the original text (`I/O`, `1.65V`, `1.9/597p`);
//   - corner          → a `(typ, min, max)` tuple;
//   - table           → `{ header = [...], data = [[...]] }`;
//   - lines           → an ordered list of free-text lines.
//
// Quantities keep their unit exactly as written: no normalization, no scaling.
//
// The decision order is content-driven first, schema-assisted second: the schema
// tells which params exist and how many tokens each consumes, while the shape of
// the text decides what the keyword's own value is. `__header__.format` is only a
// weak hint — real files disagree with it often enough to matter.
//
// `NA` and empty tokens are missing: named params carrying them are dropped, while
// table cells keep their text so no row information is lost.
//
// Design — one traversal object, one shared ledger, then one unit per schema axis:
//
//   1. `NodeParser::parse` resolves the node against its schema spec (preprocess already
//      resolved it, so this only re-reads the mark);
//   2. it reads the node's `content` lines and hands them to the body parser the
//      declared `format` selects, which turns tokens into `ParsedValue` through the
//      declared param type.
//
// The file is ordered by rank, not by call direction: the traversal object
// `NodeParser` and its shared ledger `ParamCollector` come first, the entry point that
// starts the traversal follows, then the units those drive — `parse_format` for the
// `format` axis, `parse_value` for the `param_type` axis — and finally `helper`, which
// does the flat line and token work for both. A new format is therefore one body parser
// plus one match arm, and a new param type one arm of
// `parse_value::parse_param_value`. The frontend stays structure-only: every semantic
// reading of a value (corner extraction, quantity typing, column naming) happens here.
//
// `__schema__.required` is not consumed by this phase: it says whether a keyword
// must appear, which is a validation concern, not a parsing one.
//
// File layout:
//
//   [1] data structures         — the parsed tree types and their constructors;
//   [2] node parser             — the traversal object: resolve, recurse, dispatch;
//   [3] stateful helpers        — `ParamCollector`, the param / unmatched ledger;
//   [4] pipeline entry          — the minimal traversal driver;
//   [5] format parsing          — `mod parse_format`: `format` → a node body's fields;
//   [6] value reading           — `mod parse_value`: typed values and token primitives;
//   [7] flat helpers            — `mod helper`: line, token, table and field utils.
// =============================================================================

//! Second phase of the backend: schema-driven parsing of every `content` member.
//!
//! [`content_parse`] is the entry point; it drives [`NodeParser`], whose `parse` dispatch
//! selects a body parser from [`parse_format`]. Single values are typed by
//! [`parse_value`] — where `quantity` is intentionally *not* a variant, since a
//! quantity is emitted as its original text — and the flat line / token utilities live
//! in [`helper`].

use crate::frontend::SectionNode;
use crate::schema::{
    find_field, normalize_keyword, to_snake_key, Occurrence, ParamType, SectionFormat, SectionSpec,
};

use super::pre_process::{resolve_spec, KeywordMark};

use helper::{collect_content_lines, split_field_line};
use parse_format::{parse_file_header_body, parse_table_body, parse_text_body, parse_vt_table_body};
use parse_value::parse_param_value;

// Re-exported for the validation phase, which reads the same table cells to check
// monotonicity, density and magnitude.
pub(crate) use parse_value::split_number_unit;

// =============================================================================
// [1] Data Structures & Constructors
// =============================================================================

/// Key used for content lines that no schema declaration accounts for.
///
pub const UNMATCHED_LINES_KEY: &str = "lines";
pub const IV_COLUMN_NAMES: [&str; 4] = ["Voltage", "I(typ)", "I(min)", "I(max)"];
pub const VT_COLUMN_NAMES: [&str; 4] = ["Time", "V(typ)", "V(min)", "V(max)"];

/// A corner triple in IBIS order: typical, minimum, maximum.
///
/// Each element keeps the original text (`"3.3000V"`); an absent or `NA` element is
/// stored as an empty string.
#[derive(Debug, Clone, PartialEq)]
pub struct Corner(
    pub String, // Typical element, kept as written.
    pub String, // Minimum element, kept as written.
    pub String, // Maximum element, kept as written.
);

/// A row-oriented table: the column names plus the data rows.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedTable {
    pub header: Vec<String>,   // Column names; empty when the schema declares no source.
    pub data: Vec<Vec<String>>, // Cell text in column order; a short row means trailing cells were absent.
}

/// The value carried by one parsed field.
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedValue {
    Text(String),       // Plain text: identifiers, enumerations and quantities as written.
    Corner(Corner),     // Corner triple `(typ, min, max)`.
    Table(ParsedTable), // Row-oriented table (`{ header = [], data = [] }`).
    Lines(Vec<String>), // Ordered free-text lines.
}

/// One typed field of a parsed node.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedField {
    pub key: String,        // Output key: the keyword's own snake key or a param name.
    pub value: ParsedValue, // Typed value of the field.
}

impl ParsedField {
    /// Builds the field holding a node's own value, keyed by its snake-cased keyword.
    fn self_value(spec: &SectionSpec, value: ParsedValue) -> ParsedField {
        ParsedField { key: to_snake_key(&spec.name), value }
    }

    /// Builds the `lines` field that keeps content no declaration accounts for.
    fn unmatched(lines: Vec<String>) -> ParsedField {
        ParsedField { key: UNMATCHED_LINES_KEY.to_string(), value: ParsedValue::Lines(lines) }
    }
}

/// One parsed keyword node — the AST node with its content turned into fields.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedNode {
    pub keyword: String,           // Canonical schema name; the raw keyword when unknown.
    pub occurrence: Occurrence,    // Single (`[X]`) or multiple (`[[X]]`) inside its parent.
    pub fields: Vec<ParsedField>,  // Keyword's own value (if any), then its named params.
    pub children: Vec<ParsedNode>, // Child sections in source order.
}

impl ParsedNode {
    /// Looks up one field of this node by key.
    ///
    /// Takes the `key` to find; returns the matching field when present.
    pub fn field(&self, key: &str) -> Option<&ParsedField> {
        self.fields.iter().find(|field| field.key == key)
    }
}

// =============================================================================
// [2] Node Parser
// =============================================================================

/// One node of the traversal, with everything its parsing needs.
struct NodeParser<'a> {
    node: &'a SectionNode,                     // AST node whose content is being read.
    parent_spec: Option<&'static SectionSpec>, // Schema scope of the parent, used for lookup.
    marks: &'a [KeywordMark],                  // Preprocess marks, in the shared pre-order.
    cursor: &'a mut usize,                     // Position in `marks`; advances in pre-order.
}

impl<'a> NodeParser<'a> {
    /// Creates the parser of one AST `node` inside a schema scope.
    fn new(
        node: &'a SectionNode,
        parent_spec: Option<&'static SectionSpec>,
        marks: &'a [KeywordMark],
        cursor: &'a mut usize,
    ) -> NodeParser<'a> {
        NodeParser { node, parent_spec, marks, cursor }
    }

    /// Parses the node: resolve its spec, read its body, then recurse into children.
    fn parse(&mut self) -> ParsedNode {
        // ── 1. Schema lookup: the preprocess mark wins when it names the same keyword,
        //    and consuming it advances the shared cursor; otherwise the schema is read
        //    directly and the cursor stays put ──
        let normalized_keyword = normalize_keyword(&self.node.keyword);
        let matching_mark = self
            .marks
            .get(*self.cursor)
            .filter(|mark| normalize_keyword(&mark.keyword) == normalized_keyword);
        let spec = match matching_mark {
            Some(mark) => {
                *self.cursor += 1;
                mark.spec.or_else(|| resolve_spec(&self.node.keyword, self.parent_spec))
            }
            None => resolve_spec(&self.node.keyword, self.parent_spec),
        };

        // ── 2. Content mapping: `__schema__.format` selects the body parser, while a
        //    keyword the schema does not know keeps its lines verbatim ──
        let lines = collect_content_lines(self.node);
        let fields = match spec {
            Some(section) if !lines.is_empty() => match section.format {
                SectionFormat::FileHeader => parse_file_header_body(section, &lines),
                SectionFormat::Text => parse_text_body(section, &lines),
                SectionFormat::TextLines => {
                    vec![ParsedField::self_value(section, ParsedValue::Lines(lines))]
                }
                SectionFormat::Table | SectionFormat::IvTable => parse_table_body(section, &lines),
                SectionFormat::VtTable => parse_vt_table_body(section, &lines),
            },
            Some(_) => Vec::new(),
            None if !lines.is_empty() => vec![ParsedField::unmatched(lines)],
            None => Vec::new(),
        };

        // ── 3. Children: same traversal order, so the shared cursor stays aligned ──
        let mut children: Vec<ParsedNode> = Vec::with_capacity(self.node.children.len());
        for child in &self.node.children {
            let mut child_parser = NodeParser::new(child, spec, self.marks, &mut *self.cursor);
            children.push(child_parser.parse());
        }

        let (keyword, occurrence) = match spec {
            Some(section) => (section.name.clone(), section.occurrence.clone()),
            None => (self.node.keyword.clone(), Occurrence::Once),
        };

        ParsedNode { keyword, occurrence, fields, children }
    }
}

// =============================================================================
// [3] Stateful Helpers
// =============================================================================

/// Splits the content lines of one section into params and unmatched lines.
///
/// The text-shaped bodies share this bookkeeping: a line naming a declared
/// `__param__` contributes its typed field plus whatever text the param did not
/// consume, while every other line is kept verbatim so no row information is lost.
struct ParamCollector<'spec> {
    spec: &'spec SectionSpec,       // Declared params the collector recognizes.
    param_fields: Vec<ParsedField>, // Typed fields of the declared params found so far.
    unmatched_lines: Vec<String>,   // Lines no declaration accounted for, in source order.
}

impl<'spec> ParamCollector<'spec> {
    /// Creates a collector for the declared params of one section.
    fn new(spec: &'spec SectionSpec) -> ParamCollector<'spec> {
        ParamCollector { spec, param_fields: Vec::new(), unmatched_lines: Vec::new() }
    }

    /// Feeds one content line to the collector.
    ///
    /// Takes the `line`; a declared param line yields its field, every other is kept.
    fn collect(&mut self, line: &str) {
        let Some((key, tokens)) = split_field_line(line) else {
            self.unmatched_lines.push(line.to_string());
            return;
        };
        let Some(field_spec) = find_field(self.spec, &key) else {
            self.unmatched_lines.push(line.to_string());
            return;
        };
        let Some(value) = parse_param_value(&field_spec.param_type, &tokens) else {
            self.unmatched_lines.push(line.to_string());
            return;
        };

        // The declared type says how many tokens the value consumes; anything the line
        // carries beyond them is kept, so no information is lost.
        let consumed = match field_spec.param_type {
            ParamType::Text => tokens.len(),
            ParamType::Quantity => 1.min(tokens.len()),
            ParamType::Corner => 3.min(tokens.len()),
        };
        let leftover = tokens[consumed..].join(" ");
        self.param_fields.push(ParsedField { key: field_spec.key.clone(), value });
        if !leftover.is_empty() {
            self.unmatched_lines.push(leftover);
        }
    }

    /// Splits the collector into what it gathered.
    ///
    /// Takes no argument; returns the param fields plus the unmatched lines.
    fn into_parts(self) -> (Vec<ParsedField>, Vec<String>) {
        (self.param_fields, self.unmatched_lines)
    }
}

// =============================================================================
// [4] Pipeline Entry
// =============================================================================

/// Parses every node of the AST into typed fields.
///
/// # Parameters
///
/// * `tree` — Root-level sections produced by the frontend.
/// * `marks` — Marks from the preprocess phase, consumed in the same pre-order.
///
/// # Returns
///
/// * `Vec<ParsedNode>` — The parsed tree, mirroring the AST structure.
pub fn content_parse(tree: &[SectionNode], marks: &[KeywordMark]) -> Vec<ParsedNode> {
    // One shared cursor: the marks are consumed exactly once, in the same pre-order
    // as the traversal below.
    let mut cursor = 0;
    tree.iter()
        .map(|node| NodeParser::new(node, None, marks, &mut cursor).parse())
        .collect()
}

// =============================================================================
// [5] Format Parsing
// =============================================================================

/// `format` → fields: one body parser per `__schema__.format` value.
///
/// Each function reads the cleaned lines of one node body and returns its fields,
/// mirroring [`parse_value`] on the format axis. The text-shaped bodies share their
/// bookkeeping through [`ParamCollector`]; the table-shaped ones go through
/// [`helper`].
mod parse_format {
    use crate::schema::{find_field, HeaderFormat, SectionSpec};

    use super::{ParamCollector, ParsedField, ParsedTable, ParsedValue, VT_COLUMN_NAMES};

    use super::helper::{
        resolve_table_header_and_rows, split_field_line, split_header_line, split_tokens,
    };
    use super::parse_value::parse_self_value;

    // -------------------------- parse strategies --------------------------

    /// `format = "file_header"` — one entry of the file header (`[IBIS ver] 2.1`).
    ///
    /// Takes the header `spec` and the cleaned `lines`; returns the entry's field.
    pub(super) fn parse_file_header_body(spec: &SectionSpec, lines: &[String]) -> Vec<ParsedField> {
        // A multi-line entry (`[Notes] …`) keeps its lines verbatim.
        if spec.header_format == HeaderFormat::TextLines {
            return vec![ParsedField::self_value(spec, ParsedValue::Lines(lines.to_vec()))];
        }

        let (header_text, data_lines) = split_header_line(lines);
        let Some(header) = header_text else {
            return Vec::new();
        };
        if header.is_empty() {
            return Vec::new();
        }

        let mut fields: Vec<ParsedField> = Vec::new();
        fields.push(ParsedField::self_value(spec, ParsedValue::Text(header.to_string())));
        if !data_lines.is_empty() {
            fields.push(ParsedField::unmatched(data_lines.to_vec()));
        }
        fields
    }

    /// `format = "text"` — a single record (`[Model]`, `[Package]`, `[Ramp]`, …).
    ///
    /// Takes the `spec` and the cleaned `lines`; returns the keyword's own value plus
    /// every named param found in the text.
    pub(super) fn parse_text_body(spec: &SectionSpec, lines: &[String]) -> Vec<ParsedField> {
        let (header_text, data_lines) = split_header_line(lines);
        let mut collector = ParamCollector::new(spec);

        // ── The keyword line may itself carry a param
        //    (`[Receiver Thresholds] Vth = 0.9V`); when it does not, it stays the
        //    keyword's own value instead of becoming unmatched content ──
        let header_is_param_line = header_text.is_some_and(|header| {
            split_field_line(header).is_some_and(|(key, _)| find_field(spec, &key).is_some())
        });
        if header_is_param_line {
            collector.collect(header_text.expect("the check above implies a keyword line"));
        }

        // ── Remaining lines: named params, otherwise unmatched content ──
        for line in data_lines {
            collector.collect(line);
        }

        let (param_fields, unmatched_lines) = collector.into_parts();
        let mut fields: Vec<ParsedField> = Vec::new();

        // ── The keyword's own value, unless the keyword line was itself a param line ──
        if let Some(header) = header_text
            && !header_is_param_line
        {
            let own_value = if spec.fields.is_empty() {
                parse_self_value(spec, header)
            } else {
                ParsedValue::Text(header.to_string())
            };
            fields.push(ParsedField::self_value(spec, own_value));
        }

        fields.extend(param_fields);
        if !unmatched_lines.is_empty() {
            fields.push(ParsedField::unmatched(unmatched_lines));
        }
        fields
    }

    /// `format = "table"` / `"IV-table"` — row-oriented data.
    ///
    /// Column names come from the schema when the section declares params, and from
    /// the source header row otherwise (an empty header row when neither exists).
    ///
    /// Takes the `spec` and the cleaned `lines`; returns the table field.
    pub(super) fn parse_table_body(spec: &SectionSpec, lines: &[String]) -> Vec<ParsedField> {
        // A row-oriented section whose manual header line leaves the leading identifier
        // column unnamed declares it first in `ibis_schema.toml` as `col0`
        // (`[Pin]`, `[Diff Pin]`, `[Pin Mapping]`, `[Pin EMI]`, …), so the declared
        // column names always line up with the data cells.
        // `header = "text"` means the keyword line carries the node name instead of
        // column names (`[Model Selector] <name>`): the name becomes a field and the
        // remaining lines stay an ordered list until the schema names them.
        if spec.header_format == HeaderFormat::Text {
            let (keyword_text, body_lines) = split_header_line(lines);
            let mut fields: Vec<ParsedField> = Vec::new();
            if let Some(header) = keyword_text
                && !header.is_empty()
            {
                fields.push(ParsedField::self_value(spec, ParsedValue::Text(header.to_string())));
            }
            if !body_lines.is_empty() {
                fields.push(ParsedField::unmatched(body_lines.to_vec()));
            }
            return fields;
        }

        let (header_names, data_lines) = resolve_table_header_and_rows(spec, lines);
        let data: Vec<Vec<String>> = data_lines.iter().map(|line| split_tokens(line)).collect();
        let table = ParsedTable { header: header_names, data };
        vec![ParsedField::self_value(spec, ParsedValue::Table(table))]
    }

    /// `format = "VT-table"` — fixture params followed by waveform rows.
    ///
    /// Lines that name a declared param become fields (`R_fixture = 50.000`); every
    /// other line is a waveform row and is collected into a table with the conventional
    /// columns (`Time`, `V(typ)`, `V(min)`, `V(max)`).
    ///
    /// Takes the `spec` and the cleaned `lines`; returns the param fields followed by
    /// the waveform table, keyed by the keyword's own snake key.
    pub(super) fn parse_vt_table_body(spec: &SectionSpec, lines: &[String]) -> Vec<ParsedField> {
        let mut collector = ParamCollector::new(spec);

        // ── Fixture params become fields; every other line is a waveform row ──
        for line in lines {
            collector.collect(line);
        }

        let (mut fields, waveform_rows) = collector.into_parts();
        if !waveform_rows.is_empty() {
            let table = ParsedTable {
                header: VT_COLUMN_NAMES.iter().map(|name| name.to_string()).collect(),
                data: waveform_rows.iter().map(|row| split_tokens(row)).collect(),
            };
            fields.push(ParsedField::self_value(spec, ParsedValue::Table(table)));
        }
        fields
    }
}


// =============================================================================
// [6] Value Reading
// =============================================================================

/// `param_type` → `ParsedValue`: the readers that type one value of one line.
///
/// The three declared value types share the rule that a missing value (`NA` or no
/// token at all) never becomes a value, so "missing" is decided once per type here
/// and the callers only see `Option`.
mod parse_value {
    use crate::schema::{HeaderFormat, ParamType, SectionSpec};

    use super::helper::split_tokens;
    use super::{Corner, ParsedValue};

    // -------------------------- numeric primitives --------------------------

    /// Whether a token is an IBIS missing value.
    ///
    /// Takes a raw `token`; returns `true` for an empty or `NA` token (case-insensitive).
    pub(super) fn is_missing_token(token: &str) -> bool {
        let trimmed = token.trim();
        trimmed.is_empty() || trimmed.eq_ignore_ascii_case("NA")
    }

    /// Splits a token into its numeric mantissa and its raw unit suffix.
    ///
    /// Takes a raw `token`; returns `Some((value, unit))` when the token is a number
    /// with an optional unit suffix (`2.69p` → `(2.69, Some("p"))`, `3.3` → `(3.3,
    /// None)`), and `None` otherwise. No scaling is applied: the unit is kept verbatim.
    pub(crate) fn split_number_unit(token: &str) -> Option<(f64, Option<String>)> {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            return None;
        }

        let bytes = trimmed.as_bytes();
        let mut index = 0;

        // ── Optional sign ──
        let has_sign = bytes[index] == b'+' || bytes[index] == b'-';
        if has_sign {
            index += 1;
        }

        // ── Mantissa: integer part and optional fractional part ──
        let mut has_digit = false;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
            has_digit = true;
        }
        if index < bytes.len() && bytes[index] == b'.' {
            index += 1;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
                has_digit = true;
            }
        }
        if !has_digit {
            return None;
        }

        // ── Optional exponent ──
        if index < bytes.len() && (bytes[index] == b'e' || bytes[index] == b'E') {
            index += 1;
            if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') {
                index += 1;
            }
            let exponent_start = index;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            if index == exponent_start {
                return None;
            }
        }

        // The tail after the mantissa must be a unit: either nothing, or a short
        // alphabetic prefix / unit (`V`, `mA`, `nH`, `Ohm`). This is what keeps a date
        // such as `12-08-2024` from being mistaken for a quantity.
        let mantissa = &trimmed[..index];
        let suffix = &trimmed[index..];
        let is_unit = suffix.is_empty()
            || (suffix.len() <= 4
                && suffix.chars().all(|character| character.is_ascii_alphabetic()));
        if !is_unit {
            return None;
        }

        let value: f64 = mantissa.parse().ok()?;
        let unit = if suffix.is_empty() {
            None
        } else {
            Some(suffix.to_string())
        };
        Some((value, unit))
    }

    /// Whether a token is a physical quantity.
    ///
    /// Takes a raw `token`; returns `true` for a plain number with optional unit
    /// (`0.8`, `-2mA`, `0.1250k`) and for an IBIS ratio of two such numbers
    /// (`1.9/597p`). Missing tokens and plain identifiers return `false`.
    pub(super) fn is_quantity_token(token: &str) -> bool {
        let trimmed = token.trim();
        if is_missing_token(trimmed) {
            return false;
        }
        if let Some((numerator, denominator)) = trimmed.split_once('/') {
            let numerator_is_quantity = is_quantity_token(numerator);
            let denominator_is_quantity = is_quantity_token(denominator);
            return numerator_is_quantity && denominator_is_quantity;
        }
        split_number_unit(trimmed).is_some()
    }

    // -------------------------- param type readers --------------------------

    /// Converts the tokens after a param key into the type `__param__` declares.
    ///
    /// Takes the declared `param_type` and the `tokens`; returns the typed value, or
    /// `None` when the value is missing (`NA`, or no token at all).
    pub(super) fn parse_param_value(
        param_type: &ParamType,
        tokens: &[String],
    ) -> Option<ParsedValue> {
        match param_type {
            // `text` — everything after the key is the value.
            ParamType::Text => {
                let joined = tokens.join(" ");
                (!is_missing_token(&joined)).then_some(ParsedValue::Text(joined))
            }
            // `quantity` — one physical quantity (`1.65V`, `10mA`).
            ParamType::Quantity => {
                let first_token = tokens.first()?;
                (!is_missing_token(first_token)).then(|| ParsedValue::Text(first_token.clone()))
            }
            // `corner` — a `typ min max` triple, missing when every token is missing.
            ParamType::Corner => {
                let all_missing = tokens.iter().all(|token| is_missing_token(token));
                if tokens.is_empty() || all_missing {
                    return None;
                }
                Some(ParsedValue::Corner(build_corner(tokens)))
            }
        }
    }

    /// Decides the keyword's own value for a param-less section.
    ///
    /// Takes the `spec` and the keyword-line `header`; returns a corner tuple when the
    /// header holds three quantities (or the schema declares a corner header), and the
    /// header text otherwise.
    pub(super) fn parse_self_value(spec: &SectionSpec, header: &str) -> ParsedValue {
        let tokens = split_tokens(header);
        let carries_three_quantities =
            tokens.len() >= 3 && tokens.iter().take(3).all(|token| is_quantity_token(token));
        let looks_like_corner =
            spec.header_format == HeaderFormat::Corner || carries_three_quantities;
        if looks_like_corner {
            ParsedValue::Corner(build_corner(&tokens))
        } else {
            ParsedValue::Text(header.to_string())
        }
    }

    /// Builds a corner triple from the leading tokens of a line.
    ///
    /// Takes the candidate `tokens`; returns the triple with missing elements as empty
    /// strings (up to three tokens are consumed).
    fn build_corner(tokens: &[String]) -> Corner {
        let mut values: [String; 3] = [String::new(), String::new(), String::new()];
        for (position, token) in tokens.iter().take(3).enumerate() {
            if !is_missing_token(token) {
                values[position] = token.clone();
            }
        }
        let [typical, minimum, maximum] = values;
        Corner(typical, minimum, maximum)
    }
}

// =============================================================================
// [7] Flat Helpers
// =============================================================================

/// Line, token, table and field helpers — the flat layer under the stages above.
///
/// Nothing here knows about a node or a schema scope beyond what it is handed: the
/// functions take lines, tokens or a spec and return plain data, which keeps the
/// traversal code free of string surgery.
mod helper {
    use crate::frontend::SectionNode;
    use crate::schema::{find_field, HeaderFormat, SectionFormat, SectionSpec};

    use super::parse_value::is_quantity_token;
    use super::IV_COLUMN_NAMES;

    // -------------------------- line & token iteration --------------------------

    /// Reads the non-empty content lines of a node with inline comments stripped.
    ///
    /// Takes the AST `node`; returns its cleaned lines.
    pub(super) fn collect_content_lines(node: &SectionNode) -> Vec<String> {
        node.content
            .iter()
            .map(|raw_line| match raw_line.split_once('|') {
                Some((before_comment, _)) => before_comment.trim(),
                None => raw_line.trim(),
            })
            .filter(|cleaned| !cleaned.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// Splits a line into whitespace-separated tokens.
    ///
    /// Takes a `line`; returns its tokens in source order.
    pub(super) fn split_tokens(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    /// Splits a field line into its key and value tokens.
    ///
    /// Accepts both `Key value…` and `Key = value…` spellings, and strips the quotes a
    /// key such as `"dv/dt_r"` needs. Takes a `line`; returns the key with its tokens.
    pub(super) fn split_field_line(line: &str) -> Option<(String, Vec<String>)> {
        let mut tokens = split_tokens(line).into_iter();
        let first_token = tokens.next()?;
        let key = first_token.trim_end_matches('=').trim_matches('"').to_string();
        if key.is_empty() {
            return None;
        }
        let mut values: Vec<String> = tokens.collect();
        if values.first().map(String::as_str) == Some("=") {
            values.remove(0);
        }
        Some((key, values))
    }

    /// Splits cleaned content lines into the keyword-line text and the data lines.
    ///
    /// Takes the cleaned `lines`; returns the optional keyword-line text plus the rest.
    pub(super) fn split_header_line(lines: &[String]) -> (Option<&str>, &[String]) {
        match lines.split_first() {
            Some((header, rest)) => (Some(header.as_str()), rest),
            None => (None, &[]),
        }
    }

    // -------------------------- table layout --------------------------

    /// Resolves a table's column names and the lines that carry data.
    ///
    /// Detection order:
    ///
    ///   0. `format = "IV-table"` — an I-V curve keeps its conventional columns
    ///      (`Voltage`, `I(typ)`, `I(min)`, `I(max)`);
    ///   1. the declared `__param__` names — the schema wins when it lists columns, and
    ///      columns the source omits (for example `col0` of `[Series Pin Mapping]`) are
    ///      still named;
    ///   2. the text on the keyword line, when the schema marks the header as
    ///      `table_header` and that text is not pure data;
    ///   3. the first content line, when the keyword line carries no text and that line
    ///      is not pure data;
    ///   4. otherwise the table has no column names (`header = []`).
    ///
    /// A leading line adopted as a column-name row is excluded from the data rows.
    /// Column names that live only in `|` comments stay unavailable: the frontend drops
    /// comment lines before the backend ever sees them.
    ///
    /// Takes the `spec` and the cleaned `lines`; returns the column names plus the data
    /// lines.
    pub(super) fn resolve_table_header_and_rows<'a>(
        spec: &SectionSpec,
        lines: &'a [String],
    ) -> (Vec<String>, &'a [String]) {
        let declared: Vec<String> = spec.fields.iter().map(|field| field.key.clone()).collect();
        let (keyword_text, body_lines) = split_header_line(lines);
        let expects_column_row = spec.header_format == HeaderFormat::TableHeader;

        // ── Rule 2: the keyword line may already name the columns ──
        let mut keyword_row: Option<&str> = None;
        if expects_column_row
            && let Some(text) = keyword_text
            && !text.is_empty()
            && is_column_header_row(spec, text)
        {
            keyword_row = Some(text);
        }

        // ── Rule 3: otherwise the first content line may name the columns ──
        let mut body_header_row: Option<&str> = None;
        if expects_column_row
            && keyword_row.is_none()
            && let Some(first_line) = body_lines.first()
            && is_column_header_row(spec, first_line)
        {
            body_header_row = Some(first_line.as_str());
        }

        let data_lines: &[String] = if keyword_row.is_some() {
            body_lines
        } else if body_header_row.is_some() {
            &body_lines[1..]
        } else {
            lines
        };

        // ── Rule 0: an I-V curve keeps its conventional column names ──
        // ── Rule 1: declared params win over names taken from the text ──
        let columns = if spec.format == SectionFormat::IvTable {
            IV_COLUMN_NAMES.iter().map(|name| name.to_string()).collect()
        } else if !declared.is_empty() {
            declared
        } else if let Some(text) = keyword_row {
            split_tokens(text)
        } else if let Some(first_line) = body_header_row {
            split_tokens(first_line)
        } else {
            Vec::new()
        };

        (columns, data_lines)
    }

    /// Whether a leading line names the table's columns instead of carrying data.
    ///
    /// When the section declares params, the line is a column-name row only if every
    /// one of its tokens is a declared column (`[Pin] signal_name model_name R_pin …`).
    /// A row such as `50ohm_ODT_800 Non-Driving` therefore stays data. Without declared
    /// columns the weaker test applies: any line that is not pure numeric data is taken
    /// as the column-name row, which is all a curve section can offer.
    ///
    /// Takes the `spec` and the candidate `line`; returns `true` for a column-name row.
    fn is_column_header_row(spec: &SectionSpec, line: &str) -> bool {
        let tokens = split_tokens(line);
        if tokens.is_empty() {
            return false;
        }
        if spec.fields.is_empty() {
            return tokens.iter().all(|token| !is_quantity_token(token));
        }
        tokens.iter().all(|token| find_field(spec, token).is_some())
    }

}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use super::parse_value::{is_missing_token, is_quantity_token, split_number_unit};
    use crate::backend::pre_process::mark_keywords;
    use crate::backend::ValidationCollector;
    use crate::frontend::NodeKind;

    /// Builds an AST node for tests.
    fn section(keyword: &str, content: Vec<&str>, children: Vec<SectionNode>) -> SectionNode {
        SectionNode {
            keyword: keyword.to_string(),
            kind: NodeKind::Regular,
            content: content.into_iter().map(str::to_string).collect(),
            children,
        }
    }

    /// Runs preprocess plus content_parse over a test tree.
    fn parse_tree(tree: &[SectionNode]) -> Vec<ParsedNode> {
        let mut collector = ValidationCollector::new();
        let marks = mark_keywords(tree, &mut collector);
        assert!(collector.errors.is_empty(), "unexpected: {:?}", collector.errors);
        content_parse(tree, &marks)
    }

    #[test]
    fn test_keyword_line_value_is_keyed_by_the_keyword() {
        let tree = vec![section(
            "Component",
            vec!["STM32F103C8"],
            vec![section("Manufacturer", vec!["STMicroelectronics NV"], vec![])],
        )];
        let parsed = parse_tree(&tree);

        let component = &parsed[0];
        assert_eq!(component.keyword, "Component");
        assert_eq!(component.occurrence, Occurrence::Multiple);
        assert_eq!(
            component.field("component"),
            Some(&ParsedField { key: "component".into(), value: ParsedValue::Text("STM32F103C8".into()) })
        );

        let manufacturer = &component.children[0];
        assert_eq!(manufacturer.keyword, "Manufacturer");
        assert_eq!(
            manufacturer.field("manufacturer"),
            Some(&ParsedField {
                key: "manufacturer".into(),
                value: ParsedValue::Text("STMicroelectronics NV".into()),
            })
        );
    }

    #[test]
    fn test_package_corner_params() {
        let tree = vec![section(
            "Component",
            vec!["MyChip"],
            vec![section(
                "Package",
                vec!["R_pkg 250.0m 225.0m 275.0m", "L_pkg 1nH 0.9nH 1.1nH"],
                vec![],
            )],
        )];
        let parsed = parse_tree(&tree);
        let package = &parsed[0].children[0];

        assert_eq!(package.keyword, "Package");
        let expected_corner = Corner("250.0m".into(), "225.0m".into(), "275.0m".into());
        assert_eq!(
            package.field("r_pkg"),
            Some(&ParsedField { key: "r_pkg".into(), value: ParsedValue::Corner(expected_corner) })
        );
        let expected_inductance = Corner("1nH".into(), "0.9nH".into(), "1.1nH".into());
        assert_eq!(
            package.field("l_pkg"),
            Some(&ParsedField { key: "l_pkg".into(), value: ParsedValue::Corner(expected_inductance) })
        );
    }

    #[test]
    fn test_pin_table_uses_declared_columns_and_keeps_short_rows() {
        let tree = vec![section(
            "Component",
            vec!["MyChip"],
            vec![section(
                "Pin",
                vec!["signal_name model_name R_pin L_pin C_pin", "PC13 IO8TC", "PA0 IO8TC 5.0mOhm 1.2nH 2.0pF"],
                vec![],
            )],
        )];
        let parsed = parse_tree(&tree);
        let pin = &parsed[0].children[0];

        // The schema declares `col0` first: `[Pin]` rows start with the pin name,
        // which the keyword line's column names (`signal_name model_name …`) omit.
        let expected_table = ParsedTable {
            header: vec![
                "col0".into(),
                "signal_name".into(),
                "model_name".into(),
                "R_pin".into(),
                "L_pin".into(),
                "C_pin".into(),
            ],
            data: vec![
                vec!["PC13".into(), "IO8TC".into()],
                vec![
                    "PA0".into(),
                    "IO8TC".into(),
                    "5.0mOhm".into(),
                    "1.2nH".into(),
                    "2.0pF".into(),
                ],
            ],
        };
        assert_eq!(
            pin.field("pin"),
            Some(&ParsedField { key: "pin".into(), value: ParsedValue::Table(expected_table) })
        );
    }

    #[test]
    fn test_declared_params_win_as_column_names() {
        let tree = vec![section(
            "Component",
            vec!["MyChip"],
            vec![section("Pin", vec!["signal_name model_name", "PA0 IO8TC"], vec![])],
        )];
        let parsed = parse_tree(&tree);
        let pin = &parsed[0].children[0];

        // The schema declares six columns (leading `col0` plus five), so the two names
        // on the keyword line do not shorten the header; that line is still consumed as
        // the column-name row.
        let expected_table = ParsedTable {
            header: vec![
                "col0".into(),
                "signal_name".into(),
                "model_name".into(),
                "R_pin".into(),
                "L_pin".into(),
                "C_pin".into(),
            ],
            data: vec![vec!["PA0".into(), "IO8TC".into()]],
        };
        assert_eq!(
            pin.field("pin"),
            Some(&ParsedField { key: "pin".into(), value: ParsedValue::Table(expected_table) })
        );
    }

    #[test]
    fn test_first_content_line_serves_as_header_when_the_keyword_line_is_empty() {
        let tree = vec![section(
            "External Circuit",
            vec!["MyCircuit"],
            vec![section("Port Map", vec!["port_name model_name", "P1 M1"], vec![])],
        )];
        let parsed = parse_tree(&tree);
        let port_map = &parsed[0].children[0];

        let expected_table = ParsedTable {
            header: vec!["port_name".into(), "model_name".into()],
            data: vec![vec!["P1".into(), "M1".into()]],
        };
        assert_eq!(
            port_map.field("port_map"),
            Some(&ParsedField {
                key: "port_map".into(),
                value: ParsedValue::Table(expected_table),
            })
        );
    }

    #[test]
    fn test_series_pin_mapping_header_fills_the_omitted_column() {
        let tree = vec![section(
            "Component",
            vec!["MyChip"],
            vec![section(
                "Series Pin Mapping",
                vec!["pin_2 model_name function_table_group", "166P 166N rterm_100"],
                vec![],
            )],
        )];
        let parsed = parse_tree(&tree);
        let mapping = &parsed[0].children[0];

        let expected_table = ParsedTable {
            header: vec![
                "col0".into(),
                "pin_2".into(),
                "model_name".into(),
                "function_table_group".into(),
            ],
            data: vec![vec!["166P".into(), "166N".into(), "rterm_100".into()]],
        };
        assert_eq!(
            mapping.field("series_pin_mapping"),
            Some(&ParsedField {
                key: "series_pin_mapping".into(),
                value: ParsedValue::Table(expected_table),
            })
        );
    }

    #[test]
    fn test_corner_header_sections_become_tuples() {
        let tree = vec![section(
            "Model",
            vec!["IO8FT"],
            vec![section("Voltage Range", vec!["3.3000V 2.0000V 3.6000V"], vec![])],
        )];
        let parsed = parse_tree(&tree);
        let voltage_range = &parsed[0].children[0];

        let expected_corner = Corner("3.3000V".into(), "2.0000V".into(), "3.6000V".into());
        assert_eq!(
            voltage_range.field("voltage_range"),
            Some(&ParsedField {
                key: "voltage_range".into(),
                value: ParsedValue::Corner(expected_corner),
            })
        );
    }

    #[test]
    fn test_model_params_accept_both_spellings_and_symbol_keys() {
        let tree = vec![section(
            "Model",
            vec![
                "IO8FT",
                "Model_type  I/O",
                "Polarity Non-Inverting",
                "Vinl = 0.8",
                "C_comp  1.12p 0.79p 1.15p",
                "Vinh+ NA",
            ],
            vec![section(
                "Ramp",
                vec![
                    "R_load = 1k",
                    "\"dv/dt_r\" 1.926/597.107p 1.171/792.551p 1.993/430.881p",
                ],
                vec![],
            )],
        )];
        let parsed = parse_tree(&tree);
        let model = &parsed[0];

        assert_eq!(
            model.field("model"),
            Some(&ParsedField { key: "model".into(), value: ParsedValue::Text("IO8FT".into()) })
        );
        assert_eq!(
            model.field("model_type"),
            Some(&ParsedField { key: "model_type".into(), value: ParsedValue::Text("I/O".into()) })
        );
        assert_eq!(
            model.field("polarity"),
            Some(&ParsedField {
                key: "polarity".into(),
                value: ParsedValue::Text("Non-Inverting".into()),
            })
        );
        assert_eq!(
            model.field("vinl"),
            Some(&ParsedField { key: "vinl".into(), value: ParsedValue::Text("0.8".into()) })
        );
        let expected_c_comp = Corner("1.12p".into(), "0.79p".into(), "1.15p".into());
        assert_eq!(
            model.field("c_comp"),
            Some(&ParsedField { key: "c_comp".into(), value: ParsedValue::Corner(expected_c_comp) })
        );
        // `Vinh+ NA` is missing → the param is dropped.
        assert!(model.field("vinh+").is_none());

        let ramp = &model.children[0];
        assert_eq!(
            ramp.field("r_load"),
            Some(&ParsedField { key: "r_load".into(), value: ParsedValue::Text("1k".into()) })
        );
        let expected_slew = Corner(
            "1.926/597.107p".into(),
            "1.171/792.551p".into(),
            "1.993/430.881p".into(),
        );
        assert_eq!(
            ramp.field("dv/dt_r"),
            Some(&ParsedField { key: "dv/dt_r".into(), value: ParsedValue::Corner(expected_slew) })
        );
    }

    #[test]
    fn test_param_less_table_keeps_rows_without_inventing_columns() {
        let tree = vec![section(
            "Component",
            vec!["MyChip"],
            vec![section("Node Declarations", vec!["U1.CLK", "U1.DAT"], vec![])],
        )];
        let parsed = parse_tree(&tree);
        let declarations = &parsed[0].children[0];

        let expected_table = ParsedTable {
            header: Vec::new(),
            data: vec![vec!["U1.CLK".into()], vec!["U1.DAT".into()]],
        };
        assert_eq!(
            declarations.field("node_declarations"),
            Some(&ParsedField {
                key: "node_declarations".into(),
                value: ParsedValue::Table(expected_table),
            })
        );
    }

    #[test]
    fn test_iv_table_uses_the_conventional_column_names() {
        let tree = vec![section(
            "Model",
            vec!["IO8FT"],
            vec![section(
                "Pulldown",
                vec!["-3.3 -2mA -2mA -1mA", "0 0A 0A 0A"],
                vec![],
            )],
        )];
        let parsed = parse_tree(&tree);
        let pulldown = &parsed[0].children[0];

        // `format = "IV-table"` fixes the columns, so no header row is needed and every
        // content line stays a data row.
        let expected_table = ParsedTable {
            header: vec!["Voltage".into(), "I(typ)".into(), "I(min)".into(), "I(max)".into()],
            data: vec![
                vec!["-3.3".into(), "-2mA".into(), "-2mA".into(), "-1mA".into()],
                vec!["0".into(), "0A".into(), "0A".into(), "0A".into()],
            ],
        };
        assert_eq!(
            pulldown.field("pulldown"),
            Some(&ParsedField {
                key: "pulldown".into(),
                value: ParsedValue::Table(expected_table),
            })
        );
    }

    #[test]
    fn test_model_selector_keeps_the_list_lines() {
        let tree = vec![section(
            "Model Selector",
            vec!["RDQS#", "NF_IN_800   Not Used if RDQS is disabled"],
            vec![],
        )];
        let parsed = parse_tree(&tree);
        let selector = &parsed[0];

        assert_eq!(
            selector.field("model_selector"),
            Some(&ParsedField {
                key: "model_selector".into(),
                value: ParsedValue::Text("RDQS#".into()),
            })
        );
        assert_eq!(
            selector.field("lines"),
            Some(&ParsedField {
                key: "lines".into(),
                value: ParsedValue::Lines(vec!["NF_IN_800   Not Used if RDQS is disabled".into()]),
            })
        );
    }

    #[test]
    fn test_file_header_entries_are_parsed_per_entry() {
        let tree = vec![section(
            "File_Header",
            vec![],
            vec![
                section("IBIS ver", vec!["2.1"], vec![]),
                section("Notes", vec!["first note", "second note"], vec![]),
            ],
        )];
        let parsed = parse_tree(&tree);
        let header = &parsed[0];

        assert_eq!(header.keyword, "File_Header");
        assert_eq!(
            header.children[0].field("ibis_ver"),
            Some(&ParsedField { key: "ibis_ver".into(), value: ParsedValue::Text("2.1".into()) })
        );
        assert_eq!(
            header.children[1].field("notes"),
            Some(&ParsedField {
                key: "notes".into(),
                value: ParsedValue::Lines(vec!["first note".into(), "second note".into()]),
            })
        );
    }

    #[test]
    fn test_waveform_fixture_params_and_data_rows() {
        let tree = vec![section(
            "Model",
            vec!["IO8FT"],
            vec![section(
                "Rising Waveform",
                vec!["R_fixture= 50.000", "0.0 0.0 0.0 0.0", "0.1 0.9 0.5 0.7"],
                vec![],
            )],
        )];
        let parsed = parse_tree(&tree);
        let waveform = &parsed[0].children[0];

        assert_eq!(
            waveform.field("r_fixture"),
            Some(&ParsedField { key: "r_fixture".into(), value: ParsedValue::Text("50.000".into()) })
        );
        // The waveform rows become a V-t table with the conventional columns.
        let expected_table = ParsedTable {
            header: vec!["Time".into(), "V(typ)".into(), "V(min)".into(), "V(max)".into()],
            data: vec![
                vec!["0.0".into(), "0.0".into(), "0.0".into(), "0.0".into()],
                vec!["0.1".into(), "0.9".into(), "0.5".into(), "0.7".into()],
            ],
        };
        assert_eq!(
            waveform.field("rising_waveform"),
            Some(&ParsedField {
                key: "rising_waveform".into(),
                value: ParsedValue::Table(expected_table),
            })
        );
    }

    #[test]
    fn test_add_submodel_uses_its_declared_columns() {
        let tree = vec![section(
            "Model",
            vec!["IO8FT"],
            vec![section("Add Submodel", vec!["50ohm_ODT_800 Non-Driving"], vec![])],
        )];
        let parsed = parse_tree(&tree);
        let add_submodel = &parsed[0].children[0];

        assert_eq!(add_submodel.keyword, "Add_Submodel");
        let expected_table = ParsedTable {
            header: vec!["submodel_name".into(), "mode".into()],
            data: vec![vec!["50ohm_ODT_800".into(), "Non-Driving".into()]],
        };
        assert_eq!(
            add_submodel.field("add_submodel"),
            Some(&ParsedField {
                key: "add_submodel".into(),
                value: ParsedValue::Table(expected_table),
            })
        );
    }

    #[test]
    fn test_leftover_tokens_of_a_param_are_reported_as_lines() {
        // `[Model]` declares `vinl` as a single quantity, so the two extra values are
        // reported back instead of being dropped silently. (`[Model Spec]` declares
        // corners, so it consumes all three.)
        let tree = vec![section("Model", vec!["IO8FT", "Vinl 0.75 0.69 0.81"], vec![])];
        let parsed = parse_tree(&tree);
        let model = &parsed[0];

        assert_eq!(
            model.field("vinl"),
            Some(&ParsedField { key: "vinl".into(), value: ParsedValue::Text("0.75".into()) })
        );
        assert_eq!(
            model.field("lines"),
            Some(&ParsedField {
                key: "lines".into(),
                value: ParsedValue::Lines(vec!["0.69 0.81".into()]),
            })
        );
    }

    // -------------------------- value primitives --------------------------

    #[test]
    fn test_is_missing_token() {
        assert!(is_missing_token(""));
        assert!(is_missing_token("   "));
        assert!(is_missing_token("NA"));
        assert!(is_missing_token("na"));
        assert!(!is_missing_token("0"));
    }

    #[test]
    fn test_split_number_unit_keeps_the_original_unit() {
        assert_eq!(split_number_unit("2.69p"), Some((2.69, Some("p".to_string()))));
        assert_eq!(split_number_unit("-2mA"), Some((-2.0, Some("mA".to_string()))));
        assert_eq!(split_number_unit("3.3"), Some((3.3, None)));
        assert_eq!(split_number_unit("0.1250k"), Some((0.125, Some("k".to_string()))));
    }

    #[test]
    fn test_split_number_unit_rejects_non_quantities() {
        assert_eq!(split_number_unit("I/O"), None);
        assert_eq!(split_number_unit("PC13-ANTI_TAMP"), None);
        assert_eq!(split_number_unit("12-08-2024"), None);
        assert_eq!(split_number_unit(""), None);
    }

    #[test]
    fn test_is_quantity_token_covers_ratios_and_dates() {
        assert!(is_quantity_token("0.8"));
        assert!(is_quantity_token("1.65V"));
        assert!(is_quantity_token("1.9/597p"));
        assert!(is_quantity_token("50.00Ohm"));
        assert!(!is_quantity_token("NA"));
        assert!(!is_quantity_token("12-08-2024"));
        assert!(!is_quantity_token("IO8TC"));
    }
}
