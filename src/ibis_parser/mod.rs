//! IBIS parsing types (re-export compatibility layer).
//!
//! The numerically-typed strong model that used to live in
//! `ibis2ibstoml::schema::keyword_hierarchy` no longer exists: the backend now
//! produces the typed parsed tree (`ibis2ibstoml::backend::ParsedNode` and its
//! value types). The old import paths are kept as aliases so existing consumers
//! keep compiling:
//!
//! - `ibis_parser::keyword_hierarchy` — the backend module with the parsed tree;
//! - `ibis_parser::model` — legacy alias for the same module.
//!
//! Values stay text-only: quantities keep their original spelling and unit
//! (`1.65V` stays `"1.65V"`), corners are `(typ, min, max)` triples and tables are
//! `{ header, data }`.

pub use ibis2ibstoml::backend as keyword_hierarchy;

// 兼容旧引用路径 `ibis_parser::ibis_parser::model`。
pub use ibis2ibstoml::backend as model;
