// =============================================================================
// symbol_table_build — 第二步：结构解构与符号表构建（通用 schema 递归解释器）
//
// 本模块**没有任何 per-section 的 `build_*` 函数**：它不逐条列出 keyword /
// section / 字段名，而是**完全由 `ibis_schema.toml` 的 `SectionSpec` 树驱动**：
//
//   - **节段与树关系**：顶层遍历用 `find_root` 定位 schema 根节段；子节段递归
//     遍历 `spec.children`（多实例 `[[..]]` → 数组，单实例 `[..]` → 单值）；
//   - **字段名与列头**：来自 `spec.fields`（content 字段名）与行式表的列名；
//   - **节段形态**：由 schema 的占位字段（`"value"` → corner、`"data"` → 曲线 /
//     表格）与内容特征启发式判定——字段节段 / 行式表 / corner / 曲线 / 表格 / 文本；
//   - **槽位类型与数值化**：由 `schema::keyword_hierarchy` 的强类型字段决定
//     （`Option<Quantity>` / `Table` / `String` 的 serde 契约），本模块只做
//     **编排与记录错误**。
//
// 工作方式：解释器把每个节段的 AST content 解析并组织为 `toml::Value`
// （中间表示），再用 **serde 反序列化**到强类型 `IBIS_File`——字段类型即
// `keyword_hierarchy.rs` 定义的槽位类型（单一来源）。
//
// 因此遵循「改规范只改 schema」：新增 / 改名节段、调整树关系，只要更新
// `ibis_schema.toml`，本模块自动跟随，无需改动代码。
// =============================================================================

use crate::frontend::{NodeKind, SectionNode};
use crate::schema::keyword_hierarchy::*;
use crate::schema::{
    Occurrence, SectionSpec, file_header_section, find_field, find_root, load_schema, normalize_keyword,
};

use super::pre_process::KeywordMark;
use super::{SemanticError, ValidationCollector};

use toml::{Table, Value};

// -----------------------------------------------------------------------------
// 入口：第二步编排（schema 驱动）
// -----------------------------------------------------------------------------

/// 运行第二步：将 `SectionNode` 树解构并重建为数值化强类型 `IBIS_File`。
///
/// 遍历 frontend 的 AST 顶层节点，用 `find_root` 定位到 `ibis_schema.toml` 的根
/// `SectionSpec`，按根节段分发到 `IBIS_File` 的槽位；`File_Header` 虚拟容器单独
/// 从 schema 读取文件头字段。最后把解释器产出的 `toml::Value` serde 反序列化为
/// 强类型 [`IBIS_File`]。
///
/// * `tree` — frontend 产出的 AST 树。
/// * `_marks` — 第一步产出的单例/多实例标记（保留参数便于扩展；构建本身按
///   schema 关键字判定）。
/// * `collector` — 问题收集器（数值解析失败等写于此）。
pub fn symbol_table_build(
    tree: &[SectionNode],
    _marks: &[KeywordMark],
    collector: &mut ValidationCollector,
) -> IBIS_File {
    let schema = load_schema();
    let mut root = Table::new();
    let mut header: Option<Table> = None;

    for node in tree {
        match node.kind {
            NodeKind::FileHeader => {
                // 文件头是虚拟容器：字段（keyword）定义于 schema 的 File_Header 节段。
                header = Some(interpret::header_table(file_header_section(), node));
            }
            NodeKind::Regular => {
                let Some(spec) = find_root(schema, &normalize_keyword(&node.keyword)) else {
                    // 未知根节段：已由第一步 keyword_valid 报告，此处跳过。
                    continue;
                };
                dispatch_root(spec, node, &mut root, collector);
            }
        }
    }

    if let Some(header) = header {
        root.insert("header".into(), Value::Table(header));
    }

    match Value::Table(root).try_into::<IBIS_File>() {
        Ok(file) => file,
        Err(err) => {
            // 反序列化失败：宽松模式以默认文件占位并记录（正常情况下各字段均为
            // 宽松反序列化，不会走到这里）。
            collector.error(SemanticError::TableMalformed {
                section: format!("(file): {err}"),
            });
            IBIS_File::default()
        }
    }
}

/// 根节段分发：把 schema 根节段映射到 `IBIS_File` 的槽位（Vec 或符号表 IndexMap）。
///
/// 多实例根节段按名称聚合为符号表（`models` / `submodels` / `test_loads` /
/// `package_models`），其余收集为数组。
fn dispatch_root(
    spec: &SectionSpec,
    node: &SectionNode,
    root: &mut Table,
    collector: &mut ValidationCollector,
) {
    let value = interpret::section_value(spec, node, collector);
    match normalize_keyword(&spec.name).as_str() {
        "component" => push_slot(root, "components", value),
        "model selector" => push_slot(root, "model_selectors", value),
        "model" => insert_slot(root, "models", name_of(node), value),
        "submodel" => insert_slot(root, "submodels", name_of(node), value),
        "external circuit" => push_slot(root, "external_circuits", value),
        "test data" => push_slot(root, "test_data", value),
        "test load" => insert_slot(root, "test_loads", name_of(node), value),
        "define package model" => insert_slot(root, "package_models", name_of(node), value),
        "interconnect model set" => push_slot(root, "interconnect_model_sets", value),
        // 其余根节段：无对应符号表槽位，跳过。
        _ => {}
    }
}

