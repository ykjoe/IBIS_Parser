# IBIS Parser 技术方案 v5（多级 AST 树架构）

## 1. 当前问题

### 1.1 命名不规范
| 当前命名 | 应为 | 理由 |
|---------|------|------|
| `kw_ibis_ver` | `kw_file_header_ibis_ver` | 遵循 `kw_1级_2级` 规范 |
| `kw_file_name` | `kw_file_header_file_name` | 同上 |
| `file_header_keyword` 分组 | 合并到 `second_level_keyword` | 文件头字段本质上是二级关键词 |

### 1.2 Rust 端缺乏真正的树结构
当前 [`build_toml_from_blocks`](src/ibis2ibstoml/frontend.rs:336) 是扁平的 if-else 逻辑：
- 手动判断 `first_level_keyword` → 收集 children
- 无法扩展支持更深层级的嵌套
- 树构建和 TOML 序列化耦合在一起

### 1.3 Pest 语法无法解析真实 IBIS 文件
`(...)*` 重复不会在迭代间跳过空白/换行，导致 `[IBIS ver] 2.1\n` 解析失败，始终走 fallback 路径。

---

## 2. v5 架构变更

### 2.1 完整命名规范

所有关键词规则遵循 `kw_1级_2级_3级`：

```pest
// -------- file header keyword --------
kw_file_header_ibis_ver       = { "[" ~ "IBIS ver" ~ "]" }
kw_file_header_comment_char   = { "[" ~ "Comment Char" ~ "]" }
kw_file_header_file_name      = { "[" ~ "File name" ~ "]" }
kw_file_header_file_rev       = { "[" ~ "File Rev" ~ "]" }
kw_file_header_date           = { "[" ~ "Date" ~ "]" }
kw_file_header_source         = { "[" ~ "Source" ~ "]" }
kw_file_header_notes          = { "[" ~ "Notes" ~ "]" }
kw_file_header_disclaimer     = { "[" ~ "Disclaimer" ~ "]" }
kw_file_header_copyright      = { "[" ~ "Copyright" ~ "]" }

// -------- component keyword --------
kw_component                       = { "[" ~ "Component" ~ "]" }
kw_component_manufacturer          = { "[" ~ "Manufacturer" ~ "]" }
kw_component_package               = { "[" ~ "Package" ~ "]" }
kw_component_pin                   = { "[" ~ "Pin" ~ "]" }
kw_component_pin_mapping           = { "[" ~ "Pin Mapping" ~ "]" }
// ... etc

// -------- model keyword --------
kw_model                          = { "[" ~ "Model" ~ "]" }
kw_model_model_spec               = { "[" ~ "Model Spec" ~ "]" }
kw_model_pulldown                 = { "[" ~ "Pulldown" ~ "]" }
// ... etc
```

### 2.2 Pest 分组规则

```pest
// 第一级容器（输出为 [[array-of-tables]]）
first_level_keyword = {
    kw_component | kw_model | kw_submodel | kw_external_circuit
    | kw_test_data | kw_test_load | kw_define_package_model
    | kw_interconnect_model_set | kw_model_selector
}

// 第二级关键词（输出为 [parent.child]）
second_level_keyword = {
    kw_file_header_ibis_ver | kw_file_header_comment_char
    | kw_file_header_file_name | kw_file_header_file_rev
    | kw_file_header_date | kw_file_header_source
    | kw_file_header_notes | kw_file_header_disclaimer
    | kw_file_header_copyright
    | kw_component_manufacturer | kw_component_package | kw_component_pin
    | kw_component_pin_mapping | kw_component_package_model
    | kw_component_series_pin_mapping | kw_component_differential_pin_mapping
    | kw_model_model_spec | kw_model_temperature_range | kw_model_voltage_range
    | kw_model_pulldown | kw_model_pullup | kw_model_gnd_clamp
    | kw_model_power_clamp | kw_model_gnd_table | kw_model_power_table
    | kw_model_ramp | kw_model_rising_waveform | kw_model_falling_waveform
    | kw_model_series_current | kw_model_series_mosfet
    | kw_model_threshold_sensitivity | kw_model_driver_schedule
    | kw_model_reference_supply
    | kw_submodel_pulldown | kw_submodel_pullup | kw_submodel_gnd_clamp
    | kw_submodel_power_clamp | kw_submodel_ramp
    | kw_external_circuit_circuit_model | kw_external_circuit_port_map
    | kw_test_load_reference_supply | kw_test_load_pulldown
    | kw_test_load_pullup | kw_test_load_gnd_clamp | kw_test_load_power_clamp
    | kw_define_package_model_package_model | kw_define_package_model_pin_mapping
    | kw_interconnect_model_set_interconnect_model
    | kw_interconnect_model_set_port_map
    | kw_interconnect_model_set_manchester_encoder
    | kw_interconnect_model_set_pin_mapping
    | kw_model_selector_model_list
}
```

