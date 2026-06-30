# IBIS Parser 技术方案分析

## 1. 项目现状分析

### 当前架构

```
src/
├── main.rs                    # 入口：读取 .ibs 文件并调用解析器
├── lib.rs                     # 库入口（目前为 trivial 占位）
└── ibis_parser/
    ├── mod.rs                 # 模块声明
    ├── core.rs                # 解析器入口（未实现，仅 stub）
    └── ibis_structure.rs      # 完整的 AST 数据结构（已定义）
tests/
└── f103c8.ibs                 # 测试用 IBIS 文件
```

### 已具备的基础

- **AST 数据结构已完整定义**：[`ibis_structure.rs`](src/ibis_parser/ibis_structure.rs) 已覆盖 IBIS 规范的全部主要节段：
  - `IBIS_File`（根节点）→ 含 header / components / models / submodels / external_circuits / test_data / test_loads / package_models / interconnect_model_sets
  - `IBIS_FileHeader` → 文件头信息
  - `IBIS_Component` → 元件定义（含 Package / Pin / Diff Pin / Pin Mapping 等）
  - `IBIS_Model` → 模型定义（含 IV 曲线 / Ramp / 波形 / 温度电压范围等）
  - 及其他辅助结构（`Triplet<T>`、`IBIS_TableData`、`IBIS_CornerValue` 等）
- **`serde` 已引入**，可方便序列化/反序列化解析结果

### 待实现的核心

- [`core.rs:ibis_file_parse()`](src/ibis_parser/core.rs:7) — 解析器主函数目前为 `todo!()`
- 需要将原始 `.ibs` 文本转换为 `IBIS_File` AST

## 2. IBIS 文件格式特征分析

### 格式概览

IBIS (I/O Buffer Information Specification) 文件是一种**基于行的、关键字驱动的文本格式**：

| 特征 | 描述 | 示例 |
|------|------|------|
| **节段标记** | `[关键字]` 独立成行 | `[Component]`, `[Model]` |
| **键值对** | `关键字: 值` 或 `关键字 = 值` | `Model_type I/O`, `R_load = 1.0000k` |
| **表格数据** | 列头 + 多行数值 | [Pulldown] 下的 V/I 曲线 |
| **注释** | 以 `|` 开头，可出现在行首或行末 | `\| Reference voltage...` |
| **单位后缀** | 数值后跟 SI 单位 | `1.12pF`, `3.3000V`, `100.0MOhm` |
| **SI 前缀** | p, n, u, m, k, M, G, 等 | `597.107p`, `0.1250k` |
| **Corner 值** | typ/min/max 三值组 | `1.12p 0.79p 1.15p` |
| **特殊格式** | Ramp 的 `dV/dt_r` | `1.926/597.107p` |

### 主要节段结构

```
[IBIS ver]       → 版本号
[File name]      → 文件名
[File Rev]       → 修订版本
[Component]      → 元件定义开始（包含 [Package], [Pin], [Diff Pin] 等子节段）
[Model]          → 模型定义（包含 [Pulldown], [Pullup], [GND_clamp], [Ramp] 等子节段）
[End]            → 文件结束
```

## 3. 技术选型分析：pest vs 其他方案

### 方案 A：纯 pest（PEG 解析器生成器）

**pest** 使用声明式 `.pest` 语法文件，自动生成解析器。

| 优势 | 劣势 |
|------|------|
| 语法与代码分离，易维护 | IBIS 的注释 `|` 散布在各处，PEG 处理复杂 |
| 自动生成 Parse Tree | 表格数据（N 列 × M 行）用 PEG 描述不自然 |
| 内置错误定位 | 单位后缀需要额外的 AST 后处理 |
| 社区活跃，生态成熟 | SI 前缀数值解析（如 `1.12p`）需要自定义规则 |
| 适合结构化的嵌套语法 | 对纯行式格式反而增加复杂度 |

**适用 pest 的子语法**：
- Corner 值组：`1.12p 0.79p 1.15p`
- Ramp 表达式：`1.926/597.107p`
- 基本数值 + 单位：`3.3000V`, `50.0pF`
- 节段标记：`[Keyword]`
- 简单键值对：`key = value`

### 方案 B：纯手写解析器（State Machine + Line-by-line）

| 优势 | 劣势 |
|------|------|
| 完全控制解析逻辑 | 需要手动处理大量模式匹配 |
| 适合行式格式 | 状态管理较复杂（嵌套节段） |
| 无额外依赖 | 代码量较大 |
| 性能最优 | 边界情况需仔细处理 |

### 方案 C：`nom` 组合子解析器

| 优势 | 劣势 |
|------|------|
| 组合式构建，代码复用性好 | 学习曲线较陡 |
| 流式处理能力强 | IBIS 是行格式，流式优势不明显 |
| 错误恢复能力好 | 类型系统复杂，编译时间长 |

### 方案 D：混合方案（推荐 ✅）

使用 **pest 处理结构化子语法** + **手写状态机进行节段调度和表格解析**：