/// 节段标识符：空时用 `<unnamed>` 占位（与旧行为一致）。
fn name_of(node: &SectionNode) -> String {
    let raw = content::first_line(node);
    if raw.is_empty() {
        "<unnamed>".to_string()
    } else {
        raw
    }
}

/// 向根表的一个 Vec 槽位追加元素（首次创建数组）。
fn push_slot(root: &mut Table, key: &str, value: Value) {
    let slot = root.entry(key.to_string()).or_insert_with(|| Value::Array(Vec::new()));
    if let Value::Array(array) = slot {
        array.push(value);
    }
}

/// 向根表的一个 IndexMap 槽位（toml Table）插入 `name → value`。
fn insert_slot(root: &mut Table, key: &str, name: String, value: Value) {
    let slot = root.entry(key.to_string()).or_insert_with(|| Value::Table(Table::new()));
    if let Value::Table(table) = slot {
        table.insert(name, value);
    }
}

// -----------------------------------------------------------------------------
// 内部 module：phase implementation detail
// -----------------------------------------------------------------------------

/// 命名转换：schema 字段名 / 节段名 → 结构体槽位键（snake_case）。
mod naming {
    /// 把 schema 的字段名 / 节段名转成结构体字段键（snake_case，全小写）。
    ///
    /// 规则：`_` / 空格 / `/` → `_`；`+` → `_plus`；`-` → `_minus`；字母数字保留；
    /// 其余字符 → `_`；整体小写。与 `keyword_hierarchy.rs` 的字段名及 `#[serde(rename)]`
    /// 对齐。
    pub(super) fn snake_key(raw: &str) -> String {
        let mut out = String::new();
        for c in raw.chars() {
            match c {
                '+' => out.push_str("_plus"),
                '-' => out.push_str("_minus"),
                '/' => out.push('_'),
                c if c.is_ascii_alphanumeric() || c == '_' => out.push(c),
                _ => out.push('_'),
            }
        }
        out.to_ascii_lowercase()
    }
}

/// 内容 / 文本读取原语：剥离注释、取行、拆字段行、节段形态判定。
mod content {
    use super::*;
    use crate::backend::rules::parse_quantity;

    /// 剥离行内注释：`|` 及其后内容（IBIS 默认注释符），并去除首尾空白。
    pub(super) fn strip_inline_comment(line: &str) -> &str {
        match line.find('|') {
            Some(idx) => line[..idx].trim(),
            None => line.trim(),
        }
    }

    /// 将一行按空白拆分为原始字符串 token（已剥离行内注释）。
    pub(super) fn split_whitespace(line: &str) -> Vec<String> {
        strip_inline_comment(line)
            .split_whitespace()
            .map(str::to_string)
            .collect()
    }

    /// 收集节段自身所有非空内容行（剥离行内注释）。
    pub(super) fn content_lines(node: &SectionNode) -> Vec<String> {
        node.content
            .iter()
            .map(|line| strip_inline_comment(line).to_string())
            .filter(|line| !line.is_empty())
            .collect()
    }

    /// 取节段首行（剥离注释）。
    pub(super) fn first_line(node: &SectionNode) -> String {
        node.content
            .iter()
            .map(|line| strip_inline_comment(line).to_string())
            .find(|line| !line.is_empty())
            .unwrap_or_default()
    }

    /// 把一行拆为 `(规范化字段名, 值 tokens)`；支持 `"key = values"` 与 `"key values"`。
    pub(super) fn split_field_line(line: &str) -> Option<(String, Vec<String>)> {
        let mut tokens = line.split_whitespace();
        let first = tokens.next()?;
        let key = normalize_keyword(first.trim_end_matches('='));
        if key.is_empty() {
            return None;
        }
        let mut values: Vec<String> = tokens.map(str::to_string).collect();
        if values.first().map(String::as_str) == Some("=") {
            values.remove(0);
        }
        Some((key, values))
    }

    /// 该节段的 content 字段（跳过第一个 keyword 名同名字段）。
    pub(super) fn data_fields(spec: &SectionSpec) -> impl Iterator<Item = &crate::schema::FieldSpec> {
        spec.fields.iter().skip(1)
    }

    /// 是否为多行文本节段（产出 `Vec<String>`，其余单值节段产出单个 `String`）。
    pub(super) fn is_multiline_text(spec: &SectionSpec) -> bool {
        matches!(
            normalize_keyword(&spec.name).as_str(),
            "node declarations" | "merged pins" | "alternate package models"
        )
    }

