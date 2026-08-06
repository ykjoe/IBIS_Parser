//! Low-level infrastructure — cross-stage shared data types for this crate.
//!
//! # Design constraints
//!
//! `core` hosts only "low-level infrastructure" (e.g., the pest-generated rule
//! enum [`Rule`]); it does **not** carry any business-stage data:
//!
//! - AST data structures (`NodeKind` / `SectionNode` / `ParsedBlock`) → [`frontend::ast_builder`]
//! - Lexical / syntax / recovery logic → [`frontend::lexical_analysis`] / [`frontend::syntax_analysis`] / [`frontend::recovery`]

pub use crate::frontend::Rule;
