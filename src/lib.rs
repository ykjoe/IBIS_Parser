//! IBIS Parser — command-line front end for the `ibis2ibstoml` conversion pipeline.
//!
//! Arguments are parsed with `clap` and progress is reported through a
//! `colored`-based [`Logger`]. One IBIS input file can be exported to a TOML
//! intermediate representation (`--toml`, written to `<INPUT_STEM>.ibs.toml`)
//! and/or an ngspice Code Model C source (`--cm`, written to `<INPUT_STEM>.cm`)
//! by [`cli::run_pipeline`].
//!
//! # Examples
//!
//! Parse a command line without executing the pipeline:
//!
//! ```rust
//! use clap::Parser;
//! use ibis_parser::CliCmds;
//!
//! let cli_cmds = CliCmds::try_parse_from([
//!     "ibis_parser",
//!     "--quiet",
//!     "chip.ibs",
//!     "--toml",
//! ])
//! .expect("arguments should be valid");
//! assert!(cli_cmds.global.quiet);
//! assert!(cli_cmds.toml);
//! ```

// =============================================================================
// Internal notes — this package ships both the `ibis_parser` library and the
// binary. `src/main.rs` only parses `CliCmds` and calls [`run`].
// =============================================================================

pub mod ibis_parser;

pub use crate::ibis_parser::cli::{OutputTarget, PipelineError, run_pipeline};
pub use crate::ibis_parser::cli_commands::{CliCmds, GlobalOptions};
pub use crate::ibis_parser::logger::{LogLevel, Logger};
pub use crate::ibis_parser::run;
