//! 语义层与语法层 —— `ibis_schema.toml` 的两面。
//!
//! 本文件只做两件事，各占一个内联模块：
//!
//! - `types`：**语义层**。schema.toml 里每一项「是什么意思」——节段与参数的
//!   类型、各元数据属性的取值集合、固定列名与固定键名、层级取值、以及解析出来
//!   的值模型（[`ParsedValue`] / [`Corner`] / [`ParsedTable`] / [`ParsedField`]）；
//! - `naming`：**语法层**。怎么读 schema.toml 里的字符串——关键词归一化、
//!   元数据键识别、输出键拼写（[`normalize_keyword`] / [`to_snake_key`] /
//!   [`is_metadata_key`]）。
//!
//! 本文件不读 TOML：它只说 schema「是什么」。

// =============================================================================
// spec — `ibis_schema.toml` 的语义层与语法层
//
// Design constraints:
//   - `types` 是语义层，`naming` 是语法层；两层不混放；
//   - 每个元数据枚举自带词汇表：接受的拼写只在它的 `ALL` 里出现一次，
//     `from_schema_str` 与 `as_schema_str` 都从 `ALL` 读，新增取值只改一处；
//   - 纯数据加纯字符串函数：无 I/O、无缓存、无 panic；
//   - `SectionSpec` 保留 `__param__` 与子节段的声明顺序，两者在下游都有含义。
// =============================================================================

pub use naming::{is_metadata_key, normalize_keyword, to_snake_key};
pub use types::{
    keyword_level, Corner, FieldSpec, HeaderFormat, Occurrence, ParamType, ParsedField,
    ParsedTable, ParsedValue, SectionFormat, SectionSpec, CORNER_NAMES, FILE_HEADER_CONTAINER,
    IV_COLUMN_NAMES, TERMINATOR_KEYWORD, UNMATCHED_LINES_KEY, VT_COLUMN_NAMES,
};

/// 语义层 —— schema.toml 里每一项的含义。
mod types {
    // ───────────────────────── 节段与参数 ─────────────────────────

    /// How often a keyword may appear inside its parent scope.
    ///
    /// Taken from the TOML table shape of the keyword: `[X]` is single, `[[X]]`
    /// is multiple. The AST does not carry this information, so it lives here.
    #[derive(Debug, Clone, PartialEq)]
    pub enum Occurrence {
        /// `[X]` — at most once inside the parent scope.
        Once,
        /// `[[X]]` — may appear several times (emitted as sibling nodes).
        Multiple,
    }

    /// `__schema__.format` — how the content below a keyword is organized.
    #[derive(Debug, Clone, PartialEq)]
    pub enum SectionFormat {
        /// A file header entry (`IBIS ver`, `File name`, `Notes`, …).
        FileHeader,
        /// A single record: the identifier may go to the header, the rest to params.
        Text,
        /// Free text spanning several lines.
        TextLines,
        /// Tabular or list-shaped data (row-oriented).
        Table,
        /// An I-V curve: row-oriented numeric data whose columns are fixed
        /// ([`IV_COLUMN_NAMES`](super::types::IV_COLUMN_NAMES)).
        IvTable,
        /// A V-t waveform: fixture params followed by row-oriented numeric data whose
        /// columns are fixed ([`VT_COLUMN_NAMES`](super::types::VT_COLUMN_NAMES)).
        VtTable,
    }

