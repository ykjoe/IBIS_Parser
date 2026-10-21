//! IBIS Parser — command-line entry point.
//!
//! Parses arguments with `clap` and delegates to [`ibis_parser::run`], which
//! builds the logger and runs the conversion pipeline.

use clap::Parser;
use ibis_parser::CliCmds;

fn main() -> std::process::ExitCode {
    let cli_cmds = CliCmds::parse();
    ibis_parser::run(cli_cmds)
}