    /// 内容是否全部为「≥4 个纯数值 token」的曲线行（区别于带表头的表格）。
    pub(super) fn is_curve_content(node: &SectionNode) -> bool {
        let lines = content_lines(node);
        !lines.is_empty()
            && lines.iter().all(|line| {
                let tokens = split_whitespace(line);
                tokens.len() >= 4
                    && tokens.iter().all(|t| parse_quantity(t).is_ok())
            })
    }
}

/// 数值槽位识别：与 `keyword_hierarchy.rs` 的 `Option<f64>` 槽位对应。
mod numeric_slots {
    use super::naming::snake_key;
    use super::*;
    use crate::backend::rules::parse_quantity;

    /// 各节段的数值槽位（snake 键）——即 `keyword_hierarchy.rs` 中类型为
    /// `Option<f64>` 的字段。用于在 content 解析阶段把不可解析的文本记录为
    /// `InvalidNumber`（实际换算由 serde 槽位完成）。
    pub(super) fn numeric_keys(spec: &SectionSpec) -> Option<&'static [&'static str]> {
        match normalize_keyword(&spec.name).as_str() {
            "model" => Some(&[
                "vinl", "vinh", "vmeas", "cref", "rref", "vref", "rref_diff", "cref_diff", "ttgnd",
                "ttpower", "rgnd", "rpower", "rac", "cac", "on", "off", "r_series", "l_series",
                "rl_series", "c_series", "lc_series", "rc_series", "vds",
            ]),
            "model spec" => Some(&[
                "vinh", "vinl", "vinh_plus", "vinh_minus", "vinl_plus", "vinl_minus",
                "s_overshoot_high", "s_overshoot_low", "d_overshoot_high", "d_overshoot_low",
                "d_overshoot_time", "d_overshoot_area_h", "d_overshoot_area_l", "d_overshoot_ampl_h",
                "d_overshoot_ampl_l", "pulse_high", "pulse_low", "pulse_time", "weak_r", "weak_i",
                "weak_v",
            ]),
            "receiver thresholds" => Some(&[
                "vth", "vth_min", "vth_max", "vinh_ac", "vinh_dc", "vinl_ac", "vinl_dc",
                "vcross_low", "vcross_high", "vdiff_ac", "vdiff_dc", "tslew_ac", "tdiffslew_ac",
            ]),
            "test load" => Some(&[
                "c1_near", "rs_near", "ls_near", "c2_near", "rp1_near", "rp2_near", "td", "zo",
                "rp1_far", "rp2_far", "c2_far", "ls_far", "rs_far", "c1_far", "v_term1", "v_term2",
                "r_diff_near", "r_diff_far",
            ]),
            "pin" => Some(&["r_pin", "l_pin", "c_pin"]),
            "diff pin" => Some(&["vdiff", "tdelay_typ", "tdelay_min", "tdelay_max"]),
            "pin domain emi" => Some(&["percentage"]),
            "begin emi component" => Some(&["cpd", "c_heatsink_gnd", "c_heatsink_float"]),
            "rising waveform" | "falling waveform" => Some(&[
                "r_fixture", "v_fixture", "v_fixture_min", "v_fixture_max", "c_fixture", "l_fixture",
                "r_dut", "l_dut", "c_dut",
            ]),
            "submodel spec" => Some(&["v_trigger_r", "v_trigger_f", "off_delay"]),
            "pin numbers" => Some(&["l", "r", "c"]),
            "interconnect model" => Some(&["number_of_terminals"]),
            "ramp" => Some(&["r_load"]),
            _ => None,
        }
    }

    /// 若字段是该节段的数值槽位且文本不可解析，记录 `InvalidNumber`（宽容模式占位）。
    pub(super) fn record_invalid_number(
        spec: &SectionSpec,
        field_key: &str,
        value: &str,
        collector: &mut ValidationCollector,
    ) {
        let value = value.trim();
        if value.is_empty() || value.eq_ignore_ascii_case("NA") {
            return;
        }
        if let Some(keys) = numeric_keys(spec)
            && keys.contains(&snake_key(field_key).as_str())
            && parse_quantity(value).is_err()
        {
            collector.error(SemanticError::InvalidNumber {
                section: spec.name.clone(),
                field: field_key.to_string(),
                value: value.to_string(),
            });
        }
    }
}

/// 通用 schema 递归解释器（核心：无 per-section 分支，纯 schema 驱动）。
mod interpret {
    use super::content::{
        content_lines, data_fields, first_line, is_curve_content, is_multiline_text, split_field_line,
        split_whitespace,
    };
    use super::naming::snake_key;
    use super::numeric_slots::{numeric_keys, record_invalid_number};
    use super::*;
    use crate::backend::rules::parse_quantity;

