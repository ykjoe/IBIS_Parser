// =============================================================================
// pre_process — first phase: keyword marking and scope resolution
//
// This phase walks the frontend AST once, in pre-order, and annotates every node
// with what the schema says about its keyword:
//
//   - occurrence — `[X]` (single) versus `[[X]]` (multiple);
//   - scope path — the dotted path of the node inside the document;
//   - spec       — the schema specification that `content_parse` reuses.
//
// Structural problems (keywords the schema does not know) are collected instead
// of aborting, so the lenient mode can still produce a parse.
// =============================================================================

//! First phase of the backend: keyword marking plus the shared scope resolver.
//!
//! The marks produced here are consumed by `content_parse` in the same pre-order,
//! which keeps keyword resolution in one place instead of repeating it per stage.

use crate::frontend::SectionNode;
use crate::schema::{
    file_header_section, find_child, find_descendant, find_root, load_schema, normalize_keyword,
    Occurrence, SectionSpec, FILE_HEADER_CONTAINER,
};

use super::{Diagnostic, ValidationCollector};

/// One keyword annotated by the preprocess phase.
#[derive(Debug, Clone)]
pub struct KeywordMark {
    pub keyword: String,                  // Canonical schema name; the raw keyword when unknown.
    pub occurrence: Occurrence,            // Single (`[X]`) or multiple (`[[X]]`) inside its parent.
    pub scope_path: String,               // Dotted path from the root, e.g. "Component.Pin".
    pub spec: Option<&'static SectionSpec>, // Schema spec of the keyword; `None` when unknown.
}

/// Marks every node of the AST with its schema occurrence, scope path and spec.
///
/// # Parameters
///
/// * `tree` — Root-level sections produced by the frontend.
/// * `collector` — Sink for structural problems (unknown keywords).
///
/// # Returns
///
/// * `Vec<KeywordMark>` — Marks in pre-order, aligned with the AST traversal that
///   `content_parse` performs.
pub fn mark_keywords(tree: &[SectionNode], collector: &mut ValidationCollector) -> Vec<KeywordMark> {
    let mut marks: Vec<KeywordMark> = Vec::new();
    for node in tree {
        mark_node(node, None, "", &mut marks, collector);
    }
    marks
}

/// Resolves the schema spec of a keyword inside an optional parent scope.
///
/// The frontend flattens sections deeper than two levels, so resolution falls back
/// from direct child to any descendant and finally to the top-level sections.
///
/// Takes the `keyword` and the `parent_spec` (or `None` at the document root);
/// returns the matching spec when the schema knows the keyword.
pub(crate) fn resolve_spec(
    keyword: &str,
    parent_spec: Option<&'static SectionSpec>,
) -> Option<&'static SectionSpec> {
    if let Some(parent) = parent_spec {
        if let Some(child) = find_child(parent, keyword) {
            return Some(child);
        }
        if let Some(descendant) = find_descendant(parent, keyword) {
            return Some(descendant);
        }
    }
    find_section_anywhere(keyword)
}

/// Finds a section anywhere in the schema, whatever its depth.
///
/// This is the fallback used when the AST lost its nesting: the frontend's
/// line-by-line recovery marks every block as a generic keyword, so each section
/// arrives as a top-level sibling and scope-based resolution cannot reach it.
///
/// Ambiguous names (`Pin Mapping`, `Pulldown`) resolve to the first match in schema
/// order; their shapes agree closely enough for parsing, but the ambiguity is a
/// known limitation until the recovery path keeps the hierarchy.
///
/// Takes the `keyword`; returns the first matching section, or `None` when the
/// schema does not know it.
fn find_section_anywhere(keyword: &str) -> Option<&'static SectionSpec> {
    if let Some(root) = find_root(keyword) {
        return Some(root);
    }

    let normalized = normalize_keyword(keyword);
    let is_header_container = normalized == normalize_keyword(FILE_HEADER_CONTAINER);
    if is_header_container {
        return Some(file_header_section());
    }
    let header = file_header_section();
    let mut frontier: Vec<&'static SectionSpec> = header.children.iter().collect();
    while !frontier.is_empty() {
        let mut next_frontier: Vec<&'static SectionSpec> = Vec::new();
        for candidate in frontier {
            let candidate_name = normalize_keyword(&candidate.name);
            if candidate_name == normalized {
                return Some(candidate);
            }
            next_frontier.extend(candidate.children.iter());
        }
        frontier = next_frontier;
    }

    for root in load_schema() {
        if let Some(found) = find_descendant(root, keyword) {
            return Some(found);
        }
    }
    None
}