    impl SectionFormat {
        /// Every spelling `__schema__.format` accepts, paired with its variant.
        ///
        /// This is the vocabulary of the attribute: the spelling of a format lives
        /// here and nowhere else, so both directions and the error messages read
        /// from this one table.
        pub const ALL: [(&'static str, SectionFormat); 6] = [
            ("file_header", SectionFormat::FileHeader),
            ("text", SectionFormat::Text),
            ("textlines", SectionFormat::TextLines),
            ("table", SectionFormat::Table),
            ("IV-table", SectionFormat::IvTable),
            ("VT-table", SectionFormat::VtTable),
        ];

        /// Reads a `__schema__.format` spelling.
        ///
        /// Takes the raw `raw` value; returns the matching variant, and `None` when
        /// the spelling is not in [`ALL`](SectionFormat::ALL).
        pub fn from_schema_str(raw: &str) -> Option<SectionFormat> {
            let matching_entry = Self::ALL.iter().find(|(spelling, _)| *spelling == raw);
            matching_entry.map(|(_, format)| format.clone())
        }

        /// Spells this format the way `ibis_schema.toml` does.
        ///
        /// Takes no argument; returns the spelling listed in
        /// [`ALL`](SectionFormat::ALL) for this variant.
        pub fn as_schema_str(&self) -> &'static str {
            let matching_entry = Self::ALL.iter().find(|(_, format)| format == self);
            let spelling = matching_entry.map(|(spelling, _)| *spelling);
            spelling.expect("every SectionFormat variant is listed in ALL")
        }
    }

    /// `__header__.format` — how the text right after `[Keyword]` is organized.
    #[derive(Debug, Clone, PartialEq)]
    pub enum HeaderFormat {
        /// No text follows the keyword.
        None,
        /// A single text / identifier token.
        Text,
        /// Free text spanning several lines.
        TextLines,
        /// A header row naming the table columns.
        TableHeader,
        /// One corner triple (`typ min max`).
        Corner,
    }

    impl HeaderFormat {
        /// Every spelling `__header__.format` accepts, paired with its variant.
        pub const ALL: [(&'static str, HeaderFormat); 5] = [
            ("none", HeaderFormat::None),
            ("text", HeaderFormat::Text),
            ("textlines", HeaderFormat::TextLines),
            ("table_header", HeaderFormat::TableHeader),
            ("corner", HeaderFormat::Corner),
        ];

        /// Reads a `__header__.format` spelling.
        ///
        /// Takes the raw `raw` value; returns the matching variant, and `None` when
        /// the spelling is not in [`ALL`](HeaderFormat::ALL).
        pub fn from_schema_str(raw: &str) -> Option<HeaderFormat> {
            let matching_entry = Self::ALL.iter().find(|(spelling, _)| *spelling == raw);
            matching_entry.map(|(_, format)| format.clone())
        }

        /// Spells this header format the way `ibis_schema.toml` does.
        ///
        /// Takes no argument; returns the spelling listed in
        /// [`ALL`](HeaderFormat::ALL) for this variant.
        pub fn as_schema_str(&self) -> &'static str {
            let matching_entry = Self::ALL.iter().find(|(_, format)| format == self);
            let spelling = matching_entry.map(|(spelling, _)| *spelling);
            spelling.expect("every HeaderFormat variant is listed in ALL")
        }
    }

    /// The declared value type of one `__param__` entry.
    #[derive(Debug, Clone, PartialEq)]
    pub enum ParamType {
        /// Text, enumeration or name.
        Text,
        /// Physical quantity carrying its unit (`1.65V`, `10mA`, `1.9/597p`).
        Quantity,
        /// Corner triple (`typ min max`).
        Corner,
    }

    impl ParamType {
        /// Every spelling a `__param__` value accepts, paired with its variant.
        pub const ALL: [(&'static str, ParamType); 3] = [
            ("text", ParamType::Text),
            ("quantity", ParamType::Quantity),
            ("corner", ParamType::Corner),
        ];

        /// Reads a `__param__` type spelling.
        ///
        /// Takes the raw `raw` value; returns the matching variant, and `None` when
        /// the spelling is not in [`ALL`](ParamType::ALL).
        pub fn from_schema_str(raw: &str) -> Option<ParamType> {
            let matching_entry = Self::ALL.iter().find(|(spelling, _)| *spelling == raw);
            matching_entry.map(|(_, param_type)| param_type.clone())
        }

        /// Spells this param type the way `ibis_schema.toml` does.
        ///
        /// Takes no argument; returns the spelling listed in
        /// [`ALL`](ParamType::ALL) for this variant.
        pub fn as_schema_str(&self) -> &'static str {
            let matching_entry = Self::ALL.iter().find(|(_, param_type)| param_type == self);
            let spelling = matching_entry.map(|(spelling, _)| *spelling);
            spelling.expect("every ParamType variant is listed in ALL")
        }
    }

