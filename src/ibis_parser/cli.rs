//! Conversion pipeline — orchestration from one IBIS input to output files.
//!
//! # Responsibilities
//!
//! - Validate that at least one output target (`--toml` / `--cm`) was selected.
//! - Collect the requested [`OutputTarget`] values into a `Vec`.
//! - Read the input IBIS file once, then loop over the targets.
//! - Derive `<INPUT_STEM>.ibs.toml` / `<INPUT_STEM>.cm`, render and write them.
//! - Report progress through [`Logger`] and failures through [`PipelineError`].
//!
//! # Input / Output
//!
//! | Input | Output |
//! |-------|--------|
//! | [`CliCmds`] plus [`Logger`] | Files on disk, `Result<(), PipelineError>` |
//!
//! # Related modules
//!
//! - [`crate::ibis_parser::cli_commands`] — provides the input path and output
//!   targets.
//! - [`crate::ibis_parser::logger`] — receives progress and status messages.

// =============================================================================
// Internal notes — helpers are grouped into three private modules:
//   - `targets` — flag → OutputTarget selection (table-driven);
//   - `output`  — path derivation, file writing and logging;
//   - `render`  — target-specific document/source generation.
// Adding a new format touches `targets::OUTPUT_TARGETS`, the `OutputTarget`
// enum, `targets::{suffix, label}`, `render`, and the matching CLI flag.
// =============================================================================

use std::fs;

use crate::ibis_parser::cli_commands::CliCmds;
use crate::ibis_parser::logger::Logger;

pub use targets::OutputTarget;

/// Output-target selection — CLI flags in, `Vec<OutputTarget>` out.
///
/// Adding a target requires one enum variant, one suffix/label arm and one row
/// in [`OUTPUT_TARGETS`]; the collection loop itself stays untouched.
mod targets {
    //! Flag-driven output-target selection.

    use super::PipelineError;
    use crate::ibis_parser::cli_commands::CliCmds;

    /// Export targets supported by the pipeline.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum OutputTarget {
        Toml,      // Structured TOML intermediate representation (`ibstoml`).
        CodeModel, // ngspice Code Model C source.
    }

    /// Every supported target together with the CLI flag that selects it.
    ///
    /// Declaration order fixes the processing order. A new CLI output flag only
    /// needs one row here plus the matching enum/render arms.
    const OUTPUT_TARGETS: &[(OutputTarget, fn(&CliCmds) -> bool)] = &[
        (OutputTarget::Toml, |cli_cmds| cli_cmds.toml),
        (OutputTarget::CodeModel, |cli_cmds| cli_cmds.cm),
    ];

    impl OutputTarget {
        /// File suffix appended to the input stem for this target.
        pub(super) fn suffix(self) -> &'static str {
            match self {
                Self::Toml => ".ibs.toml",
                Self::CodeModel => ".cm",
            }
        }

        /// Human-readable label used in log messages.
        pub(super) fn label(self) -> &'static str {
            match self {
                Self::Toml => "TOML",
                Self::CodeModel => "Code Model",
            }
        }
    }

    /// Collects the output targets selected on the command line.
    ///
    /// Takes the parsed `cli_cmds`; returns the requested targets in declaration
    /// order, or a usage error when no target was selected.
    pub(super) fn collect(cli_cmds: &CliCmds) -> Result<Vec<OutputTarget>, PipelineError> {
        let mut selected_targets = Vec::new();
        for &(target, is_selected) in OUTPUT_TARGETS {
            if is_selected(cli_cmds) {
                selected_targets.push(target);
            }
        }

        if selected_targets.is_empty() {
            return Err(PipelineError::usage(
                "no output target selected; pass --toml, --cm, or both",
            ));
        }

        Ok(selected_targets)
    }
}

/// Failure returned by the conversion pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineError {
    pub exit_code: u8,   // Process exit code the CLI should return.
    pub message: String, // Human-readable failure description.
}

impl PipelineError {
    /// Builds an error with an explicit exit code.
    ///
    /// # Parameters
    ///
    /// * `exit_code` — Process exit code to return.
    /// * `message` — Human-readable failure description.
    ///
    /// # Returns
    ///
    /// * [`PipelineError`] — The constructed error.
    ///
    /// # Panics
    ///
    /// Does not panic.
    pub fn new(exit_code: u8, message: impl Into<String>) -> Self {
        Self {
            exit_code,
            message: message.into(),
        }
    }

    /// Builds an error for a command line that selects no output target.
    ///
    /// # Parameters
    ///
    /// * `message` — Human-readable usage description.
    ///
    /// # Returns
    ///
    /// * [`PipelineError`] — The constructed error with `exit_code == 2`.
    ///
    /// # Panics
    ///
    /// Does not panic.
    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(EXIT_CODE_USAGE, message)
    }

    /// Builds an error for a failed read, parse or write.
    ///
    /// # Parameters
    ///
    /// * `message` — Human-readable failure description.
    ///
    /// # Returns
    ///
    /// * [`PipelineError`] — The constructed error with `exit_code == 1`.
    ///
    /// # Panics
    ///
    /// Does not panic.
    pub fn failure(message: impl Into<String>) -> Self {
        Self::new(EXIT_CODE_FAILURE, message)
    }
}

