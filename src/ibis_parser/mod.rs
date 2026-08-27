//! IBIS numerical strong-type definitions (re-export compatibility layer).
//!
//! Re-exports the **numerically-typed** strong model from the `ibis2ibstoml`
//! `schema::keyword_hierarchy` so that the `ibis_parser::schema::keyword_hierarchy`
//! path stays usable by older consumers.
//!
//! All electrical quantities are `f64` (zero string pollution: `1.12p` →
//! `1.12e-12`); `NA` maps to `None`. See
//! [`ibis2ibstoml::schema::keyword_hierarchy`](ibis2ibstoml::schema::keyword_hierarchy)
//! for the authoritative definitions.

pub use ibis2ibstoml::schema::keyword_hierarchy;

// 兼容旧引用路径 `ibis_parser::ibis_parser::model`。
pub use ibis2ibstoml::schema::keyword_hierarchy as model;
