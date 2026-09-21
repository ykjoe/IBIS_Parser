//! Frontend module — the only public interface: IBIS text in, AST tree out.
//!
//! Internally organized as a pipeline of stages (lexical → syntax → AST), all
//! of which are private submodules:
//!
//! - [`lexical_analysis`] — reading primitives (`[Keyword]` header token →
//!   keyword name, content line → normalized text)
//! - [`syntax_analysis`] — produce a flat `ParsedBlock` list (line
//!   classification + block grouping, with each block's level read from the schema)
//! - [`ast_builder`] — AST data structures + `build_section_tree` (`ParsedBlock` → `SectionNode` tree)
//!
//! Each stage exposes its functional primitives as plain functions:
//!
//! | Stage | Functional primitives |
//! |-------|-----------------------|
//! | lexical | keyword-name / content-line reading (`parser` module) |
//! | syntax | line classification + block grouping (schema-derived levels) |
//! | AST | file-header classification + tree building |
//!
//! These internals are consumed by the pipeline; only the module doc-level
//! [`parse`] is public. Other layers (e.g. the backend) define their own
//! capabilities as needed.
//!
//! # Design constraints
//!
//! - Exposes capabilities only through [`parse`]; internal flow is not public
//! - All values are preserved as raw strings; no numeric conversion or unit scaling
//! - Does not distinguish `[[array-of-tables]]` from `[...]`; that decision belongs to the backend
//! - No grammar engine: the nesting level of every keyword comes from
//!   [`ibis_schema.toml`](crate::schema), the only structural data source

mod ast_builder;
mod lexical_analysis;
mod syntax_analysis;

pub use ast_builder::{NodeKind, ParsedBlock, SectionNode};

/// Parse IBIS text into an abstract syntax tree (the frontend's only public
/// interface).
///
/// # Pipeline
///
/// 1. **Syntax** — [`syntax_analysis::group_lines_to_blocks`]: raw text → flat
///    [`ParsedBlock`] list, each keyword header given its schema depth.
/// 2. **AST** — [`ast_builder::build_section_tree`]: flat blocks → multi-level
///    [`SectionNode`] tree.
///
/// # Reading model
///
/// Grouping is line-by-line and total: a `[Keyword]` header opens a block, the
/// lines that follow accumulate into it, and `|`-prefixed or blank lines are
/// skipped. The nesting level of a keyword is read from
/// [`ibis_schema.toml`](crate::schema) — the only structural data source — so
/// there is no grammar engine and no second parsing path to keep in sync.
///
/// # Parameters
///
/// * `content` — A string containing the full text of an IBIS file.
///
/// # Returns
///
/// * `Ok(Vec<SectionNode>)` — The root-level AST, including the `[File_Header]`
///   virtual node.
/// * `Err(String)` — A human-readable error message if parsing fails.
///
/// # Errors
///
/// The current implementation never returns `Err`: grouping always yields a
/// block list that can be turned into a tree.
///
/// # Panics
///
/// Does not panic under normal operation.
pub fn parse(content: &str) -> Result<Vec<SectionNode>, String> {
    // Syntax stage: raw text → flat block list (levels read from the schema).
    let blocks = syntax_analysis::group_lines_to_blocks(content);

    // AST building (blocks → multi-level section tree).
    let ast_tree = build_tree_from_blocks(&blocks);

    Ok(ast_tree)
}

/// Build a root-level section tree [`SectionNode`] from a flat block list.
///
/// Handles multiple root-level groups by repeatedly invoking
/// [`ast_builder::build_section_tree`].
fn build_tree_from_blocks(blocks: &[ParsedBlock]) -> Vec<SectionNode> {
    let mut tree: Vec<SectionNode> = Vec::new();
    let mut block_index = 0;
    while block_index < blocks.len() {
        let (mut nodes, next_index) = ast_builder::build_section_tree(blocks, block_index, &[]);
        tree.append(&mut nodes);
        block_index = next_index;
    }
    tree
}