### 2.3 空白/换行处理

`ibis_file` 使用原子规则 `@{ ... }` 匹配单行内容，避免 pest 的 WS 跳跃复杂性：

```pest
// -------- line-level content (matches one full line) --------
ibis_line = { text_line ~ (NEWLINE | EOI) }

// -------- file topology (line-oriented) --------
ibis_file = {
    SOI ~
    ( ibis_line )*
    ~ EOI
}
```

然后 Rust 端对每行内容用 `kv_line` / `data_line` 规则做二次解析。这样 pest 语法简单可靠，结构化解析在 Rust 中完成。

> **或者**：如果想让 pest 做完整的行内解析，使用 `~` 操作符的自动 WS 跳跃特性，将 `ibis_file` 定义为逐行匹配：
> 
> ```pest
> content_line = { (si_number | ident | "/" | "=")+ }
> 
> ibis_file = {
>     SOI ~
>     ( NEWLINE
>     | file_header_keyword | first_level_keyword | second_level_keyword
>     | kw_end | keyword | content_line | comment
>     )*
>     ~ EOI
> }
> ```
>
> 关键：`NEWLINE = _{ "\r\n" | "\n" | "\r" }` 作为静默规则消耗换行符。

---

## 3. Rust 多级 AST 树设计

### 3.1 树节点类型

```rust
/// 标记一个节段在 TOML 输出中的角色。
enum NodeKind {
    /// [[Component]] — 一级容器，输出为 array-of-tables。
    /// 在 TOML 中用 [[Section]] 表示，可以有多个同名实例。
    FirstLevel,

    /// [Component.Pin] — 二级子节段，输出为父级下的子 table。
    /// 在 TOML 中用 [Parent.Child] 表示。
    SecondLevel,

    /// [File_Header] — 虚拟容器，不映射到 IBIS 关键词，
    /// 由 Rust 自动将连续的文件头字段收集到此节点下。
    FileHeader,

    /// [Unknown] — 未识别的关键词，保留原样输出。
    /// 用于处理 pest 通用 keyword 规则匹配的节段。
    Unknown,
}

/// 层次化 IBIS 节段树节点。
struct SectionNode {
    /// 节段关键词名（如 "Component", "IBIS ver", "Pin"）。
    keyword: String,

    /// 节段角色，决定 TOML 输出格式。
    kind: NodeKind,

    /// 属于该节段的原始内容行。
    content: Vec<String>,

    /// 子节段（仅 FirstLevel 和 FileHeader 有）。
    children: Vec<SectionNode>,
}
```

### 3.2 树构建算法

摒弃当前的 [`build_toml_from_blocks`](src/ibis2ibstoml/frontend.rs:336)，改用递归的树构建器：

