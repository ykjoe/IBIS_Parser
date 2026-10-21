//! Logger — Level-filtered, colorized terminal output.
//!
//! # Responsibilities
//!
//! - Map the global verbosity flags to a [`LogLevel`].
//! - Prefix every message with a colored tag (`[INFO]`, `[WARN]`, ...).
//! - Send errors and warnings to stderr, progress messages to stdout.
//!
//! # Input / Output
//!
//! | Input | Output |
//! |-------|--------|
//! | Message text plus a [`LogLevel`] | A single colored line on stdout / stderr |
//!
//! # Related modules
//!
//! - [`crate::ibis_parser::cli_commands`] — produces the verbosity selected by
//!   the user.
//! - [`crate::ibis_parser::cli`] — consumes [`Logger`] for pipeline output.

// =============================================================================
// Internal notes — `colored` already honors `NO_COLOR` and non-TTY output; an
// explicit `--no-color` therefore only needs to force the override to `false`.
// The override is process-wide, which is fine for a CLI.
// =============================================================================

use colored::{Color, Colorize};

/// Verbosity threshold of a [`Logger`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Quiet,   // Only errors are printed.
    Normal,  // Errors, warnings, info and success messages are printed.
    Verbose, // Additionally prints `[VERBOSE]` messages.
    Debug,   // Additionally prints `[DEBUG]` messages.
}

/// A minimal console logger with colored severity tags.
#[derive(Debug, Clone)]
pub struct Logger {
    log_level: LogLevel, // Messages below this threshold are discarded.
}

impl Logger {
    /// Creates a logger for the given verbosity and color preference.
    ///
    /// # Parameters
    ///
    /// * `log_level` — Minimum severity that will be printed.
    /// * `no_color` — When `true`, ANSI escapes are disabled process-wide.
    ///
    /// # Returns
    ///
    /// * [`Logger`] — The configured logger.
    ///
    /// # Panics
    ///
    /// Does not panic.
    pub fn new(log_level: LogLevel, no_color: bool) -> Self {
        if no_color {
            colored::control::set_override(false);
        }

        Self { log_level }
    }

    /// Prints an error message to stderr. Shown even with `--quiet`.
    ///
    /// # Parameters
    ///
    /// * `message` — Text printed after the red `[ERROR]` tag.
    ///
    /// # Returns
    ///
    /// * `()` — Nothing.
    ///
    /// # Panics
    ///
    /// Panics only if writing to stderr fails, for example on a closed pipe.
    pub fn error(&self, message: impl AsRef<str>) {
        self.emit(
            LogLevel::Quiet,
            true,
            "[ERROR]",
            Color::Red,
            message.as_ref(),
        );
    }

    /// Prints a warning to stderr. Hidden with `--quiet`.
    ///
    /// # Parameters
    ///
    /// * `message` — Text printed after the yellow `[WARN]` tag.
    ///
    /// # Returns
    ///
    /// * `()` — Nothing.
    ///
    /// # Panics
    ///
    /// Panics only if writing to stderr fails, for example on a closed pipe.
    pub fn warn(&self, message: impl AsRef<str>) {
        self.emit(
            LogLevel::Normal,
            true,
            "[WARN]",
            Color::Yellow,
            message.as_ref(),
        );
    }

    /// Prints an informational message to stdout. Hidden with `--quiet`.
    ///
    /// # Parameters
    ///
    /// * `message` — Text printed after the cyan `[INFO]` tag.
    ///
    /// # Returns
    ///
    /// * `()` — Nothing.
    ///
    /// # Panics
    ///
    /// Panics only if writing to stdout fails, for example on a closed pipe.
    pub fn info(&self, message: impl AsRef<str>) {
        self.emit(
            LogLevel::Normal,
            false,
            "[INFO]",
            Color::Cyan,
            message.as_ref(),
        );
    }

    /// Prints a success message to stdout. Hidden with `--quiet`.
    ///
    /// # Parameters
    ///
    /// * `message` — Text printed after the green `[SUCCESS]` tag.
    ///
    /// # Returns
    ///
    /// * `()` — Nothing.
    ///
    /// # Panics
    ///
    /// Panics only if writing to stdout fails, for example on a closed pipe.
    pub fn success(&self, message: impl AsRef<str>) {
        self.emit(
            LogLevel::Normal,
            false,
            "[SUCCESS]",
            Color::Green,
            message.as_ref(),
        );
    }

    /// Prints a verbose message to stdout. Requires `-v`.
    ///
    /// # Parameters
    ///
    /// * `message` — Text printed after the blue `[VERBOSE]` tag.
    ///
    /// # Returns
    ///
    /// * `()` — Nothing.
    ///
    /// # Panics
    ///
    /// Panics only if writing to stdout fails, for example on a closed pipe.
    pub fn verbose(&self, message: impl AsRef<str>) {
        self.emit(
            LogLevel::Verbose,
            false,
            "[VERBOSE]",
            Color::Blue,
            message.as_ref(),
        );
    }

    /// Prints a debug message to stdout. Requires `-vv`.
    ///
    /// # Parameters
    ///
    /// * `message` — Text printed after the magenta `[DEBUG]` tag.
    ///
    /// # Returns
    ///
    /// * `()` — Nothing.
    ///
    /// # Panics
    ///
    /// Panics only if writing to stdout fails, for example on a closed pipe.
    pub fn debug(&self, message: impl AsRef<str>) {
        self.emit(
            LogLevel::Debug,
            false,
            "[DEBUG]",
            Color::Magenta,
            message.as_ref(),
        );
    }

    /// Writes a tagged message when `required_level` is enabled.
    ///
    /// Takes the severity threshold, destination, tag, tag color and text.
    fn emit(
        &self,
        required_level: LogLevel,
        use_stderr: bool,
        prefix: &str,
        color: Color,
        message: &str,
    ) {
        if self.log_level < required_level {
            return;
        }

        let painted_prefix = prefix.color(color).bold();
        if use_stderr {
            eprintln!("{} {}", painted_prefix, message);
        } else {
            println!("{} {}", painted_prefix, message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_level_ordering_matches_verbosity() {
        assert!(LogLevel::Quiet < LogLevel::Normal);
        assert!(LogLevel::Normal < LogLevel::Verbose);
        assert!(LogLevel::Verbose < LogLevel::Debug);
    }

    #[test]
    fn test_logger_new_keeps_requested_level() {
        let logger = Logger::new(LogLevel::Normal, false);
        assert_eq!(logger.log_level, LogLevel::Normal);
    }
}
