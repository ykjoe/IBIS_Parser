//! First-pass IBIS-to-TOML conversion pipeline.
//!
//! This module implements the "format reshaping layer" of the IBIS parser:
//! raw IBIS text → pest full parsing → TOML serialization.
//!
//! # Submodules
//!
//! - [`core`] — Top-level orchestration: read file, call pipeline.
//! - [`frontend`] — Pest-based full parsing + TOML serialization (merged frontend).
//! - [`backend`] — (Planned) Semantic analysis layer.
//!
//! # Design constraint
//!
//! All values are preserved as raw strings; no numerical conversion, unit scaling,
//! or semantic validation is performed at this layer.

pub mod core;
pub mod frontend;
