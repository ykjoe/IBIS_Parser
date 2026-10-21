//! CLI framework for the `ibis_parser` binary.
//!
//! # Responsibilities
//!
//! - Own the runtime entry point [`run`].
//! - Build the [`Logger`] from the global flags before running the pipeline.
//! - Translate [`PipelineError`](crate::ibis_parser::cli::PipelineError) values
//!   into process exit codes.
//!
//! # Input / Output
//!
//! | Input | Output |
//! |-------|--------|
//! | Parsed [`CliCmds`] value | [`std::process::ExitCode`] |
//!
//! # Related modules
//!
//! - [`cli_commands`] — `clap` argument model.
//! - [`cli`] — IBIS → TOML / Code Model orchestration.
//! - [`logger`] — colored, level-filtered terminal output.

// =============================================================================
// Internal notes — the binary never calls `cli::run_pipeline` directly; routing
// through [`run`] keeps failure logging and exit codes consistent.
// =============================================================================

pub mod cli;
pub mod cli_commands;
pub mod logger;

use std::process::ExitCode;

use crate::ibis_parser::cli_commands::CliCmds;
use crate::ibis_parser::logger::Logger;

/// Executes one parsed command line and maps its outcome to an exit code.
///
/// # Parameters
///
/// * `cli_cmds` — The fully parsed [`CliCmds`] value produced by `clap`.
///
/// # Returns
///
/// * `ExitCode::SUCCESS` — Every requested output was written.
/// * Another `ExitCode` — The pipeline failed; its message has already been
///   printed to stderr.
///
/// # Errors
///
/// Does not return `Result`. Pipeline failures are logged through
/// [`Logger::error`] and converted into the
/// [`PipelineError`](crate::ibis_parser::cli::PipelineError) exit code.
pub fn run(cli_cmds: CliCmds) -> ExitCode {
    let logger = Logger::new(cli_cmds.global.log_level(), cli_cmds.global.no_color);
    let pipeline_result = cli::run_pipeline(&cli_cmds, &logger);

    match pipeline_result {
        Ok(()) => ExitCode::SUCCESS,
        Err(pipeline_error) => {
            logger.error(&pipeline_error.message);
            ExitCode::from(pipeline_error.exit_code)
        }
    }
}
