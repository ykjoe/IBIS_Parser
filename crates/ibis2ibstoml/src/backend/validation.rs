// =============================================================================
// validation — third phase: inspect the parsed tree and locate diagnostics
//
// The phase consumes what `content_parse` produced: the `ParsedNode` tree. Every
// value in it still carries its original text (`Text`, `Lines`, corner elements,
// table cells) and the canonical keyword name, which is all this inspection needs.
//
// This is the last phase of the backend: it runs on the parsed tree that the previous
// phase handed over, and the findings it collects close the run together with those of
// `pre_process`, which the pipeline turns into the report.
//
// The backend only owns two questions here, and nothing else:
//
//   - `valid_kw_tree` — the *dependent required* rule. `__schema__.required` on a
//     child keyword means "if the parent appears, this child must appear too"; a parent
//     the file omits tells us nothing about its children, so an absent parent is never
//     a finding. A missing child is an Error, because no simulator can build the model
//     from a partial section;
//   - `valid_param`   — the *shape* rule, run over the sections whose content is
//     structured: corner triples (`HeaderFormat::Corner`) and row-oriented tables
//     (`Table`, `IV-table`, `VT-table`). A cell or corner element must read as a
//     quantity — a number with an optional unit, or scientific notation — and a table
//     the schema expects to be column-named must carry its header row. A violation is a
//     Warning: the file still parses and a simulator interpolates around most of it.
//
// Everything else the specification says about a file — character set, line ending,
// bracket padding, scope membership, curve monotonicity — is the business of the
// phases that produce the tree (the frontend reads the text; `pre_process` resolves
// scope and reports unknown keywords; `content_parse` types the values), never of
// this inspection.
//
// Diagnostics carry a scope path but no line numbers: the parse keeps lines without
// their source numbers, so a finding names the node (and quotes the offending text).
//
// File layout:
//
//   [1] diagnostics   — `mod diagnostics`, the finding model; its public API is
//                       re-exported right below the module, the helper surface stays
//                       crate-private;
//   [2] entry         — `validate`, the single walk, plus its module-private helpers;
//   [3] keyword tree  — `mod valid_kw_tree`, the dependent-required rule;
//   [4] parameters    — `mod valid_param`, the corner and table shape rules.
// =============================================================================

//! Third phase of the backend: content inspection with located diagnostics.
//!
//! [`validate`] consumes the [`ParsedNode`] tree of `content_parse`, runs the
//! dependent-required and parameter-format rules over it and pushes the findings into
//! a [`ValidationCollector`]; the backend turns that collector into the
//! [`ValidationReport`] that ends the run.

use crate::schema::SectionSpec;

use super::content_parse::ParsedNode;
use super::pre_process::resolve_spec;

// =============================================================================
// [1] Diagnostics
// =============================================================================

/// The finding model: severity, rule list, one finding, the report and the collector.
///
/// This inline module only describes what a finding looks like; the checks themselves
/// live in [`valid_kw_tree`] and [`valid_param`]. Its public surface is re-exported
/// below, while the collector stays crate-private.
mod diagnostics {
    use std::fmt;

