//! schema — IBIS 结构规范数据源（唯一 IBIS 结构数据源）。
//!
//! 从 [`ibis_schema.toml`](ibis_schema.toml) 加载 IBIS 手册结构（keyword 树 +
//! 同名 section + content 字段与类型），供 backend 结构校验、frontend pest 生成
//! 与示例模板使用。
//!
//! # 设计约定
//!
//! - `[Section]`   → 单实例 keyword 的同名 section（TOML 单表）
//! - `[[Section]]` → 多实例 keyword 的同名 section（TOML 数组表）
//! - 第一个字段名 = keyword 名本身；无标识符（空类型）写 `"()"`。
//!
//! 加载使用 `toml::Value` 手动遍历（表名为动态 keyword），无需 serde 反序列化
//! 目标类型；`Occurrence` 由 `[..]` / `[[..]]` 形态推断。

pub mod keyword_hierarchy;

use std::sync::OnceLock;

use toml::Value;

/// 一个 keyword 在父作用域内的出现频次（由 `[..]` / `[[..]]` 推断）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Occurrence {
    /// 至多出现一次（TOML 单表 `[...]`）。
    Once,
    /// 可出现多次（TOML 数组表 `[[...]]`）。
    Multiple,
}

/// 一个 content 字段（第一个字段 = keyword 名本身）。
///
/// 字段的 **类型** 由 `schema::keyword_hierarchy` 的结构体字段定义（单一来源）；本表只承载
/// 字段名（key）与 IBIS 手册中的**规范名称**（canonical name）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSpec {
    /// content 字段名（如 `"IBIS ver"` / `"Pin"` / `"signal_name"`）。
    pub key: String,
    /// IBIS 手册中的正式术语名（canonical name，如 `[Pin]` 的 `"signal_name"`）。
    pub spec_name: String,
}

/// 加载后的节段结构（由 ibis_schema.toml 反序列化）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionSpec {
    /// 节段名（如 `"Model"` / `"Pin"`）。
    pub name: String,
    /// 出现频次（由 `[..]` / `[[..]]` 推断）。
    pub occurrence: Occurrence,
    /// content 字段（有标识符时第一个字段 = keyword 名；空类型为 `"()"`）。
    pub fields: Vec<FieldSpec>,
    /// 子节段（递归）。
    pub children: Vec<SectionSpec>,
}

impl SectionSpec {
    /// 该节段是否有子节段。
    pub fn is_container(&self) -> bool {
        !self.children.is_empty()
    }
}

/// 关键字归一化：小写 + `_` 与空格等价（3.2 §7）。
pub fn normalize_keyword(keyword: &str) -> String {
    keyword.trim().to_ascii_lowercase().replace('_', " ")
}

/// 全局 schema（懒加载单例）。
static SCHEMA: OnceLock<Vec<SectionSpec>> = OnceLock::new();

/// 加载并解析 ibis_schema.toml，产出根节段列表。
///
/// 解析失败视为编程错误（schema 是随 crate 分发的静态数据），直接 panic。
pub fn load_schema() -> &'static [SectionSpec] {
    SCHEMA.get_or_init(|| parse_schema(include_str!("ibis_schema.toml")))
}

/// 解析 schema TOML 文本 → 根节段列表。
fn parse_schema(text: &str) -> Vec<SectionSpec> {
    let root: Value = toml::from_str(text).expect("ibis_schema.toml 应可被 toml 解析");
    let table = root.as_table().expect("ibis_schema.toml 顶层应为表");
    table
        .iter()
        .map(|(name, value)| parse_section_value(name, value))
        .collect()
}

/// 解析一个表项为节段：`Table` → 单实例；`Array`（数组表）→ 多实例。
fn parse_section_value(name: &str, value: &Value) -> SectionSpec {
    let (fields, children) = match value {
        Value::Table(section_table) => {
            (collect_fields(section_table), collect_children(section_table))
        }
        Value::Array(array) => {
            // [[Section]] 数组表：取第一个表元素（schema 中通常只有一个）。
            let first = array
                .first()
                .and_then(Value::as_table)
                .expect("[[Section]] 的元素应为表");
            (collect_fields(first), collect_children(first))
        }
        other => panic!("ibis_schema.toml 顶层 '{name}' 应为表或数组表，得到 {other:?}"),
    };
    let mut fields = fields;
    // toml 的 Map 按 key 字母序排序，不保留书写顺序；
    // 把「keyword 名同名字段」排到最前，保持"第一个字段 = keyword 名"约定。
    let normalized = normalize_keyword(name);
    fields.sort_by_key(|field| {
        if normalize_keyword(&field.key) == normalized {
            0
        } else {
            1
        }
    });
    SectionSpec {
        name: name.to_string(),
        occurrence: if matches!(value, Value::Array(_)) { Occurrence::Multiple } else { Occurrence::Once },
        fields,
        children,
    }
}

/// 收集节段内的 content 字段（`Value::String` 成员）。
fn collect_fields(section_table: &toml::map::Map<String, Value>) -> Vec<FieldSpec> {
    section_table
        .iter()
        .filter_map(|(key, value)| {
            value.as_str().map(|spec_name| FieldSpec {
                key: key.clone(),
                spec_name: spec_name.to_string(),
            })
        })
        .collect()
}

