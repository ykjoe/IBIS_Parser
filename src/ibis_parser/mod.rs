//! IBIS semantic structure definitions (planned second-pass layer).
//!
//! This module defines the strongly-typed AST structures that represent a fully
//! parsed IBIS file. These types are the target of the planned semantic analysis
//! phase, which consumes the TOML output from [`super::ibis2ibstoml`] and converts
//! it into validated, type-checked data.

pub mod ibis_structure;
