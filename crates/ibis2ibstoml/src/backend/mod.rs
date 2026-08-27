//! Backend module — the semantic layer of the pipeline.
//!
//! Consumes the [`SectionNode`](crate::frontend::SectionNode) tree produced by
//! the frontend and produces a numerically-typed strong model [`IBIS_File`]
//! ([`ibis_structure`]) through a **three-step** pipeline:
//!
//! 1. [`pre_process`]（第一步）— 关键字与多实例标记：识别单例（`File_Header`、
//!    `Ramp`）与多实例（`[Model]` / `[Pin]`）关键字，并对照
//!    [`crate::schema::keyword_hierarchy`] 校验结构层级（父子嵌套关系）。
//! 2. [`symbol_table_build`]（第二步）— 结构解构与符号表构建：消费 AST，将
//!    `content` 文本行彻底解析并剥离，重建为保留「数值 + 单位」的强类型领域模型
//!    与符号表（`IndexMap<String, IBIS_Model>` / `Vec<Component_Pin>` / `Table`）。
//! 3. [`validation`]（第三步）— 物理与逻辑数据校验：IV 曲线电压严格单调递增、
//!    $V_{min}\le V_{typ}\le V_{max}$、`Component_Pin.model_name` 符号引用存在性。
//!
//! 核心设计原则（零字符串污染）：
//!
//! - **Rebuild 而非 In-place**：AST 是一次性语法结构，只读解构，不保留、不原地修改。
//! - 所有带工程单位的文本（`1.12p` / `10mA` / `3.3V`）保留为 `Quantity { value, unit }`，
//!   不做归一化换算；`NA` → 缺失（`None`）。字符串仅保留在标识符 / 自由文本字段。
//!
//! 规范以**声明式数据源**集中存放，代码只读不重复实现：
//!
//! - [`crate::schema::keyword_hierarchy`] — 关键字作用域树注册表（名称 / 必填 / 单次多次 /
//!   子作用域）、强类型领域模型与基础值类型（`Quantity` / `Table` 等）；
//! - [`crate::backend::rules`] — 数值解析规则（`parse_quantity` / `quantity_to_f64`）。
//!
//! 校验提供**严格 / 宽松双模式**：
//!
//! - [`semantic_parse`] — 严格：任一阶段首个错误即返回 `Err(SemanticError)`。
//! - [`semantic_parse_lenient`] — 宽松：问题写入 [`ValidationReport`]，不阻断转换。

pub(crate) mod pre_process;
pub(crate) mod rules;
pub(crate) mod symbol_table_build;
pub(crate) mod validation;

use crate::frontend::SectionNode;
use crate::schema::keyword_hierarchy::IBIS_File;

// -----------------------------------------------------------------------------
// 错误模型
// -----------------------------------------------------------------------------

