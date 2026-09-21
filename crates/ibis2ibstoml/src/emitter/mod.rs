// =============================================================================
// emitter — output stage: the parsed tree becomes a TOML document
//
// The serializer implementation lives in `toml.rs`; this module declares it and
// re-exports the entry point, so callers depend on `emitter` only.
//
// The emitter consumes the backend's typed parsed tree (`backend::ParsedNode`):
// it never touches the frontend AST, and it performs no semantic work — every
// value is written exactly as it was parsed (quantities keep their unit).
// =============================================================================

//! Output stage of the pipeline.
//!
//! [`serialize_parsed_tree`] renders the backend's parsed tree into TOML:
//! `[Path.Keyword]` tables, `[[Path.Keyword]]` arrays-of-tables, corner arrays,
//! `{ header, data }` tables and quoted symbolic keys.

pub mod toml;

pub use toml::serialize_parsed_tree;
