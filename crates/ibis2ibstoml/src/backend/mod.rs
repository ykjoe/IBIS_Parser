// =============================================================================
// backend — semantic layer: frontend AST → typed parsed tree + validation report
//
// The backend is three phases; each one hands its output to the next, and the last
// one closes the run with the report:
//
//   1. `pre_process`   — tree         → marks   (occurrence, scope path and the schema
//                        spec of every node; unknown keywords are reported);
//   2. `content_parse` — tree + marks → parsed  (every `content` line becomes a typed
//                        field, a corner tuple or a table);
//   3. `validation`    — parsed       → report  (dependent-required keywords and the
//                        shape of corners and structured tables).
//
// The report is the end of the backend: `run_backend` returns the parsed tree and the
// report together, and the public entry points only decide what to do with the
// findings — strict mode fails on the first Error, lenient mode keeps them all.
//
// The frontend AST is read-only here: nothing is written back, and every value is
// rebuilt from the text against `ibis_schema.toml`.
//
// File layout:
//
//   [1] module wiring — the phases plus the value and problem types they share;
//   [2] public API    — the strict / lenient entry points;
//   [3] pipeline      — the three phases and the data they hand over.
// =============================================================================

//! Semantic layer of the pipeline: the frontend AST becomes a typed parsed tree.
//!
//! The three phases — preprocess, content parsing, validation — run in that order and
//! each one feeds the next; the run ends with the [`ValidationReport`].
//!
//! # Examples
//!
//! ```ignore
//! let tree = ibis2ibstoml::frontend::parse("[IBIS ver] 2.1\n")?;
//! let parsed = ibis2ibstoml::backend::semantic_parse(&tree)?;
//! ```

// =============================================================================
// [1] Module Wiring
// =============================================================================

pub(crate) mod content_parse;
pub(crate) mod pre_process;
pub(crate) mod validation;

pub use content_parse::{content_parse, Corner, ParsedField, ParsedNode, ParsedTable, ParsedValue};
pub use validation::{Diagnostic, Rule, Severity, ValidationReport};
pub(crate) use validation::ValidationCollector;

use crate::frontend::SectionNode;

use self::content_parse::content_parse as parse_content;
use self::pre_process::mark_keywords;

// =============================================================================
// [2] Public API
// =============================================================================

/// Runs the backend phases and returns the parsed tree under **strict** rules.
///
/// # Parameters
///
/// * `tree` — Root-level sections produced by the frontend.
///
/// # Returns
///
/// * `Ok(Vec<ParsedNode>)` — The parsed tree with typed fields.
/// * `Err(Diagnostic)` — The first finding of severity `Error` of the run.
///
/// # Errors
///
/// Returns `Err` when a phase reports an error, for example an unknown keyword or a
/// keyword whose `required = true` children are missing.
pub fn semantic_parse(tree: &[SectionNode]) -> Result<Vec<ParsedNode>, Diagnostic> {
    let (parsed, report) = run_backend(tree);
    match report.errors.first() {
        Some(diagnostic) => Err(diagnostic.clone()),
        None => Ok(parsed),
    }
}

/// Runs the backend phases and returns the parsed tree under **lenient** rules.
///
/// # Parameters
///
/// * `tree` — Root-level sections produced by the frontend.
///
/// # Returns
///
/// * `Ok((Vec<ParsedNode>, ValidationReport))` — The parsed tree plus every finding
///   the three phases collected.
/// * `Err(Diagnostic)` — Reserved for problems that make parsing impossible.
///
/// # Errors
///
/// Currently never returns `Err`: every problem is collected instead.
pub fn semantic_parse_lenient(
    tree: &[SectionNode],
) -> Result<(Vec<ParsedNode>, ValidationReport), Diagnostic> {
    Ok(run_backend(tree))
}

// =============================================================================
// [3] Pipeline
// =============================================================================

/// Runs the three phases in order and returns what the backend produces.
///
/// The phases hand their output to the next one, so nothing is recomputed:
///
/// ```text
/// pre_process(tree)         → marks
/// content_parse(tree, marks) → parsed
/// validation(parsed)         → report
/// ```
///
/// Validation closes the run: this function only returns once it has turned the
/// findings of all three phases into the [`ValidationReport`].
///
/// Takes the frontend `tree`; returns the parsed tree plus the report of the run.
fn run_backend(tree: &[SectionNode]) -> (Vec<ParsedNode>, ValidationReport) {
    let mut collector = ValidationCollector::new();

    // ── Phase 1: keyword marking — occurrence, scope path and spec per node ──
    let marks = mark_keywords(tree, &mut collector);

    // ── Phase 2: content parsing — consumes the marks, yields the typed tree ──
    let parsed = parse_content(tree, &marks);

    // ── Phase 3: validation — consumes the parsed tree and closes the run ──
    validation::validate(&parsed, &mut collector);

    (parsed, collector.into_report())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::NodeKind;

    /// Builds an AST node for tests.
    fn section(keyword: &str, content: Vec<&str>) -> SectionNode {
        SectionNode {
            keyword: keyword.to_string(),
            kind: NodeKind::Regular,
            content: content.into_iter().map(str::to_string).collect(),
            children: Vec::new(),
        }
    }

    #[test]
    fn test_strict_mode_returns_the_first_problem() {
        let tree = vec![section("Mystery Section", vec!["x"])];
        let diagnostic = semantic_parse(&tree).expect_err("strict mode stops on the first error");

        assert_eq!(diagnostic.rule, Rule::UnknownKeyword);
        assert_eq!(diagnostic.scope, "(file)");
        assert!(diagnostic.message.contains("Mystery Section"));
    }

    #[test]
    fn test_lenient_mode_reports_but_still_parses() {
        let tree = vec![section("Mystery Section", vec!["some text"])];
        let (parsed, report) = semantic_parse_lenient(&tree).expect("lenient mode never fails");

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].keyword, "Mystery Section");
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].scope, "(file)");
        assert!(parsed[0].field("lines").is_some());
    }

    #[test]
    fn test_the_report_collects_the_findings_of_every_phase() {
        // `pre_process` flags the unknown section, `validation` the incomplete
        // `[Component]` (Manufacturer, Package and Pin are all required).
        let tree = vec![
            section("Component", vec!["MyChip"]),
            section("Mystery Section", vec!["some text"]),
        ];
        let (parsed, report) = semantic_parse_lenient(&tree).expect("lenient mode never fails");

        assert_eq!(parsed.len(), 2);
        assert!(
            report.errors.iter().any(|finding| finding.rule == Rule::UnknownKeyword),
            "pre_process finding missing: {report:?}"
        );
        assert!(
            report
                .errors
                .iter()
                .any(|finding| finding.rule == Rule::MissingRequiredKeyword),
            "validation finding missing: {report:?}"
        );
    }
}