```
build_tree(blocks, start_index) → (roots, next_index)

对 blocks[start_index..] 做以下处理：

Phase A: 文件头收集
  如果当前 block 的 rule == second_level_keyword 且 keyword 前缀为文件头字段:
    创建 FileHeader 虚拟父节点
    收集后续连续的 second_level_keyword（文件头字段）作为子节点
    返回 ([FileHeader 节点], next_index)

Phase B: 一级容器处理
  如果当前 block 的 rule == first_level_keyword:
    创建 FirstLevel 节点
    block_index + 1
    递归收集子节点（直到遇到另一个 first_level_keyword 或 kw_end）:
      每个 second_level_keyword block → SecondLevel 子节点
      每个 keyword (通用) block → Unknown 子节点
    返回 ([FirstLevel 节点], next_index)

Phase C: kw_end → 跳过
Phase D: 其他 → Unknown 节点
```

```rust
fn build_section_tree(
    blocks: &[ParsedBlock],
    start: usize,
    stop_rules: &[Rule],   // 遇到这些规则时停止收集
) -> (Vec<SectionNode>, usize) {
    let mut nodes = Vec::new();
    let mut i = start;

    // Phase A: 文件头收集
    if i < blocks.len() && is_file_header_field(&blocks[i]) {
        let mut children = Vec::new();
        while i < blocks.len() && is_file_header_field(&blocks[i]) {
            children.push(SectionNode {
                keyword: blocks[i].keyword.clone(),
                kind: NodeKind::SecondLevel,
                content: blocks[i].content.clone(),
                children: Vec::new(),
            });
            i += 1;
        }
        nodes.push(SectionNode {
            keyword: "File_Header".into(),
            kind: NodeKind::FileHeader,
            content: Vec::new(),
            children,
        });
        return (nodes, i);
    }

    while i < blocks.len() {
        // 检查是否遇到停止规则
        if stop_rules.contains(&blocks[i].rule) || blocks[i].rule == Rule::kw_end {
            break;
        }

        match blocks[i].rule {
            // Phase B: 一级容器
            r if r == Rule::first_level_keyword => {
                let name = blocks[i].keyword.clone();
                let content = blocks[i].content.clone();
                i += 1;

                // 递归收集子节点
                let (children, next) = build_section_tree(
                    blocks, i,
                    &[Rule::first_level_keyword, Rule::kw_end],
                );
                i = next;

                nodes.push(SectionNode {
                    keyword: name,
                    kind: NodeKind::FirstLevel,
                    content,
                    children,
                });
            }

            // Phase C: 其他二级/通用关键词 → 作为单例或子节点
            _ => {
                nodes.push(SectionNode {
                    keyword: blocks[i].keyword.clone(),
                    kind: NodeKind::SecondLevel,
                    content: blocks[i].content.clone(),
                    children: Vec::new(),
                });
                i += 1;
            }
        }
    }

    (nodes, i)
}
```

### 3.3 树 → TOML 序列化

```rust
fn serialize_tree(
    nodes: &[SectionNode],
    parent_path: &str,      // 父级路径，如 "Component"
    output: &mut String,
) {
    for node in nodes {
        let full_path = if parent_path.is_empty() {
            toml_section_name(&node.keyword)
        } else {
            format!("{}.{}", parent_path, toml_section_name(&node.keyword))
        };

        // 输出表头
        match node.kind {
            NodeKind::FirstLevel => {
                writeln!(output, "[[{}]]", full_path);
            }
            _ => {
                writeln!(output, "[{}]", full_path);
            }
        }

        // 输出内容
        emit_content(output, &node);

        // 递归输出子节点（FileHeader 的子节点使用 full_path 作为 parent_path）
        serialize_tree(&node.children, &full_path, output);
    }
}
```

### 3.4 判断文件头字段

如何区分 `second_level_keyword` 中的文件头字段和其他二级关键词？

方案：直接检查 `keyword` 字符串是否以 `file_header` 前缀开头。
在 Rust 中就是 `keyword.starts_with("file_header")`。

