# IBIS Parser 技术方案 v3（当前架构）

## 1. 项目现状

### 1.1 当前文件结构

```
src/
├── main.rs                          # 入口：读取 .ibs → 输出 .ibs.toml
├── lib.rs                           # 库入口：暴露 ibis2ibstoml / ibis_parser
├── ibis2ibstoml/                    # ✅ 第一层主模块（活跃开发中）
│   ├── mod.rs                       # 模块声明
│   ├── core.rs                      # 公开 API：ibs2ibstoml(path) -> Result<String>
│   ├── ibis.pest                    # pest 抽象语法文件（仅定义 keyword_header, si_number 等通用规则）
│   ├── lexical_analy.rs             # 词法分析器：pest 分词 → Token (keyword, content_lines)
│   └── syntax_analy.rs              # 语法分析器：keyword 分类 → 树构建 → TOML 序列化
└── ibis_parser/                     # 🗄️ 第二层语义参考模块（当前不依赖）
    ├── mod.rs
    └── ibis_structure.rs            # 强类型 AST 定义（仅供第二层使用）
tests/
├── header_parse_test.rs             # 文件头解析集成测试
└── examples/
    ├── f103c8.ibs                   # STM32F103C8 真实 IBIS 样本
    ├── cyclone2.ibs                 # Cyclone II 样本
    ├── invchain_test_0614.ibs       # 逆变器链测试样本
    ├── u26a_800.ibs                 # U26A-800 样本
    └── virtex5.ibs                  # Virtex-5 样本
plans/
├── ibis_parser_architecture.md      # 本文档（v3 当前架构）
├── ibis_parser_architecture_2.md    # v2 旧方案（废弃参考）
└── ibis_struct.toml                 # 目标 TOML 骨架参考（基于 IBIS 关键词树）
```

### 1.2 核心技术栈

| 组件 | 用途 | 层级 |
|------|------|------|
| [`pest`](Cargo.toml:12) + [`pest_derive`](Cargo.toml:13) | 第一层：词法分析（抽象分词） | 词法层 ✅ |
| 纯 Rust 字符串判定（[`syntax_analy.rs`](src/ibis2ibstoml/syntax_analy.rs)） | 第一层：关键词分类 + 树构建 | 语法层 ✅ |
| [`toml`](Cargo.toml:10) | 第二层：TOML 反序列化+语义验证 | 第二层 🔲 |
| [`serde`](Cargo.toml:9) | 第二层：强类型反序列化 | 第二层 🔲 |

---

## 2. 架构变更说明（v2 → v3）

### v3 核心变更：词法/语法分析分离

```
v2（旧）：
IBIS 文件 → [parser.rs + converter.rs + ibis.pest（含具体 keyword 规则）] → .ibs.toml

v3（当前）：
IBIS 文件 → [ibis.pest（抽象规则）]  →  [lexical_analy.rs（分词）]  →  [syntax_analy.rs（分类+树+TOML）] → .ibs.toml
                ↑ 词法层（pest）              ↑ 词法层                      ↑ 语法层
```

### v3 关键决策

| 决策 | v2 | v3 | 理由 |
|------|----|----|------|
| **pest 的角色** | 定义具体 keyword 规则（`kw_ibis_ver`、`kw_component` 等） | **仅定义抽象规则**（`keyword_header`、`text_line`、`si_number` 等） | 具体 keyword 判定是语法任务，不应在 pest 中定义 |
| **keyword 分类方式** | pest rule 匹配 + Rust enum 双重判定 | **纯 Rust 字符串 match** | 简化 pest，所有分类逻辑集中在一处 |
| **文件结构** | `parser.rs` + `converter.rs` | `lexical_analy.rs` + `syntax_analy.rs` | 职责边界更清晰 |
| **`parse_header_line`** | 依赖 pest 具体 rule 进行匹配 | **纯字符串操作**（`find(']')` + `match keyword_part`） | pest 只负责抽象分词，不负责具体字段识别 |