/// 收集节段内的子节段（`Value::Table` 或 `Value::Array` 成员）。
fn collect_children(section_table: &toml::map::Map<String, Value>) -> Vec<SectionSpec> {
    section_table
        .iter()
        .filter(|(_, value)| matches!(value, Value::Table(_) | Value::Array(_)))
        .map(|(name, value)| parse_section_value(name, value))
        .collect()
}

/// 在根节段列表中按规范化名查找。
pub fn find_root<'a>(sections: &'a [SectionSpec], normalized_name: &str) -> Option<&'a SectionSpec> {
    sections
        .iter()
        .find(|spec| normalize_keyword(&spec.name) == normalized_name)
}

/// 在节段的子节段中按规范化名查找。
pub fn find_child<'a>(spec: &'a SectionSpec, normalized_name: &str) -> Option<&'a SectionSpec> {
    spec.children
        .iter()
        .find(|child| normalize_keyword(&child.name) == normalized_name)
}

/// 在节段的 content 字段中按规范化名查找。
///
/// 兼容虚拟容器 `File_Header` 把子 keyword（如 "IBIS ver"）表示为平铺字段的写法。
pub fn find_field<'a>(spec: &'a SectionSpec, normalized_name: &str) -> Option<&'a FieldSpec> {
    spec.fields
        .iter()
        .find(|field| normalize_keyword(&field.key) == normalized_name)
}

/// 文件头节段（虚拟容器），供 frontend 文件头字段判定复用。
pub fn file_header_section() -> &'static SectionSpec {
    let root = load_schema();
    find_root(root, &normalize_keyword("File_Header"))
        .expect("ibis_schema.toml 应包含 File_Header 虚拟节段")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_keyword() {
        assert_eq!(normalize_keyword("Diff_pin"), "diff pin");
        assert_eq!(normalize_keyword("IBIS Ver"), "ibis ver");
        assert_eq!(normalize_keyword("File_Name"), "file name");
    }

    #[test]
    fn test_load_schema_has_required_roots() {
        let root = load_schema();
        for expected in [
            "File_Header",
            "Component",
            "Model Selector",
            "Model",
            "Submodel",
            "External Circuit",
            "Test Data",
            "Test Load",
            "Define Package Model",
            "Interconnect Model Set",
        ] {
            let normalized = normalize_keyword(expected);
            assert!(
                find_root(root, &normalized).is_some(),
                "ibis_schema.toml 应包含根节段 '{expected}'"
            );
        }
    }

    #[test]
    fn test_no_duplicate_root_names() {
        let root = load_schema();
        let mut seen = std::collections::HashSet::new();
        for spec in root {
            let normalized = normalize_keyword(&spec.name);
            assert!(
                seen.insert(normalized.clone()),
                "根节段名 '{normalized}' 重复"
            );
        }
    }

    #[test]
    fn test_component_structure() {
        let root = load_schema();
        let component = find_root(root, &normalize_keyword("Component")).unwrap();
        assert_eq!(component.occurrence, Occurrence::Multiple);
        // 第一个字段 = keyword 名本身（标识符）。
        assert_eq!(component.fields.first().map(|f| f.key.as_str()), Some("component"));
        // 子节段应含 Manufacturer / Package / Pin。
        for expected in ["Manufacturer", "Package", "Pin"] {
            assert!(
                find_child(component, &normalize_keyword(expected)).is_some(),
                "Component 应含子节段 '{expected}'"
            );
        }
    }

    #[test]
    fn test_package_first_field_is_keyword_name() {
        let root = load_schema();
        let component = find_root(root, &normalize_keyword("Component")).unwrap();
        let package = find_child(component, &normalize_keyword("Package")).unwrap();
        // 空类型（无标识符）：第一个字段名 = keyword 名本身。
        assert_eq!(
            package.fields.first().map(|f| f.key.as_str()),
            Some("package"),
            "Package 为空类型，首字段应为同名 keyword"
        );
    }

    #[test]
    fn test_every_keyword_has_same_name_section() {
        // 完整性：递归遍历所有节段，断言第一个字段名 = 节段名（空类型为 "()"）。
        // 跳过 File_Header 虚拟容器（其字段是各文件头 keyword，本身即真实 keyword 名）。
        fn check(spec: &SectionSpec) {
            if normalize_keyword(&spec.name) != normalize_keyword("File_Header")
                && let Some(first) = spec.fields.first()
            {
                let normalized_field = normalize_keyword(&first.key);
                let normalized_section = normalize_keyword(&spec.name);
                assert!(
                    normalized_field == normalized_section,
                    "节段 '{}' 的第一个字段应为同名 keyword，实际为 '{}'",
                    spec.name,
                    first.key
                );
            }
            for child in &spec.children {
                check(child);
            }
        }
        for spec in load_schema() {
            check(spec);
        }
    }

    #[test]
    fn test_schema_keywords_parseable_by_frontend() {
        // schema↔pest 一致性：schema 中每个节段名（递归）都能被 frontend 解析为
        // 一个合法节段（pest kw_* 精确规则或 generic `keyword` fallback 均可）。
        fn check(spec: &SectionSpec) {
            let content = format!("[{}]\n", spec.name);
            let tree = crate::frontend::parse(&content).unwrap_or_else(|e| {
                panic!("schema 节段名 '[{}]' 无法被 frontend 解析: {e}", spec.name)
            });
            assert!(
                !tree.is_empty(),
                "schema 节段名 '[{}]' 解析后无节段",
                spec.name
            );
            for child in &spec.children {
                check(child);
            }
        }
        for spec in load_schema() {
            check(spec);
        }
    }
}