    /// One named sub-parameter declared by `__param__`.
    #[derive(Debug, Clone, PartialEq)]
    pub struct FieldSpec {
        pub key: String,             // Param name as spelled in the schema (e.g. "R_pin", "dv/dt_r").
        pub param_type: ParamType,   // Declared value type, driving how tokens are consumed.
    }

    /// The complete structural specification of one keyword.
    #[derive(Debug, Clone, PartialEq)]
    pub struct SectionSpec {
        pub name: String,                 // Keyword name as spelled in the schema (e.g. "Model_Spec").
        pub occurrence: Occurrence,       // Single (`[X]`) or multiple (`[[X]]`) inside the parent.
        pub required: bool,               // `__schema__.required`: must the keyword appear.
        pub format: SectionFormat,        // `__schema__.format`: body organization of this keyword.
        pub header_format: HeaderFormat,  // `__header__.format`: text directly after the keyword.
        pub fields: Vec<FieldSpec>,       // Ordered `__param__` entries; empty when `"none"`.
        pub children: Vec<SectionSpec>,   // Ordered child keyword sections.
    }

    // ───────────────────────── 名称与键 ─────────────────────────

    /// Name of the virtual container that groups the file header entries.
    ///
    /// The container is not a keyword of the IBIS manual: the frontend creates it
    /// and every later layer recognises it by this name.
    pub const FILE_HEADER_CONTAINER: &str = "File_Header";

    /// Normalized spelling of the `[End]` terminator keyword.
    ///
    /// Stored already normalized (lowercase, no `_`), so it compares directly
    /// against the output of [`normalize_keyword`](super::naming::normalize_keyword).
    /// `[End]` closes the file and is the only keyword that yields no section.
    pub const TERMINATOR_KEYWORD: &str = "end";

    /// Output key of the content lines no schema declaration accounts for.
    ///
    /// A value of this key always carries [`ParsedValue::Lines`].
    pub const UNMATCHED_LINES_KEY: &str = "lines";

    /// Columns of an `IV-table` row, in the order the manual lists them.
    pub const IV_COLUMN_NAMES: [&str; 4] = ["Voltage", "I(typ)", "I(min)", "I(max)"];

    /// Columns of a `VT-table` row, in the order the manual lists them.
    pub const VT_COLUMN_NAMES: [&str; 4] = ["Time", "V(typ)", "V(min)", "V(max)"];

    /// Element names of a corner triple, in the order [`Corner`] stores them.
    pub const CORNER_NAMES: [&str; 3] = ["typ", "min", "max"];

    // ───────────────────────── 层级 ─────────────────────────

    /// Keyword levels — the nesting level of a keyword inside the schema tree.
    ///
    /// Levels are the only structural tag the pipeline carries: the syntax stage
    /// derives them from [`find_level`](crate::schema::find_level), and tree
    /// building branches on them. All real levels belong to one family — the depth
    /// `N` inside the schema tree: [`ROOT`](keyword_level::ROOT) is `N = 1`,
    /// [`SECOND_LEVEL`](keyword_level::SECOND_LEVEL) is `N = 2`, a grandchild is
    /// `N = 3`. [`TERMINATOR`](keyword_level::TERMINATOR) is the one level outside
    /// that family: it marks `[End]`, which yields no node.
    pub mod keyword_level {
        /// `[End]` — the terminator keyword: it closes the file and yields no node.
        pub const TERMINATOR: usize = 0;
        /// Depth `N = 1` — a top-level section (`Component`, `Model`, `Submodel`, …):
        /// it owns the sections that follow it.
        pub const ROOT: usize = 1;
        /// Depth `N = 2` — a section sitting directly under a top-level section
        /// (`Component.Manufacturer`, `Component.Package`, …).
        ///
        /// A keyword the schema tree does not contain is placed at this depth, since
        /// it sits directly under the top-level section it appears in.
        pub const SECOND_LEVEL: usize = 2;
    }