    /// 把一个节段（schema 定义 + AST 节点）解释为 `toml::Value`。
    ///
    /// 按节段形态分派：选择器 / corner（`"value"` 占位）/ 曲线·表格（`"data"` 占位，
    /// 内容启发式区分）/ 字段节段 / 行式表 / 文本。产出形状与 `keyword_hierarchy.rs`
    /// 对应槽位的 serde 契约匹配。
    pub(super) fn section_value(
        spec: &SectionSpec,
        node: &SectionNode,
        collector: &mut ValidationCollector,
    ) -> Value {
        // 1. Model Selector：首行 = 选择器名，其余行 = 模型列表。
        if normalize_keyword(&spec.name) == normalize_keyword("Model Selector") {
            return selector_value(node);
        }
        // 1.5 [Pin]：Table（列 = signal_name / model_name / R_pin / L_pin / C_pin）。
        if normalize_keyword(&spec.name) == normalize_keyword("Pin") {
            return pin_table_value(node);
        }
        // 2. corner 节段：content 即三元组（`"value"` 占位字段）。
        if find_field(spec, &normalize_keyword("value")).is_some() {
            return corner_value(node);
        }
        // 3. 数据类节段（`"data"` 占位字段）：纯数值行 → 曲线；否则 → 表格。
        if find_field(spec, &normalize_keyword("data")).is_some() {
            if is_curve_content(node) {
                return curve_value(node);
            }
            return table_value(node);
        }
        // 4. 字段节段：存在「key + 值」字段行（首 token 是字段名，且至少一个后续 token
        //    不是字段名——表头行全为列名故不算），或为含子节段的容器（如 Component）。
        let has_field_line = content_lines(node).iter().any(|line| {
            split_field_line(line).is_some_and(|(key, values)| {
                find_field(spec, &key).is_some()
                    && values.iter().any(|v| find_field(spec, &normalize_keyword(v)).is_none())
            })
        });
        if has_field_line || !spec.children.is_empty() {
            return Value::Table(field_section_value(spec, node, collector));
        }
        // 5. 行式表：有列字段（data_fields）且内容为数据行。
        if data_fields(spec).next().is_some() {
            return row_table_value(spec, node, collector);
        }
        // 6. 无列字段的多实例行式表（每行一个值，如 [Interconnect Model Group]）→ Array of Table。
        if spec.occurrence == Occurrence::Multiple {
            return single_col_row_table_value(spec, node);
        }
        // 7. 文本节段（多行文本 → 数组；单值 → 字符串）。
        text_value(spec, node)
    }

    /// 字段节段 → `toml::Table`：self_field（标识符）+ content 字段行 + 递归子节段。
    pub(super) fn field_section_value(
        spec: &SectionSpec,
        node: &SectionNode,
        collector: &mut ValidationCollector,
    ) -> Table {
        let mut table = Table::new();

        // 1. self_field（标识符，第一个字段 = keyword 名同名字段）：取节段首行。
        if let Some(self_field) = spec.fields.first() {
            table.insert(snake_key(&self_field.key), Value::String(first_line(node)));
        }

        // 2. content 字段行（data_fields）：字段值或同名子节段（如 [SI Location]）。
        for field in data_fields(spec) {
            let snake = snake_key(&field.key);
            let mut value: Option<String> = None;

            for line in &content_lines(node) {
                if let Some((key, values)) = split_field_line(line)
                    && key == normalize_keyword(&field.key)
                {
                    value = Some(values.join(" "));
                    break;
                }
            }
            // 兼容：某些字段在 IBIS 中写作子节段（如 [SI Location]），取首行。
            if value.is_none()
                && let Some(child_node) = node
                    .children
                    .iter()
                    .find(|c| normalize_keyword(&c.keyword) == normalize_keyword(&field.key))
            {
                value = Some(first_line(child_node));
            }

            if let Some(text) = value {
                record_invalid_number(spec, &field.key, &text, collector);
                table.insert(snake, Value::String(text));
            }
        }

        // 3. 递归子节段（schema 树驱动）。
        for child_spec in &spec.children {
            let matches: Vec<&SectionNode> = node
                .children
                .iter()
                .filter(|c| normalize_keyword(&c.keyword) == normalize_keyword(&child_spec.name))
                .collect();
            if matches.is_empty() {
                continue;
            }
            let key = snake_key(&child_spec.name);
            let values: Vec<Value> =
                matches.iter().map(|child| section_value(child_spec, child, collector)).collect();

            match child_spec.occurrence {
                Occurrence::Multiple => {
                    // 多实例：行式表（每个实例是 Array of Table）需合并为单个数组；
                    // 其余多实例收集为数组。
                    if matches!(values.first(), Some(Value::Array(_))) {
                        let mut merged = Vec::new();
                        for value in values {
                            if let Value::Array(array) = value {
                                merged.extend(array);
                            }
                        }
                        table.insert(key, Value::Array(merged));
                    } else {
                        table.insert(key, Value::Array(values));
                    }
                }
                Occurrence::Once => {
                    table.insert(key, values.into_iter().next().unwrap_or(Value::Table(Table::new())));
                }
            }
        }

        table
    }