### v3 相比 v2 的优势

1. **pest 纯粹性**：ibis.pest 只描述"语法形状"（keyword 是什么样的，si_number 是什么样的），不描述"语义标签"（哪些 keyword 叫 Component）
2. **keyword 分类集中**：所有 keyword 判定在一个 `classify()` 函数中完成，便于维护
3. **减少 pest 规则膨胀**：pest 规则数量从 30+ 条降至 ~15 条，编译速度更快
4. **容错性更强**：pest 不认识的 keyword 不会导致解析失败，而是以 `Other` 类型保留

---

## 3. 第一层实现详解（格式重塑层）

### 3.1 基本原则

第一层的全部使命可以概括为三句话：

> **读入 IBIS 行文本 → 按 `[Keyword]` 切块 → 按关键词类型输出 TOML 骨架**

在这个过程中（与 v2 保持一致）：
- ❌ 不解析 `R_pkg`、`L_pkg`、`C_pkg` 各自是什么
- ❌ 不把 `1.12p` 转成 `1.12e-12`
- ❌ 不检查 `Temperature Range` 的三个值谁大谁小
- ❌ 不验证 `Pin` 引用的 `Model` 是否存在
- ✅ 只清洗行、按关键词分割、构建树、输出 TOML 字符串

### 3.2 词法分析层（[`lexical_analy.rs`](src/ibis2ibstoml/lexical_analy.rs)）

**职责**：原始 IBIS 文本 → `Vec<Token>`，其中 `Token { keyword: String, content: Vec<String> }`

```
输入：原始 IBIS 文本行
├─ clean()              去除 | 注释（保护括号内的 |）
├─ extract_keyword_name()   使用 pest 的 keyword_header 抽象规则提取 keyword 名
├─ 行分组：keyword 行 → 开始新 Token；非 keyword 行 → 追加到当前 Token 的 content
└─ 输出 Vec<Token>
    • keyword 为原始字符串（如 "IBIS ver"、"Component"）
    • 不做任何 keyword 分类或语义判断
```

**关键设计**：`IbisParser` 结构体从 pest 生成，但 pest grammar 中**没有任何具体 keyword 规则**。`extract_keyword_name()` 使用的唯一 pest rule 是 `keyword_header`，这是一个仅识别 `[文本]` 形式的抽象规则。

### 3.3 语法分析层（[`syntax_analy.rs`](src/ibis2ibstoml/syntax_analy.rs)）

**职责**：`Vec<Token>` → `.ibs.toml` 字符串

```
Phase 1: Classify（keyword 分类）
─────────────────────────────────────────────
输入：词法分析输出的 Token 列表
├─ classify()  纯字符串 match，将 raw keyword 映射为 Keyword enum
├─ FILE_HEADER_KEYWORDS 常量的 9 个文件头关键词 → FileHeaderField
├─ 9 个容器关键词 → Component / Model / Submodel 等
├─ "End" → End（输出时跳过）
└─ 其他 → Other（保留原样输出）

Phase 2: Build Tree（树构建）
─────────────────────────────────────────────
输入：已分类的 (Keyword, Vec<String>) blocks 列表
├─ 文件头字段（连续的 FileHeaderField）→ 自动收集到 [File_Header] 虚拟父节点下
├─ 容器关键词 → [[array]] 父节点 + 自动收集后续子节段
├─ End → 跳过
└─ 其他 → [table] 单例节点

Phase 3: Serialize（TOML 输出）
─────────────────────────────────────────────
输入：层次化 Section 树
├─ [[Component]] 或 [Component.Package] 等表头
├─ 内容写入规则：
│   ├─ 0 行 → 空表
│   ├─ 1 行 → key = "单行字符串"
│   └─ N 行 → key = [ "行1", "行2", ... ]  一维字符串数组
└─ 所有值一律用双引号包裹，纯 TOML 字符串
```

### 3.4 Keyword 分类策略