/// Exit code used when the command line selects no output target.
const EXIT_CODE_USAGE: u8 = 2;

/// Exit code used when reading, parsing or writing fails.
const EXIT_CODE_FAILURE: u8 = 1;

/// Runs every requested conversion for one IBIS input file.
///
/// Output paths are always derived from the input stem: `Toml` writes
/// `<INPUT_STEM>.ibs.toml` and `CodeModel` writes `<INPUT_STEM>.cm`, in the same
/// directory as the input file.
///
/// # Parameters
///
/// * `cli_cmds` — Parsed command line with the input path and output targets.
/// * `logger` — Logger used for progress and success messages.
///
/// # Returns
///
/// * `Ok(())` — Every requested output was written.
/// * `Err(PipelineError)` — No output target was selected, or a step failed.
///
/// # Errors
///
/// Returns [`PipelineError::usage`] when neither `--toml` nor `--cm` is set, and
/// [`PipelineError::failure`] when the input cannot be read, parsed or written.
pub fn run_pipeline(cli_cmds: &CliCmds, logger: &Logger) -> Result<(), PipelineError> {
    let selected_targets = targets::collect(cli_cmds)?;

    logger.verbose(format!("reading {}", cli_cmds.input.display()));
    let ibis_content = fs::read_to_string(&cli_cmds.input).map_err(|error| {
        PipelineError::failure(format!(
            "failed to read {}: {error}",
            cli_cmds.input.display()
        ))
    })?;

    for target in selected_targets {
        output::write(&ibis_content, &cli_cmds.input, target, logger)?;
    }

    Ok(())
}

/// Output writing — path derivation, rendering dispatch and file writes.
mod output {
    //! One target in, one file on disk out.

    use std::fs;
    use std::path::{Path, PathBuf};

    use super::PipelineError;
    use super::render;
    use super::targets::OutputTarget;
    use crate::ibis_parser::logger::Logger;

    /// Renders one target and writes it next to `input_path`.
    ///
    /// Takes the input text, the source path, the target kind and the logger;
    /// returns `Ok(())` after the file is written.
    pub(super) fn write(
        ibis_content: &str,
        input_path: &Path,
        target: OutputTarget,
        logger: &Logger,
    ) -> Result<(), PipelineError> {
        let output_path = derived_output_path(input_path, target.suffix());

        logger.info(format!(
            "converting IBIS to {}: {}",
            target.label(),
            output_path.display()
        ));

        let output_content = render::render(ibis_content, input_path, target, logger)?;

        fs::write(&output_path, output_content).map_err(|error| {
            PipelineError::failure(format!(
                "failed to write {}: {error}",
                output_path.display()
            ))
        })?;

        logger.success(format!(
            "wrote {} output to {}",
            target.label(),
            output_path.display()
        ));
        Ok(())
    }

    /// Builds the output path next to `input_path` by appending `suffix` to its
    /// stem.
    ///
    /// `input_path` of `f103c8.ibs` with `suffix` `.cm` yields `f103c8.cm`.
    fn derived_output_path(input_path: &Path, suffix: &str) -> PathBuf {
        let input_stem = input_path.file_stem().unwrap_or_default();
        let mut output_name = input_stem.to_os_string();
        output_name.push(suffix);

        input_path.with_file_name(output_name)
    }
}

/// Rendering — target-specific document/source generation.
mod render {
    //! One target in, one rendered string out.

    use std::path::Path;

    use ibis2ibstoml::frontend::{self, SectionNode};

    use super::PipelineError;
    use super::targets::OutputTarget;
    use crate::ibis_parser::logger::Logger;

    /// Dispatches to the renderer that matches `target`.
    pub(super) fn render(
        ibis_content: &str,
        input_path: &Path,
        target: OutputTarget,
        logger: &Logger,
    ) -> Result<String, PipelineError> {
        match target {
            OutputTarget::Toml => render_toml_document(ibis_content),
            OutputTarget::CodeModel => render_code_model(ibis_content, input_path, logger),
        }
    }

    /// Renders the TOML intermediate representation.
    fn render_toml_document(ibis_content: &str) -> Result<String, PipelineError> {
        ibis2ibstoml::parse_to_toml(ibis_content).map_err(|error| {
            PipelineError::failure(format!("IBIS -> TOML conversion failed: {error}"))
        })
    }