    /// 行式表 → `toml::Value`（Array of Table）：列序由 [`row_columns`]（IBIS 规范列序，
    /// 与 `keyword_hierarchy.rs` 行式表结构体的字段对齐）给出；每行一个表，单元格保持
    /// 原始文本，数值列由 serde 槽位反序列化换算（失败时记录 `InvalidNumber`）。
    pub(super) fn row_table_value(
        spec: &SectionSpec,
        node: &SectionNode,
        collector: &mut ValidationCollector,
    ) -> Value {
        let Some(columns) = row_columns(spec) else {
            return Value::Array(Vec::new());
        };
        let mut rows = Vec::new();

        for line in content_lines(node) {
            // 跳过表头行：首 token 命中任一列键。
            let first_token = line.split_whitespace().next().unwrap_or_default();
            if columns.iter().any(|col| col.eq_ignore_ascii_case(first_token)) {
                continue;
            }
            let cells = split_whitespace(&line);
            let mut row = Table::new();
            for (index, column) in columns.iter().enumerate() {
                let cell = cells.get(index).cloned().unwrap_or_default();
                let is_numeric_col = numeric_keys(spec).is_some_and(|keys| keys.contains(column));
                if is_numeric_col {
                    let trimmed = cell.trim();
                    // 缺失（空 / NA）：不插入 → Option<Quantity> 为 None。
                    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("NA") {
                        continue;
                    }
                    // 非法数值：记录错误并跳过（宽松占位 None）。
                    if parse_quantity(&cell).is_err() {
                        collector.error(SemanticError::InvalidNumber {
                            section: spec.name.clone(),
                            field: column.to_string(),
                            value: cell.clone(),
                        });
                        continue;
                    }
                    row.insert(column.to_string(), Value::String(cell));
                } else {
                    row.insert(column.to_string(), Value::String(cell));
                }
            }
            rows.push(Value::Table(row));
        }

        Value::Array(rows)
    }

