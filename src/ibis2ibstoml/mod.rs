//! First-pass IBIS-to-TOML conversion pipeline.
//!
//! This module implements the "format reshaping layer" of the IBIS parser:
//! raw IBIS text → PEST tokenization → keyword classification → section tree → TOML.
//!
//! # Submodules
//!
//! - [`core`] — Top-level orchestration: read file, call pipeline.
//! - [`lexical_analy`] — PEST-based lexical analysis yielding [`Token`](lexical_analy::Token)s.
//! - [`syntax_analy`] — Keyword classification, tree building, TOML serialisation.
//!
//! # Design constraint
//!
//! All values are preserved as raw strings; no numerical conversion, unit scaling,
//! or semantic validation is performed at this layer.

pub mod core;
pub mod lexical_analy;
pub mod syntax_analy;
