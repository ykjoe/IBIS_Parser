//! Output module — export the AST tree to various target formats.
//!
//! # Submodules
//!
//! - [`toml`] — Generate a TOML string from the
//!   [`SectionNode`](crate::frontend::ast_builder::SectionNode) tree.
//!
//! # Design constraints
//!
//! - All values are preserved as raw strings; the output layer performs no
//!   numeric conversion.
//! - Only TOML is currently supported; JSON / YAML etc. may be added later.

pub mod toml;

pub use toml::serialize_tree;
pub use toml::serialize_tree_to_string;