    /// How serious one finding is.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Severity {
        Error,   // Content breaks a "shall"; strict mode stops at the first one.
        Warning, // Content still parses, but a convention or plausibility check failed.
    }

    /// The inspection item that produced a finding — one variant per rule.
    ///
    /// A rule fixes its own severity and its stable name, so a report can be filtered
    /// by rule without knowing which function emitted the finding.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Rule {
        UnknownKeyword,         // Keyword the schema does not know; reported by `pre_process`.
        MissingRequiredKeyword, // Child a present keyword must carry, but does not.
        InvalidFormat,          // Corner or table value that is not a quantity, or a missing header.
    }

    impl Rule {
        /// Returns the severity fixed for the rule.
        pub fn severity(self) -> Severity {
            match self {
                Rule::InvalidFormat => Severity::Warning,
                _ => Severity::Error,
            }
        }

        /// Returns the stable `snake_case` label of the rule.
        pub fn name(self) -> &'static str {
            match self {
                Rule::UnknownKeyword => "unknown_keyword",
                Rule::MissingRequiredKeyword => "missing_required_keyword",
                Rule::InvalidFormat => "invalid_format",
            }
        }
    }

    /// One finding: the rule that fired, its severity, the scope and the detail.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Diagnostic {
        pub rule: Rule,         // Inspection item that fired.
        pub severity: Severity, // Severity fixed by that rule.
        pub scope: String,      // Dotted scope path of the node, or "(file)" at the root.
        pub message: String,    // Human-readable detail, quoting the offending text.
    }

    impl Diagnostic {
        /// Builds a finding at the severity of its own rule.
        ///
        /// Crate-private: only the backend phases construct findings.
        pub(crate) fn new(rule: Rule, scope: &str, message: String) -> Diagnostic {
            Diagnostic { rule, severity: rule.severity(), scope: scope.to_string(), message }
        }

        /// Builds the finding `pre_process` reports for an unknown keyword.
        ///
        /// Crate-private: the resolving phase is its only caller.
        pub(crate) fn unknown_keyword(scope: &str, keyword: &str) -> Diagnostic {
            Diagnostic::new(
                Rule::UnknownKeyword,
                scope,
                format!("unknown keyword '{keyword}' in scope '{scope}'"),
            )
        }
    }

    impl fmt::Display for Diagnostic {
        /// Renders the finding as `[rule] scope: message`.
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "[{}] {}: {}", self.rule.name(), self.scope, self.message)
        }
    }

    /// Findings collected while running the phases, split by severity.
    #[derive(Debug, Clone, PartialEq, Default)]
    pub struct ValidationReport {
        pub errors: Vec<Diagnostic>,   // Findings that stop strict mode.
        pub warnings: Vec<Diagnostic>, // Findings a lenient run reports and keeps going on.
    }

    impl ValidationReport {
        /// Returns `true` when both severities are empty.
        pub fn is_empty(&self) -> bool {
            self.errors.is_empty() && self.warnings.is_empty()
        }
    }

    /// Internal sink the phases push their findings to.
    ///
    /// Crate-private: it is the phases' working surface, never part of a public API.
    #[derive(Debug, Default)]
    pub(crate) struct ValidationCollector {
        pub(crate) errors: Vec<Diagnostic>,   // Findings of severity `Error`.
        pub(crate) warnings: Vec<Diagnostic>, // Findings of severity `Warning`.
    }

    impl ValidationCollector {
        /// Returns a collector with nothing recorded.
        pub(crate) fn new() -> Self {
            Self::default()
        }

        /// Records one finding under its own severity.
        pub(crate) fn report(&mut self, diagnostic: Diagnostic) {
            match diagnostic.severity {
                Severity::Error => self.errors.push(diagnostic),
                Severity::Warning => self.warnings.push(diagnostic),
            }
        }

        /// Converts the collected findings into a user-facing [`ValidationReport`].
        pub(crate) fn into_report(self) -> ValidationReport {
            ValidationReport { errors: self.errors, warnings: self.warnings }
        }
    }
}

// Public area: the finding model is reachable at the `validation` top level (and
// re-exported from there by `backend/mod.rs` and `lib.rs`). The collector stays
// crate-private, for the backend phases only.
pub use diagnostics::{Diagnostic, Rule, Severity, ValidationReport};
pub(crate) use diagnostics::ValidationCollector;

// =============================================================================
// [2] Entry
// =============================================================================

/// Scope label of a top-level node.
const ROOT_SCOPE: &str = "(file)";

/// Runs both rule families over the parsed tree.
///
/// This is the last backend phase: the caller closes the run by turning the collector
/// into the report.
///
/// # Parameters
///
/// * `parsed` — The `content_parse` tree; the input this phase consumes.
/// * `collector` — Sink for the findings.
pub(crate) fn validate(parsed: &[ParsedNode], collector: &mut ValidationCollector) {
    for node in parsed {
        walk(node, None, ROOT_SCOPE, collector);
    }
}

/// Validates one parsed node, then recurses into its children.
///
/// Module-private: the walk is driven by [`validate`] alone.
fn walk(
    node: &ParsedNode,
    parent_spec: Option<&'static SectionSpec>,
    parent_scope: &str,
    collector: &mut ValidationCollector,
) {
    let spec = resolve_spec(&node.keyword, parent_spec);
    let scope = if parent_scope == ROOT_SCOPE {
        node.keyword.clone()
    } else {
        format!("{parent_scope}.{}", node.keyword)
    };

    valid_kw_tree::validate_keyword_tree(node, spec, &scope, collector);
    valid_param::validate_parameters(node, spec, &scope, collector);

    for child in &node.children {
        walk(child, spec, &scope, collector);
    }
}

// =============================================================================
// [3] Keyword Tree Validation
// =============================================================================

