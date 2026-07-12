# IBIS Parser — 编程规范手册

> **作用域**：本项目全部 Rust 源文件、PEST 语法文件、测试文件。
> **目标**：建立一套人与 AI 协作时共同遵守的、可自动化检查的编码契约，消除歧义、保证一致性。

---

## 目录

1. [通用原则](#1-通用原则)
2. [PEST 语法规范](#2-pest-语法规范)
3. [Rust 命名规范](#3-rust-命名规范)
4. [Rust 控制流与布局规范](#4-rust-控制流与布局规范)
5. [Rust 类型系统规范](#5-rust-类型系统规范)
6. [Rust 函数规范](#6-rust-函数规范)
7. [Rust 文档与注释规范](#7-rust-文档与注释规范)
8. [Rust 错误处理规范](#8-rust-错误处理规范)
9. [测试规范](#9-测试规范)
10. [模块组织规范](#10-模块组织规范)
11. [AI 协作契约](#11-ai-协作契约)

---

## 1. 通用原则

### 1.1 核心信条

| 原则 | 说明 |
|------|------|
| **显式优于隐式** | 所有数据提取、状态转换、类型转换必须显式命名，禁止隐式链式调用 |
| **语义即命名** | 任何标识符（变量/函数/类型）都必须传达完整的业务含义，禁止单字母或模糊缩写 |
| **强类型优先** | 状态、关键词、解析模式必须用 `enum` 表达，严禁使用 `String` / `&str` 做运行时字符串比对 |
| **分层隔离** | 各层级模块通过明确定义的接口通信，禁止跨层级引用内部类型 |
| **可审阅性** | 任何表达式若需读者滚动或横向扫视才能理解，必须拆分为具名中间步骤 |

### 1.2 文件头模板

每个 `.rs` 文件必须以下列格式开头：

```rust
// =============================================================================
// module_or_file_name — one-line description
//
// Optional: extended notes on design constraints, important assumptions,
// and boundary conventions with other modules.
// =============================================================================
```

每个 `.pest` 文件：

```
// =============================================================================
// file_name — one-line description
//
// Design constraints and usage boundary notes.
// =============================================================================
```

---

## 2. PEST 语法规范

### 2.1 规则命名语义

所有 PEST 规则必须遵循以下前缀约定：

| 前缀 | 用途 | 示例 |
|------|------|------|
| `kw_` | IBIS 关键词头（`[Keyword]`） | `kw_component`, `kw_ibis_ver` |
| `line_` | 单行完整规则（关键词头 + 值） | `line_component`, `line_pin_data` |
| `header_` | 文件头部专用规则 | `header_ibis_ver`, `header_notes_line` |
| `si_` | 物理量 / 数学表达式 | `si_number`, `si_prefix` |

**禁止**：
- 不使用前缀的顶层规则（通用基元如 `ident`, `WHITESPACE` 除外）
- 模糊命名如 `value`, `data`, `content` 作为顶层规则名

### 2.2 布局与格式

```pest
// ✅ 正确：多层嵌套必须换行 + 垂直对齐
si_number = @{
    ("+" | "-")? ~
    (
        (ASCII_DIGIT+ ~ "." ~ ASCII_DIGIT*) |
        ("." ~ ASCII_DIGIT+) |
        ASCII_DIGIT+
    ) ~
    ( ^"e" ~ ("+" | "-")? ~ ASCII_DIGIT+ )? ~
    ( si_prefix ~ unit? | unit )?
}

// ❌ 错误：多层 ~ 和 | 挤在单行
si_number = @{ ("+" | "-")? ~ (ASCII_DIGIT+ ~ "." ~ ASCII_DIGIT*) | ("." ~ ASCII_DIGIT+) | ASCII_DIGIT+ ~ (^"e" ~ ("+" | "-")? ~ ASCII_DIGIT+)? ~ (si_prefix ~ unit? | unit)? }
```

**规则**：
1. 任何包含 `~` 或 `|` 的规则，若顶级选择 / 序列超过 2 个元素，必须换行
2. 子表达式缩进 4 个空格，同层操作符垂直对齐
3. 原子规则（`@{ ... }`）若内部简单（≤3 个元素）可保持单行
4. 规则之间空 1 行分隔；区块之间空 3–4 行

### 2.3 规则类型标注

每条规则必须显式标注类型：

| 标注 | 含义 | 行为 |
|------|------|------|
| `{ ... }` | 复合规则（默认静默） | 内部规则不暴露到 Pairs |
| `_{ ... }` | 静默规则 | 完全从 AST 中隐藏 |
| `@{ ... }` | 原子规则 | 内部视为一个不可分割的 token |
| `!{ ... }` | 推送规则 | 保留所有内部结构 |

### 2.4 区块组织

PEST 文件必须按以下区块顺序组织，区块间用分隔线隔开：

```pest
// =============================================================================
// Shared primitives
// =============================================================================

// -------------------------- basic symbols --------------------------
WHITESPACE = ...
comment_line = ...

// -------------------------- math expressions --------------------------
si_prefix = ...
si_number = ...

// =============================================================================
// File Header Section
// =============================================================================

// ---------------------------- keywords ----------------------------
kw_ibis_ver = ...

// ---------------------------- contents ----------------------------
header_ibis_ver = ...
```

**规则**：
- 区块分隔线：`// =============================================================================`（77 列等号）
- 子区块分隔线：`// -------------------------- name --------------------------`（60 列减号）
- 关键字子区块：`// ============================ keywords ============================`（42 个等号）

### 2.5 规则数量控制

单个 PEST 文件的规则数量不应超过 60 条。当语法复杂度超过此阈值时，必须拆分为多个 `.pest` 文件。

---

## 3. Rust 命名规范

### 3.1 禁止清单

| 禁止形式 | 示例 | 替换 |
|---------|------|------|
| 单字母变量 | `a`, `b`, `t`, `o`, `i`, `j` | `accumulator`, `buffer`, `trimmed_line`, `output`, `row_index`, `column_index` |
| 模糊缩写 | `val`, `tmp`, `kw`, `sec`, `esc` | `value`, `temporary_buffer`, `keyword`, `section_name`, `escape_sequence` |
| 类型名做变量名 | `str`, `vec`, `pair` | `input_string`, `content_lines`, `current_pair` |
| 匈牙利命名 | `str_name`, `i_count` | `name`, `count` |

### 3.2 命名风格

| 类别 | 风格 | 示例 |
|------|------|------|
| 类型 / enum / trait | `PascalCase` | `IbisParser`, `Keyword`, `ParserState` |
| 变量 / 函数 | `snake_case` | `trimmed_line`, `output_buffer`, `extract_value_after_keyword` |
| 常量 | `SCREAMING_SNAKE_CASE` | `FILE_HEADER_KEYWORDS`, `MAX_LINE_LENGTH` |
| 宏 | `snake_case!` | `try_parse!`, `assert_eq!` |

### 3.3 语义化命名规则

变量名必须包含足够的业务上下文，使读者无需查看实现即可理解用途。

```rust
// ✅ 正确：完整语义
let trimmed_line = line.trim();
let closing_bracket_position = trimmed_line.find(']');
let keyword_header_part = &trimmed_line[..=closing_bracket_position];
let remaining_text = after_bracket.trim().to_string();

// ❌ 错误：模糊缩写
let t = line.trim();
let pos = t.find(']');
let head = &t[..=pos];
let rest = after.trim().to_string();
```

**规则**：变量名的作用域越大，命名越应完整。3 行内的短期局部变量可适当精简（如 `pair`），但仍需传达语义。

---

## 4. Rust 控制流与布局规范

### 4.1 match 表达式

禁止将 `match` 分支挤在单行。

```rust
// ✅ 正确
match value {
    Some(inner_value) => {
        let result = process(inner_value);
        result
    }
    None => {
        return default_value;
    }
}

// ❌ 错误
match value { Some(x) => x, None => return None }
```

**规则**：
- `=> {` 后换行
- 多语句分支必须用 `{ }` 包裹
- 单表达式分支若简单（≤40 字符）可省略 `{}`
- 同一 `match` 内风格必须一致

### 4.2 if let / while let

```rust
// ✅ 正确
if let Ok(parsed_pairs) = IbisParser::parse(Rule::keyword_header, keyword_header_part) {
    let first_pair = parsed_pairs.into_iter().next()?;
}
// ❌ 错误
if let Ok(pairs) = IbisParser::parse(Rule::keyword_header, keyword_header_part) { let first = pairs.into_iter().next()?; }
```

### 4.3 循环

```rust
// ✅ 正确
for raw_line in content.lines() {
    let Some(cleaned_line) = clean(raw_line) else {
        continue;
    };
    process_line(cleaned_line);
}

// ❌ 错误
for line in content.lines() { let Some(c) = clean(line) else { continue; }; process(c); }
```

### 4.4 链式调用拆分

任何超过 2 级的方法调用链必须拆分为具名中间变量。

```rust
// ✅ 正确
let parsed_pairs = IbisParser::parse(Rule::keyword_header, keyword_header_part)?;
let first_pair = parsed_pairs.into_iter().next()?;
let keyword_pair = first_pair.into_inner().next()?;
let keyword_name = keyword_pair.as_str().trim();

// ❌ 错误：超过 2 级链式调用
let keyword_name = IbisParser::parse(Rule::keyword_header, keyword_header_part)?
    .into_iter().next()?
    .into_inner().next()?
    .as_str().trim();
```

**规则**：任何包含 `.next()` / `.unwrap()` / `?` 的链条超过 2 级调用，必须拆分为具名步骤。

---

## 5. Rust 类型系统规范

### 5.1 状态机枚举

**严禁**使用 `String` / `&str` 记录解析或运行时状态。所有状态必须定义为强类型 `enum`。

```rust
// ✅ 正确：强类型枚举
#[derive(Debug, Clone, PartialEq)]
enum ParserState {
    /// Currently parsing the file header section.
    InHeader,
    /// Currently parsing a Component section, carrying the component name.
    InComponent(String),
    /// Currently parsing a Model section, carrying the model name.
    InModel(String),
    /// End-of-file marker encountered.
    InEnd,
}

// ❌ 错误：魔术字符串状态机
let mut current_section: String = String::new();
let state = "in_header";
```

**规则**：
- 所有 enum 必须派生 `Debug, Clone, PartialEq`
- 每个变体必须有 `///` 文档注释
- 若变体携带数据，数据字段也必须有文档注释

### 5.2 enum 业务分类

所有业务分类必须使用 `enum`，运行时通过 `match` 分发，禁止字符串比对。

```rust
// ✅ 正确
#[derive(Debug, Clone, PartialEq)]
enum Keyword {
    /// Array-parent container: `[Component]`
    Component,
    /// Array-parent container: `[Model]`
    Model,
    /// Singleton table: `[End]`
    End,
    /// Unrecognized keyword, preserved as-is.
    Other(String),
}

// ❌ 错误：运行时字符串比对
fn is_array_parent(keyword: &str) -> bool {
    keyword == "Component" || keyword == "Model"
}
```

### 5.3 struct 定义

所有 struct 字段必须显式标注类型，禁止依赖类型推断。

```rust
// ✅ 正确：行尾注释，简洁不重复
#[derive(Debug, Clone)]
struct ParsedBlock {
    keyword: String,       // Raw keyword name (e.g., "Component", "IBIS ver").
    rule: Rule,            // Pest rule variant that matched this header.
    content: Vec<String>,  // Content lines belonging to this block.
}

// ❌ 错误：/// 注释冗余，重复字段名，占用额外行
#[derive(Debug, Clone)]
struct Section {
    /// The keyword name.
    keyword: String,
    /// The content lines belonging to this section.
    content: Vec<String>,
}
```

**规则**：
- 字段注释用行尾 `//` 而非字段上方 `///`
- 注释内容应补充字段名无法表达的语义，**禁止重复字段名**
- 必须派生 `Debug`（`Clone` 视情况）
- 字段命名必须完整：`kind` 而非 `k`，`content` 而非 `c`
- `Vec<T>` 字段不包装 `Option`，空 Vec 即表示"无数据"
- 行尾 `//` 注释右对齐（同一结构体内保持一致的缩进位置）

### 5.4 泛型约束

泛型约束使用 `where` 子句放置于函数签名末尾，提升可读性。

```rust
// ✅ 正确
pub fn parse_file<P: AsRef<Path>>(path: P) -> Result<IBIS_File, String>
// 或使用 where
pub fn parse_file<P>(path: P) -> Result<IBIS_File, String>
where
    P: AsRef<Path>,
```

### 5.5 newtype 模式

对于有特殊约束的原始类型，使用 newtype 模式包裹以增强类型安全。

```rust
// ✅ 正确
/// A TOML-safe, double-quoted string value.
struct TomlString(String);

impl From<&str> for TomlString {
    fn from(raw: &str) -> Self {
        TomlString(format!("\"{}\"", raw.replace('\\', "\\\\").replace('"', "\\\"")))
    }
}
```

---

## 6. Rust 函数规范

### 6.1 函数命名动词表

| 动作 | 前缀 / 动词 | 示例 |
|------|-----------|------|
| 解析 | `parse_` | `parse_header_line()` |
| 转换 | `to_` / `as_` / `from_` | `ibs2toml()`, `as_str()` |
| 提取 | `extract_` | `extract_value_after_keyword()` |
| 判断 | `is_` / `has_` / `contains_` | `is_continuation_line()`, `is_array_parent()` |
| 分类 | `classify_` | `classify_keyword()` |
| 构建 | `build_` / `construct_` | `build_tree()` |
| 序列化 | `serialize_` / `write_` | `serialize_section()` |
| 转换状态 | `transition_to_` | `transition_to_header_state()` |

**禁止**：
- 无动词开头（`keyword_classify()` — 应为 `classify_keyword()`）
- 模糊命名（`process()`, `handle()`, `do_something()`）
- 非标准缩写（`split_kw` — 应为 `split_keyword` 或 `extract_keyword_and_rest`）

### 6.2 参数命名

```rust
// ✅ 正确：参数名自文档化
pub fn parse_header_line(line: &str) -> Option<(&'static str, String)>
fn serialize_section(section: &Section, parent_path: &str, output_buffer: &mut String)
fn collect_child_sections(
    blocks: &[(Keyword, Vec<String>)],
    start_index: usize,
) -> (Vec<Section>, usize)

// ❌ 错误：参数名无含义
fn parse_header_line(s: &str) -> Option<(&'static str, String)>
```

### 6.3 返回值规范

- 可能失败的函数必须返回 `Result<T, E>` 或 `Option<T>`
- 禁止通过哨兵值（`-1`, `null`, 空字符串）表示错误
- 复杂返回值应使用具名元组或结构体

```rust
// ✅ 正确：具名元组
fn collect_child_sections(...) -> (Vec<Section>, usize)

// ❌ 错误：通过注释约定（读者必须看实现才知道）
fn collect_child_sections(...) -> Vec<Section>  // 最后一个元素是 next_index
```

---

## 7. Rust 文档与注释规范

### 7.1 函数文档注释（`///`）— Rustdoc 标准 + 定制扩展

严格遵循 [Rust RFC 1574] 与 [API Guidelines] 的 rustdoc 风格，并在此基础上增加定制化的 `# Parameters` / `# Returns` 区块，以提升参数可读性。

所有 `pub` 函数必须包含以下结构：

```rust
/// One-line description of the function's purpose (Brief).
///
/// Extended description. Explain what the function does, including any
/// important details about its behavior, preconditions, or side effects.
/// Use backticks for code references: [`build_tree`], [`Section`], [`Keyword`].
///
/// # Parameters
///
/// * `content` — Description of the first parameter. Explain what it
///   represents, expected format, and any constraints or boundary values.
/// * `parent_path` — Description of the second parameter.
///
/// # Returns
///
/// * `Ok(String)` — The TOML-formatted output string. Contains the fully
///   converted representation of the input IBIS content.
/// * `Err(String)` — A human-readable error message describing why parsing
///   or conversion failed.
///
/// # Errors
///
/// Returns `Err` if the input is not valid IBIS syntax, if a required
/// keyword is missing, or if numeric conversion fails.
///
/// # Panics
///
/// Does not panic under normal operation. Panics only if the internal
/// assertion on section nesting depth is violated (a programming error).
///
/// # Examples
///
/// ```ignore
/// let result = ibs2toml("[IBIS ver] 2.1").unwrap();
/// assert!(result.contains("ibis_ver"));
/// ```
pub fn ibs2toml(content: &str) -> Result<String, String>
```

### 7.2 文档章节顺序

区块必须严格按以下顺序排列：

```
/// 1. Brief — 一行函数功能摘要。
/// 2. (blank line)
/// 3. Extended description — 详细描述（可选，可跨多段）。
/// 4. # Parameters — 列出所有参数（`pub` 函数有参数时**强制**）。
/// 5. # Returns — 说明返回值含义；若为 `Result` / `Option` 需说明内部包裹数据（`pub` 函数**强制**）。
/// 6. # Errors — 说明何时返回 `Err`（函数返回 `Result` 时**强制**）。
/// 7. # Panics — 说明何时会 panic（函数可能 panic 时**强制**）。
/// 8. # Safety — 仅 `unsafe` 函数**强制**。
/// 9. # Examples — 至少一个完整、可运行的代码示例（**推荐**，解析/转换函数**强烈建议**）。
```

### 7.3 内部注释（`//`）

```rust
// ── Phase A: collect file header fields into [File_Header] ──
// ── Phase B: process remaining blocks ──
```

**规则**：
- 注释用 `//` 而非 `//!`
- 使用全角破折号 `──` 包裹阶段标题，使其在视觉上突出
- 复杂算法必须在代码块上方用注释说明思路和引用来源

### 7.4 区块分隔线

```rust
// =============================================================================
// Public API
// =============================================================================

// ---------------------------------------------------------------------------
// File header parsing
// ---------------------------------------------------------------------------
```

### 7.5 TODO / FIXME 注释

```rust
// TODO(#issue_number): description of what needs to be done
// FIXME: description of known issue and why it exists
```

所有 TODO 必须关联 Issue 编号或明确的责任人。

### 7.6 常量与类型文档

```rust
/// Keywords that belong to the IBIS file header section.
///
/// All such fields are grouped under `[File_Header]` in the TOML output.
const FILE_HEADER_KEYWORDS: &[&str] = &[
    "IBIS ver",
    "Comment Char",
    "File name",
    "File Rev",
    "Date",
    "Source",
    "Notes",
    "Disclaimer",
    "Copyright",
];
```

### 7.7 定制规则总结 — 函数级文档 `///` 速查

为便于快速记忆，以下总结全部 Rust 文档规范要点：

| 规则 | 说明 |
|------|------|
| 格式 | `///` 注释，第一行为 Brief，空行后接详细描述 |
| `# Parameters` | 用无序列表列出所有参数：`` * `name` — description `` |
| `# Returns` | 说明返回值含义；`Result` 需分别说明 `Ok` 和 `Err` |
| `# Errors` | 返回 `Result` 时必须说明 `Err` 条件 |
| `# Panics` | 函数可能 panic 时必须说明触发条件 |
| `# Examples` | 提供至少一个完整可运行示例，用代码块包裹 |
| 内联代码 | 正文中所有变量名、类型名、函数名用反引号包裹 |
| `# Arguments` | 已被 `# Parameters` 替代，不再使用 |

> **冲突处理**：如果本条规范（7. Rust 文档与注释规范）与文件中其他部分的规范冲突，**一律以本条为准**。
>
> 另见：[7.8 Crate / Module 级别文档（`//!`）](#78-crate--module-级别文档-front-page-标准)

### 7.8 Crate / Module 级别文档（`//!`）— Front-Page 标准

#### 7.8.1 作用范围与语法规约

| 位置 | 注释标记 | 用途 | 生成到 rustdoc 哪个页面 |
|------|---------|------|------------------------|
| `lib.rs` (crate 根) | `//!` | Crate 首页（front-page） | crate 的顶层文档页 |
| `mod.rs` 或 `{module}.rs` | `//!` | 模块概览文档 | 该模块的文档页 |
| 非 `lib.rs`/`mod.rs` 的普通 `.rs` 文件 | `//!` | 文件级职责说明 | 所属模块文档页内 |

**与 Section 1.2 文件头模板的区别**：
- Section 1.2 的 `// === ... ===` 是**内部注释**，仅开发者在 IDE/源码中可见，不参与 rustdoc 生成
- 本节 `//!` 是**Rust 文档注释**，会出现在 `cargo doc` 生成的 HTML 文档中，面向库的用户
- 两者可以并存：`//!` 文档注释在上方，`// ===` 内部注释在下方

#### 7.8.2 Crate 根文档（`lib.rs`）— Front-Page 结构

`lib.rs` 中的 `//!` 注释构成 crate 的首页。官方 rustdoc 推荐按以下层次逐步丰富：

**第一层 — 起步版（必须）**

```rust
//! IBIS Parser — Parse IBIS chip model description files and convert them to TOML.
//!
//! This library provides a complete IBIS 7.0 parsing pipeline, including lexical
//! analysis, syntax analysis, semantic construction, and TOML serialization.
```

**第二层 — 增补 `# Examples`（推荐）**

在第一层基础上，增加一个完整、可运行的使用示例。示例不应使用捷径，应完整展示从 `use` 到调用的全过程，方便用户直接复制运行：

```rust
//! IBIS Parser — Parse IBIS chip model description files and convert them to TOML.
//!
//! # Examples
//!
//! Basic usage: parse IBIS file content and output TOML.
//!
//! ```rust
//! use ibis_parser::ibs2toml;
//!
//! let ibis_content = "\
//! [IBIS ver] 7.0
//! [Component] TEST_CHIP
//! Manufacturer Test Corp
//! [End]
//! ";
//!
//! let toml_output = ibs2toml(ibis_content).expect("parsing failed");
//! assert!(toml_output.contains("ibis_ver"));
//! println!("{}", toml_output);
//! ```
```

**第三层 — 增补 `# Features` / 功能清单（成熟期）**

当 crate 发展到较成熟阶段，应在首页列出所有功能特性和可选 feature flag：

```rust
//! IBIS Parser — Parse IBIS chip model description files and convert them to TOML.
//!
//! # Examples
//!
//! ...（same as above）
//!
//! # Features
//!
//! This crate provides:
//!
//! - Full IBIS 7.0 grammar parsing (generated via PEST)
//! - Complete pipeline: lexical analysis → syntax analysis → semantic construction
//! - TOML serialization output
//! - Support for all standard sections: `Component`, `Model`, `Pin`, etc.
//!
//! Optional Cargo features:
//!
//! - `serde` (default): Enable serialization support
//! - `strict`: Enable strict mode, error on non-standard IBIS files
```

#### 7.8.3 首页编写原则

参照 Rust 官方推荐的渐进式策略：

| 原则 | 说明 |
|------|------|
| **一句话定位** | 首页第一行必须让用户立刻知道这个 crate 是做什么的、在 Rust 生态中的位置 |
| **示例先行** | `# Examples` 区块必须展示最核心的用例，使用完整代码块（可复制粘贴运行） |
| **逐步充实** | 不需要一步到位。从简介 + 示例开始，随项目成熟逐步增加 Features、参考链接、边界说明 |
| **面向用户** | 站在库的使用者视角表达，而非实现者。避免暴露内部实现细节 |
| **内联注释辅助** | 对于复杂示例，使用行内注释逐行解释（参考 `futures` crate 的做法） |

#### 7.8.4 模块级文档

每个模块文件（`mod.rs` 或 `{module}.rs`）应在文件顶部使用 `//!` 说明该模块的职责、输入/输出约定以及与其他模块的关系：

```rust
//! Lexical analysis module — Tokenize raw IBIS text into tagged tokens line by line.
//!
//! # Responsibilities
//!
//! - Strip comment lines (starting with the `Comment Char`) and blank lines
//! - Recognize `[Keyword]` markers and classify them into the [`Keyword`] enum
//! - Handle continuation lines (starting with `|`), folding them into complete logical lines
//! - Produce `(Keyword, Vec<String>)` block tuples for the syntax analysis module
//!
//! # Input / Output
//!
//! | Input | Output |
//! |-------|--------|
//! | Raw IBIS file text (`&str`) | `Vec<(Keyword, Vec<String>)>` |
//!
//! # Related modules
//!
//! - [`super::syntax_analy`] — consumes this module's output to build the Section tree
//! - [`super::core`] — top-level orchestrator, entry point into this module
```

**规则**：
- 每个 `pub mod` 文件**必须**有 `//!` 文档注释
- 非公开模块（`mod` 不带 `pub`）**建议**添加 `//!` 文档
- 模块文档应包含：一句话职责描述、核心输入/输出约定、关联模块的交叉引用

#### 7.8.5 `//!` 与 `//` 的分工关系

```
//! This is a rustdoc doc-comment. It appears in the HTML generated by `cargo doc`.
//! It is intended for library users, describing the public API, usage examples,
//! and design intent.
//!
//! The `=====` block below is an internal comment, visible only in source code,
//! intended for maintainers.

// =============================================================================
// Internal implementation notes — visible only in source code.
// =============================================================================
```

**规则**：
- `//!` 放在文件头部，先于任何 `use` 语句和 `// ===` 内部注释
- `// ===` 内部注释放在 `//!` 之后
- `//!` 内容应保持稳定、精炼，随公共 API 变化而更新
- `// ===` 内部注释可包含实现细节、TODO、维护备注等

---

## 8. Rust 错误处理规范

### 8.1 错误类型

```rust
// ✅ 正确：语义化错误信息
pub fn parse_header_line(line: &str) -> Option<(&'static str, String)>
pub fn ibs2toml(content: &str) -> Result<String, String>

// ✅ 正确：携带上下文信息
Err(format!("Unrecognized keyword at line {}: '{}'", line_number, raw_keyword))
```

**规则**：
- 生产代码**禁止**使用 `.unwrap()`（仅测试代码允许）
- 禁止使用 `.expect("")` 但不提供有意义的 panic 信息
- 优先使用 `Result<T, String>` 保持简单，复杂场景可定义 `Error` enum

### 8.2 错误传播

```rust
// ✅ 正确：使用 ? 操作符传播错误
fn extract_keyword_from_line(line: &str) -> Option<(Keyword, Option<String>)> {
    let closing_bracket_position = trimmed_line.find(']')?;  // None → 提前返回
    // ...
}
```

---

## 9. 测试规范

### 9.1 测试文件组织

| 测试类型 | 位置 | 命名 |
|---------|------|------|
| 单元测试 | 源文件末尾 `#[cfg(test)]` 模块内 | 随源文件 |
| 集成测试 | `tests/` 目录 | `{module}_test.rs` |

### 9.2 测试函数命名

```rust
// ✅ 正确：test_ + 被测函数 + _ + 具体场景
#[test]
fn test_parse_ibis_ver() { ... }
#[test]
fn test_parse_file_name() { ... }
#[test]
fn test_clean_removes_comment_bars_outside_brackets() { ... }
#[test]
fn test_clean_preserves_bars_inside_brackets() { ... }
#[test]
fn test_keyword_classify_known() { ... }
#[test]
fn test_keyword_classify_unknown() { ... }

// ❌ 错误：无意义或过于宽泛的命名
#[test]
fn test1() { ... }
#[test]
fn test_clean() { ... }  // 太模糊，应描述具体场景
```

### 9.3 测试断言

```rust
// ✅ 正确：使用语义化断言 + 错误消息
assert!(!header.ibis_ver.is_empty(), "IBIS ver should not be empty");
assert_eq!(keyword, Keyword::Component);
assert!(result.contains("[[Component]]"), "TOML output should contain array parent header");

// ❌ 错误
assert!(header.ibis_ver != "");       // 缺少错误消息
assert_eq!(true, result.is_ok());     // 反模式：直接 assert!(result.is_ok())
```

### 9.4 测试数据

- 测试数据文件存放在 `tests/examples/` 目录
- 使用相对路径引用测试数据
- 复杂解析测试应打印中间结果以便人工审查

---

## 10. 模块组织规范

### 10.1 导入顺序

导入必须按以下顺序分组，组间空一行：

```rust
// 1. 标准库
use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::Path;

// 2. 外部 crate
use pest::iterators::Pairs;
use pest::Parser;

// 3. 内部模块
use crate::ibis2ibstoml::parser::IbisParser;
use crate::ibis2ibstoml::parser::Rule;
```

### 10.2 循环依赖禁止

- 第一层模块**禁止**依赖第二层模块的内部类型
- 模块间通过公共 API 函数通信
- 文件模块只导出一个公共结构体 / 函数时，使用 `pub use` 重导出

---

## 11. AI 协作契约

> 以下规则专为人与 AI 协作时设计，确保 AI 生成的代码行为可预测、风格一致。

### 11.1 AI 代码生成检查清单

当 AI 生成或修改代码时，必须按以下顺序自查：

- [ ] 是否引入了单字母变量？（如 `a`, `b`, `i`, `t` → 必须替换为完整语义名）
- [ ] 是否有超过 2 级的 `.next()` / `.unwrap()` / `?` 链式调用？（→ 必须拆分为具名变量）
- [ ] match / if-let / 循环体是否挤在单行？（→ 必须展开）
- [ ] 是否用了 `String` 做状态标记？（→ 必须改为 `enum`）
- [ ] PEST 规则是否有多层 `~` / `|` 挤在单行？（→ 必须换行对齐）
- [ ] 函数名是否有模糊缩写？（如 `kw`, `sec`, `esc` → 必须展开）
- [ ] 返回值是否为 `Option` 或 `Result`？（→ 禁止用哨兵值）
- [ ] 所有 `pub` 函数是否有完整的 `///` 文档注释（含 `# Parameters`, `# Returns`, `# Errors`, `# Panics`）？（→ 必须添加）
- [ ] 每个 `pub mod` 文件是否有 `//!` 模块级文档？（→ 必须添加）
- [ ] `lib.rs` 是否有 `//!` crate 首页文档？（→ 必须添加）
- [ ] 是否遵循了分层隔离原则？（→ 第一层不做语义分析）
- [ ] enum / struct 字段是否都有文档注释？（→ 必须添加）

### 11.2 AI 代码修改规则

1. **最小改动原则**：AI 应尽量保留原有代码结构，只修改不符合规范的局部，不应大规模重构未涉及的代码段
2. **命名修改必须全局一致**：若重命名函数或变量，必须同时更新所有调用点和测试引用
3. **新增代码继承风格**：AI 新增的函数 / 类型必须遵循文件中已有的命名风格和结构模式
4. **测试先行**：AI 新增任何解析 / 转换逻辑前，应先找到或创建对应的测试用例

### 11.3 代码审查优先级

审查代码时按此优先级检查：

| 优先级 | 类别 | 检查项 |
|--------|------|--------|
| P0 | 正确性 | 类型安全、错误处理、边界条件 |
| P1 | 规范遵守 | 以上所有章节的硬性规则 |
| P2 | 可读性 | 命名清晰度、注释完整性 |
| P3 | 性能 | 不必要的分配、克隆、遍历 |

---

## 附录 A：规范术语表

| 术语 | 含义 | 规范命名 |
|------|------|---------|
| Keyword | IBIS 文件中的 `[Section Keyword]` | `Keyword` (enum) |
| Section | 一个关键词块及其内容 | `Section` (struct) |
| Block | 解析阶段的 `(Keyword, Vec<String>)` 元组 | `block` (变量) |
| Corner Value | typ/min/max 三元组 | `CornerValue` / `Triplet<T>` |
| Array Parent | 可包含子节段的容器关键词 | `is_array_parent()` |
| Continuation | 以 `|` 开头的续行 | `is_continuation_line()` |

## 附录 B：速查卡

```rust
// ──── 变量命名 ────
let trimmed_line = line.trim();              // ✅ 完整语义
let t = line.trim();                          // ❌ 单字母

// ──── 链式调用 ────
let first_pair = pairs.into_iter().next()?;  // ✅ 具名中间变量
let value = pairs.into_iter().next()?.into_inner().next()?;  // ❌ 超过 2 级

// ──── 状态机 ────
enum ParserState { InHeader, InModel(String) }  // ✅ 强类型
let state = "in_header";                         // ❌ 魔术字符串

// ──── 控制流 ────
match value {
    Some(x) => {
        process(x)
    }
    None => {
        return None;
    }
}
match value { Some(x) => x, None => return None }  // ❌ 单行

// ──── PEST 多行 ────
si_number = @{                                // ✅ 换行对齐
    ("+" | "-")? ~
    (ASCII_DIGIT+ ~ "." ~ ASCII_DIGIT*)
};
si_number = @{ ("+" | "-")? ~ (ASCII_DIGIT+ ~ "." ~ ASCII_DIGIT*) };  // ❌ 单行
```
