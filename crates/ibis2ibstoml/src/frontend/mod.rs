//! Frontend module — the only public interface: IBIS text in, AST tree out.
//!
//! Internally organized as a pipeline of stages (lexical → syntax → AST →
//! recovery), all of which are private submodules:
//!
//! - [`lexical_analysis`] — pest rule export + keyword/content token extraction
//! - [`syntax_analysis`] — flatten pairs into a flat `ParsedBlock` list
//! - [`ast_builder`] — AST data structures + `build_section_tree` (`ParsedBlock` → `SectionNode` tree)
//! - [`recovery`] — Fault-tolerant fallback parsing (line-by-line when pest fails)
//!
//! Each stage owns its functional capabilities as traits and a carrier type
//! that implements them:
//!
//! | Stage | Functional capability trait | Carrier |
//! |-------|-----------------------------|---------|
//! | lexical | [`KeywordNameParser`], [`ContentParser`] | `LexicalAnalysis` |
//! | syntax | [`LineClassParser`] | `SyntaxAnalysis` |
//! | AST | [`HeaderFieldParser`] | `AstBuilder` |
//!
//! These traits are internal and consumed by the pipeline; only the module
//! doc-level [`parse`] is public. Other layers (e.g. the backend) define their
//! own capabilities as needed.
//!
//! # Design constraints
//!
//! - Exposes capabilities only through [`parse`]; internal flow is not public
//! - All values are preserved as raw strings; no numeric conversion or unit scaling
//! - Does not distinguish `[[array-of-tables]]` from `[...]`; that decision belongs to the backend

mod ast_builder;
mod recovery;
mod syntax_analysis;
mod lexical_analysis;

use pest::Parser;

pub use ast_builder::{NodeKind, ParsedBlock, SectionNode};
pub use lexical_analysis::Rule;

/// Parse IBIS text into an abstract syntax tree (the frontend's only public
/// interface).
///
/// # Pipeline
///
/// 1. **Lexical** — [`lexical_analysis::IbisParser::parse`] full pest parsing
///    (falls back to [`recovery`] on failure).
/// 2. **Syntax** — [`syntax_analysis::group_pairs_to_blocks`]: pairs → flat
///    [`ParsedBlock`] list.
/// 3. **AST** — [`ast_builder::build_section_tree`]: flat blocks → multi-level
///    [`SectionNode`] tree.
///
/// # Timing gate
///
/// The primary path uses the pest grammar for all three stages. When the full
/// pest parse fails, [`recovery`] takes over, consuming the lexical
/// [`KeywordNameParser`] and syntax [`LineClassParser`] capabilities to parse
/// line by line, then reuses the same AST builder.
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
/// fails, [`recovery`] takes over and always yields a block list that can be
/// turned into a tree.
///
/// # Panics
///
/// Does not panic under normal operation.
pub fn parse(content: &str) -> Result<Vec<SectionNode>, String> {
    // Stage carriers implementing the functional capabilities used by the
    // fault-tolerant recovery path.
    let lexical = lexical_analysis::LexicalAnalysis;
    let syntax = syntax_analysis::SyntaxAnalysis;

    // Timing gate: try the primary pest path first.
    let blocks = match lexical_analysis::IbisParser::parse(Rule::ibis_file, content) {
        Ok(pairs) => syntax_analysis::group_pairs_to_blocks(pairs),
        // Fall back to the line-by-line path when pest fails.
        Err(_parse_error) => recovery::recover_blocks(content, &lexical, &syntax),
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