/// Structured semantic error emitted by the backend.
///
/// 变体按三步走分组：第一步 标记 / 第二步 重建 / 第三步 校验。
#[derive(Debug, Clone, PartialEq)]
pub enum SemanticError {
    // -------- 第一步：关键字与多实例标记 --------
    /// 未知关键字（当前作用域下未登记）。
    UnknownKeyword { scope: String, keyword: String },
    /// 已知但出现在错误的作用域。
    KeywordOutOfScope { keyword: String, expected_scope: String },
    // -------- 第二步：结构解构与符号表构建 --------
    /// 必填子关键字缺失。
    MissingRequiredKeyword { scope: String, keyword: String },
    /// 单次关键字出现多次。
    KeywordAppearsMoreThanOnce { scope: String, keyword: String },
    /// 必填内容字段缺失（如 `[Model]` 缺 `Model_type`）。
    MissingRequiredField { section: String, field: String },
    /// 数值解析失败（零字符串污染：构建期即解析）。
    InvalidNumber { section: String, field: String, value: String },
    /// 表格列数不一致。
    TableMalformed { section: String },
    // -------- 第三步：物理与逻辑数据校验 --------
    /// 角点三元组顺序违规（min ≤ typ ≤ max 失败，数值化比较）。
    ValueOrderViolation { section: String, field: String, values: Vec<f64> },
    /// I-V 曲线单调性违规（电压非严格递增）。
    NonMonotonicCurve { section: String, detail: String },
    /// 引用指向不存在的目标（如 Pin 引用不存在的 model）。
    ReferenceNotFound { from: String, target: String },
    /// 3.2 语法规则违规（字符集 / 行长 / 文件名等）。
    InvalidSyntax { rule: &'static str, detail: String },
    /// 保留字误用。
    ReservedWordMisuse { word: String, detail: String },
    /// validator 声明式校验未通过（corner 范围 / IV 单调性 / 必填等）。
    ValidationFailed { section: String, field: String, message: String },
}

impl SemanticError {
    /// 问题所属的作用域/节段（用于 `ValidationReport` 定位）。
    pub fn scope(&self) -> &str {
        match self {
            SemanticError::UnknownKeyword { scope, .. }
            | SemanticError::MissingRequiredKeyword { scope, .. }
            | SemanticError::KeywordAppearsMoreThanOnce { scope, .. } => scope,
            SemanticError::MissingRequiredField { section, .. }
            | SemanticError::InvalidNumber { section, .. }
            | SemanticError::TableMalformed { section }
            | SemanticError::ValueOrderViolation { section, .. }
            | SemanticError::NonMonotonicCurve { section, .. }
            | SemanticError::ValidationFailed { section, .. } => section,
            SemanticError::KeywordOutOfScope { .. }
            | SemanticError::ReferenceNotFound { .. }
            | SemanticError::ReservedWordMisuse { .. }
            | SemanticError::InvalidSyntax { .. } => "(file)",
        }
    }
}

impl std::fmt::Display for SemanticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SemanticError::UnknownKeyword { scope, keyword } => {
                write!(f, "Unknown keyword '{keyword}' in scope '{scope}'")
            }
            SemanticError::KeywordOutOfScope { keyword, expected_scope } => {
                write!(f, "Keyword '{keyword}' not expected in scope '{expected_scope}'")
            }
            SemanticError::MissingRequiredKeyword { scope, keyword } => {
                write!(f, "Missing required keyword '{keyword}' in scope '{scope}'")
            }
            SemanticError::KeywordAppearsMoreThanOnce { scope, keyword } => {
                write!(f, "Keyword '{keyword}' appears more than once in scope '{scope}'")
            }
            SemanticError::MissingRequiredField { section, field } => {
                write!(f, "Missing required field '{field}' in section '{section}'")
            }
            SemanticError::InvalidNumber { section, field, value } => {
                write!(f, "Invalid number '{value}' for '{field}' in section '{section}'")
            }
            SemanticError::TableMalformed { section } => {
                write!(f, "Table in section '{section}' has inconsistent column counts")
            }
            SemanticError::ValueOrderViolation { section, field, values } => {
                write!(f, "Corner '{field}' in section '{section}' violates min <= typ <= max ({values:?})")
            }
            SemanticError::NonMonotonicCurve { section, detail } => {
                write!(f, "I-V curve in section '{section}' is not monotonic: {detail}")
            }
            SemanticError::ReferenceNotFound { from, target } => {
                write!(f, "Reference '{from}' points to nonexistent target '{target}'")
            }
            SemanticError::InvalidSyntax { rule, detail } => {
                write!(f, "Syntax rule '{rule}' violated: {detail}")
            }
            SemanticError::ReservedWordMisuse { word, detail } => {
                write!(f, "Reserved word '{word}' used improperly: {detail}")
            }
            SemanticError::ValidationFailed { section, field, message } => {
                write!(f, "Validation failed in '{section}.{field}': {message}")
            }
        }
    }
}

// -----------------------------------------------------------------------------
// 宽松模式：校验报告
// -----------------------------------------------------------------------------