/// Dependent-required rule — `__schema__.required` of every child keyword.
///
/// A child marked `required = true` must be present *once its parent is present*: the
/// parent is the switch, the child the obligation it carries. A parent the file never
/// writes imposes nothing, so an absent top-level section never makes its required
/// children findings — this is what keeps the rule from contradicting optional
/// sections.
mod valid_kw_tree {
    use crate::schema::{normalize_keyword, SectionSpec};

    use super::{Diagnostic, ParsedNode, Rule, ValidationCollector};

    /// Reports every required child keyword the present `node` fails to carry.
    ///
    /// Super-private: [`super::walk`] is the only caller. Does nothing when the schema
    /// does not know the keyword.
    pub(super) fn validate_keyword_tree(
        node: &ParsedNode,
        spec: Option<&'static SectionSpec>,
        scope: &str,
        collector: &mut ValidationCollector,
    ) {
        let Some(spec) = spec else {
            return;
        };
        for child_spec in &spec.children {
            if !child_spec.required {
                continue;
            }
            let present = node.children.iter().any(|child| {
                normalize_keyword(&child.keyword) == normalize_keyword(&child_spec.name)
            });
            if present {
                continue;
            }
            collector.report(Diagnostic::new(
                Rule::MissingRequiredKeyword,
                scope,
                format!(
                    "`{}` requires the child section `{}`, which is missing",
                    spec.name, child_spec.name
                ),
            ));
        }
    }

}

// =============================================================================
// [4] Parameter Format Validation
// =============================================================================

/// Parameter-format rule — corner triples and the cells of structured tables.
///
/// The schema tells which sections carry physical values: a corner header
/// (`HeaderFormat::Corner`), a declared corner / quantity parameter, a curve table
/// (`IV-table` / `VT-table`, numeric throughout) or a table column declared as a
/// quantity. This rule reads those values and warns when one of them is not a
/// quantity, so a typo shows up before a simulator silently mis-reads it.
mod valid_param {
    use crate::backend::content_parse::{
        split_number_unit, Corner, ParsedNode, ParsedTable, ParsedValue,
    };
    use crate::schema::{find_field, HeaderFormat, ParamType, SectionFormat, SectionSpec};

    use super::{Diagnostic, Rule, ValidationCollector};

    /// Runs every shape check over one node.
    ///
    /// Super-private: [`super::walk`] is the only caller. Does nothing when the schema
    /// does not know the keyword.
    pub(super) fn validate_parameters(
        node: &ParsedNode,
        spec: Option<&'static SectionSpec>,
        scope: &str,
        collector: &mut ValidationCollector,
    ) {
        let Some(spec) = spec else {
            return;
        };
        for field in &node.fields {
            match &field.value {
                ParsedValue::Corner(corner) => {
                    validate_corner(&field.key, corner, scope, collector);
                }
                ParsedValue::Table(table) => {
                    validate_table(spec, &field.key, table, scope, collector);
                }
                _ => {}
            }
        }
    }

    /// Checks the three elements of a corner triple (`typ min max`).
    ///
    /// Module-private, a helper of [`validate_parameters`]: an absent element (`NA`,
    /// empty) is skipped, because the corner simply leaves that corner unspecified.
    fn validate_corner(
        key: &str,
        corner: &Corner,
        scope: &str,
        collector: &mut ValidationCollector,
    ) {
        for (position, element) in [&corner.0, &corner.1, &corner.2].into_iter().enumerate() {
            if is_quantity(element) {
                continue;
            }
            collector.report(Diagnostic::new(
                Rule::InvalidFormat,
                scope,
                format!(
                    "corner `{key}` element {} is `{element}`, not a number with a unit or scientific notation",
                    position + 1
                ),
            ));
        }
    }

    /// Checks one table: its header row and every numeric cell.
    ///
    /// Module-private, a helper of [`validate_parameters`].
    fn validate_table(
        spec: &'static SectionSpec,
        key: &str,
        table: &ParsedTable,
        scope: &str,
        collector: &mut ValidationCollector,
    ) {
        let expects_header = spec.header_format == HeaderFormat::TableHeader
            || matches!(spec.format, SectionFormat::IvTable | SectionFormat::VtTable);
        if expects_header && table.header.is_empty() {
            collector.report(Diagnostic::new(
                Rule::InvalidFormat,
                scope,
                format!("table `{key}` has no column header row"),
            ));
        }

        for column in numeric_columns(spec, table) {
            let name = table.header.get(column).map(String::as_str).unwrap_or("?");
            for (row_index, row) in table.data.iter().enumerate() {
                let Some(cell) = row.get(column) else {
                    continue;
                };
                if is_quantity(cell) {
                    continue;
                }
                collector.report(Diagnostic::new(
                    Rule::InvalidFormat,
                    scope,
                    format!(
                        "table `{key}` row {} column `{name}` is `{cell}`, not a number with a unit or scientific notation",
                        row_index + 1
                    ),
                ));
            }
        }
    }