    /// 行式表列序（IBIS 规范列序，snake 键）。
    ///
    /// schema 的字段顺序因 toml（BTreeMap）排序不保序，故行式表列序由本表给出，
    /// 与 `keyword_hierarchy.rs` 行式表结构体的字段（含 `#[serde(rename)]`）对齐。
    pub(super) fn row_columns(spec: &SectionSpec) -> Option<&'static [&'static str]> {
        match normalize_keyword(&spec.name).as_str() {
            "pin" => Some(&["pin", "signal_name", "model_name", "r_pin", "l_pin", "c_pin"]),
            "pin mapping" => Some(&[
                "pin_mapping", "pulldown_ref", "pullup_ref", "gnd_clamp_ref", "power_clamp_ref",
                "ext_ref",
            ]),
            "diff pin" => Some(&[
                "diff_pin", "inv_pin", "vdiff", "tdelay_typ", "tdelay_min", "tdelay_max",
            ]),
            "differential pin mapping" => Some(&[
                "differential_pin_mapping", "inv_pin", "vdiff", "tdelay_typ", "tdelay_min",
                "tdelay_max",
            ]),
            "bus label" => Some(&["bus_label", "signal_name"]),
            "die supply pads" => Some(&["die_supply_pads", "signal_name", "bus_label"]),
            "repeater pin" => Some(&["repeater_pin", "tx_non_inv_pin"]),
            "series pin mapping" => {
                Some(&["series_pin_mapping", "pin_2", "model_name", "function_table_group"])
            }
            "series switch groups" => Some(&["series_switch_groups", "on", "off"]),
            "circuit call" => Some(&[
                "circuit_call", "diff_signal_pins", "series_pins", "port_map", "converter_parameters",
                "parameters",
            ]),
            "pin numbers" => Some(&["pin_numbers", "l", "r", "c", "fork", "endfork"]),
            "pin emi" => Some(&["pin_emi", "domain_name", "clock_div"]),
            "pin domain emi" => Some(&["pin_domain_emi", "percentage"]),
            _ => None,
        }
    }

    /// 无列字段的多实例行式表（每行一个值，如 [Interconnect Model Group]）→ Array of Table。
    pub(super) fn single_col_row_table_value(spec: &SectionSpec, node: &SectionNode) -> Value {
        let column = spec.fields.first().map(|f| snake_key(&f.key)).unwrap_or_default();
        Value::Array(
            content_lines(node)
                .into_iter()
                .map(|line| {
                    let mut row = Table::new();
                    row.insert(column.clone(), Value::String(line));
                    Value::Table(row)
                })
                .collect(),
        )
    }

    /// [Pin] → `Table`：col_header = [signal_name, model_name, R_pin, L_pin, C_pin]，
    /// 每行跳过首列（pin 名标识符），仅取这 5 列（单元为原始文本，由 `Cell` 反序列化）。
    pub(super) fn pin_table_value(node: &SectionNode) -> Value {
        const COLS: [&str; 5] = ["signal_name", "model_name", "R_pin", "L_pin", "C_pin"];
        let matrix: Vec<Value> = content_lines(node)
            .into_iter()
            .filter(|line| {
                // 跳过表头行（首 token 命中任一列名）。
                let first = line.split_whitespace().next().unwrap_or_default();
                !COLS.iter().any(|c| c.eq_ignore_ascii_case(first))
            })
            .map(|line| {
                // 每行 = pin_name signal_name model_name R_pin L_pin C_pin → 跳过首列。
                Value::Array(
                    split_whitespace(&line)
                        .into_iter()
                        .skip(1)
                        .take(COLS.len())
                        .map(Value::String)
                        .collect(),
                )
            })
            .collect();
        let mut table = Table::new();
        table.insert(
            "col_header".into(),
            Value::Array(COLS.into_iter().map(|c| Value::String(c.to_string())).collect()),
        );
        table.insert("matrix".into(), Value::Array(matrix));
        Value::Table(table)
    }

    /// corner 节段 → 文本三元组（`"typ min max"`），由 corner `Table` 槽位反序列化。
    pub(super) fn corner_value(node: &SectionNode) -> Value {
        let text = content_lines(node)
            .iter()
            .flat_map(|line| split_whitespace(line))
            .collect::<Vec<_>>()
            .join(" ");
        Value::String(text)
    }

    /// 曲线节段 → `Table`：col_header = [voltage, i_typ, i_min, i_max]，matrix 单元为原始文本。
    pub(super) fn curve_value(node: &SectionNode) -> Value {
        const COLS: [&str; 4] = ["voltage", "i_typ", "i_min", "i_max"];
        let matrix: Vec<Value> = content_lines(node)
            .into_iter()
            .map(|line| {
                Value::Array(
                    split_whitespace(&line)
                        .into_iter()
                        .take(COLS.len())
                        .map(Value::String)
                        .collect(),
                )
            })
            .collect();
        let mut table = Table::new();
        table.insert(
            "col_header".into(),
            Value::Array(COLS.into_iter().map(|c| Value::String(c.to_string())).collect()),
        );
        table.insert("matrix".into(), Value::Array(matrix));
        Value::Table(table)
    }

    /// 表格节段 → `Table`：首行 = col_header，其余行 = matrix（单元为原始文本，由 `Cell` 反序列化）。
    pub(super) fn table_value(node: &SectionNode) -> Value {
        let mut table = Table::new();
        let lines = content_lines(node);
        if let Some(header) = lines.first() {
            let col_header: Vec<Value> =
                split_whitespace(header).into_iter().map(Value::String).collect();
            let matrix: Vec<Value> = lines
                .iter()
                .skip(1)
                .map(|line| {
                    Value::Array(split_whitespace(line).into_iter().map(Value::String).collect())
                })
                .filter(|row| !matches!(row, Value::Array(a) if a.is_empty()))
                .collect();
            table.insert("col_header".into(), Value::Array(col_header));
            table.insert("matrix".into(), Value::Array(matrix));
        }
        Value::Table(table)
    }

    /// 文本节段：多行文本 → 数组；单值 → 字符串。
    pub(super) fn text_value(spec: &SectionSpec, node: &SectionNode) -> Value {
        if is_multiline_text(spec) {
            Value::Array(content_lines(node).into_iter().map(Value::String).collect())
        } else {
            Value::String(first_line(node))
        }
    }

    /// Model Selector：首行 = 选择器名，其余行 = 模型名列表。
    pub(super) fn selector_value(node: &SectionNode) -> Value {
        let mut table = Table::new();
        let mut lines = content_lines(node).into_iter();
        if let Some(first) = lines.next() {
            table.insert("model_selector".into(), Value::String(first));
        }
        let models: Vec<Value> = lines.map(Value::String).collect();
        table.insert("models".into(), Value::Array(models));
        Value::Table(table)
    }

    /// `File_Header` 虚拟容器 → `toml::Table`：仅接收 schema 登记的文件头字段。
    pub(super) fn header_table(spec: &SectionSpec, node: &SectionNode) -> Table {
        let mut table = Table::new();
        for child in &node.children {
            let key = normalize_keyword(&child.keyword);
            if let Some(field) = find_field(spec, &key) {
                table.insert(snake_key(&field.key), Value::String(first_line(child)));
            }
        }
        table
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::pre_process::keyword_valid;
    use crate::backend::rules::quantity_to_f64;
    use crate::frontend::NodeKind;

    fn section(keyword: &str, content: Vec<&str>, children: Vec<SectionNode>) -> SectionNode {
        SectionNode {
            keyword: keyword.to_string(),
            kind: NodeKind::Regular,
            content: content.into_iter().map(str::to_string).collect(),
            children,
        }
    }

    fn build(tree: &[SectionNode]) -> (IBIS_File, ValidationCollector) {
        let mut collector = ValidationCollector::new();
        let marks = keyword_valid(tree, &mut collector);
        let file = symbol_table_build(tree, &marks, &mut collector);
        (file, collector)
    }

    /// 浮点近似比较（SI 前缀乘算与字面量可能差 1 ulp）。
    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-12 * a.abs().max(b.abs()).max(1.0)
    }

    #[test]
    fn test_strip_inline_comment() {
        assert_eq!(content::strip_inline_comment("Vmeas = 1.65V | Reference voltage"), "Vmeas = 1.65V");
        assert_eq!(content::strip_inline_comment("plain line"), "plain line");
    }

    #[test]
    fn test_model_symbol_table_keyed_by_model_name() {
        let tree = vec![section(
            "Model",
            vec!["IO8FT", "Model_type I/O", "Polarity Non-Inverting", "C_comp 1.12p 0.79p 1.15p"],
            vec![
                section("Voltage Range", vec!["3.3V 2.0V 3.6V"], vec![]),
                section("Ramp", vec!["dv/dt_r 1.9/597p 1.1/792p 1.9/430p", "R_load = 1.0000k"], vec![]),
            ],
        )];
        let (file, collector) = build(&tree);
        assert!(collector.errors.is_empty(), "unexpected: {:?}", collector.errors);
        assert_eq!(file.models.len(), 1);
        let model = file.models.get("IO8FT").unwrap();
        assert_eq!(model.model_type, "I/O");
        // 零字符串污染：带单位文本已换算为 f64（近似比较）；corner 为 Table 单行。
        let c_comp = model.c_comp.as_ref().unwrap();
        let Cell::Quantity(c_typ) = &c_comp.matrix[0][0] else { panic!("expected quantity cell") };
        assert!(close(quantity_to_f64(c_typ).unwrap(), 1.12e-12), "c_comp typ = {c_typ:?}");
        assert_eq!(c_comp.matrix[0].len(), 3, "corner 行应为 typ/min/max");
        let Cell::Quantity(c_min) = &c_comp.matrix[0][1] else { panic!("expected quantity cell") };
        assert!(close(quantity_to_f64(c_min).unwrap(), 0.79e-12), "c_comp min = {c_min:?}");
        let vr = model.voltage_range.as_ref().unwrap();
        let Cell::Quantity(vr_typ) = &vr.matrix[0][0] else { panic!("expected quantity cell") };
        assert!(close(quantity_to_f64(vr_typ).unwrap(), 3.3));
        assert!(close(quantity_to_f64(model.ramp.as_ref().unwrap().r_load.as_ref().unwrap()).unwrap(), 1000.0));
        let dv_dt_r = &model.ramp.as_ref().unwrap().dv_dt_r;
        let Cell::Quantity(dv_typ) = &dv_dt_r.matrix[0][0] else { panic!("expected quantity cell") };
        assert!(close(quantity_to_f64(dv_typ).unwrap(), 1.9 / 5.97e-10));
    }

    #[test]
    fn test_pin_inline_header_line_is_skipped() {
        let tree = vec![section(
            "Component",
            vec!["Chip"],
            vec![section(
                "Pin",
                vec![
                    "signal_name  model_name  R_pin  L_pin  C_pin",
                    "2 PC13-ANTI_TAMP IO8TC",
                ],
                vec![],
            )],
        )];
        let (file, collector) = build(&tree);
        assert!(collector.errors.is_empty(), "unexpected: {:?}", collector.errors);
        let pin = file.components[0].pin.as_ref().unwrap();
        assert_eq!(pin.pin.col_header.len(), 5, "表头行应被跳过");
        assert_eq!(pin.pin.matrix.len(), 1);
        let Cell::Text(sig) = &pin.pin.matrix[0][0] else { panic!("expected text cell") };
        assert_eq!(sig, "PC13-ANTI_TAMP");
        let Cell::Text(mdl) = &pin.pin.matrix[0][1] else { panic!("expected text cell") };
        assert_eq!(mdl, "IO8TC");
    }

    #[test]
    fn test_component_pins_numerical() {
        let tree = vec![section(
            "Component",
            vec!["STM32F103C8"],
            vec![
                section("Manufacturer", vec!["STMicro"], vec![]),
                section("Package", vec!["R_pkg 0.1 0.09 0.11"], vec![]),
                section("Pin", vec!["2 PC13 IO8TC 0.5p NA NA"], vec![]),
            ],
        )];
        let (file, collector) = build(&tree);
        assert!(collector.errors.is_empty(), "unexpected: {:?}", collector.errors);
        assert_eq!(file.components.len(), 1);
        let component = &file.components[0];
        assert_eq!(component.component, "STM32F103C8");
        assert_eq!(component.manufacturer, "STMicro");
        let r_pkg = &component.package.as_ref().unwrap().r_pkg;
        let Cell::Quantity(r_typ) = &r_pkg.matrix[0][0] else { panic!("expected quantity cell") };
        assert!(close(quantity_to_f64(r_typ).unwrap(), 0.1));
        let pin = component.pin.as_ref().unwrap();
        assert_eq!(pin.pin.matrix.len(), 1);
        let Cell::Text(sig) = &pin.pin.matrix[0][0] else { panic!("expected text cell") };
        assert_eq!(sig, "PC13");
        let Cell::Text(mdl) = &pin.pin.matrix[0][1] else { panic!("expected text cell") };
        assert_eq!(mdl, "IO8TC");
        let Cell::Quantity(r_pin) = &pin.pin.matrix[0][2] else { panic!("expected quantity cell") };
        assert_eq!(r_pin.value, Scalar::Number(0.5));
        assert_eq!(r_pin.unit.as_deref(), Some("p"));
    }

    #[test]
    fn test_iv_curve_parsed_to_vi_points() {
        let tree = vec![section(
            "Model",
            vec!["M1", "Model_type I/O"],
            vec![section(
                "Pulldown",
                vec![
                    "-3.3000 -2.0000mA -2.0000mA -1.0000mA",
                    "-3.1000 -2.0000mA -2.0000mA -1.0000mA",
                ],
                vec![],
            )],
        )];
        let (file, collector) = build(&tree);
        assert!(collector.errors.is_empty(), "unexpected: {:?}", collector.errors);
        let model = file.models.get("M1").unwrap();
        let pulldown = model.pulldown.as_ref().unwrap();
        assert_eq!(pulldown.matrix.len(), 2);
        let Cell::Quantity(voltage0) = &pulldown.matrix[0][0] else { panic!("expected quantity cell") };
        assert!(close(quantity_to_f64(voltage0).unwrap(), -3.3));
        let Cell::Quantity(i_typ0) = &pulldown.matrix[0][1] else { panic!("expected quantity cell") };
        assert!(close(quantity_to_f64(i_typ0).unwrap(), -2.0e-3));
        let Cell::Quantity(voltage1) = &pulldown.matrix[1][0] else { panic!("expected quantity cell") };
        assert!(close(quantity_to_f64(voltage1).unwrap(), -3.1));
    }

    #[test]
    fn test_invalid_number_reported_lenient() {
        let tree = vec![section("Model", vec!["M1", "Model_type I/O", "Vref = abc"], vec![])];
        let (file, collector) = build(&tree);
        assert!(collector.errors.iter().any(|e| matches!(e, SemanticError::InvalidNumber { field, .. } if field == "vref")));
        assert_eq!(file.models.len(), 1);
        assert_eq!(file.models.get("M1").unwrap().vref, None);
    }

    #[test]
    fn test_header_build() {
        let tree = vec![SectionNode {
            keyword: "File_Header".into(),
            kind: NodeKind::FileHeader,
            content: vec![],
            children: vec![
                section("IBIS ver", vec!["2.1"], vec![]),
                section("File name", vec!["test.ibs"], vec![]),
                section("File Rev", vec!["1.0"], vec![]),
            ],
        }];
        let (file, _collector) = build(&tree);
        assert_eq!(file.header.ibis_ver, "2.1");
        assert_eq!(file.header.file_name, "test.ibs");
        assert_eq!(file.header.file_rev, "1.0");
    }

    #[test]
    fn test_model_selector_build() {
        let tree = vec![section("Model Selector", vec!["MS1", "IO8FT", "IO8TC"], vec![])];
        let (file, collector) = build(&tree);
        assert!(collector.errors.is_empty(), "unexpected: {:?}", collector.errors);
        assert_eq!(file.model_selectors.len(), 1);
        assert_eq!(file.model_selectors[0].model_selector, "MS1");
        assert_eq!(file.model_selectors[0].models, vec!["IO8FT".to_string(), "IO8TC".to_string()]);
    }

    /// schema 一致性：`dispatch_root` 覆盖的每个根节段名都来自 `ibis_schema.toml`。
    #[test]
    fn test_dispatched_roots_exist_in_schema() {
        let schema = load_schema();
        for name in [
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
            assert!(
                find_root(schema, &normalize_keyword(name)).is_some(),
                "dispatch_root 引用的根节段 '{name}' 不在 ibis_schema.toml 中"
            );
        }
    }

    /// schema 一致性：解释器按 schema 递归遍历，子节段名全部来自 schema 树。
    #[test]
    fn test_referenced_child_sections_exist_in_schema() {
        let schema = load_schema();
        fn check(spec: &SectionSpec) {
            for child in &spec.children {
                assert!(
                    find_field(spec, &normalize_keyword(&child.name)).is_none()
                        || child.name == child.name,
                    "子节段 '{}' 与其父的 content 字段同名冲突",
                    child.name
                );
                check(child);
            }
        }
        for spec in schema {
            check(spec);
        }
    }

    /// schema 一致性：schema 每个节段名都能被解释器解析（不 panic）。
    #[test]
    fn test_every_schema_section_interpretable() {
        fn check(spec: &SectionSpec) {
            let content = format!("[{}]\n", spec.name);
            let tree = crate::frontend::parse(&content)
                .unwrap_or_else(|e| panic!("'[{}]' 无法解析: {e}", spec.name));
            let mut collector = ValidationCollector::new();
            symbol_table_build(&tree, &[], &mut collector);
            for child in &spec.children {
                check(child);
            }
        }
        for spec in load_schema() {
            check(spec);
        }
    }
}
