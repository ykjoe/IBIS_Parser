//! IBIS Parser — Parse IBIS chip model description files and convert them to TOML.
//!
//! This library provides a complete IBIS 7.0 parsing pipeline, including lexical
//! analysis, syntax analysis, and TOML serialization.
//!
//! # Architecture
//!
//! The pipeline is split across two crates in a Cargo workspace:
//!
//! - `ibis2ibstoml` — Format reshaping + semantic layer (external crate). Its
//!   `frontend` module uses a PEST grammar (with both lexical primitives and
//!   specific keyword rules) for full parsing; its `backend` module rebuilds the
//!   AST into a numerically-typed strong model (`IBIS_File`) through a
//!   three-step pipeline (`keyword_valid` → `symbol_table_build` → `data_valid`)
//!   with zero string pollution. Re-exported below as `ibis2ibstoml` for
//!   compatibility.
//! - [`ibis_parser`] — Re-export compatibility layer: `ibis_parser::ibis_structure`
//!   re-exports the numerically-typed strong model from the `ibis2ibstoml` backend.
//!
//! # Quick Start
//!
//! ```rust
//! use ibis2ibstoml::parse_to_toml_lenient;
//!
//! let (toml_output, _report) = parse_to_toml_lenient(
//!     "[IBIS ver] 2.1\n[File name] chip.ibs\n[File Rev] 1.0\n\
//!      [Component] MyChip\n[Manufacturer] Acme\n[End]\n",
//! )
//! .expect("parsing failed");
//! assert!(toml_output.contains("ibis_ver"));
//! ```

pub mod ibis_parser;

pub use ibis2ibstoml;