    /// Returns the indexes of the columns that carry physical values.
    ///
    /// Module-private, a helper of [`validate_table`]: a curve table is numeric
    /// throughout, while every other table only declares some columns as quantities.
    fn numeric_columns(spec: &SectionSpec, table: &ParsedTable) -> Vec<usize> {
        let curve = matches!(spec.format, SectionFormat::IvTable | SectionFormat::VtTable);
        table
            .header
            .iter()
            .enumerate()
            .filter(|(_, header)| {
                curve
                    || find_field(spec, header)
                        .is_some_and(|field| field.param_type != ParamType::Text)
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// Whether a cell or corner element reads as a physical quantity.
    ///
    /// Module-private, a helper of [`validate_corner`] and [`validate_table`]: accepts
    /// a number with an optional unit (`3.3V`, `2.69e-12`) and an IBIS ratio of two
    /// such numbers (`1.926/597.107p`); a missing value counts as a quantity.
    fn is_quantity(text: &str) -> bool {
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("NA") {
            return true;
        }
        if let Some((numerator, denominator)) = trimmed.split_once('/') {
            return split_number_unit(numerator).is_some()
                && split_number_unit(denominator).is_some();
        }
        split_number_unit(trimmed).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Corner, Occurrence, ParsedField, ParsedTable, ParsedValue};

    /// Builds a parsed node from field values.
    fn parsed(keyword: &str, fields: Vec<ParsedField>, children: Vec<ParsedNode>) -> ParsedNode {
        ParsedNode {
            keyword: keyword.to_string(),
            occurrence: Occurrence::Once,
            fields,
            children,
        }
    }

    /// Builds a text field.
    fn text_field(key: &str, value: &str) -> ParsedField {
        ParsedField { key: key.to_string(), value: ParsedValue::Text(value.to_string()) }
    }

    /// Builds a corner field.
    fn corner_field(key: &str, typical: &str, minimum: &str, maximum: &str) -> ParsedField {
        ParsedField {
            key: key.to_string(),
            value: ParsedValue::Corner(Corner(
                typical.to_string(),
                minimum.to_string(),
                maximum.to_string(),
            )),
        }
    }

    /// Builds a table field.
    fn table_field(key: &str, header: Vec<&str>, rows: Vec<Vec<&str>>) -> ParsedField {
        let table = ParsedTable {
            header: header.into_iter().map(str::to_string).collect(),
            data: rows
                .into_iter()
                .map(|row| row.into_iter().map(str::to_string).collect())
                .collect(),
        };
        ParsedField { key: key.to_string(), value: ParsedValue::Table(table) }
    }

    /// Runs the checks over a parsed tree.
    fn inspect(parsed: Vec<ParsedNode>) -> ValidationReport {
        let mut collector = ValidationCollector::new();
        validate(&parsed, &mut collector);
        collector.into_report()
    }

    /// Returns whether a report holds a finding of the given rule at any severity.
    fn holds(report: &ValidationReport, rule: Rule) -> bool {
        report
            .errors
            .iter()
            .chain(report.warnings.iter())
            .any(|finding| finding.rule == rule)
    }

    /// Returns the first finding of the given rule.
    fn finding(report: &ValidationReport, rule: Rule) -> &Diagnostic {
        report
            .errors
            .iter()
            .chain(report.warnings.iter())
            .find(|finding| finding.rule == rule)
            .unwrap_or_else(|| panic!("no {rule:?} finding in {report:?}"))
    }

    /// A complete `[Component]`: Manufacturer, Package and Pin are all present.
    fn complete_component() -> ParsedNode {
        parsed(
            "Component",
            vec![text_field("component", "MyChip")],
            vec![
                parsed("Manufacturer", vec![text_field("manufacturer", "STMicro")], vec![]),
                parsed(
                    "Package",
                    vec![corner_field("r_pkg", "250.0m", "225.0m", "275.0m")],
                    vec![],
                ),
                parsed(
                    "Pin",
                    vec![table_field("pin", vec!["col0", "signal_name"], vec![vec!["PA0", "IO8TC"]])],
                    vec![],
                ),
            ],
        )
    }

    #[test]
    fn test_complete_component_reports_nothing() {
        let report = inspect(vec![complete_component()]);
        assert!(report.is_empty(), "unexpected findings: {report:?}");
    }

    #[test]
    fn test_missing_required_child_is_an_error() {
        // `[Component]` appears without Manufacturer, Package or Pin.
        let report = inspect(vec![parsed(
            "Component",
            vec![text_field("component", "MyChip")],
            vec![],
        )]);

        let missing = finding(&report, Rule::MissingRequiredKeyword);
        assert_eq!(missing.severity, Severity::Error);
        assert_eq!(missing.scope, "Component");
        assert!(missing.message.contains("Manufacturer"), "{}", missing.message);
    }

    #[test]
    fn test_required_children_are_dependent_on_the_parent() {
        // The parent never appears, so its required children are free to be absent.
        let report = inspect(vec![parsed("Model", vec![text_field("model", "IO8FT")], vec![])]);

        assert!(!holds(&report, Rule::MissingRequiredKeyword), "{report:?}");
    }

    #[test]
    fn test_partially_present_required_children_are_reported() {
        let component = parsed(
            "Component",
            vec![text_field("component", "MyChip")],
            vec![parsed("Manufacturer", vec![text_field("manufacturer", "STMicro")], vec![])],
        );
        let report = inspect(vec![component]);

        let messages: Vec<&str> = report
            .errors
            .iter()
            .filter(|finding| finding.rule == Rule::MissingRequiredKeyword)
            .map(|finding| finding.message.as_str())
            .collect();
        assert_eq!(messages.len(), 2, "Package and Pin are both missing: {messages:?}");
    }

    #[test]
    fn test_file_header_requires_its_own_entries() {
        // `[File_Header]` is present with two of the three required entries.
        let header = parsed(
            "File_Header",
            vec![],
            vec![
                parsed("IBIS_Ver", vec![text_field("ibis_ver", "2.1")], vec![]),
                parsed("File_Name", vec![text_field("file_name", "test.ibs")], vec![]),
            ],
        );
        let report = inspect(vec![header]);

        let missing = finding(&report, Rule::MissingRequiredKeyword);
        assert!(missing.message.contains("File_Rev"), "{}", missing.message);
    }

    #[test]
    fn test_corner_with_a_non_quantity_element_warns() {
        let report = inspect(vec![parsed(
            "Voltage Range",
            vec![corner_field("voltage_range", "fast", "2.0V", "3.6V")],
            vec![],
        )]);

        let invalid = finding(&report, Rule::InvalidFormat);
        assert_eq!(invalid.severity, Severity::Warning);
        assert!(invalid.message.contains("fast"), "{}", invalid.message);
    }

    #[test]
    fn test_curve_cell_that_is_not_numeric_warns() {
        let report = inspect(vec![parsed(
            "Pulldown",
            vec![table_field(
                "pulldown",
                vec!["Voltage", "I(typ)", "I(min)", "I(max)"],
                vec![
                    vec!["0", "0A", "0A", "0A"],
                    vec!["sweep", "1mA", "1mA", "1mA"],
                ],
            )],
            vec![],
        )]);

        let invalid = finding(&report, Rule::InvalidFormat);
        assert_eq!(invalid.severity, Severity::Warning);
        assert!(invalid.message.contains("sweep"), "{}", invalid.message);
    }

    #[test]
    fn test_table_without_its_header_row_warns() {
        let report = inspect(vec![parsed(
            "Pin",
            vec![table_field("pin", Vec::new(), vec![vec!["PA0", "IO8TC"]])],
            vec![],
        )]);

        let invalid = finding(&report, Rule::InvalidFormat);
        assert!(invalid.message.contains("no column header"), "{}", invalid.message);
    }

    #[test]
    fn test_scientific_notation_and_ratios_pass() {
        let report = inspect(vec![parsed(
            "Ramp",
            vec![corner_field(
                "dv/dt_r",
                "1.926/597.107p",
                "1.171/792.551p",
                "2.69e-12",
            )],
            vec![],
        )]);

        assert!(report.is_empty(), "unexpected findings: {report:?}");
    }

    #[test]
    fn test_rule_names_and_severities_are_stable() {
        assert_eq!(Rule::UnknownKeyword.name(), "unknown_keyword");
        assert_eq!(Rule::MissingRequiredKeyword.name(), "missing_required_keyword");
        assert_eq!(Rule::InvalidFormat.name(), "invalid_format");
        assert_eq!(Rule::MissingRequiredKeyword.severity(), Severity::Error);
        assert_eq!(Rule::InvalidFormat.severity(), Severity::Warning);
        assert!(!holds(&ValidationReport::default(), Rule::InvalidFormat));
    }
}
