//! CLI definition — `clap` argument model for the `ibis_parser` binary.
//!
//! # Responsibilities
//!
//! - Describe the input file and the `--toml` / `--cm` output flags.
//! - Translate verbosity flags into a [`LogLevel`] for [`Logger`].
//!
//! # Input / Output
//!
//! | Input | Output |
//! |-------|--------|
//! | Process arguments (`argv`) | A parsed [`CliCmds`] value |
//!
//! # Related modules
//!
//! - [`crate::ibis_parser::cli`] — consumes [`CliCmds`] to run the conversion.
//! - [`crate::ibis_parser::logger`] — consumes [`GlobalOptions::log_level`].

// =============================================================================
// Internal notes — the CLI is intentionally flat: one required INPUT plus two
// optional, value-less output flags. Output paths are derived from the input
// stem, so both outputs can be produced from a single IBIS read.
// =============================================================================

use std::path::PathBuf;

use clap::{ArgAction, Args, Parser};

use crate::ibis_parser::logger::LogLevel;

/// Top-level command-line definition.
///
/// Takes one IBIS input file and at least one output target: `--toml` for the
/// TOML intermediate representation and `--cm` for the ngspice Code Model C
/// source. Output paths are derived from the input file stem.
///
/// # Examples
///
/// ```rust
/// use clap::Parser;
/// use ibis_parser::CliCmds;
///
/// let cli_cmds = CliCmds::try_parse_from([
///     "ibis_parser",
///     "-v",
///     "chip.ibs",
///     "--toml",
///     "--cm",
/// ])
/// .expect("arguments should be valid");
/// assert_eq!(cli_cmds.global.verbose, 1);
/// assert!(cli_cmds.toml);
/// assert!(cli_cmds.cm);
/// ```
#[derive(Debug, Clone, Parser)]
#[command(
    name = "ibis_parser",
    version,
    about = "Parse IBIS model files and export TOML and/or ngspice Code Model output.",
    long_about = None,
    propagate_version = true
)]
pub struct CliCmds {
    /// Options shared by every conversion step.
    #[command(flatten)]
    pub global: GlobalOptions,

    /// Path to the input `.ibs` file.
    #[arg(value_name = "INPUT")]
    pub input: PathBuf,

    /// Export the TOML intermediate representation to `<INPUT_STEM>.ibs.toml`.
    #[arg(long)]
    pub toml: bool,

    /// Export the ngspice Code Model C source to `<INPUT_STEM>.cm`.
    #[arg(long = "cm")]
    pub cm: bool,
}

/// Flags that apply to every conversion step.
#[derive(Debug, Clone, Args)]
pub struct GlobalOptions {
    /// Increase log verbosity: `-v` enables verbose, `-vv` enables debug.
    #[arg(short, long, global = true, action = ArgAction::Count)]
    pub verbose: u8,

    /// Suppress all output except errors.
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    /// Disable ANSI colors in log output.
    #[arg(long, global = true)]
    pub no_color: bool,
}

impl GlobalOptions {
    /// Maps the verbosity flags to a [`LogLevel`].
    ///
    /// `--quiet` wins over `-v`; otherwise each `-v` raises the level.
    ///
    /// # Parameters
    ///
    /// * `self` — The parsed global flags.
    ///
    /// # Returns
    ///
    /// * [`LogLevel`] — `Quiet`, `Normal`, `Verbose` or `Debug`.
    ///
    /// # Panics
    ///
    /// Does not panic.
    pub fn log_level(&self) -> LogLevel {
        if self.quiet {
            return LogLevel::Quiet;
        }

        match self.verbose {
            0 => LogLevel::Normal,
            1 => LogLevel::Verbose,
            _ => LogLevel::Debug,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parses_both_output_flags() {
        let cli_cmds = CliCmds::try_parse_from(["ibis_parser", "chip.ibs", "--toml", "--cm"])
            .expect("arguments should parse");

        assert_eq!(cli_cmds.input, PathBuf::from("chip.ibs"));
        assert!(cli_cmds.toml);
        assert!(cli_cmds.cm);
    }

    #[test]
    fn test_cli_requires_input_file() {
        let parse_result = CliCmds::try_parse_from(["ibis_parser", "--toml"]);

        assert!(parse_result.is_err(), "INPUT must be required");
    }

    #[test]
    fn test_log_level_maps_quiet_flag() {
        let options = GlobalOptions {
            verbose: 2,
            quiet: true,
            no_color: false,
        };

        assert_eq!(options.log_level(), LogLevel::Quiet);
    }

    #[test]
    fn test_log_level_maps_verbose_count() {
        let normal = GlobalOptions {
            verbose: 0,
            quiet: false,
            no_color: false,
        };
        let verbose = GlobalOptions {
            verbose: 1,
            quiet: false,
            no_color: false,
        };
        let debug = GlobalOptions {
            verbose: 2,
            quiet: false,
            no_color: false,
        };

        assert_eq!(normal.log_level(), LogLevel::Normal);
        assert_eq!(verbose.log_level(), LogLevel::Verbose);
        assert_eq!(debug.log_level(), LogLevel::Debug);
    }
}
