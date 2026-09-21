//! The single loader of the IBIS 7.0 structural specification.
//!
//! This module turns `ibis_schema.toml` into a [`SectionSpec`] tree: `[X]` vs
//! `[[X]]` gives the **single / multiple** occurrence ([`Occurrence`]),
//! `__schema__.format` gives **how header / params / text body are organized**
//! ([`SectionFormat`]), `__header__.format` gives **how the text right after the
//! keyword is organized** ([`HeaderFormat`]), and `__param__` gives the **named
//! sub-parameters with their value types** ([`FieldSpec`] / [`ParamType`]).
//!
//! # Layout
//!
//! | File | Responsibility |
//! |------|----------------|
//! | `spec.rs` | the semantics (`types`) and the syntax (`naming`) of the schema: every type, every name and every fixed value list |
//! | `loader.rs` | `ibis_schema.toml` → cached `SectionSpec` tree (`include_str!` + `OnceLock`) |
//! | `lookup.rs` | the query primitives consumed by the rest of the pipeline |
//!
//! Every IBIS structure constant and every value the schema talks about is defined
//! here, so no frontend, backend or emitter module spells one of its own.
//!
//! # Examples
//!
//! ```rust
//! use ibis2ibstoml::schema::{find_child, find_level, find_root, Occurrence};
//!
//! let component = find_root("Component").expect("Component is registered");
//! assert_eq!(component.occurrence, Occurrence::Multiple);
//!
//! let pin = find_child(component, "Pin").expect("Component.Pin is registered");
//! // `col0` names the leading pin-name column that the manual's header line omits.
//! assert_eq!(pin.fields.len(), 6);
//! assert_eq!(pin.fields[0].key, "col0");
//!
//! // The nesting level of a keyword is a lookup, not a field of the spec.
//! assert_eq!(find_level("Pin"), Some(2));
//! ```

// =============================================================================
// schema — the single loader of the IBIS 7.0 structural specification
//            (`ibis_schema.toml` → `SectionSpec` tree)
//
// This directory parses `ibis_schema.toml` (the only structural data source) once,
// on first use, into a strongly-typed `SectionSpec` tree. It is read-only for
// every consumer: the frontend uses it to classify file-header fields and to read
// the level of a keyword, and the backend uses it for preprocess / content_parse /
// validation. Changing the specification means editing the TOML only — never this
// code.
//
// Boundary conventions:
//   - Read-only: the schema is never written back, and no semantic validation
//     happens here;
//   - Order-preserving: relies on the `toml` crate's `preserve_order` feature so
//     that the declaration order of `__param__` is the field order;
//   - Case-insensitive with `_` equivalent to a space: every lookup goes through
//     `normalize_keyword`.
// =============================================================================

mod loader;
mod lookup;
mod spec;

pub use lookup::{
    file_header_section, find_child, find_descendant, find_field, find_level, find_root,
    load_schema,
};
pub use spec::{
    keyword_level, normalize_keyword, to_snake_key, Corner, FieldSpec, HeaderFormat, Occurrence,
    ParamType, ParsedField, ParsedTable, ParsedValue, SectionFormat, SectionSpec, CORNER_NAMES,
    FILE_HEADER_CONTAINER, IV_COLUMN_NAMES, TERMINATOR_KEYWORD, UNMATCHED_LINES_KEY,
    VT_COLUMN_NAMES,
};