```rust
fn classify(raw_keyword: &str) -> Keyword {
    match raw_keyword {
        // 9 个容器关键词 → array parents
        "Component"              => Keyword::Component,
        "Model"                  => Keyword::Model,
        "Submodel"               => Keyword::Submodel,
        "External Circuit"       => Keyword::ExternalCircuit,
        "Test Data"              => Keyword::TestData,
        "Test Load"              => Keyword::TestLoad,
        "Define Package Model"   => Keyword::DefinePackageModel,
        "Interconnect Model Set" => Keyword::InterconnectModelSet,
        "Model Selector"         => Keyword::ModelSelector,
        // 结束标记
        "End"                    => Keyword::End,
        // 文件头字段
        raw if FILE_HEADER_KEYWORDS.contains(&raw) => Keyword::FileHeaderField(...),
        // 其他
        other                    => Keyword::Other(other),
    }
}
```

**关键约束：**
- pest **不参与** `classify()` 的任何环节——所有 keyword 判定是纯字符串操作
- 即使 pest 不识别的 keyword，也会以 `Other` 类型保留并输出，不会导致解析失败
- 9 个容器关键词和 9 个文件头关键词通过 `const` 常量维护，集中管理

---

---

## 4. 第一层期望输出示例（最终目标）

### 4.1 文件头部节段

文件头部所有关键词统一收纳在 `[File_Header]` 下，每个头部字段作为其子 table：

```toml
[File_Header]

[File_Header.IBIS_ver]
ibis_ver = "2.1"

[File_Header.File_name]
file_name = "f103c8.ibs"

[File_Header.File_rev]
file_rev = "1.1"

[File_Header.Date]
date = "12-08-2024"
```

**设计理由**：文件头部在 IBIS 规范中是一个逻辑整体（文件元信息），统一用 `[File_Header]` 包裹既保持了 IBIS 关键词的原始映射，又在 TOML 中自然表达了"这些都属于文件头"的语义。

### 4.2 容器节段（Component / Model）

容器关键词输出为 `[[array-of-tables]]`。内容行的输出格式根据数据特征分为两类：

**行内结构规整的数据**（如 Package 的三行固定格式、表格型 IV 曲线）→ 优先输出为 **二维字符串数组**，每行按空白符分割为独立的字符串元素：

```toml
[[Component]]
component = "STM32F103C8"

[Component.Manufacturer]
manufacturer = "STMicroelectronics NV"

[Component.Package]
package = [
    ["R_pkg", "0.000", "0.000", "0.000"],
    ["L_pkg", "0.000H", "0.000H", "0.000H"],
    ["C_pkg", "0.000F", "0.000F", "0.000F"],
]

[Component.Pin]
pin = [
    ["2", "PC13-ANTI_TAMP", "IO8TC"],
    ["3", "PC14-OSC32_IN", "IO8TC"],
    ["5", "OSC_IN", "IO8TC"],
    ["6", "OSC_OUT", "IO8TC"],
    ["10", "PA0-WKUP", "IO8TC"],
    ...
]

[Model.Pulldown]
pulldown = [
    ["-3.3000", "-2.0000mA", "-2.0000mA", "-1.0000mA"],
    ["-3.1000", "-2.0000mA", "-2.0000mA", "-1.0000mA"],
    ...
]
```

**行内结构松散或不易分割的数据**（如 Ramp 的多字段混合行）→ 保持为 **一维字符串数组**：

```toml
[[Model]]
model = "IO8FT"

[Model.Ramp]
ramp = [
    "dV/dt_r 1.926/597.107p 1.171/792.551p 1.993/430.881p",
    "dV/dt_f 2.083/427.561p 1.367/1.031n 2.548/480.447p",
    "R_load = 1.0000k",
]
```

**关键原则：**
- 每个 `[Keyword]` 节段的内容行 → TOML 字符串数组（一维或二维）
- 二维数组分割标准：**仅按空白符分割**，不做字段类型推断（所有元素仍是 TOML 字符串）
- 不做数值转换（`"-2.0000mA"`、`"1.926/597.107p"` 就是字符串）
- 分割与否的判断依据：该节段的行结构在 IBIS 规范中是否有**固定的列数**
- 第二层可以自由选择使用一维原始行或二维预分割行，互不冲突

