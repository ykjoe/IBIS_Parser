//! Abstract syntax tree — AST data structures and construction.
//!
//! This module defines the core data structures produced by the syntax
//! analysis stage (`NodeKind` / `SectionNode` / `ParsedBlock`) and recursively
//! builds the multi-level [`SectionNode`] tree from the flat [`ParsedBlock`]
//! list — i.e., "building the abstract syntax tree" ([`build_section_tree`]).
//!
//! Every block carries the nesting level, which the schema owns
//! ([`keyword_level`](crate::schema::keyword_level)), so tree building branches on
//! integers rather than on grammar tags. Tree building also classifies which
//! consecutive blocks belong to the virtual
//! [`File_Header`](crate::schema::FILE_HEADER_CONTAINER) container via
//! [`header_field::is_header_field_keyword`], so that rule lives here at the AST
//! stage.

pub use ast_types::{NodeKind, ParsedBlock, SectionNode};
pub use tree_builder::build_section_tree;

/// AST data structures — node kinds and the flat block intermediate form.
pub(crate) mod ast_types {
    /// Role of a section node in the TOML output.
    ///
    /// The frontend does NOT distinguish array-of-tables (`[[...]]`) from
    /// regular tables (`[...]`); that is a backend concern.
    #[derive(Debug, Clone, PartialEq)]
    pub enum NodeKind {
        FileHeader,                             // `[File_Header]` — virtual container for file header fields.
        Regular,                                // `[Section]` or `[Parent.Child]` — regular section.
    }

    /// A node in the hierarchical IBIS section tree.
    #[derive(Debug, Clone)]
    pub struct SectionNode {
        pub keyword: String,                    // Keyword name (e.g., "Component", "IBIS ver", "Pin").
        pub kind: NodeKind,                     // Role determining TOML output format.
        pub content: Vec<String>,               // Content lines belonging directly to this section.
        pub children: Vec<SectionNode>,         // Child sections nested under this node.
    }

    /// A parsed keyword block with its content lines.
    #[derive(Debug, Clone)]
    pub struct ParsedBlock {
        pub keyword: String,                    // Raw keyword name (e.g., "Component", "IBIS ver", "Pin").
        pub level: usize,                       // Nesting level read from the schema (`schema::keyword_level`).
        pub content: Vec<String>,               // Content lines belonging to this block.
    }
}

/// File-header field classification — tell header fields from ordinary sections.
mod header_field {
    use crate::frontend::ast_builder::ast_types::ParsedBlock;
    use crate::schema::{file_header_section, normalize_keyword};

    /// Whether a keyword names a file header field (case-insensitive).
    ///
    /// Takes a keyword; returns `true` for known header fields such as
    /// "IBIS ver" / "File name". 判定依据取自 [`ibis_schema.toml`](crate::schema) 的
    /// `File_Header` 虚拟节段字段，不再硬编码。
    pub(super) fn is_header_field_keyword(keyword: &str) -> bool {
        let normalized = normalize_keyword(keyword);
        file_header_section()
            .fields
            .iter()
            .any(|field| normalize_keyword(&field.key) == normalized)
    }

    /// Whether a parsed block is a file header field (wraps
    /// `is_header_field_keyword` on the block's keyword).
    pub(super) fn is_file_header_field(block: &ParsedBlock) -> bool {
        is_header_field_keyword(&block.keyword)
    }
}

/// Tree building — recursively construct the section tree from flat blocks.
mod tree_builder {
    use crate::frontend::ast_builder::ast_types::{NodeKind, ParsedBlock, SectionNode};
    use crate::schema::{keyword_level, FILE_HEADER_CONTAINER};

    use super::header_field::is_file_header_field;