但用户不希望字符串匹配。替代方案：在 Rule 匹配层面区分。

由于 pest 的 `second_level_keyword` 分组规则匹配后，`pair.as_rule()` 返回 `Rule::second_level_keyword`，我们无法直接区分内层是文件头字段还是组件子字段。

**推荐方案**：在 Rust 中维护一个 `FILE_HEADER_FIELDS` 常量集，用于快速判断：

```rust
/// Known file header keyword names (for grouping under [File_Header]).
const FILE_HEADER_FIELD_NAMES: &[&str] = &[
    "IBIS ver", "Comment Char", "File name", "File Rev",
    "Date", "Source", "Notes", "Disclaimer", "Copyright",
];
```

这比 `rule_is_*` 函数更简洁，且语义明确——它不是一个分类函数，而是一个已知数据集合。

---

## 4. v5 数据流

```
IBIS 文本
  │
  ▼
[IbisParser::parse(Rule::ibis_file, content)]  ← pest 完整解析
  │  如果失败 → fallback_parse_to_toml()
  │
  ▼
[Pair 树遍历 → ParsedBlock 列表]               ← 将 pest pairs 转为 flat blocks
  │
  ▼
[build_section_tree()]                          ← 递归构建多级 AST 树
  │  ├─ 文件头字段 → 收集到 FileHeader 虚拟父节点
  │  ├─ first_level_keyword → 递归收集子节点
  │  └─ 其他 → 单例节点
  │
  ▼
[serialize_tree()]                              ← 递归序列化为 TOML
  │  ├─ FirstLevel → [[Section]]
  │  ├─ FileHeader → [Section]（无 key） + 子节点
  │  └─ SecondLevel/Unknown → [Parent.Child]
  │
  ▼
.ibs.toml
```

---

## 5. 实施步骤

### 步骤 1：重命名 pest 规则
- `kw_ibis_ver` → `kw_file_header_ibis_ver`
- `kw_comment_char` → `kw_file_header_comment_char`
- `kw_file_name` → `kw_file_header_file_name`
- `kw_file_rev` → `kw_file_header_file_rev`
- `kw_date` → `kw_file_header_date`
- `kw_source` → `kw_file_header_source`
- `kw_notes` → `kw_file_header_notes`
- `kw_disclaimer` → `kw_file_header_disclaimer`
- `kw_copyright` → `kw_file_header_copyright`
- 删除 `file_header_keyword` 分组
- 将上述规则加入 `second_level_keyword` 分组

### 步骤 2：修复 pest 空白/换行处理
- 添加 `NEWLINE = _{ "\r\n" | "\n" | "\r" }`
- 在 `ibis_file` 中添加 `NEWLINE` 和 `WHITESPACE` 作为消费选项

### 步骤 3：实现多级 AST 树
- 定义 `NodeKind` 枚举和 `SectionNode` 结构体
- 实现 `build_section_tree()` 递归构建器
- 实现 `serialize_tree()` 递归序列化器
- 删除旧的 `build_toml_from_blocks()`

### 步骤 4：更新测试
- 更新测试断言匹配新命名
- 添加多级嵌套测试
- 验证真实 IBIS 样本

---

## 6. 关键决策记录

| 决策 | 选择 | 理由 |
|------|------|------|
| `kw_1级_2级` 命名 | 所有关键词规则 | 统一规范，命名自解释 |
| 文件头字段分类 | Rust 端 `FILE_HEADER_FIELD_NAMES` 常量 | 避免 pest 分组冗余，集中管理 |
| 空白/换行处理 | `NEWLINE` + `WHITESPACE` 作为 `ibis_file` 的消费选项 | 让 pest 正确匹配真实 IBIS 内容 |
| 树结构 | 递归 `build_section_tree()` | 支持任意深度的节段嵌套 |
| TOML 序列化 | 递归 `serialize_tree()` | 与树结构一一对应 |