### 4.3 为什么保持粗粒度？

| 理由 | 说明 |
|------|------|
| **IBIS 规范是终极指南** | TOML 结构只需对齐 IBIS 关键词树，不需要对齐字段级语义 |
| **避免语义下沉** | 一旦开始拆 `Pin` 的行、识别 `R_pkg` / `L_pkg`，就已经在行语义上下判断了 |
| **最大容错率** | 厂商 IBIS 文件格式变体极多，保持字符串数组 100% 保护原始数据 |
| **第二层做"该做的事"** | 第二层反序列化 TOML 时，对着规范一条条解析，不会被第一层的"猜测"误导 |

---

## 5. 待完成的工作

### 5.1 第一层：关键词覆盖度扩展

确保所有 IBIS 节段被正确识别和映射。文件头部所有关键词统一收纳在 `[File_Header]` 下：

| 节段关键词 | TOML 输出 | 状态 |
|------------|-----------|------|
| `[IBIS ver]` | `[File_Header.IBIS_ver]` → `ibis_ver` 键 | ✅ 已实现 |
| `[Comment Char]` | `[File_Header.Comment_Char]` | ✅ 已实现 |
| `[File name]` | `[File_Header.File_name]` | ✅ 已实现 |
| `[File Rev]` | `[File_Header.File_Rev]` | ✅ 已实现 |
| `[Date]` | `[File_Header.Date]` | ✅ 已实现 |
| `[Source]` | `[File_Header.Source]` | ✅ 已实现 |
| `[Notes]` | `[File_Header.Notes]` | ✅ 已实现 |
| `[Disclaimer]` | `[File_Header.Disclaimer]` | ✅ 已实现 |
| `[Copyright]` | `[File_Header.Copyright]` | ✅ 已实现 |
| `[Component]` | `[[Component]]` | ✅ 已实现 |
| `[Model]` | `[[Model]]` | ✅ 已实现 |
| `[Submodel]` | `[[Submodel]]` | 🔲 待加入 |
| `[External Circuit]` | `[[External_Circuit]]` | 🔲 待加入 |
| `[Test Data]` | `[[Test_Data]]` | 🔲 待加入 |
| `[Test Load]` | `[[Test_Load]]` | 🔲 待加入 |
| `[Define Package Model]` | `[[Define_Package_Model]]` | 🔲 待加入 |
| `[Interconnect Model Set]` | `[[Interconnect_Model_Set]]` | 🔲 待加入 |
| `[Model Selector]` | `[[Model_Selector]]` | 🔲 待加入 |
| `[End]` | 忽略 | ✅ 已实现 |
| 其他子节段（Package / Pin / Pulldown 等） | 自动作为父节段的 children | ✅ 已实现 |

### 5.2 `Keyword::classify()` 数组完善

v3 架构中，关键词分类通过 [`Keyword::classify()`](src/ibis2ibstoml/syntax_analy.rs:144) 的 `match` 表达式完成。目前 9 个容器关键词已全部实现：

```rust
fn classify(raw_keyword: &str) -> Keyword {
    match raw_keyword {
        "Component"              => Keyword::Component,
        "Model"                  => Keyword::Model,
        "Submodel"               => Keyword::Submodel,
        "External Circuit"       => Keyword::ExternalCircuit,
        "Test Data"              => Keyword::TestData,
        "Test Load"              => Keyword::TestLoad,
        "Define Package Model"   => Keyword::DefinePackageModel,
        "Interconnect Model Set" => Keyword::InterconnectModelSet,
        "Model Selector"         => Keyword::ModelSelector,
        // ... 文件头字段和 End 等
    }
}
```

