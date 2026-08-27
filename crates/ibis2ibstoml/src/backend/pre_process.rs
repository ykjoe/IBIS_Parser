// =============================================================================
// pre_process — 第一步：关键字与多实例标记 + 结构层级合法性校验
//
// 遍历 frontend 产出的 `SectionNode` 树，对照 [`crate::schema`]（从
// `ibis_schema.toml` 加载）完成两件事：
//
//   1. **单例 / 多实例标记**：识别并标记每个关键字是单例（`File_Header`、
//      `Ramp`）还是多实例（`[Model]`、`[Pin]`），产出 [`KeywordMark`] 列表；
//   2. **结构层级初步校验**：父子嵌套关系是否合法（未知关键字 / 作用域外）。
//
// 关键字匹配：大小写不敏感，`_` 与空格等价（3.2 §7），见
// [`normalize_keyword`](crate::schema::normalize_keyword)。
// 校验失败记入 [`ValidationCollector`]（严格模式在 `mod.rs` 汇总后报错）。
// 必填性校验由 `schema::keyword_hierarchy` 的 validator 声明式承担，本模块不做。
// =============================================================================

use crate::backend::{SemanticError, ValidationCollector};
use crate::frontend::SectionNode;
use crate::schema::{
    Occurrence, SectionSpec, find_child, find_field, find_root, load_schema, normalize_keyword,
};

/// 一个关键字在 AST 中被标记的出现类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OccurrenceMark {
    /// 单例：父作用域内至多出现一次（如 File_Header、Ramp）。
    Singleton,
    /// 多实例：父作用域内可出现多次（如 Model、Pin）。
    Multi,
}

/// 单例 / 多实例标记：记录规范化关键字名、出现类别与作用域路径。
#[derive(Debug, Clone, PartialEq)]
pub struct KeywordMark {
    pub keyword: String,
    pub occurrence: OccurrenceMark,
    pub scope_path: String,
}

impl KeywordMark {
    pub fn from_spec(spec: &'static SectionSpec, scope_path: String) -> Self {
        let occurrence = match spec.occurrence {
            Occurrence::Once => OccurrenceMark::Singleton,
            Occurrence::Multiple => OccurrenceMark::Multi,
        };
        KeywordMark { keyword: spec.name.to_string(), occurrence, scope_path }
    }
}

/// 运行第一步：关键字与多实例标记 + 结构层级校验。
///
/// 返回按出现顺序排列的 [`KeywordMark`] 列表；结构性问题写入 `collector`。
pub fn keyword_valid(tree: &[SectionNode], collector: &mut ValidationCollector) -> Vec<KeywordMark> {
    let mut marks = Vec::new();
    let schema = load_schema();
    for node in tree {
        let normalized = normalize_keyword(&node.keyword);
        match find_root(schema, &normalized) {
            Some(spec) => {
                mark_scope(node, spec, spec.name.to_string(), &mut marks, collector);
            }
            None => {
                collector.error(SemanticError::UnknownKeyword {
                    scope: "(file)".to_string(),
                    keyword: node.keyword.clone(),
                });
            }
        }
    }
    marks
}

