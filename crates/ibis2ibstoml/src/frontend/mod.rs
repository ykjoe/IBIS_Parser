//! 前端模块 — 唯一公开接口：输入 IBIS 文本，输出抽象语法树。
//!
//! 内部按 Pipeline 阶段组织（词法 → 语法 → AST → 容错），各阶段均为私有子模块：
//!
//! - [`lexical_analysis`] — 词法分析：pest 规则导出 + 关键词/内容 token 提取
//! - [`syntax_analysis`] — 语法分析：pairs 展平为扁平 `ParsedBlock` 列表
//! - [`ast_builder`] — 抽象语法树：AST 数据结构 + `build_section_tree`（`ParsedBlock` → `SectionNode` 树）
//! - [`recovery`] — 容错回退解析（pest 失败时逐行解析）
//!
//! # 设计约束
//!
//! - 仅通过 [`parse`] 对外提供能力，内部流程不公开
//! - 所有值以原始字符串保留，不进行数值转换或单位换算
//! - 不区分 `[[array-of-tables]]` 与 `[...]`，该决策属于后端职责

mod ast_builder;
mod recovery;
mod syntax_analysis;
mod lexical_analysis;

use pest::Parser;

pub use ast_builder::{NodeKind, ParsedBlock, SectionNode};
pub use lexical_analysis::Rule;

/// 前端唯一公开接口：输入 IBIS 文本 → 输出抽象语法树。
///
/// # Pipeline
///
/// 1. **词法** — [`lexical_analysis::IbisParser::parse`] pest 完整解析（失败 → [`recovery`] 容错回退）
/// 2. **语法** — [`syntax_analysis::group_pairs_to_blocks`] pairs → 扁平 [`ParsedBlock`]
/// 3. **AST** — [`ast_builder::build_section_tree`] `ParsedBlock` → 多级 [`SectionNode`] 树
///
/// # Parameters
///
/// * `content` — A string containing the full text of an IBIS file.
///
/// # Returns
///
/// * `Ok(Vec<SectionNode>)` — 根级抽象语法树（含 `[File_Header]` 虚拟节点）。
/// * `Err(String)` — A human-readable error message if parsing fails.
pub fn parse(content: &str) -> Result<Vec<SectionNode>, String> {
    // ── Phase 1: 词法（pest 完整解析；失败走容错回退）──
    let parsed_pairs = match lexical_analysis::IbisParser::parse(Rule::ibis_file, content) {
        Ok(pairs) => pairs,
        Err(_parse_error) => {
            let blocks = recovery::recover_blocks(content);
            return Ok(build_tree_from_blocks(&blocks));
        }
    };

    // ── Phase 2: 语法（pairs → 扁平 block）──
    let blocks = syntax_analysis::group_pairs_to_blocks(parsed_pairs);

    // ── Phase 3: AST（block → 多级节段树）──
    Ok(build_tree_from_blocks(&blocks))
}

/// 将扁平 block 列表构建为根级节段树（处理多个根级分组）。
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
