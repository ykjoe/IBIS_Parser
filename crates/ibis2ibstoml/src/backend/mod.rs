//! Backend module — reserved semantic processing / data-conversion interface.
//!
//! Not yet implemented. It will later consume the
//! [`SectionNode`](crate::frontend::SectionNode) tree produced by the frontend
//! to perform semantic analysis, numeric conversion, unit scaling, and
//! `[[array-of-tables]]` detection before handing off to
//! [`emitter`](crate::emitter) for output.
//!
//! # Design constraints
//!
//! - The frontend does no semantic analysis; the backend does no text parsing.
//! - Stages communicate through public APIs; cross-layer references to internal
//!   types are forbidden.
//!
//! This is currently a placeholder; semantic processing and data conversion
//! are planned for a later milestone.