/// 宽松模式下收集的校验问题（错误 + 警告）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ValidationReport {
    pub errors: Vec<Issue>,
    pub warnings: Vec<Issue>,
}

/// 一条校验问题（带作用域定位与可读信息）。
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub scope: String,
    pub message: String,
}

/// 内部问题收集器：严格模式取首个错误；宽松模式全量收集。
#[derive(Debug, Default)]
pub(crate) struct ValidationCollector {
    pub errors: Vec<SemanticError>,
    pub warnings: Vec<SemanticError>,
}

impl ValidationCollector {
    pub(crate) fn new() -> Self {
        Self::default()
    }
    pub(crate) fn error(&mut self, error: SemanticError) {
        self.errors.push(error);
    }
}

// -----------------------------------------------------------------------------
// 编排入口（三步走）
// -----------------------------------------------------------------------------

/// 运行三步走流水线（keyword_valid → symbol_table_build → data_valid），
/// 返回数值化强类型文件与收集器。
fn run_phases(tree: &[SectionNode], collector: &mut ValidationCollector) -> IBIS_File {
    // 第一步：关键字与多实例标记 + 结构层级校验。
    let marks = pre_process::keyword_valid(tree, collector);
    // 第二步：结构解构与符号表构建（Rebuild + 数值化；宽松模式失败值以 None 占位）。
    let file = symbol_table_build::symbol_table_build(tree, &marks, collector);
    // 第三步：物理与逻辑数据校验（单调性 / 范围 / 符号引用）。
    validation::data_valid(&file, collector);
    file
}

/// Consume a [`SectionNode`] tree and produce a numerically-typed strong model
/// [`IBIS_File`] under **strict** validation: the first error from any step
/// aborts the parse.
///
/// # Parameters
///
/// * `tree` — The frontend-produced AST tree.
///
/// # Returns
///
/// * `Ok(IBIS_File)` — The numerically-typed semantic model (no string pollution).
/// * `Err(SemanticError)` — The first structured semantic error.
pub fn semantic_parse(tree: &[SectionNode]) -> Result<IBIS_File, SemanticError> {
    let mut collector = ValidationCollector::new();
    let file = run_phases(tree, &mut collector);
    if let Some(first_error) = collector.errors.into_iter().next() {
        return Err(first_error);
    }
    Ok(file)
}

/// Consume a [`SectionNode`] tree and produce a numerically-typed strong model
/// [`IBIS_File`] under **lenient** validation: issues are collected into a
/// [`ValidationReport`] instead of aborting the conversion.
///
/// # Parameters
///
/// * `tree` — The frontend-produced AST tree.
///
/// # Returns
///
/// * `Ok((IBIS_File, ValidationReport))` — The numerically-typed strong model plus
///   the collected issues (errors + warnings).
/// * `Err(SemanticError)` — Only returned if the semantic mapping itself fails
///   (currently never happens; unknown sections are skipped).
pub fn semantic_parse_lenient(
    tree: &[SectionNode],
) -> Result<(IBIS_File, ValidationReport), SemanticError> {
    let mut collector = ValidationCollector::new();
    let file = run_phases(tree, &mut collector);
    let report = ValidationReport {
        errors: collector
            .errors
            .into_iter()
            .map(|error| Issue { scope: error.scope().to_string(), message: error.to_string() })
            .collect(),
        warnings: collector
            .warnings
            .into_iter()
            .map(|error| Issue { scope: error.scope().to_string(), message: error.to_string() })
            .collect(),
    };
    Ok((file, report))
}

