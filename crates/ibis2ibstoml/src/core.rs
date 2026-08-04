//! 底层基础设施 — 本 crate 全局性、跨阶段公用的数据类型。
//!
//! # 设计约束
//!
//! `core` 只承载"底层基础设施"（如 pest 生成的规则枚举 [`Rule`]），
//! **不承载**任何具体业务子阶段的数据：
//!
//! - 抽象语法树数据结构（`NodeKind` / `SectionNode` / `ParsedBlock`）→ [`frontend::ast_builder`]
//! - 词法 / 语法 / 容错逻辑 → [`frontend::tokenizer`] / [`frontend::syntax_analysis`] / [`frontend::recovery`]

pub use crate::frontend::Rule;
