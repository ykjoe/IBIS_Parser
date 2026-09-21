// =============================================================================
// schema — the single loader of the IBIS 7.0 structural specification
//            (`ibis_schema.toml` → `SectionSpec` tree)
//
// This module parses `ibis_schema.toml` (the only structural data source) once,
// on first use, into a strongly-typed `SectionSpec` tree. It is read-only for
// every consumer: the frontend uses it to classify file-header fields, and the
// backend uses it for preprocess / content_parse / validation. Changing the
// specification means editing the TOML only — never this code.
//
// Layout:
//   - `spec`   — data structures (`SectionSpec` / `FieldSpec` + three enums)
//                and naming helpers;
//   - `loader` — TOML → `SectionSpec` tree (`include_str!` + `OnceLock` cache)
//                and the lookup primitives consumed by the rest of the pipeline.
//
// Boundary conventions:
//   - Read-only: the schema is never written back, and no semantic validation
//     happens here;
//   - Order-preserving: relies on the `toml` crate's `preserve_order` feature so
//     that the declaration order of `__param__` is the field order;
//   - Case-insensitive with `_` equivalent to a space: every lookup goes through
//     `normalize_keyword`.
// =============================================================================

//! The single loader of the IBIS 7.0 structural specification.
//!
//! This module turns `ibis_schema.toml` into a [`SectionSpec`] tree: `[X]` vs
//! `[[X]]` gives the **single / multiple** occurrence ([`Occurrence`]),
//! `__schema__.format` gives **how header / params / text body are organized**
//! ([`SectionFormat`]), `__header__.format` gives **how the text right after the
//! keyword is organized** ([`HeaderFormat`]), and `__param__` gives the **named
//! sub-parameters with their value types** ([`FieldSpec`] / [`ParamType`]).
//!
//! # Examples
//!
//! ```rust
//! use ibis2ibstoml::schema::{find_child, find_root, Occurrence};
//!
//! let component = find_root("Component").expect("Component is registered");
//! assert_eq!(component.occurrence, Occurrence::Multiple);
//!
//! let pin = find_child(component, "Pin").expect("Component.Pin is registered");
//! // `col0` names the leading pin-name column that the manual's header line omits.
//! assert_eq!(pin.fields.len(), 6);
//! assert_eq!(pin.fields[0].key, "col0");
//! ```

pub use loader::{
    file_header_section, find_child, find_descendant, find_field, find_root, load_schema,
};
pub use spec::{
    normalize_keyword, to_snake_key, FieldSpec, HeaderFormat, Occurrence, ParamType,
    SectionFormat, SectionSpec,
};

/// Data structures and naming helpers — the strongly-typed form of the schema.
mod spec {
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
        /// An I-V curve: row-oriented numeric data whose columns are fixed by the
        /// IBIS convention (`Voltage`, `I(typ)`, `I(min)`, `I(max)`).
        IvTable,
        /// A V-t waveform: fixture params followed by row-oriented numeric data whose
        /// columns are fixed by the IBIS convention (`Time`, `V(typ)`, `V(min)`,
        /// `V(max)`).
        VtTable,
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

/// TOML → `SectionSpec` tree, plus the lookup primitives shared by all stages.
mod loader {
    use std::sync::OnceLock;

    use super::spec::{
        is_metadata_key, normalize_keyword, FieldSpec, HeaderFormat, Occurrence, ParamType,
        SectionFormat, SectionSpec,
    };

    /// The schema source, embedded at compile time and parsed only once per process.
    const SCHEMA_SOURCE: &str = include_str!("ibis_schema.toml");

    /// Canonical name of the virtual `[File_Header]` container.
    const FILE_HEADER_CONTAINER: &str = "File_Header";

    /// Process-wide cache of the parsed schema.
    static SCHEMA: OnceLock<SchemaTables> = OnceLock::new();

    /// The parsed schema: top-level sections plus the virtual file header container.
    #[derive(Debug, Clone, PartialEq)]
    struct SchemaTables {
        sections: Vec<SectionSpec>,   // Top-level keywords, excluding file header entries.
        file_header: SectionSpec,     // Virtual `[File_Header]` container grouping those entries.
    }