/// 递归标记一个作用域实例及其子实例，并对未知子关键字记录结构错误。
fn mark_scope(
    node: &SectionNode,
    spec: &'static SectionSpec,
    scope_path: String,
    marks: &mut Vec<KeywordMark>,
    collector: &mut ValidationCollector,
) {
    marks.push(KeywordMark::from_spec(spec, scope_path.clone()));

    for child in &node.children {
        let normalized = normalize_keyword(&child.keyword);
        if let Some(child_spec) = find_child(spec, &normalized) {
            let child_path = format!("{}.{}", scope_path, child_spec.name);
            mark_scope(child, child_spec, child_path, marks, collector);
        } else if let Some(_field) = find_field(spec, &normalized) {
            // 虚拟容器（如 File_Header）把子 keyword 存为字段：标记为单例、无子作用域。
            marks.push(KeywordMark {
                keyword: child.keyword.clone(),
                occurrence: OccurrenceMark::Singleton,
                scope_path: format!("{}.{}", scope_path, child.keyword),
            });
        } else {
            collector.error(SemanticError::UnknownKeyword {
                scope: scope_path.clone(),
                keyword: child.keyword.clone(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::NodeKind;

    fn section(keyword: &str, content: Vec<&str>, children: Vec<SectionNode>) -> SectionNode {
        SectionNode {
            keyword: keyword.to_string(),
            kind: NodeKind::Regular,
            content: content.into_iter().map(str::to_string).collect(),
            children,
        }
    }

    #[test]
    fn test_file_header_is_singleton() {
        let tree = vec![section(
            "File_Header",
            vec![],
            vec![
                section("IBIS ver", vec!["2.1"], vec![]),
                section("File name", vec!["test.ibs"], vec![]),
            ],
        )];
        let mut collector = ValidationCollector::new();
        let marks = keyword_valid(&tree, &mut collector);
        assert!(collector.errors.is_empty(), "unexpected: {:?}", collector.errors);

        let header = marks.iter().find(|m| m.keyword == "File_Header").unwrap();
        assert_eq!(header.occurrence, OccurrenceMark::Singleton);
        let ibis_ver = marks.iter().find(|m| m.keyword == "IBIS ver").unwrap();
        assert_eq!(ibis_ver.occurrence, OccurrenceMark::Singleton);
        assert_eq!(ibis_ver.scope_path, "File_Header.IBIS ver");
    }

    #[test]
    fn test_model_is_multi_and_pin_is_singleton() {
        let tree = vec![section(
            "Component",
            vec!["Chip"],
            vec![section("Pin", vec!["1 SIG M1"], vec![])],
        )];
        let mut collector = ValidationCollector::new();
        let marks = keyword_valid(&tree, &mut collector);
        let component = marks.iter().find(|m| m.keyword == "Component").unwrap();
        // [[Component]] 为多实例（一个文件可有多个 [Component]）。
        assert_eq!(component.occurrence, OccurrenceMark::Multi);
        // [Pin] 为 Table 单实例（每个 Component 一个 Pin 表）。
        let pin = marks.iter().find(|m| m.keyword == "Pin").unwrap();
        assert_eq!(pin.occurrence, OccurrenceMark::Singleton);
    }

    #[test]
    fn test_ramp_is_singleton_inside_model() {
        let tree = vec![section("Model", vec!["M1"], vec![section("Ramp", vec!["dv/dt_r 1/1"], vec![])])];
        let mut collector = ValidationCollector::new();
        let marks = keyword_valid(&tree, &mut collector);
        let ramp = marks.iter().find(|m| m.keyword == "Ramp").unwrap();
        assert_eq!(ramp.occurrence, OccurrenceMark::Singleton);
        assert_eq!(ramp.scope_path, "Model.Ramp");
    }

    #[test]
    fn test_unknown_root_keyword_is_reported() {
        let tree = vec![section("Mystery Section", vec!["x"], vec![])];
        let mut collector = ValidationCollector::new();
        let marks = keyword_valid(&tree, &mut collector);
        assert!(marks.is_empty());
        assert!(matches!(
            collector.errors.first(),
            Some(SemanticError::UnknownKeyword { keyword, .. }) if keyword == "Mystery Section"
        ));
    }

    #[test]
    fn test_unknown_child_keyword_is_reported() {
        let tree = vec![section("Component", vec!["Chip"], vec![section("Bogus", vec!["v"], vec![])])];
        let mut collector = ValidationCollector::new();
        let _ = keyword_valid(&tree, &mut collector);
        assert!(matches!(
            collector.errors.first(),
            Some(SemanticError::UnknownKeyword { keyword, .. }) if keyword == "Bogus"
        ));
    }
}
