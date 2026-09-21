//! Schema loading — embed `ibis_schema.toml` and parse it into a cached tree.
//!
//! The TOML is embedded at compile time and parsed once per process, so every
//! consumer reads the same immutable [`SectionSpec`] tree.
//!
//! The file is organized into three inline modules, plus the two accessors the
//! rest of the schema exposes:
//!
//! | Inline module | Responsibility |
//! |---------------|----------------|
//! | `cache` | the embedded source, the `OnceLock` cache and the top-level split |
//! | `section` | one TOML table → one `SectionSpec` (metadata, params, children) |
//! | `decode` | the metadata strings → the schema enums |
//!
//! [`top_level_sections`] and [`file_header_container`] are the only things
//! `lookup` needs from here.

// =============================================================================
// loader — `ibis_schema.toml` → cached `SectionSpec` tree
//
// Design constraints:
//   - The source is a compile-time asset (`include_str!`), so every failure is a
//     `panic!` with the offending section named: a malformed schema is a
//     programming error, never a runtime condition;
//   - Parsing happens once (`OnceLock`): lookups never re-read the TOML;
//   - Declaration order is preserved throughout, because the order of `__param__`
//     and of the child sections is meaningful downstream.
// =============================================================================

use super::spec::SectionSpec;

/// Returns the top-level keyword sections of the cached schema.
///
/// Takes no argument; returns them in `ibis_schema.toml` declaration order,
/// excluding the file header entries.
pub(super) fn top_level_sections() -> &'static [SectionSpec] {
    let schema_tables = cache::tables();
    &schema_tables.sections
}

/// Returns the virtual `[File_Header]` container of the cached schema.
///
/// Takes no argument; returns the container that groups the nine header entries.
pub(super) fn file_header_container() -> &'static SectionSpec {
    let schema_tables = cache::tables();
    &schema_tables.file_header
}

/// Schema cache — embed the TOML source and keep the parsed tree for the process.
mod cache {
    use std::sync::OnceLock;

    use crate::schema::spec::{
        FieldSpec, HeaderFormat, Occurrence, ParamType, SectionFormat, SectionSpec,
        FILE_HEADER_CONTAINER,
    };

    use super::section::{build_section, split_section_value};

    /// The schema source, embedded at compile time and parsed only once per process.
    const SCHEMA_SOURCE: &str = include_str!("ibis_schema.toml");

    /// Process-wide cache of the parsed schema.
    static SCHEMA: OnceLock<SchemaTables> = OnceLock::new();

    /// The parsed schema: top-level sections plus the virtual file header container.
    #[derive(Debug, Clone, PartialEq)]
    pub(super) struct SchemaTables {
        pub(super) sections: Vec<SectionSpec>,   // Top-level keywords, excluding file header entries.
        pub(super) file_header: SectionSpec,     // Virtual `[File_Header]` container grouping those entries.
    }

    /// Returns the cached schema, parsing it on first use.
    ///
    /// Takes no argument; returns the process-wide [`SchemaTables`]. Panics when
    /// the embedded schema is malformed.
    pub(super) fn tables() -> &'static SchemaTables {
        SCHEMA.get_or_init(|| build_tables(SCHEMA_SOURCE))
    }

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
}

/// Section reading — turn one TOML table into one [`SectionSpec`].
mod section {
    use crate::schema::spec::{
        is_metadata_key, FieldSpec, HeaderFormat, Occurrence, SectionFormat, SectionSpec,
    };

    use super::decode::{parse_header_format, parse_param_type, parse_section_format};

    /// Reads one TOML entry as a section table plus its occurrence.
    ///
    /// Takes a `keyword` and its TOML `value`; returns the section table together
    /// with `Occurrence::Multiple` for `[[X]]` and `Occurrence::Once` for `[X]`.
    pub(super) fn split_section_value<'a>(
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
    pub(super) fn build_section(
        name: &str,
        table: &toml::Table,
        occurrence: Occurrence,
    ) -> SectionSpec {
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
}

/// Value decoding — turn the metadata strings into the schema enums.
///
/// The accepted spellings are not repeated here: every enum owns its own
/// vocabulary in its `ALL` table, so this module only resolves a spelling into its
/// variant and, when it cannot, reports that same vocabulary.
mod decode {
    use crate::schema::spec::{HeaderFormat, ParamType, SectionFormat};

    /// Renders the spellings one vocabulary accepts, for an error message.
    ///
    /// Takes one enum's `ALL` table; returns its spellings joined with commas.
    fn known_spellings<T>(all: &[(&'static str, T)]) -> String {
        let spellings: Vec<&str> = all.iter().map(|(spelling, _)| *spelling).collect();
        spellings.join(", ")
    }

    /// Parses `__schema__.format` into [`SectionFormat`].
    ///
    /// Takes a section `name` and its raw `format`; returns the parsed enum, and
    /// panics when the schema spells a format this build does not know.
    pub(super) fn parse_section_format(section: &str, format_raw: &str) -> SectionFormat {
        SectionFormat::from_schema_str(format_raw).unwrap_or_else(|| {
            let known = known_spellings(&SectionFormat::ALL);
            panic!(
                "ibis_schema.toml: `__schema__.format = \"{format_raw}\"` of `{section}` is unknown (known: {known})"
            );
        })
    }

    /// Parses `__header__.format` into [`HeaderFormat`].
    ///
    /// Takes a section `name` and its raw `format`; returns the parsed enum, and
    /// panics when the schema spells a header format this build does not know.
    pub(super) fn parse_header_format(section: &str, format_raw: &str) -> HeaderFormat {
        HeaderFormat::from_schema_str(format_raw).unwrap_or_else(|| {
            let known = known_spellings(&HeaderFormat::ALL);
            panic!(
                "ibis_schema.toml: `__header__.format = \"{format_raw}\"` of `{section}` is unknown (known: {known})"
            );
        })
    }

    /// Parses one `__param__` value into a [`ParamType`].
    ///
    /// Takes the section `name`, the param `key` and the raw parameter `value`;
    /// returns the parsed enum, and panics when the schema spells a param type this
    /// build does not know.
    pub(super) fn parse_param_type(section: &str, key: &str, value: &toml::Value) -> ParamType {
        let type_raw = value.as_str().unwrap_or_else(|| {
            panic!("ibis_schema.toml: param `{key}` of `{section}` must be a type string");
        });
        ParamType::from_schema_str(type_raw).unwrap_or_else(|| {
            let known = known_spellings(&ParamType::ALL);
            panic!(
                "ibis_schema.toml: param `{key}` of `{section}` has unknown type `{type_raw}` (known: {known})"
            );
        })
    }
}
