//! IBIS Parser — Parse IBIS chip model description files and convert them to TOML.
//!
//! This library provides a complete IBIS 7.0 parsing pipeline, including lexical
//! analysis, syntax analysis, and TOML serialization.
//!
//! # Architecture
//!
//! The pipeline uses a frontend/backend architecture:
//!
//! - [`ibis2ibstoml`] — First-pass format reshaping layer. The [`frontend`](ibis2ibstoml::frontend)
//!   module uses a PEST grammar (with both lexical primitives and specific keyword rules)
//!   for full parsing and directly serialises to TOML. All values remain as raw strings;
//!   no numerical conversion or unit scaling is performed.
//! - [`ibis_parser`] — (Planned) Second-pass semantic parsing layer. Converts the TOML
//!   string into strongly-typed AST structures.
//!
//! # Quick Start
//!
//! ```rust
//! use ibis_parser::ibis2ibstoml::core::ibs2ibstoml;
//!
//! let toml_output = ibs2ibstoml("tests/examples/f103c8.ibs")
//!     .expect("parsing failed");
//! assert!(toml_output.contains("ibis_ver"));
//! println!("{}", toml_output);
//! ```

pub mod ibis2ibstoml;
pub mod ibis_parser;
