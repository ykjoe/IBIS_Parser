//! 抽象语法树 — AST 数据结构与构建。
//!
//! 该模块定义语法分析阶段产出的核心数据结构（`NodeKind` / `SectionNode` / `ParsedBlock`），
//! 并负责将扁平 [`ParsedBlock`] 列表递归构建为多级 [`SectionNode`] 树，
//! 即"构建抽象语法树"（[`build_section_tree`]）。

use crate::core::Rule;

// =============================================================================
// Multi-level AST tree types
// =============================================================================

/// Role of a section node in the TOML output.
///
/// The frontend does NOT distinguish array-of-tables (`[[...]]`)
/// from regular tables (`[...]`); that is a backend concern.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    /// `[File_Header]` — virtual container for file header fields.
    FileHeader,
    /// `[Section]` or `[Parent.Child]` — regular section.
    Regular,
}

/// A node in the hierarchical IBIS section tree.
#[derive(Debug, Clone)]
pub struct SectionNode {
    pub keyword: String,                    // Keyword name (e.g., "Component", "IBIS ver", "Pin").
    pub kind: NodeKind,                     // Role determining TOML output format.
    pub content: Vec<String>,               // Content lines belonging directly to this section.
    pub children: Vec<SectionNode>,         // Child sections nested under this node.
}

// =============================================================================
// Intermediate representation — flat blocks from pest pairs
// =============================================================================

/// A parsed keyword block with its content lines.
#[derive(Debug, Clone)]
pub struct ParsedBlock {
    pub keyword: String,                    // Raw keyword name (e.g., "Component", "IBIS ver", "Package").
    pub rule: Rule,                         // Pest rule variant that matched this keyword header.
    pub content: Vec<String>,               // Content lines belonging to this block.
}

// =============================================================================
// File header field detection
// =============================================================================

/// Known file header keyword names, used to group them under `[File_Header]`.
const FILE_HEADER_FIELD_NAMES: &[&str] = &[
    "IBIS ver", "Comment Char", "File name", "File Rev",
    "Date", "Source", "Notes", "Disclaimer", "Copyright",
];

/// Check whether a parsed block is a file header field.
fn is_file_header_field(block: &ParsedBlock) -> bool {
    FILE_HEADER_FIELD_NAMES.contains(&block.keyword.as_str())
}

// =============================================================================
// AST building — flat blocks → hierarchical tree
// =============================================================================

/// Build a hierarchical section tree from flat parsed blocks.
///
/// # Parameters
///
/// * `blocks` — Flat list of parsed keyword blocks in file order.
/// * `start` — Starting index for this recursion level.
/// * `stop_rules` — List of rule variants that should stop child collection.
///
/// # Returns
///
/// A tuple of `(Vec<SectionNode>, usize)` — the built nodes and the next index to process.
pub fn build_section_tree(
    blocks: &[ParsedBlock],
    start: usize,
    stop_rules: &[Rule],
) -> (Vec<SectionNode>, usize) {
    let mut nodes: Vec<SectionNode> = Vec::new();
    let mut block_index = start;

    // ── Phase A: file header grouping ──
    if block_index < blocks.len() && is_file_header_field(&blocks[block_index]) {
        let mut children: Vec<SectionNode> = Vec::new();
        while block_index < blocks.len() && is_file_header_field(&blocks[block_index]) {
            children.push(SectionNode {
                keyword: blocks[block_index].keyword.clone(),
                kind: NodeKind::Regular,
                content: blocks[block_index].content.clone(),
                children: Vec::new(),
            });
            block_index += 1;
        }
        nodes.push(SectionNode {
            keyword: "File_Header".into(),
            kind: NodeKind::FileHeader,
            content: Vec::new(),
            children,
        });
        return (nodes, block_index);
    }

    // ── Phase B: process remaining blocks ──
    while block_index < blocks.len() {
        let block = &blocks[block_index];

        // Check stop rules
        if stop_rules.contains(&block.rule) {
            break;
        }

        if block.rule == Rule::kw_end {
            // Skip [End] marker
            block_index += 1;
            continue;
        }

        if block.rule == Rule::first_level_keyword {
            // First-level container: create parent node, recursively collect children
            let keyword_name = block.keyword.clone();
            let content = block.content.clone();
            block_index += 1;

            // Recursively collect child blocks
            let (children, next_index) = build_section_tree(
                blocks,
                block_index,
                &[Rule::first_level_keyword, Rule::kw_end],
            );
            block_index = next_index;

            nodes.push(SectionNode {
                keyword: keyword_name,
                kind: NodeKind::Regular,
                content,
                children,
            });
        } else {
            // Second-level or generic keyword → child or singleton node
            nodes.push(SectionNode {
                keyword: block.keyword.clone(),
                kind: NodeKind::Regular,
                content: block.content.clone(),
                children: Vec::new(),
            });
            block_index += 1;
        }
    }

    (nodes, block_index)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_section_tree_file_header() {
        let blocks = vec![
            ParsedBlock { keyword: "IBIS ver".into(), rule: Rule::second_level_keyword, content: vec!["2.1".into()] },
            ParsedBlock { keyword: "File name".into(), rule: Rule::second_level_keyword, content: vec!["test.ibs".into()] },
        ];
        let (tree, _) = build_section_tree(&blocks, 0, &[]);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].keyword, "File_Header");
        assert_eq!(tree[0].children.len(), 2);
        assert_eq!(tree[0].children[0].keyword, "IBIS ver");
    }

    #[test]
    fn test_build_section_tree_component_with_children() {
        let blocks = vec![
            ParsedBlock { keyword: "Component".into(), rule: Rule::first_level_keyword, content: vec!["MyComp".into()] },
            ParsedBlock { keyword: "Manufacturer".into(), rule: Rule::second_level_keyword, content: vec!["Acme".into()] },
            ParsedBlock { keyword: "Package".into(), rule: Rule::second_level_keyword, content: vec!["R_pkg 0.1".into()] },
        ];
        let (tree, _) = build_section_tree(&blocks, 0, &[]);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].kind, NodeKind::Regular);
        assert_eq!(tree[0].children.len(), 2);
    }

    #[test]
    fn test_is_file_header_field() {
        let header_block = ParsedBlock {
            keyword: "IBIS ver".into(),
            rule: Rule::second_level_keyword,
            content: vec![],
        };
        assert!(is_file_header_field(&header_block));

        let non_header_block = ParsedBlock {
            keyword: "Pin".into(),
            rule: Rule::second_level_keyword,
            content: vec![],
        };
        assert!(!is_file_header_field(&non_header_block));
    }
}