/// Convenience wrapper that converts errors into human-readable strings
/// (compatible with the older `String`-based API style).
///
/// # Parameters
///
/// * `tree` — The frontend-produced AST tree.
///
/// # Returns
///
/// * `Ok(IBIS_File)` — The numerically-typed semantic model.
/// * `Err(String)` — A human-readable error message.
pub fn semantic_parse_string(tree: &[SectionNode]) -> Result<IBIS_File, String> {
    semantic_parse(tree).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::{NodeKind, SectionNode};

    /// Build a bare `SectionNode` quickly in tests.
    fn section(keyword: &str, content: Vec<&str>, children: Vec<SectionNode>) -> SectionNode {
        SectionNode {
            keyword: keyword.to_string(),
            kind: NodeKind::Regular,
            content: content.into_iter().map(str::to_string).collect(),
            children,
        }
    }

    /// Build a `[File_Header]` virtual node (children are header fields).
    fn header_node(children: Vec<SectionNode>) -> SectionNode {
        SectionNode {
            keyword: "File_Header".into(),
            kind: NodeKind::FileHeader,
            content: vec![],
            children,
        }
    }

    fn valid_tree() -> Vec<SectionNode> {
        vec![header_node(vec![
            section("IBIS ver", vec!["2.1"], vec![]),
            section("File name", vec!["test.ibs"], vec![]),
            section("File Rev", vec!["1.0"], vec![]),
        ])]
    }

    #[test]
    fn test_semantic_parse_strict_passes_on_valid_tree() {
        let tree = valid_tree();
        assert!(semantic_parse(&tree).is_ok());
    }

    #[test]
    fn test_semantic_parse_strict_reports_missing_required_header() {
        // 缺 [IBIS ver]：必填由 validator 声明式校验（ValidationFailed）。
        let tree = vec![header_node(vec![
            section("File name", vec!["test.ibs"], vec![]),
            section("File Rev", vec!["1.0"], vec![]),
        ])];
        assert!(matches!(
            semantic_parse(&tree),
            Err(SemanticError::ValidationFailed { .. })
        ));
    }

    #[test]
    fn test_semantic_parse_strict_reports_invalid_number() {
        let tree = vec![section("Model", vec!["M1", "Model_type I/O", "Vref = abc"], vec![])];
        assert!(matches!(
            semantic_parse(&tree),
            Err(SemanticError::InvalidNumber { .. })
        ));
    }

    #[test]
    fn test_semantic_parse_lenient_collects_issues() {
        // 未知关键字在宽松模式下被收集而不阻断。
        let tree = vec![
            header_node(vec![]),
            section("Some Unknown Section", vec!["x"], vec![]),
        ];
        let (_file, report) = semantic_parse_lenient(&tree).unwrap();
        assert!(
            report.errors.iter().any(|issue| issue.message.contains("Unknown keyword")),
            "expected an unknown-keyword issue in {:?}",
            report.errors
        );
    }

    #[test]
    fn test_semantic_parse_lenient_never_blocks() {
        let tree = vec![section("Model", vec!["M1"], vec![])];
        let (file, _report) = semantic_parse_lenient(&tree).unwrap();
        assert_eq!(file.models.len(), 1);
    }

    #[test]
    fn test_model_value_preserves_value_and_unit() {
        // 保留「数值 + 单位」：`1.12p` → Quantity { value: Number(1.12), unit: "p" }。
        let tree = vec![
            header_node(vec![
                section("IBIS ver", vec!["2.1"], vec![]),
                section("File name", vec!["test.ibs"], vec![]),
                section("File Rev", vec!["1.0"], vec![]),
            ]),
            section("Model", vec!["M1", "Model_type I/O", "C_comp 1.12p"], vec![]),
        ];
        let file = semantic_parse(&tree).unwrap();
        let c_comp = file.models.get("M1").unwrap().c_comp.as_ref().unwrap();
        let crate::schema::keyword_hierarchy::Cell::Quantity(typ) = &c_comp.matrix[0][0] else {
            panic!("expected quantity cell")
        };
        assert_eq!(typ.value, crate::schema::keyword_hierarchy::Scalar::Number(1.12));
        assert_eq!(typ.unit.as_deref(), Some("p"));
    }
}
