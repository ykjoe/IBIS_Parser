//! Frontend module — the only public interface: IBIS text in, AST tree out.
//!
//! Internally organized as a pipeline of stages (lexical → syntax → AST), all
//! of which are private submodules:
//!
//! - [`lexical_analysis`] — pest rule export + keyword/content token extraction
//! - [`syntax_analysis`] — produce a flat `ParsedBlock` list (pest grouping + line-by-line fallback)
//! - [`ast_builder`] — AST data structures + `build_section_tree` (`ParsedBlock` → `SectionNode` tree)
//!
//! Each stage exposes its functional primitives as plain functions:
//!
//! | Stage | Functional primitives |
//! |-------|-----------------------|
//! | lexical | keyword-name / content-line reading (`parser` module) + pest-pair extraction |
//! | syntax | line classification + block grouping (incl. fallback recovery) |
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

mod ast_builder;
mod lexical_analysis;
mod syntax_analysis;

use pest::Parser;

pub use ast_builder::{NodeKind, ParsedBlock, SectionNode};
pub use lexical_analysis::Rule;

/// Parse IBIS text into an abstract syntax tree (the frontend's only public
/// interface).
///
/// # Pipeline
///
/// 1. **Lexical** — [`lexical_analysis::IbisParser::parse`] full pest parsing
///    (falls back to syntax-level line-by-line recovery on failure).
/// 2. **Syntax** — [`syntax_analysis::group_pairs_to_blocks`]: pairs → flat
///    [`ParsedBlock`] list.
/// 3. **AST** — [`ast_builder::build_section_tree`]: flat blocks → multi-level
///    [`SectionNode`] tree.
///
/// # Timing gate
///
/// The primary path uses the pest grammar for all three stages. When the full
/// pest parse fails, the syntax stage's [`syntax_analysis::recover_blocks`]
/// takes over, reusing the lexical keyword-name and line-classification
/// primitives to parse line by line, then reuses the same AST builder.
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
/// The current implementation never returns `Err`: when the full pest parse
/// fails, the syntax-level fallback takes over and always yields a block list
/// that can be turned into a tree.
///
/// # Panics
///
/// Does not panic under normal operation.
pub fn parse(content: &str) -> Result<Vec<SectionNode>, String> {
    // Timing gate: try the primary pest path first.
    let blocks = match lexical_analysis::IbisParser::parse(Rule::ibis_file, content) {
        Ok(pairs) => syntax_analysis::group_pairs_to_blocks(pairs),
        // Fall back to the line-by-line path when pest fails.
        Err(_parse_error) => syntax_analysis::recover_blocks(content),
    };

    // AST building (blocks → multi-level section tree).
    Ok(build_tree_from_blocks(&blocks))
}

/// Build a root-level section tree from a flat block list.
///
/// Handles multiple root-level groups by repeatedly invoking
/// [`ast_builder::build_section_tree`].
///
/// # Parameters
///
/// * `blocks` — The flat `ParsedBlock` list to build from.
///
/// # Returns
///
/// The root-level [`SectionNode`] tree.
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
