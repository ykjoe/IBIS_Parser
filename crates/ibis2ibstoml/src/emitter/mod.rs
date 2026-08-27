//! Output module — export the numerically-typed strong model to TOML.
//!
//! # Submodules
//!
//! - [`toml`] — Generate a TOML string from the numerically-typed
//!   [`IBIS_File`](crate::backend::ibis_structure::IBIS_File).
//!
//! # Design constraints
//!
//! - The emitter only consumes the numerically-typed strong model produced by
//!   the backend; it never touches the frontend `SectionNode` tree.
//! - Numerical fields are emitted as TOML numbers (`key = 0.1`), not strings.

pub mod toml;

pub use toml::serialize_ibis_file;