**注意**：v3 用 `classify()` 替代了 v2 的 `ARRAY_KEYWORDS` 常量数组，功能等价但更可维护。容器关键词触发 `[[array-of-tables]]` 输出；不在列表中的子节段（如 `Package`、`Pin`、`Pulldown`、`Ramp`）由树构建逻辑自动收集为父节段的 children。

### 5.3 文件头多行字段支持

| 字段 | 当前状态 |
|------|----------|
| `[Notes]` | 基本支持，但 continuation line 聚合需要加固 |
| `[Disclaimer]` | 同上 |
| `[Copyright]` | 同上 |

这些字段在 IBIS 中的值可能跨多行（以 `|` 续行），当前实现需要确保多行内容被正确聚合为一个字符串。

### 5.4 测试体系建设

| 测试类型 | 文件 | 现状 |
|----------|------|------|
| 文件头解析 | [`tests/header_parse_test.rs`](tests/header_parse_test.rs) | ✅ 完成 |
| 词法分析（分词） | [`lexical_analy.rs`](src/ibis2ibstoml/lexical_analy.rs) 中的单元测试 | ✅ 完成（8 个测试） |
| 语法分析（分类+树+TOML） | [`syntax_analy.rs`](src/ibis2ibstoml/syntax_analy.rs) 中的单元测试 | ✅ 完成（20+ 个测试） |
| 真实 IBIS 样本 | [`tests/examples/f103c8.ibs`](tests/examples/f103c8.ibs) | ✅ 1 个样本 |
| 更多 IBIS 样本 | `tests/examples/` | ⚠️ 已收集 5 个样本，需更多 |
| TOML 输出基线测试 | — | ❌ 未实现 |
| 完整文件转换测试 | — | ❌ 未实现 |

### 5.5 第二层：语义分析（后续项目，仅作规划）

以下所有内容**不允许**进入第一层代码：

| 语义任务 | 层 | 说明 |
|----------|-----|------|
| 字符串 → f64 转换 | 第二层 | `"1.12p"` → `1.12e-12`，使用高精度数值库 |
| SI 前缀换算 | 第二层 | `p → 10^-12`, `n → 10^-9`, `k → 10^3` |
| Corner 值验证 | 第二层 | typ/min/max 的数值顺序、缺失值检查 |
| 引用完整性检查 | 第二层 | Pin 引用的 Model 是否存在 |
| 值范围合理性 | 第二层 | 温度、电压是否在合理范围内 |
| Serde 反序列化 | 第二层 | TOML → [`IBIS_File`](src/ibis_parser/ibis_structure.rs:33) 强类型 AST |
| 行内字段分割 | 第二层 | 将 `"2 PC13-ANTI_TAMP IO8TC"` 拆分为 pin_name, signal_name, model_name |
| 业务逻辑验证 | 第二层 | IBIS 规范合规性检查 |

---

## 6. 实施路线图

### 阶段 1：结构映射全覆盖（当前冲刺）

**目标：** 所有 IBIS 关键词节段被正确识别，输出正确的 `[[array]]` / `[table]` 层次结构，内容为原始字符串数组。

- [x] pest 关键词头语法定义
- [x] 注释行清洗（clean 函数）
- [x] 关键词分割 + Array-of-Tables 树构建
- [x] 文件头部解析（键值对字符串）
- [x] Component / Model 容器支持
- [ ] `ARRAY_KEYWORDS` 扩展至全部 9 个顶级容器关键词
- [ ] Notes / Disclaimer / Copyright 多行聚合
- [ ] 子节段收集逻辑验证（每个容器下的所有非容器关键词）
- [ ] TOML 输出基线测试

### 阶段 2：样本收集与鲁棒性测试

**目标：** 通过大量真实 IBIS 样本验证格式转置的正确性。

- [ ] 从各厂商官网收集 10+ 个真实 IBIS 样本
- [ ] 建立 TOML 快照基线测试
- [ ] 边界情况处理：空文件、空节段、缺失 [End]
- [ ] 注释样式变体处理
- [ ] 错误提示（文件格式错误时的诊断信息）