    // ───────────────────────── 值模型 ─────────────────────────

    /// A corner triple in IBIS order: typical, minimum, maximum.
    ///
    /// Each element keeps the original text (`"3.3000V"`); an absent or `NA` element
    /// is stored as an empty string. The element names are
    /// [`CORNER_NAMES`](super::types::CORNER_NAMES).
    #[derive(Debug, Clone, PartialEq)]
    pub struct Corner(
        pub String, // Typical element, kept as written.
        pub String, // Minimum element, kept as written.
        pub String, // Maximum element, kept as written.
    );

    /// A row-oriented table: the column names plus the data rows.
    #[derive(Debug, Clone, PartialEq)]
    pub struct ParsedTable {
        pub header: Vec<String>,    // Column names; empty when the source declares none.
        pub data: Vec<Vec<String>>, // Cell text in column order; a short row means trailing cells were absent.
    }

    /// The value carried by one field.
    #[derive(Debug, Clone, PartialEq)]
    pub enum ParsedValue {
        Text(String),       // Plain text: identifiers, enumerations and quantities as written.
        Corner(Corner),     // Corner triple `(typ, min, max)`.
        Table(ParsedTable), // Row-oriented table (`{ header = [], data = [] }`).
        Lines(Vec<String>), // Ordered free-text lines.
    }

    /// One typed field of a section.
    #[derive(Debug, Clone, PartialEq)]
    pub struct ParsedField {
        pub key: String,        // Output key: the keyword's own snake key or a param name.
        pub value: ParsedValue, // Typed value of the field.
    }

    impl ParsedField {
        /// Builds the field holding a section's own value, keyed by its snake-cased name.
        ///
        /// Takes the section `spec` and its `value`; returns the self-value field.
        pub fn self_value(spec: &SectionSpec, value: ParsedValue) -> ParsedField {
            ParsedField { key: super::naming::to_snake_key(&spec.name), value }
        }

        /// Builds the field that keeps content no declaration accounts for.
        ///
        /// Takes the unmatched `lines`; returns them under
        /// [`UNMATCHED_LINES_KEY`](super::types::UNMATCHED_LINES_KEY) as
        /// [`ParsedValue::Lines`].
        pub fn unmatched(lines: Vec<String>) -> ParsedField {
            ParsedField { key: UNMATCHED_LINES_KEY.to_string(), value: ParsedValue::Lines(lines) }
        }
    }
}

/// 语法层 —— 怎么读 schema.toml 里的字符串。
mod naming {
    /// Whether a schema key carries keyword metadata (`__schema__` / `__header__` /
    /// `__param__`).
    ///
    /// Takes a schema `key`; returns `true` when it starts with `__`.
    pub fn is_metadata_key(key: &str) -> bool {
        key.starts_with("__")
    }

    /// Normalizes a keyword for case-insensitive and `_`-equals-space comparison.
    ///
    /// Takes a raw `keyword`; returns it lowercased with `_` folded into single
    /// spaces. For example both `"IBIS_Ver"` and `"ibis ver"` normalize to
    /// `"ibis ver"`, so schema names and AST keywords compare equal.
    pub fn normalize_keyword(keyword: &str) -> String {
        let lowered_characters = keyword.trim().chars().map(|character| match character {
            '_' => ' ',
            other => other.to_ascii_lowercase(),
        });
        let lowered: String = lowered_characters.collect();
        lowered.split_whitespace().collect::<Vec<&str>>().join(" ")
    }