    /// Build a hierarchical section tree from flat parsed blocks.
    ///
    /// Processes `blocks[start..]` recursively: consecutive file header fields
    /// are grouped under a virtual `[File_Header]` node, top-level sections
    /// (`level == ROOT`) become parent nodes that recursively collect their
    /// children, and the terminator (`level == TERMINATOR`) is skipped. Every
    /// other level is a child section, whatever its depth.
    ///
    /// # Parameters
    ///
    /// * `blocks` — Flat list of parsed keyword blocks in file order.
    /// * `start` — Starting index for this recursion level.
    /// * `stop_levels` — Keyword levels that stop child collection.
    ///
    /// # Returns
    ///
    /// * `Vec<SectionNode>` — The nodes built at this level.
    /// * `usize` — The next block index to process (after the terminator or a
    ///   stop level).
    pub fn build_section_tree(
        blocks: &[ParsedBlock],
        start: usize,
        stop_levels: &[usize],
    ) -> (Vec<SectionNode>, usize) {
        let mut nodes: Vec<SectionNode> = Vec::new();
        let mut block_index = start;

        // Phase A: group consecutive file header fields under `[File_Header]`.
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
                keyword: FILE_HEADER_CONTAINER.into(),
                kind: NodeKind::FileHeader,
                content: Vec::new(),
                children,
            });
            return (nodes, block_index);
        }

        // Phase B: process remaining blocks.
        while block_index < blocks.len() {
            let block = &blocks[block_index];

            // Stop collecting when a stop level is reached.
            if stop_levels.contains(&block.level) {
                break;
            }

            if block.level == keyword_level::TERMINATOR {
                // Skip the `[End]` marker.
                block_index += 1;
                continue;
            }

            if block.level == keyword_level::ROOT {
                // Top-level container: create a parent node and recursively collect children.
                let keyword_name = block.keyword.clone();
                let content = block.content.clone();
                block_index += 1;

                // Recursively collect child blocks.
                let (children, next_index) = build_section_tree(
                    blocks,
                    block_index,
                    &[keyword_level::ROOT, keyword_level::TERMINATOR],
                );
                block_index = next_index;

                nodes.push(SectionNode {
                    keyword: keyword_name,
                    kind: NodeKind::Regular,
                    content,
                    children,
                });
            } else {
                // Any deeper level (N >= 2) → child or singleton node.
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
}

#[cfg(test)]
mod tests {
    use crate::schema::{keyword_level, FILE_HEADER_CONTAINER};

    use super::*;

    #[test]
    fn test_build_section_tree_file_header() {
        let blocks = vec![
            ParsedBlock { keyword: "IBIS ver".into(), level: keyword_level::SECOND_LEVEL, content: vec!["2.1".into()] },
            ParsedBlock { keyword: "File name".into(), level: keyword_level::SECOND_LEVEL, content: vec!["test.ibs".into()] },
        ];
        let (tree, _) = build_section_tree(&blocks, 0, &[]);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].keyword, FILE_HEADER_CONTAINER);
        assert_eq!(tree[0].children.len(), 2);
        assert_eq!(tree[0].children[0].keyword, "IBIS ver");
    }

    #[test]
    fn test_build_section_tree_component_with_children() {
        let blocks = vec![
            ParsedBlock { keyword: "Component".into(), level: keyword_level::ROOT, content: vec!["MyComp".into()] },
            ParsedBlock { keyword: "Manufacturer".into(), level: keyword_level::SECOND_LEVEL, content: vec!["Acme".into()] },
            ParsedBlock { keyword: "Package".into(), level: keyword_level::SECOND_LEVEL, content: vec!["R_pkg 0.1".into()] },
        ];
        let (tree, _) = build_section_tree(&blocks, 0, &[]);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].kind, NodeKind::Regular);
        assert_eq!(tree[0].children.len(), 2);
    }

    #[test]
    fn test_build_section_tree_skips_the_terminator() {
        let blocks = vec![
            ParsedBlock { keyword: "Component".into(), level: keyword_level::ROOT, content: vec!["MyComp".into()] },
            ParsedBlock { keyword: "End".into(), level: keyword_level::TERMINATOR, content: vec![] },
        ];
        let (tree, next_index) = build_section_tree(&blocks, 0, &[]);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].keyword, "Component");
        assert_eq!(next_index, 2);
    }

    #[test]
    fn test_build_section_tree_treats_every_deeper_level_as_a_child() {
        // Levels 2 and 3 are the same kind of thing — a child section — so the
        // deeper one is flattened into a sibling and tree building never needs to
        // know which parent a grandchild belongs to.
        let blocks = vec![
            ParsedBlock { keyword: "Define Package Model".into(), level: keyword_level::ROOT, content: vec![] },
            ParsedBlock { keyword: "Model Data".into(), level: keyword_level::SECOND_LEVEL, content: vec![] },
            ParsedBlock { keyword: "Resistance Matrix".into(), level: 3, content: vec!["R1 R2".into()] },
        ];
        let (tree, _) = build_section_tree(&blocks, 0, &[]);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].children.len(), 2);
        assert_eq!(tree[0].children[1].keyword, "Resistance Matrix");
    }

    #[test]
    fn test_is_file_header_field() {
        let header_block = ParsedBlock {
            keyword: "IBIS ver".into(),
            level: keyword_level::SECOND_LEVEL,
            content: vec![],
        };
        assert!(header_field::is_file_header_field(&header_block));

        let non_header_block = ParsedBlock {
            keyword: "Pin".into(),
            level: keyword_level::SECOND_LEVEL,
            content: vec![],
        };
        assert!(!header_field::is_file_header_field(&non_header_block));
    }

    #[test]
    fn test_is_header_field_keyword_case_insensitive() {
        // Case-insensitive keyword matching.
        assert!(header_field::is_header_field_keyword("IBIS ver"));
        assert!(header_field::is_header_field_keyword("ibis ver"));
        assert!(header_field::is_header_field_keyword("FILE NAME"));
        assert!(!header_field::is_header_field_keyword("Component"));
    }
}
