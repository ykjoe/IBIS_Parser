//! 输出模块 — 将 AST 树导出为各目标格式。
//!
//! # 子模块
//!
//! - [`toml`] — 从 [`SectionNode`](crate::frontend::ast_builder::SectionNode) 树生成 TOML 字符串
//!
//! # 设计约束
//!
//! - 所有值以原始字符串保留，输出层不做数值转换
//! - 目前仅支持 TOML；后续可扩展 JSON / YAML 等格式

pub mod toml;

pub use toml::serialize_tree;
pub use toml::serialize_tree_to_string;