    /// Parses the IBIS AST and renders the Code Model C source.
    ///
    /// The Code Model renderer is still a placeholder, so the emitted source
    /// carries metadata comments only; a warning is logged to make that
    /// explicit.
    fn render_code_model(
        ibis_content: &str,
        input_path: &Path,
        logger: &Logger,
    ) -> Result<String, PipelineError> {
        logger.verbose(format!(
            "parsing {} AST for Code Model",
            input_path.display()
        ));

        let section_tree = frontend::parse(ibis_content).map_err(|error| {
            PipelineError::failure(format!("IBIS -> AST conversion failed: {error}"))
        })?;

        logger.warn("Code Model generator is a placeholder; writing metadata only");

        Ok(render_code_model_source(&section_tree, input_path))
    }

    /// Renders a C source placeholder from the parsed AST.
    fn render_code_model_source(section_tree: &[SectionNode], input_path: &Path) -> String {
        let mut source = String::new();
        source.push_str("/* ngspice Code Model generated by ibis_parser.\n");
        source.push_str(" *\n");
        source.push_str(&format!(" * Input IBIS file: {}\n", input_path.display()));
        source.push_str(&format!(
            " * AST root sections: {}\n",
            section_tree.len()
        ));
        source.push_str(" *\n");
        source.push_str(" * TODO(Zoo): translate IBIS model data into an ngspice code model.\n");
        source.push_str(" */\n");
        source
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use clap::Parser;

    use crate::ibis_parser::logger::LogLevel;

    /// Builds a unique path inside the system temp directory.
    fn unique_temp_path(file_name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let unique_name = format!("ibis_parser_{}_{}_{}", std::process::id(), nanos, file_name);
        std::env::temp_dir().join(unique_name)
    }

    #[test]
    fn test_output_target_suffixes() {
        assert_eq!(OutputTarget::Toml.suffix(), ".ibs.toml");
        assert_eq!(OutputTarget::CodeModel.suffix(), ".cm");
    }

    #[test]
    fn test_collect_output_targets_keeps_requested_order() {
        let cli_cmds = CliCmds::try_parse_from(["ibis_parser", "chip.ibs", "--toml", "--cm"])
            .expect("arguments should parse");

        let selected_targets = targets::collect(&cli_cmds).expect("targets should be present");

        assert_eq!(selected_targets, vec![OutputTarget::Toml, OutputTarget::CodeModel]);
    }

    #[test]
    fn test_run_pipeline_requires_output_target() {
        let cli_cmds = CliCmds::try_parse_from(["ibis_parser", "tests/examples/f103c8.ibs"])
            .expect("arguments should parse");
        let logger = Logger::new(LogLevel::Quiet, false);

        let pipeline_result = run_pipeline(&cli_cmds, &logger);

        let pipeline_error = pipeline_result.expect_err("missing output target must fail");
        assert_eq!(pipeline_error.exit_code, EXIT_CODE_USAGE);
        assert!(pipeline_error.message.contains("--toml"));
    }

    #[test]
    fn test_run_pipeline_writes_toml_output_next_to_input() {
        let input_path = unique_temp_path("sample.ibs");
        fs::copy("tests/examples/f103c8.ibs", &input_path).expect("example should be copyable");
        let input_path_text = input_path.to_str().expect("temp path is UTF-8");
        let cli_cmds = CliCmds::try_parse_from(["ibis_parser", input_path_text, "--toml"])
            .expect("arguments should parse");
        let logger = Logger::new(LogLevel::Quiet, false);
        let expected_output_path = input_path.with_extension("ibs.toml");

        run_pipeline(&cli_cmds, &logger).expect("pipeline should succeed");

        let toml_document =
            fs::read_to_string(&expected_output_path).expect("TOML output should exist");
        assert!(toml_document.contains("[File_Header]"), "{toml_document}");
        fs::remove_file(&input_path).expect("temp input should be removable");
        fs::remove_file(&expected_output_path).expect("temp output should be removable");
    }

    #[test]
    fn test_run_pipeline_writes_code_model_output_next_to_input() {
        let input_path = unique_temp_path("sample.ibs");
        fs::copy("tests/examples/f103c8.ibs", &input_path).expect("example should be copyable");
        let input_path_text = input_path.to_str().expect("temp path is UTF-8");
        let cli_cmds = CliCmds::try_parse_from(["ibis_parser", input_path_text, "--cm"])
            .expect("arguments should parse");
        let logger = Logger::new(LogLevel::Quiet, false);
        let expected_output_path = input_path.with_extension("cm");

        run_pipeline(&cli_cmds, &logger).expect("pipeline should succeed");

        let code_model_source =
            fs::read_to_string(&expected_output_path).expect("Code Model output should exist");
        assert!(code_model_source.contains("ngspice Code Model"));
        fs::remove_file(&input_path).expect("temp input should be removable");
        fs::remove_file(&expected_output_path).expect("temp output should be removable");
    }
}