    /// Turns a schema name into the output key of the section's own value.
    ///
    /// Takes a schema `name`; returns it lowercased with whitespace, `_` and `-`
    /// folded into a single `_` (`Manufacturer` → `manufacturer`, `Model Selector`
    /// → `model_selector`). Symbols such as `/` and `+` are preserved (e.g.
    /// `dv/dt_r`, `vinh+`); whether they need quoting is an emitter concern.
    pub fn to_snake_key(name: &str) -> String {
        let mut key = String::with_capacity(name.len());
        let mut previous_is_separator = false;
        for character in name.trim().chars() {
            if character.is_ascii_alphanumeric() {
                key.push(character.to_ascii_lowercase());
                previous_is_separator = false;
            } else if character.is_ascii_whitespace() || character == '_' || character == '-' {
                let is_leading_separator = key.is_empty();
                if !is_leading_separator && !previous_is_separator {
                    key.push('_');
                    previous_is_separator = true;
                }
            } else {
                key.push(character);
                previous_is_separator = false;
            }
        }
        while key.ends_with('_') {
            key.pop();
        }
        key
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_normalize_keyword_is_case_insensitive_and_underscore_equivalent() {
            assert_eq!(normalize_keyword("IBIS_Ver"), "ibis ver");
            assert_eq!(normalize_keyword("ibis ver"), "ibis ver");
            assert_eq!(normalize_keyword("  File   Name "), "file name");
            assert_eq!(normalize_keyword("Model_Spec"), "model spec");
        }

        #[test]
        fn test_to_snake_key_keeps_symbols_and_folds_separators() {
            assert_eq!(to_snake_key("Manufacturer"), "manufacturer");
            assert_eq!(to_snake_key("Model Selector"), "model_selector");
            assert_eq!(to_snake_key("IBIS_Ver"), "ibis_ver");
            assert_eq!(to_snake_key("POWER_Table"), "power_table");
            assert_eq!(to_snake_key("dv/dt_r"), "dv/dt_r");
            assert_eq!(to_snake_key("vinh+"), "vinh+");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_section_format_vocabulary_round_trips() {
        for (spelling, format) in SectionFormat::ALL {
            assert_eq!(SectionFormat::from_schema_str(spelling), Some(format.clone()));
            assert_eq!(format.as_schema_str(), spelling);
        }
        assert_eq!(SectionFormat::from_schema_str("iv-table"), None);
    }

    #[test]
    fn test_header_format_vocabulary_round_trips() {
        for (spelling, header_format) in HeaderFormat::ALL {
            assert_eq!(HeaderFormat::from_schema_str(spelling), Some(header_format.clone()));
            assert_eq!(header_format.as_schema_str(), spelling);
        }
        assert_eq!(HeaderFormat::from_schema_str("table"), None);
    }

    #[test]
    fn test_param_type_vocabulary_round_trips() {
        for (spelling, param_type) in ParamType::ALL {
            assert_eq!(ParamType::from_schema_str(spelling), Some(param_type.clone()));
            assert_eq!(param_type.as_schema_str(), spelling);
        }
        assert_eq!(ParamType::from_schema_str("lines"), None);
    }

    #[test]
    fn test_every_spelling_appears_exactly_once_in_its_table() {
        let section_spellings: Vec<&str> =
            SectionFormat::ALL.iter().map(|(spelling, _)| *spelling).collect();
        let header_spellings: Vec<&str> =
            HeaderFormat::ALL.iter().map(|(spelling, _)| *spelling).collect();
        let param_spellings: Vec<&str> =
            ParamType::ALL.iter().map(|(spelling, _)| *spelling).collect();

        assert_eq!(section_spellings.len(), SectionFormat::ALL.len());
        assert_eq!(header_spellings.len(), HeaderFormat::ALL.len());
        assert_eq!(param_spellings.len(), ParamType::ALL.len());
    }

    #[test]
    fn test_terminator_keyword_is_already_normalized() {
        assert_eq!(normalize_keyword("End"), TERMINATOR_KEYWORD);
        assert_eq!(normalize_keyword("END"), TERMINATOR_KEYWORD);
    }

    #[test]
    fn test_unmatched_field_uses_the_documented_key() {
        let field = ParsedField::unmatched(vec!["leftover".into()]);
        assert_eq!(field.key, UNMATCHED_LINES_KEY);
        assert_eq!(field.value, ParsedValue::Lines(vec!["leftover".into()]));
    }
}