    // -------------------------------------------------------------------------
    // Public API
    // -------------------------------------------------------------------------

    /// Returns the top-level keyword sections of the schema.
    ///
    /// The nine file header entries are **not** part of this list; they are grouped
    /// under the virtual container returned by [`file_header_section`].
    ///
    /// # Returns
    ///
    /// * `&'static [SectionSpec]` — Top-level sections in `ibis_schema.toml` order.
    ///
    /// # Panics
    ///
    /// Panics on the first call if `ibis_schema.toml` cannot be parsed or violates
    /// the metadata convention (missing `__schema__` / `__header__`, unknown enum
    /// value). The schema is a compile-time asset, so such a failure is a
    /// programming error rather than a runtime condition.
    pub fn load_schema() -> &'static [SectionSpec] {
        let schema_tables = tables();
        &schema_tables.sections
    }

    /// Returns the virtual `[File_Header]` container.
    ///
    /// Its `fields` hold the names of the nine file header entries (used by the
    /// frontend to decide whether a keyword is a header field) and its `children`
    /// hold their full specifications.
    ///
    /// # Returns
    ///
    /// * `&'static SectionSpec` — Container named `File_Header`.
    ///
    /// # Panics
    ///
    /// Panics on the first call if `ibis_schema.toml` is malformed (see
    /// [`load_schema`]).
    pub fn file_header_section() -> &'static SectionSpec {
        let schema_tables = tables();
        &schema_tables.file_header
    }

    /// Finds a top-level section by keyword.
    ///
    /// Matching ignores case and treats `_` as a space, so both `"Model Selector"`
    /// and `"Model_Selector"` are accepted.
    ///
    /// # Parameters
    ///
    /// * `keyword` — Keyword to look up (e.g. `"Component"`, `"Model Selector"`).
    ///
    /// # Returns
    ///
    /// * `Some(&'static SectionSpec)` — The matching top-level section.
    /// * `None` — Not registered; file header entries are reached through
    ///   [`file_header_section`] instead.
    ///
    /// # Panics
    ///
    /// Panics on the first call if `ibis_schema.toml` is malformed.
    pub fn find_root(keyword: &str) -> Option<&'static SectionSpec> {
        let normalized = normalize_keyword(keyword);
        let sections = load_schema();
        sections
            .iter()
            .find(|section| normalize_keyword(&section.name) == normalized)
    }

    /// Finds a direct child section by keyword.
    ///
    /// # Parameters
    ///
    /// * `parent` — Parent section spec, usually obtained from [`find_root`].
    /// * `keyword` — Child keyword name (e.g. `"Pin"`).
    ///
    /// # Returns
    ///
    /// * `Some(&SectionSpec)` — The matching direct child.
    /// * `None` — No such child; deeper sections need [`find_descendant`].
    pub fn find_child<'a>(parent: &'a SectionSpec, keyword: &str) -> Option<&'a SectionSpec> {
        let normalized = normalize_keyword(keyword);
        parent
            .children
            .iter()
            .find(|section| normalize_keyword(&section.name) == normalized)
    }

    /// Finds a section among **all descendants** of `parent` (breadth-first, the
    /// parent itself excluded).
    ///
    /// The frontend only recurses on `first_level_keyword`, so sections three
    /// levels deep are flattened into second-level siblings in the AST (for
    /// example `[Model Data]` and `[Resistance Matrix]` both appear directly under
    /// `[Define Package Model]`). `content_parse` therefore resolves a spec with a
    /// child → descendant → root fallback.
    ///
    /// # Parameters
    ///
    /// * `parent` — Parent section spec.
    /// * `keyword` — Keyword to look up at any depth below `parent`.
    ///
    /// # Returns
    ///
    /// * `Some(&SectionSpec)` — The shallowest matching descendant.
    /// * `None` — No such descendant.
    pub fn find_descendant<'a>(parent: &'a SectionSpec, keyword: &str) -> Option<&'a SectionSpec> {
        let normalized = normalize_keyword(keyword);
        let mut frontier: Vec<&SectionSpec> = parent.children.iter().collect();
        while !frontier.is_empty() {
            let mut next_frontier: Vec<&SectionSpec> = Vec::new();
            for candidate in frontier {
                let candidate_name = normalize_keyword(&candidate.name);
                if candidate_name == normalized {
                    return Some(candidate);
                }
                next_frontier.extend(candidate.children.iter());
            }
            frontier = next_frontier;
        }
        None
    }

    /// Finds one named sub-parameter of a section.
    ///
    /// # Parameters
    ///
    /// * `section` — Section spec to inspect.
    /// * `key` — Param name (e.g. `"R_pin"`, `"dv/dt_r"`).
    ///
    /// # Returns
    ///
    /// * `Some(&FieldSpec)` — The declared param with its [`ParamType`].
    /// * `None` — The section does not declare this param.
    pub fn find_field<'a>(section: &'a SectionSpec, key: &str) -> Option<&'a FieldSpec> {
        let normalized = normalize_keyword(key);
        section
            .fields
            .iter()
            .find(|field| normalize_keyword(&field.key) == normalized)
    }

    /// Returns the cached schema, parsing it on first use.
    ///
    /// Takes no argument; returns the process-wide [`SchemaTables`]. Panics when
    /// the embedded schema is malformed.
    fn tables() -> &'static SchemaTables {
        SCHEMA.get_or_init(|| build_tables(SCHEMA_SOURCE))
    }

    // -------------------------------------------------------------------------
    // TOML → SectionSpec tree
    // -------------------------------------------------------------------------

    /// Parses the schema source and splits it into top-level sections and the file
    /// header container.
    ///
    /// Takes the TOML `source`; returns the [`SchemaTables`] to cache.
    fn build_tables(source: &str) -> SchemaTables {
        let document: toml::Table = toml::from_str(source).unwrap_or_else(|error| {
            panic!("ibis_schema.toml is not valid TOML: {error}");
        });

        let mut sections: Vec<SectionSpec> = Vec::new();
        let mut header_sections: Vec<SectionSpec> = Vec::new();

        // ── Split sections: file header entries belong to the virtual container ──
        for (keyword, value) in &document {
            let (section_table, occurrence) = split_section_value(keyword, value);
            let section = build_section(keyword, section_table, occurrence);
            let is_file_header_entry = section.format == SectionFormat::FileHeader;
            if is_file_header_entry {
                header_sections.push(section);
            } else {
                sections.push(section);
            }
        }

        // ── Build the virtual container holding the file header entries ──
        let header_field_names = header_sections.iter().map(|section| FieldSpec {
            key: section.name.clone(),
            param_type: ParamType::Text,
        });
        let file_header = SectionSpec {
            name: FILE_HEADER_CONTAINER.to_string(),
            occurrence: Occurrence::Once,
            required: false,
            format: SectionFormat::FileHeader,
            header_format: HeaderFormat::None,
            fields: header_field_names.collect(),
            children: header_sections,
        };

        SchemaTables { sections, file_header }
    }

    /// Reads one TOML entry as a section table plus its occurrence.
    ///
    /// Takes a `keyword` and its TOML `value`; returns the section table together
    /// with `Occurrence::Multiple` for `[[X]]` and `Occurrence::Once` for `[X]`.
    fn split_section_value<'a>(
        keyword: &str,
        value: &'a toml::Value,
    ) -> (&'a toml::Table, Occurrence) {
        match value {
            toml::Value::Table(section_table) => (section_table, Occurrence::Once),
            toml::Value::Array(items) => {
                let first_item = items.first().unwrap_or_else(|| {
                    panic!("ibis_schema.toml: `[[{keyword}]]` is an empty array");
                });
                match first_item {
                    toml::Value::Table(section_table) => (section_table, Occurrence::Multiple),
                    _ => {
                        panic!("ibis_schema.toml: elements of `[[{keyword}]]` must be tables");
                    }
                }
            }
            _ => {
                panic!("ibis_schema.toml: top-level key `{keyword}` must be a table or array");
            }
        }
    }

    /// Builds one [`SectionSpec`] from its section table.
    ///
    /// Takes the section `name`, its TOML `table` and its `occurrence`; returns the
    /// complete spec (metadata, `__param__` entries and child sections).
    fn build_section(name: &str, table: &toml::Table, occurrence: Occurrence) -> SectionSpec {
        let (required, format) = read_schema_metadata(name, table);
        let header_format = read_header_metadata(name, table);
        SectionSpec {
            name: name.to_string(),
            occurrence,
            required,
            format,
            header_format,
            fields: read_fields(name, table),
            children: read_children(table),
        }
    }

    /// Reads `__schema__` (the `required` flag and the section format).
    ///
    /// Takes a section `name` and its `table`; returns `(required, format)`.
    fn read_schema_metadata(section: &str, table: &toml::Table) -> (bool, SectionFormat) {
        let metadata_entry = table.get("__schema__");
        let metadata = metadata_entry.and_then(toml::Value::as_table);
        let metadata = metadata.unwrap_or_else(|| {
            panic!("ibis_schema.toml: section `{section}` has no `__schema__` table");
        });

        let required_entry = metadata.get("required");
        let required = required_entry.and_then(toml::Value::as_bool);
        let required = required.unwrap_or_else(|| {
            panic!("ibis_schema.toml: section `{section}` has no `__schema__.required`");
        });

        let format_entry = metadata.get("format");
        let format_raw = format_entry.and_then(toml::Value::as_str);
        let format_raw = format_raw.unwrap_or_else(|| {
            panic!("ibis_schema.toml: section `{section}` has no `__schema__.format`");
        });

        (required, parse_section_format(section, format_raw))
    }

    /// Reads `__header__.format`.
    ///
    /// Takes a section `name` and its `table`; returns the parsed [`HeaderFormat`].
    fn read_header_metadata(section: &str, table: &toml::Table) -> HeaderFormat {
        let metadata_entry = table.get("__header__");
        let metadata = metadata_entry.and_then(toml::Value::as_table);
        let metadata = metadata.unwrap_or_else(|| {
            panic!("ibis_schema.toml: section `{section}` has no `__header__` table");
        });

        let format_entry = metadata.get("format");
        let format_raw = format_entry.and_then(toml::Value::as_str);
        let format_raw = format_raw.unwrap_or_else(|| {
            panic!("ibis_schema.toml: section `{section}` has no `__header__.format`");
        });

        parse_header_format(section, format_raw)
    }

    /// Reads `__param__`, whose three spellings are equivalent: the string
    /// `"none"`, an inline table and a `[X.__param__]` sub-table.
    ///
    /// Takes a section `name` and its `table`; returns the ordered [`FieldSpec`] list.
    fn read_fields(section: &str, table: &toml::Table) -> Vec<FieldSpec> {
        let param_entry = table.get("__param__");
        let Some(param_value) = param_entry else {
            return Vec::new();
        };

        match param_value {
            toml::Value::String(raw) => {
                assert!(
                    raw == "none",
                    "ibis_schema.toml: `__param__` of `{section}` must be \"none\", found \"{raw}\""
                );
                Vec::new()
            }
            toml::Value::Table(entries) => entries
                .iter()
                .map(|(param_key, param_value)| FieldSpec {
                    key: param_key.clone(),
                    param_type: parse_param_type(section, param_key, param_value),
                })
                .collect(),
            _ => {
                panic!("ibis_schema.toml: `__param__` of `{section}` must be a string or a table");
            }
        }
    }

    /// Reads the child sections of a section table, skipping metadata keys.
    ///
    /// Takes a section `table`; returns its ordered child specs.
    fn read_children(table: &toml::Table) -> Vec<SectionSpec> {
        let mut children: Vec<SectionSpec> = Vec::new();
        for (key, value) in table {
            if is_metadata_key(key) {
                continue;
            }
            let (child_table, occurrence) = split_section_value(key, value);
            let child = build_section(key, child_table, occurrence);
            children.push(child);
        }
        children
    }

    /// Parses `__schema__.format` into [`SectionFormat`].
    ///
    /// Takes a section `name` and its raw `format`; returns the parsed enum.
    fn parse_section_format(section: &str, format_raw: &str) -> SectionFormat {
        match format_raw {
            "file_header" => SectionFormat::FileHeader,
            "text" => SectionFormat::Text,
            "textlines" => SectionFormat::TextLines,
            "table" => SectionFormat::Table,
            "IV-table" => SectionFormat::IvTable,
            "VT-table" => SectionFormat::VtTable,
            other => {
                panic!(
                    "ibis_schema.toml: `__schema__.format = \"{other}\"` of `{section}` is unknown"
                );
            }
        }
    }

    /// Parses `__header__.format` into [`HeaderFormat`].
    ///
    /// Takes a section `name` and its raw `format`; returns the parsed enum.
    fn parse_header_format(section: &str, format_raw: &str) -> HeaderFormat {
        match format_raw {
            "none" => HeaderFormat::None,
            "text" => HeaderFormat::Text,
            "textlines" => HeaderFormat::TextLines,
            "table_header" => HeaderFormat::TableHeader,
            "corner" => HeaderFormat::Corner,
            other => {
                panic!(
                    "ibis_schema.toml: `__header__.format = \"{other}\"` of `{section}` is unknown"
                );
            }
        }
    }

    /// Parses one `__param__` value into a [`ParamType`].
    ///
    /// Takes the section `name`, the param `key` and the raw parameter `value`;
    /// returns the parsed enum.
    fn parse_param_type(section: &str, key: &str, value: &toml::Value) -> ParamType {
        let type_raw = value.as_str().unwrap_or_else(|| {
            panic!("ibis_schema.toml: param `{key}` of `{section}` must be a type string");
        });
        match type_raw {
            "text" => ParamType::Text,
            "quantity" => ParamType::Quantity,
            "corner" => ParamType::Corner,
            other => {
                panic!("ibis_schema.toml: param `{key}` of `{section}` has unknown type `{other}`");
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::schema::{find_root, to_snake_key};

        /// Whether the keyword is registered as a file header field.
        fn is_file_header_keyword(keyword: &str) -> bool {
            let normalized = normalize_keyword(keyword);
            let header = file_header_section();
            header
                .fields
                .iter()
                .any(|field| normalize_keyword(&field.key) == normalized)
        }

        /// Looks up a top-level section, panicking when it is missing.
        fn root(keyword: &str) -> &'static SectionSpec {
            find_root(keyword).unwrap_or_else(|| panic!("top-level section `{keyword}` is missing"))
        }

        #[test]
        fn test_file_header_container_holds_nine_entries() {
            let header = file_header_section();
            assert_eq!(header.name, "File_Header");
            assert_eq!(header.occurrence, Occurrence::Once);
            assert_eq!(header.children.len(), 9);
            assert_eq!(header.fields.len(), 9);

            let known_keywords = [
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
            for keyword in known_keywords {
                assert!(is_file_header_keyword(keyword), "`{keyword}` must be a header field");
            }
            assert!(!is_file_header_keyword("Component"));
            assert!(!is_file_header_keyword("Pin"));
        }

        #[test]
        fn test_file_header_entries_are_not_top_level_sections() {
            assert!(find_root("IBIS ver").is_none());
            assert!(find_root("Notes").is_none());
        }

        #[test]
        fn test_root_occurrence_follows_table_versus_array() {
            let multiple_keywords = [
                "Component",
                "Model",
                "Model Selector",
                "Submodel",
                "Test Data",
                "Test Load",
                "External Circuit",
                "Define Package Model",
                "Interconnect Model Set",
            ];
            for keyword in multiple_keywords {
                assert_eq!(root(keyword).occurrence, Occurrence::Multiple, "`[[{keyword}]]`");
            }
        }

        #[test]
        fn test_child_occurrence_follows_table_versus_array() {
            let component = root("Component");
            let pin = find_child(component, "Pin").expect("Component.Pin is registered");
            assert_eq!(pin.occurrence, Occurrence::Once);

            let pin_mapping =
                find_child(component, "Pin Mapping").expect("Component.Pin_Mapping is registered");
            assert_eq!(pin_mapping.occurrence, Occurrence::Multiple);
        }

        #[test]
        fn test_component_pin_fields_keep_declaration_order_and_types() {
            let pin = find_child(root("Component"), "Pin").expect("Component.Pin is registered");
            assert_eq!(pin.format, SectionFormat::Table);
            assert_eq!(pin.header_format, HeaderFormat::TableHeader);

            // `[Pin]` rows carry an unnamed leading pin-name column, declared as `col0`.
            let keys: Vec<&str> = pin.fields.iter().map(|field| field.key.as_str()).collect();
            assert_eq!(
                keys,
                vec!["col0", "signal_name", "model_name", "R_pin", "L_pin", "C_pin"]
            );

            let types: Vec<ParamType> =
                pin.fields.iter().map(|field| field.param_type.clone()).collect();
            let expected_types = vec![
                ParamType::Text,
                ParamType::Text,
                ParamType::Text,
                ParamType::Quantity,
                ParamType::Quantity,
                ParamType::Quantity,
            ];
            assert_eq!(types, expected_types);
        }

        #[test]
        fn test_param_keys_with_symbols_keep_original_spelling() {
            let ramp = find_child(root("Model"), "Ramp").expect("Model.Ramp is registered");
            let keys: Vec<&str> = ramp.fields.iter().map(|field| field.key.as_str()).collect();
            assert_eq!(keys, vec!["dv/dt_r", "dv/dt_f", "r_load"]);
            assert_eq!(ramp.fields[0].param_type, ParamType::Corner);
            assert_eq!(ramp.fields[2].param_type, ParamType::Quantity);

            let model_spec =
                find_child(root("Model"), "Model Spec").expect("Model.Model_Spec is registered");
            let has_signed_key = model_spec.fields.iter().any(|field| field.key == "vinh+");
            assert!(has_signed_key, "`vinh+` must keep its original spelling");
        }

        #[test]
        fn test_package_params_are_corner_triples() {
            let package =
                find_child(root("Component"), "Package").expect("Component.Package is registered");
            let keys: Vec<&str> = package.fields.iter().map(|field| field.key.as_str()).collect();
            assert_eq!(keys, vec!["r_pkg", "l_pkg", "c_pkg"]);
            let all_corner = package.fields.iter().all(|field| {
                field.param_type == ParamType::Corner
            });
            assert!(all_corner);
        }

        #[test]
        fn test_reference_sections_use_corner_header() {
            let model = root("Model");
            let corner_keywords = [
                "Temperature Range",
                "Voltage Range",
                "Pullup Reference",
                "Pulldown Reference",
                "Power Clamp Reference",
                "GND Clamp Reference",
                "External Reference",
            ];
            for keyword in corner_keywords {
                let section = find_child(model, keyword)
                    .unwrap_or_else(|| panic!("Model.{keyword} must be registered"));
                assert_eq!(section.header_format, HeaderFormat::Corner);
                assert!(section.fields.is_empty());
            }
        }

        #[test]
        fn test_same_keyword_in_different_scopes_stays_isolated() {
            let component = root("Component");
            let component_mapping =
                find_child(component, "Pin Mapping").expect("Component.Pin_Mapping is registered");
            let component_keys: Vec<&str> =
                component_mapping.fields.iter().map(|field| field.key.as_str()).collect();
            assert_eq!(
                component_keys,
                vec![
                    "col0",
                    "pulldown_ref",
                    "pullup_ref",
                    "gnd_clamp_ref",
                    "power_clamp_ref",
                    "ext_ref",
                ]
            );

            let interconnect = root("Interconnect Model Set");
            let interconnect_mapping = find_child(interconnect, "Pin Mapping")
                .expect("Interconnect_Model_Set.Pin_Mapping is registered");
            let interconnect_keys: Vec<&str> =
                interconnect_mapping.fields.iter().map(|field| field.key.as_str()).collect();
            assert_eq!(
                interconnect_keys,
                vec![
                    "col0",
                    "pulldown_ref",
                    "pullup_ref",
                    "gnd_clamp_ref",
                    "power_clamp_ref",
                    "ext_ref",
                ]
            );
        }

        #[test]
        fn test_descendant_lookup_reaches_third_level_sections() {
            let package_model = root("Define Package Model");
            let model_data = find_child(package_model, "Model Data").expect("Model_Data");
            let resistance_matrix = find_child(model_data, "Resistance Matrix");
            assert!(resistance_matrix.is_some());

            // The AST flattens third-level sections into second-level siblings, so a
            // direct child lookup fails while a descendant lookup must succeed.
            assert!(find_child(package_model, "Resistance Matrix").is_none());
            assert!(find_descendant(package_model, "Resistance Matrix").is_some());
            assert!(find_descendant(package_model, "Capacitance Matrix").is_some());
        }

        #[test]
        fn test_required_flags_come_from_schema() {
            let header = file_header_section();
            let ibis_ver = find_child(header, "IBIS Ver").expect("IBIS_Ver is registered");
            assert!(ibis_ver.required);
            let date = find_child(header, "Date").expect("Date is registered");
            assert!(!date.required);

            assert!(root("Component").required);
            assert!(!root("Model Selector").required);

            let manufacturer =
                find_child(root("Component"), "Manufacturer").expect("Manufacturer is registered");
            assert!(manufacturer.required);
        }

        #[test]
        fn test_table_and_textline_formats() {
            let component = root("Component");
            let node_declarations =
                find_child(component, "Node Declarations").expect("Component.Node_Declarations");
            assert_eq!(node_declarations.format, SectionFormat::Table);
            assert_eq!(node_declarations.header_format, HeaderFormat::None);
            assert!(node_declarations.fields.is_empty());

            let notes = find_child(file_header_section(), "Notes").expect("Notes is registered");
            assert_eq!(notes.format, SectionFormat::FileHeader);
            assert_eq!(notes.header_format, HeaderFormat::TextLines);

            let pulldown = find_child(root("Model"), "Pulldown").expect("Model.Pulldown");
            assert_eq!(pulldown.format, SectionFormat::IvTable);
            assert_eq!(pulldown.header_format, HeaderFormat::TableHeader);
            assert!(pulldown.fields.is_empty());
        }

        #[test]
        fn test_iv_curve_sections_use_the_iv_table_format() {
            let iv_keywords = ["Pulldown", "Pullup", "GND Clamp", "Power Clamp"];

            let model = root("Model");
            for keyword in iv_keywords {
                let section = find_child(model, keyword)
                    .unwrap_or_else(|| panic!("Model.{keyword} is registered"));
                assert_eq!(section.format, SectionFormat::IvTable, "Model.{keyword}");
            }

            let submodel = root("Submodel");
            for keyword in iv_keywords {
                let section = find_child(submodel, keyword)
                    .unwrap_or_else(|| panic!("Submodel.{keyword} is registered"));
                assert_eq!(section.format, SectionFormat::IvTable, "Submodel.{keyword}");
            }

            let test_load = root("Test Load");
            for keyword in iv_keywords {
                let section = find_child(test_load, keyword)
                    .unwrap_or_else(|| panic!("Test_Load.{keyword} is registered"));
                assert_eq!(section.format, SectionFormat::IvTable, "Test_Load.{keyword}");
            }

            // The remaining I-V style sections are marked the same way.
            for keyword in ["POWER Table", "GND Table", "Series Current"] {
                let section = find_child(root("Model"), keyword)
                    .unwrap_or_else(|| panic!("Model.{keyword} is registered"));
                assert_eq!(section.format, SectionFormat::IvTable, "Model.{keyword}");
            }
            for keyword in ["Power Pulse Table", "GND Pulse Table"] {
                let section = find_child(root("Submodel"), keyword)
                    .unwrap_or_else(|| panic!("Submodel.{keyword} is registered"));
                assert_eq!(section.format, SectionFormat::IvTable, "Submodel.{keyword}");
            }
            for waveform in ["Rising Waveform", "Falling Waveform"] {
                let waveform_section = find_child(root("Model"), waveform)
                    .unwrap_or_else(|| panic!("Model.{waveform} is registered"));
                let composite_current = find_child(waveform_section, "Composite Current")
                    .unwrap_or_else(|| panic!("Model.{waveform}.Composite_Current is registered"));
                assert_eq!(
                    composite_current.format,
                    SectionFormat::IvTable,
                    "Model.{waveform}.Composite_Current"
                );
            }

            // Ordinary tables keep the plain table format.
            let pin = find_child(root("Component"), "Pin").expect("Component.Pin");
            assert_eq!(pin.format, SectionFormat::Table);
        }

        #[test]
        fn test_waveform_spec_and_submodel_shapes() {
            let model = root("Model");
            for keyword in ["Rising Waveform", "Falling Waveform"] {
                let section = find_child(model, keyword)
                    .unwrap_or_else(|| panic!("Model.{keyword} is registered"));
                assert_eq!(section.format, SectionFormat::VtTable, "Model.{keyword}");
            }

            let submodel = root("Submodel");
            for keyword in ["Rising Waveform", "Falling Waveform"] {
                let section = find_child(submodel, keyword)
                    .unwrap_or_else(|| panic!("Submodel.{keyword} is registered"));
                assert_eq!(section.format, SectionFormat::VtTable, "Submodel.{keyword}");
            }

            let test_data = root("Test Data");
            let waveform_keywords = [
                "Rising Waveform Near",
                "Falling Waveform Near",
                "Rising Waveform Far",
                "Falling Waveform Far",
                "Diff Rising Waveform Near",
                "Diff Falling Waveform Near",
                "Diff Rising Waveform Far",
                "Diff Falling Waveform Far",
            ];
            for keyword in waveform_keywords {
                let section = find_child(test_data, keyword)
                    .unwrap_or_else(|| panic!("Test_Data.{keyword} is registered"));
                assert_eq!(section.format, SectionFormat::VtTable, "Test_Data.{keyword}");
            }

            // `[Model Spec]` sub-parameters are corner triples in IBIS 7.0, and
            // the list with its order must follow the manual.
            let model_spec = find_child(model, "Model Spec").expect("Model.Model_Spec");
            let keys: Vec<&str> = model_spec
                .fields
                .iter()
                .map(|field| field.key.as_str())
                .collect();
            assert_eq!(
                keys,
                vec![
                    "vinh", "vinl", "vinh+", "vinh-", "vinl+", "vinl-",
                    "s_overshoot_high", "s_overshoot_low",
                    "d_overshoot_high", "d_overshoot_low",
                    "d_overshoot_time", "d_overshoot_area_h", "d_overshoot_area_l",
                    "d_overshoot_ampl_h", "d_overshoot_ampl_l",
                    "pulse_high", "pulse_low", "pulse_time",
                    "vmeas", "cref", "rref", "vref",
                    "cref_rising", "cref_falling",
                    "rref_rising", "rref_falling",
                    "vref_rising", "vref_falling",
                    "vmeas_rising", "vmeas_falling",
                    "rref_diff", "cref_diff",
                    "weak_r", "weak_i", "weak_v",
                ]
            );
            let all_corner = model_spec
                .fields
                .iter()
                .all(|field| field.param_type == ParamType::Corner);
            assert!(all_corner, "Model Spec params must be corners");

            // `[Add Submodel]` is its own keyword below `[Model]`.
            let add_submodel = find_child(model, "Add Submodel").expect("Model.Add_Submodel");
            let keys: Vec<&str> = add_submodel
                .fields
                .iter()
                .map(|field| field.key.as_str())
                .collect();
            assert_eq!(keys, vec!["submodel_name", "mode"]);
        }

        #[test]
        fn test_model_selector_and_submodel_shapes() {
            let selector = root("Model Selector");
            assert_eq!(selector.format, SectionFormat::Table);
            assert_eq!(selector.header_format, HeaderFormat::Text);
            assert!(selector.fields.is_empty());

            let submodel = root("Submodel");
            assert_eq!(submodel.header_format, HeaderFormat::Text);
            assert_eq!(submodel.fields.len(), 1);
            assert_eq!(to_snake_key(&submodel.fields[0].key), "submodel_type");
        }
    }
}
