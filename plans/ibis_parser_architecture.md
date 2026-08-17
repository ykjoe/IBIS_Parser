# ibis_parser 架构书（根包 · 强类型 re-export 兼容层）

> **本文档定位**：描述根包 `ibis_parser`（Cargo workspace 根）的**当前角色**。
> 语义层实现已迁入子 crate `ibis2ibstoml` 的 backend（见 [`ibis2ibstoml_architecture.md`](ibis2ibstoml_architecture.md:1)），
> 根包退化为**强类型 re-export 兼容层**：不再承载语义解析逻辑，仅 re-export 强类型 AST 并保留既有引用路径。

---

## 1. 定位

`ibis_parser` 是仓库**根包**（`Cargo.toml` 中 `[package] name = "ibis_parser"`），同时也是 Cargo **workspace 根**（`members = ["crates/ibis2ibstoml"]`）。

**当前角色**：强类型 AST 的 re-export 兼容层。

- 语义层（数值转换 / 单位缩放 / 语义映射 / 校验）由 `ibis2ibstoml::backend` 实现
- 根包通过 `pub use ibis2ibstoml::backend::ibis_structure;` 保留旧引用路径 `ibis_parser::ibis_structure::*`
- 顶层仍 `pub use ibis2ibstoml;`，兼容 `ibis_parser::parse_to_toml` 等入口

## 2. 现状

| 项 | 内容 |
|----|------|
| 模块声明 | [`src/ibis_parser/mod.rs`](src/ibis_parser/mod.rs:8) 仅 `pub use ibis2ibstoml::backend::ibis_structure;` |
| 数据结构 | 强类型 AST 定义迁入 [`crates/ibis2ibstoml/src/backend/ibis_structure.rs`](../crates/ibis2ibstoml/src/backend/ibis_structure.rs:1)（595 行，IBIS 7.0 强类型 AST 定义） |
| 结构参考 | [`plans/ibis_struct.toml`](ibis_struct.toml:1) 描述各节段的字段与类型映射 |
| 根 lib 导出 | [`src/lib.rs`](src/lib.rs:29) `pub mod ibis_parser;` + `pub use ibis2ibstoml;` |
| CLI 入口 | [`src/main.rs`](src/main.rs:8) 调用 `ibis2ibstoml::ibs2ibstoml` |

**根包依赖**：`tauri` / `serde` / `serde_json` / `toml` / `ibis2ibstoml`（path 依赖）。`toml` / `serde` crate 保留，供将来根包侧做**消费方**（如把 `ibis2ibstoml` 产出的 TOML 反序列化到强类型）使用；是否使用待决策。

## 3. 强类型数据结构概览

定义于 [`ibis2ibstoml::backend::ibis_structure`](../crates/ibis2ibstoml/src/backend/ibis_structure.rs:1)，全部为 `#[derive(Debug)]` 类型，标注 `#![allow(non_camel_case_types)]`：

- **根**：`IBIS_File`（header + components + models + submodels + external_circuits + test_data + test_loads + package_models + interconnect_model_sets 等）
- **文件头**：`IBIS_FileHeader`
- **组件**：`IBIS_Component` 及 `PinInfo` / `PinMapping` / `DiffPin` / `BeginEmiComponent` 等子结构
- **模型**：`IBIS_Model` 及 `ModelSpec` / `ReceiverThresholds` / `Ramp` / `WaveformFixture` 等
- **其他**：`IBIS_ModelSelector` / `IBIS_Submodel` / `IBIS_ExternalCircuit` / `IBIS_TestData` / `IBIS_TestLoad` / `IBIS_DefinePackageModel` / `IBIS_InterconnectModelSet`
- **通用容器**：`Triplet<T>`（typ/min/max）、`IBIS_CornerValue`、`IBIS_TableData`

> 类型命名采用下划线风格（`IBIS_File`），与 [`ibis_struct.toml`](ibis_struct.toml:1) 中 PascalCase 的 `[IBISFileHeader]` 等存在差异，属**待统一/待决策**项之一。

## 4. 与 ibis2ibstoml 的边界

- `ibis2ibstoml`：**完整流水线**（文本 → `SectionNode` 树 → 强类型 `IBIS_File` → TOML），含语义层 backend
- `ibis_parser`：**re-export 兼容层**，不承载解析 / 转换逻辑；保留 `ibis_parser::ibis_structure` 引用路径

两层通过公共 API 通信：根包 re-export 子 crate 的强类型与顶层入口。

## 5. 开放问题（未决策）

> 随 backend 语义层迁入子 crate，原"输入形式 / array-of-tables / 数值转换归属"等开放问题已收敛；剩余待定项如下。

- [ ] 强类型序列化是否启用 serde（依赖中已有 `serde` / `toml`）：若根包需把 TOML 反序列化回强类型，则需给 `ibis2ibstoml` 的强类型加 serde derive
- [ ] 类型命名统一（`IBIS_File` vs `IBISFile`）与 `ibis_struct.toml` 参考的对齐策略
- [ ] 顶层 `parse_to_toml` 返回类型是否升级为携带结构化 `SemanticError`（当前建议保持 `String` 以兼容）

## 6. 明确不做（暂定）

- 不在根包做文本解析 / 语义解析（那是 `ibis2ibstoml` 的职责）
- 不重复维护关键词集合 / pest 语法（统一复用 `ibis2ibstoml` 的产物）
- 不重复维护强类型定义（re-export 子 crate）