### 阶段 3：第二层语义分析（后续独立项目）

- [ ] 使用 `toml` crate 反序列化 `.ibs.toml`
- [ ] 实现行内字段分割（如 Pin 行 → 结构化字段）
- [ ] 实现 SI 前缀字符串 → `f64` 的高精度转换
- [ ] Corner 值解析与验证
- [ ] 引用完整性检查与 IBIS 规范合规性验证

---

## 7. 架构总览图

```mermaid
flowchart LR
    subgraph 词法层【Lexical Analysis · pest 抽象分词】
        A[.ibs 文件] --> B[行清洗 clean]
        B -->|去除 | 注释| C[pest keyword_header 匹配]
        C -->|逐行扫描| D[Token 聚合]
        D -->|Token{keyword, content_lines}| E[Token 列表]
    end

    subgraph 语法层【Syntax Analysis · 纯 Rust 判定】
        E --> F[Keyword::classify 字符串 match]
        F --> G[分类后的 blocks]
        G --> H[树构建 build_tree]
        H -->|参照 Keyword enum array_parent| I[TOML 序列化 serialize_section]
        I -->|所有值均为 String| J[.ibs.toml]
    end

    subgraph 第二层【语义分析层 · 后续项目】
        J --> K[toml::from_str]
        K -->|反序列化| L[强类型 IBIS_File]
        L --> M[SI 单位换算]
        L --> N[Corner 值验证]
        L --> O[引用完整性检查]
        L --> P[IBIS 规范合规性]
    end

    style 词法层 fill:#4a9eff33,stroke:#4a9eff,stroke-width:2px
    style 语法层 fill:#4a9eff33,stroke:#4a9eff,stroke-width:2px
    style 第二层 fill:#ff9a4a33,stroke:#ff9a4a,stroke-width:2px
    style J fill:#f9f,stroke:#333,stroke-width:2px
```

## 8. 关键决策记录

| 决策 | 选择 | 理由 |
|------|------|------|
| 第一层输出类型 | **纯 TOML 字符串 + 字符串数组** | 禁止 f64 转换，避免精度丢失和格式吞噬 |
| 内容粒度 | **按 `[Keyword]` 节段为单位的粗粒度数组** | 不拆分行内字段，100% 保留原始数据；"丑但稳" |
| 结构映射策略 | **`Keyword` enum + `classify()` 函数** | 纯字符串 match，替代 v2 的 `ARRAY_KEYWORDS` 数组，可维护性更强 |
| 表格数据表示 | **`Vec<Vec<String>>` 二维字符串数组**（规整表格）/<br>**`Vec<String>` 一维字符串数组**（松散行） | 按空白符分割得到二维数组，所有元素仍是 TOML 字符串；不做列数校验和类型推断 |
| 数值解析时机 | **严格在第二层** | 保证第一层的纯粹性，方便独立测试和调试 |
| [`ibis_structure.rs`](src/ibis_parser/ibis_structure.rs) 的角色 | **第二层专属** | 第一层代码禁止 import 此模块 |
| [`plans/ibis_struct.toml`](plans/ibis_struct.toml) 的角色 | **第二层参考文档** | 指导第二层如何将字符串数组反序列化为强类型结构 |
| **pest 抽象边界**（v3 新增） | **pest 仅定义抽象规则，不定义具体 keyword** | keyword 分类是语法任务，不应在 pest 中定义；减少 pest 规则数量，提升编译速度 |
| **词法/语法分离**（v3 新增） | **`lexical_analy.rs` + `syntax_analy.rs`** | 职责边界清晰：词法层只负责 `[Keyword]` 识别，语法层负责分类和结构映射 |
| **header 解析方式**（v3 新增） | **纯字符串操作替代 pest rule 匹配** | `find(']')` + `match keyword_part`，不依赖 pest 的具体 rule，更灵活 |
| 测试策略 | **基于 TOML 字符串快照的基线测试** | 输出可预测，易于验证格式转置的正确性 |