/// Marks one node and recurses into its children.
///
/// Takes the AST `node`, the `parent_spec` of its scope, the `parent_scope_path`,
/// the mark `accumulator` and the problem `collector`.
fn mark_node(
    node: &SectionNode,
    parent_spec: Option<&'static SectionSpec>,
    parent_scope_path: &str,
    accumulator: &mut Vec<KeywordMark>,
    collector: &mut ValidationCollector,
) {
    let spec = resolve_spec(&node.keyword, parent_spec);
    let keyword = match spec {
        Some(section) => section.name.clone(),
        None => node.keyword.clone(),
    };
    let scope_path = if parent_scope_path.is_empty() {
        keyword.clone()
    } else {
        format!("{parent_scope_path}.{keyword}")
    };

    let occurrence = match spec {
        Some(section) => section.occurrence.clone(),
        None => Occurrence::Once,
    };

    if spec.is_none() {
        let scope = if parent_scope_path.is_empty() { "(file)" } else { parent_scope_path };
        collector.report(Diagnostic::unknown_keyword(scope, &node.keyword));
    }

    accumulator.push(KeywordMark {
        keyword,
        occurrence,
        scope_path: scope_path.clone(),
        spec,
    });

    for child in &node.children {
        mark_node(child, spec, &scope_path, accumulator, collector);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Rule;
    use crate::frontend::NodeKind;

    /// Builds an AST node for tests.
    fn section(keyword: &str, content: Vec<&str>, children: Vec<SectionNode>) -> SectionNode {
        SectionNode {
            keyword: keyword.to_string(),
            kind: NodeKind::Regular,
            content: content.into_iter().map(str::to_string).collect(),
            children,
        }
    }

    #[test]
    fn test_component_children_are_marked_with_scope_paths() {
        let tree = vec![section(
            "Component",
            vec!["MyChip"],
            vec![
                section("Manufacturer", vec!["Acme"], vec![]),
                section("Pin", vec!["signal_name model_name R_pin L_pin C_pin", "PA0 IO8TC"], vec![]),
            ],
        )];
        let mut collector = ValidationCollector::new();
        let marks = mark_keywords(&tree, &mut collector);

        assert!(collector.errors.is_empty(), "unexpected: {:?}", collector.errors);
        assert_eq!(marks.len(), 3);
        assert_eq!(marks[0].keyword, "Component");
        assert_eq!(marks[0].occurrence, Occurrence::Multiple);
        assert_eq!(marks[1].scope_path, "Component.Manufacturer");
        assert_eq!(marks[2].keyword, "Pin");
        assert_eq!(marks[2].occurrence, Occurrence::Once);
    }

    #[test]
    fn test_file_header_container_is_single_and_its_fields_are_known() {
        let tree = vec![section(
            "File_Header",
            vec![],
            vec![
                section("IBIS ver", vec!["2.1"], vec![]),
                section("Notes", vec!["note line"], vec![]),
            ],
        )];
        let mut collector = ValidationCollector::new();
        let marks = mark_keywords(&tree, &mut collector);

        assert!(collector.errors.is_empty(), "unexpected: {:?}", collector.errors);
        assert_eq!(marks[0].keyword, "File_Header");
        assert_eq!(marks[0].occurrence, Occurrence::Once);
        assert_eq!(marks[1].keyword, "IBIS_Ver");
        assert_eq!(marks[2].keyword, "Notes");
    }

    #[test]
    fn test_third_level_section_is_found_through_descendant_lookup() {
        let tree = vec![section(
            "Define Package Model",
            vec!["PKG"],
            vec![
                section("Model Data", vec![], vec![]),
                section("Resistance Matrix", vec!["1 2", "0.1 0.2"], vec![]),
            ],
        )];
        let mut collector = ValidationCollector::new();
        let marks = mark_keywords(&tree, &mut collector);

        assert!(collector.errors.is_empty(), "unexpected: {:?}", collector.errors);
        assert_eq!(marks[2].keyword, "Resistance_Matrix");
        assert_eq!(marks[2].scope_path, "Define_Package_Model.Resistance_Matrix");
    }

    #[test]
    fn test_unknown_keyword_is_collected() {
        let tree = vec![section("Mystery Section", vec!["x"], vec![])];
        let mut collector = ValidationCollector::new();
        let marks = mark_keywords(&tree, &mut collector);

        assert_eq!(marks.len(), 1);
        assert!(marks[0].spec.is_none());

        let first_error = collector.errors.first().expect("an unknown-keyword finding");
        assert_eq!(first_error.rule, Rule::UnknownKeyword);
        assert!(first_error.message.contains("Mystery Section"));
    }
}