```
┌─────────────────────────────────────────────────────────┐
│                    ibis_file_parse()                     │
│                                                         │
│  1. 预过滤：移除注释行，提取行数据                        │
│  2. 节段分割：按 [Keyword] 将文件拆分为节段块              │
│     ┌───────────────────────────────────────┐            │
│     │ 3. 分派到各节段处理器                    │            │
│     │                                       │            │
│     │ [IBIS ver]   → 手写行解析 或 pest       │            │
│     │ [Component]  → 子状态机处理子节段       │            │
│     │ [Model]      → 子状态机处理子节段       │            │
│     │ [Pulldown]   → 表格解析器              │            │
│     │ [Ramp]       → pest 解析 dv/dt 格式    │            │
│     │ Corner 值    → pest 解析三值组          │            │
│     └───────────────────────────────────────┘            │
│  4. 组装为 IBIS_File AST                                 │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

## 4. 推荐方案：pest + 手写状态机混合实现

### 理由

1. **pest 处理结构化子语法**：Corner 值组、Ramp 表达式、带单位的数值等 pest 可以优雅处理
2. **手写逻辑处理顶层结构**：节段分割、表格数据解析——这些线性/重复结构用代码比用 PEG 更自然
3. **减少整体复杂度**：不强制用 pest 描述整个 IBIS 文件，只在 pest 擅长的领域使用它

### 建议的 pest 语法边界

```pest
// 在 pest 语法中定义：
si_number  = { ASCII_DIGIT+ ~ "."? ~ ASCII_DIGIT* ~ SI_PREFIX? ~ UNIT? }
corner_value = { si_number ~ si_number? ~ si_number? }
ramp_value = { si_number ~ "/" ~ si_number }
keyword_header = { "[" ~ (ASCII_ALPHA | "_")+ ~ "]" }
```

其余部分（节段路由、表格行解析、键值对提取）用手写 Rust 实现。

### 可选替代：仅在 AST 后处理中使用 pest

如果不想增加 pest 依赖，可以将所有数值/单位解析放在 Rust 后处理中统一完成，使用正则或手写解析。

## 5. 实施计划

### 第1步：添加 pest 依赖（如采用混合方案）

在 [`Cargo.toml`](Cargo.toml) 中添加 `pest` 和 `pest_derive`。

### 第2步：编写 pest 语法文件

创建 [`src/ibis_parser/ibis.pest`](src/ibis_parser/) 定义：
- `si_number` — 带 SI 前缀和单位的数值
- `corner_value` — typ/min/max 三值组
- `ramp_value` — 如 `1.926/597.107p`
- `keyword_section` — 节段标记
- 顶层结构（可选）

### 第3步：实现节段分割器

在 [`core.rs`](src/ibis_parser/core.rs) 中实现：
- `preprocess()` — 移除注释，处理行连续性
- `split_sections()` — 按 `[Keyword]` 分割
- `identify_section()` — 识别每个节段的类型

### 第4步：实现各节段解析器

创建 [`src/ibis_parser/sections/`](src/ibis_parser/) 模块：
- `header.rs` — 解析 `[IBIS ver]`、`[File name]` 等
- `component.rs` — 解析 `[Component]` 及其子节段
- `model.rs` — 解析 `[Model]` 及其子节段
- `tabular.rs` — 解析 IV 曲线等表格数据
- `values.rs` — 数值/单位/Cerner 值解析（使用 pest）

### 第5步：单元测试

- 使用 [`tests/f103c8.ibs`](tests/f103c8.ibs) 作为测试样本
- 递增式测试：先解析节段分割、再解析单个节段、最后完整文件

### 第6步：集成到现有入口

- 完成 [`ibis_structure.rs`](src/ibis_parser/ibis_structure.rs) 的 Serde 标注（可选）
- 在 [`main.rs`](src/main.rs) 中验证解析结果

## 6. 文件结构（实施后）

```
src/
├── main.rs
├── lib.rs
└── ibis_parser/
    ├── mod.rs
    ├── core.rs                  # 主入口：协调解析流程
    ├── ibis.pest                # pest 语法定义（如使用 pest）
    ├── ibis_structure.rs        # AST 定义（已有）
    ├── preprocessor.rs          # 注释移除、行预处理
    ├── section_splitter.rs      # 按 [Keyword] 节段分割
    ├── sections/
    │   ├── mod.rs
    │   ├── header.rs            # 文件头解析
    │   ├── component.rs         # Component 节段解析
    │   ├── model.rs             # Model 节段解析
    │   └── submodel.rs          # Submodel 节段解析
    ├── tabular.rs               # 表格数据解析
    └── values.rs                # 数值/单位/SI前缀解析
```

## 7. 关于使用 pest 的最终建议

**是否使用 pest？** → **有条件地使用**

- ✅ **推荐用途**：解析 Corner 值组、Ramp 表达式、带 SI 单位的数值等**结构化子语法**
- ❌ **不推荐用途**：用 pest 描述整个 IBIS 文件的完整语法（过于复杂，得不偿失）
- 🟡 **可行替代**：完全不使用 pest，而是用手写代码 + 正则表达式解析 SI 数值，减少依赖

**底线**：pest 可以作为辅助工具在特定子语法上发挥作用，但整个解析器的主要架构应为**手写状态机 + 节段路由**模式。如果团队对 pest 不熟悉，也可以选择纯手写方案。
