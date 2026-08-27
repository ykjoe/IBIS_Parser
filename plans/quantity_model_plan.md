# IBIS→TOML 语义模型重设计：保留「数值 + 单位」（Quantity / Ratio / Triplet / Table）

> 状态：设计稿，待评审确认后实施
> 范围：ibis2ibstoml 语义层重写（数据模型 / 解析规则 / 解释器 / emitter / validation / 黄金文件）

---

## 1. 背景与问题

本项目是 IBIS → TOML 的**文本转换管线**，不是单位换算器。但当前实现把所有带工程单位的文本归一化为 SI 基单位 `f64`：

- `2.69p` → `2.69e-12`
- `10mA` → `0.01`
- `1.9/597p` → `1.9 / 5.97e-10`

后果：

1. **丢失原文信息**：原始单位（`p` / `mA` / `k`）与数值书写形式被抹掉，无法还原。
2. **破坏 TOML 可读性**：黄金文件里出现大量长尾浮点，例如
   ```toml
   c_comp = { typ = 0.00000000000269, min = 0.00000000000229, max = 0.0000000000030699999999999996 }
   ```
   完全无法直接对照 IBIS 原文阅读。
3. **单元换算不属于本工具职责**：单位归一是后续消费方（仿真工具）的职责，转换器应保真输出。

## 2. 目标与原则

- **保真**：保留原始「数值 + 单位」；不做任何归一化换算。
- **可读**：`.ibs.toml` 应能直接对照 IBIS 原文（`2.69p` 就是 `{ value = 2.69, unit = "p" }`）。
- **统一**：用少数基础类型替代原来杂多的值类型（`f64` / `Option<f64>` / `IBISCornerValue` / `ViPoint` / `IBISTableData`）。
- **保留校验语义**：数值性校验（corner 顺序、IV 单调）改为**按需解析比较**，不改变存储形态。

## 3. 新数据模型（schema 类型层）

定义位置：`crates/ibis2ibstoml/src/schema/keyword_hierarchy.rs`（schema 只提供类型信息）。

```rust
/// 比值（分式）：分子 / 分母，各自为带单位的量；保留原文不计算。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ratio {
    pub numerator: Quantity,
    pub denominator: Quantity,
}

/// 量的数值形态：普通数或比值。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Scalar {
    Number(f64),
    Ratio(Ratio),
}

/// 带单位的量：保留原始数值与单位（不做归一化换算）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quantity {
    pub value: Scalar,
    /// 原始单位（原样保留：`"p"` / `"mA"` / `"nH"` / `"V"` / `"k"` ...）；无单位 → None。
    pub unit: Option<String>,
}

/// 角点三元组：typ 必填，min / max 可选（可编码为 Table，见 §6）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Triplet {
    pub typ: Quantity,
    pub min: Option<Quantity>,
    pub max: Option<Quantity>,
}

/// 表：列头 + 真矩阵。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub col_header: Vec<String>,
    /// 矩阵每个元素为一个单元格（数值列 → Quantity；标识符列 → Text，见 §6）。
    pub matrix: Vec<Vec<Cell>>,
}

/// 单元格：数值量或文本（用于含标识符列的混合表，如 Pin）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Cell {
    Quantity(Quantity),
    Text(String),
}
```

> `Cell` 是为 Pin 这类「标识符列 + 数值列」混合表引入的（详见 §6 决策点 D1）。

### TOML 序列化形态（示意）

```toml
# 单值 Quantity（无单位）
r_load = { value = 1.0, unit = "k" }

# 角点 Triplet
c_comp = { typ = { value = 2.69, unit = "p" }, min = { value = 2.29, unit = "p" }, max = { value = 3.07, unit = "p" } }

# 曲线 Table（纯数值矩阵）
[Model.Pulldown]
col_header = ["voltage", "i_typ", "i_min", "i_max"]
matrix = [
  [ { value = -3.3 }, { value = -2, unit = "mA" }, { value = -2, unit = "mA" }, { value = -1, unit = "mA" } ],
  [ { value = 0.0 },  { value = 0,  unit = "mA" }, { value = 0,  unit = "mA" }, { value = 0,  unit = "mA" } ],
]

# 比值（Clamp 分式）
dv_dt_r = { typ = { value = { ratio = { numerator = { value = 1.9 }, denominator = { value = 597, unit = "p" } } } } }
```

> Ratio 的具体 TOML 渲染（serde 内外标签）为实现期微调项，逻辑结构如上。

## 4. 解析规则（`crates/ibis2ibstoml/src/backend/rules.rs`）

在保留现有 `parse_si_number` / `parse_opt_number`（降级为**校验/比较专用**，仅按需归一化）之外，新增：

```rust
/// 从原始文本提取 Quantity，不换算：
///   "2.69p"   → Quantity { value: Number(2.69), unit: Some("p") }
///   "1.0000k" → Quantity { value: Number(1.0),  unit: Some("k") }
///   "3.3"     → Quantity { value: Number(3.3),  unit: None }
///   "1.9/597p"→ Quantity { value: Ratio(Ratio{ numerator: Q(1.9), denominator: Q(597,"p") }), unit: None }
pub fn parse_quantity(text: &str) -> Result<Quantity, String>;

/// 校验/比较用：把 Quantity 归一化为 f64（不做存储，仅用于 corner 顺序 / IV 单调性等数值比较）。
pub fn quantity_to_f64(q: &Quantity) -> Option<f64>;
```

约定：

- 数值部分可解析 → `Scalar::Number`；含 `/` 且两侧各可解析 → `Scalar::Ratio`。
- `NA` / 空 → 由调用方（解释器）映射为 `None`（缺失），不产生 Quantity。
- `unit` = 数值后原始后缀原样保留（`p` / `mA` / `nH` / `k` / `Ohm` ...），不做单位合法性校验（那是校验层职责）。
- `parse_si_number` / `parse_opt_number` 保留但只服务于 validation 的按需比较，不再作为存储路径。

## 5. 领域模型映射（`crates/ibis2ibstoml/src/schema/keyword_hierarchy.rs`）

| 现状类型 | 现状字段示例 | 新类型 |
|---|---|---|
| `String` / `Option<String>` | `model` / `model_type` / `polarity` / `file_name` / 各种引用名 | 不变（标识符 / 自由文本） |
| `Option<f64>` | `vinl` / `vinh` / `vmeas` / `vref` / `rref` / `cref` / `r_load` / `r_pin` / `l_pin` / `c_pin` / `r_fixture` / `v_fixture` ... | `Option<Quantity>` |
| `IBISCornerValue` | `c_comp` / `voltage_range` / `temperature_range` / `r_pkg` / `l_pkg` / `c_pkg` / `dv_dt_r` / `dv_dt_f` / 各 `*_reference` | `Triplet`（或其 `Option<Triplet>`） |
| `Vec<ViPoint>` | `pulldown` / `pullup` / `gnd_clamp` / `power_clamp` / `series_current` / 各 waveform 曲线 | `Option<Table>`（纯数值矩阵） |
| `IBISTableData` | `POWER Table` / `GND Table` / `composite_current` | `Option<Table>` |
| 行式表结构体（`PinInfo` / `PinMapping` / `DiffPin` ...） | `Pin` / `Pin_Mapping` / `Diff_Pin` | 保留**行结构体**，但其中数值字段改为 `Option<Quantity>`（见 §6 决策点 D2） |

要点：

- **不再有「零字符串污染」式的强制归一化**；`de_opt_f64` / `de_vi_points` 等旧的「文本 → f64」serde 辅助将被移除或改写（解释器直接产出结构化 `Quantity`/`Triplet`/`Table` 的 toml 形态，serde 直接绑定）。
- `IndexMap` 符号表（`models` / `submodels` / `test_loads` / `package_models`）结构不变。

## 6. Triplet 编入 Table 的统一方案（回答开放问题）

IBIS 中「三元组（typ min max）」与「表」在形态上难以预判。统一约定：

**角点 / 三元组字段在领域模型里保留为 `Triplet`（语义清晰），同时提供双向编码到 `Table` 的能力：**

```rust
impl Triplet {
    /// 把三元组编码为 Table：col_header = ["typ", "min", "max"]（按出现列裁剪），一行。
    fn as_table(&self) -> Table;
    /// 从 Table 识别/提取三元组（col_header 命中 typ/min/max 的 1 行）。
    fn from_table(t: &Table) -> Option<Triplet>;
}
```

- 纯数值、含标识符列的表格 → 一律 `Table`；`Triplet` 只是「typ/min/max」常见形态的便捷类型。
- 需要时（例如按 `col_header` 通用渲染或后续工具消费）可用 `as_table()` 把三元组当作一张表处理，从而“编入 table”。

**决策点 D1 —— 混合表的单元格类型**：Pin 含 `pin_name` / `signal_name` / `model_name` 等标识符列，不能全塞进 Quantity。方案：

- (a) 引入 `Cell = Quantity | Text`，`Table.matrix` 元素为 `Cell`（本设计采用，见 §3）。
- (b) 保持 Pin 等为「行结构体 + `Option<Quantity>` 数值字段」，不套泛化 Table。

**决策点 D2 —— 行结构体去留**：推荐保留 `PinInfo` / `PinMapping` / `DiffPin` 等**行结构体**（其数值字段改 `Option<Quantity>`），而不是把 Pin 泛化成 `Table`——因为标识符列 + 数值列的混合用结构体表达更清晰；纯数值/曲线/表格类（Pulldown、POWER Table）才用 `Table`。若倾向完全泛化（一切皆 Table + Cell），也可行，需在 D1/D2 一并确认。

## 7. 解释器（`crates/ibis2ibstoml/src/backend/symbol_table_build.rs`）

- `rules::parse_quantity` 取代对 `parse_opt_f64` 的依赖（`parse_opt_f64` 仅剩校验用途）。
- 产出结构化 toml 形态：
  - `corner_value` → `{ typ = {value, unit}, min = ..., max = ... }`；
  - `curve_value` / `table_value` → `{ col_header = [...], matrix = [[ {value, unit} | "text", ...], ...] }`；
  - 字段节段 key=数值 → `Quantity`；key=标识符 → `String`。
- 形态判定从「corner / curve / table / field / text 启发式」收敛为：有列 → Table；key=数值 → Quantity；标识符 → String；`"value"` 占位（typ/min/max 形态）→ Triplet。
- `record_invalid_number` / `numeric_keys` 相关逻辑随 serde 契约移除而简化（无效数值以 `Err` 上抛或宽松占位，语义保持现状）。

## 8. Emitter（`crates/ibis2ibstoml/src/emitter/toml.rs`）

- `emit_f64` / `emit_opt_f64` → `emit_quantity`（`{ value = <n>, unit = "<u>" }`，无单位省略 unit）。
- `emit_corner` → 内联 `{ typ = {...}, min = {...}, max = {...} }`。
- `emit_vi_points` / `emit_table` → 单表 `[Section]` + `col_header` + `matrix`（`[[...]]` 逐行形态改为单表矩阵）。
- 输出示例见 §3。

## 9. Validation（`crates/ibis2ibstoml/src/backend/validation.rs`）

现状：validator 声明式（必填、corner 范围 min≤typ≤max、IV 电压单调）+ 符号引用检查。

推荐方案（保持语义，不存归一化）：

- **结构校验**（validator）不变：必填字段、非空等，作用于 `String` / `Option<Quantity>` 的存在性。
- **数值比较校验**改为**按需解析**：`rules::quantity_to_f64` 临时归一化比较，不改变存储形态：
  - `validate_corner`：typ/min/max 各 `quantity_to_f64` 后比较 min ≤ typ ≤ max；比值（Ratio）不可比较时跳过。
  - `validate_vi_points`：取 voltage 列各 `quantity_to_f64` 判严格递增；`NA` / 比值跳过。
- **符号引用检查**不变（仍是纯结构）。

备选（若需先快速上线）：validation 先降级为**纯结构校验**（必填 + 引用），数值比较后续再接；validator 数值规则暂缓。文档建议采用推荐方案。

## 10. 黄金文件迁移（`tests/examples/*.ibs.toml`）

- 全部 5 个黄金文件（`cyclone2` / `f103c8` / `invchain_test_0614` / `u26a_800` / `virtex5`）输出将**彻底变化**，需重新生成。
- 迁移方式：
  1. 用新 emitter 重新生成各 `.ibs.toml`；
  2. 人工抽查关键段（corner、曲线、比值、Pin 表）确认可读性与保真；
  3. `examples_compat_test.rs` 继续做逐字节比对（比对新生成的黄金文件）。
- 建议保留一份「旧→新」对照说明（本文件 §3 示例即对照素材）。

## 11. 实施步骤（建议顺序）

1. **类型层**：在 `keyword_hierarchy.rs` 定义 `Scalar` / `Quantity` / `Ratio` / `Triplet` / `Table` / `Cell` + serde。
2. **解析规则**：`rules.rs` 新增 `parse_quantity` / `quantity_to_f64`；`parse_opt_f64` 保留为校验专用。
3. **领域模型替换**：按 §5 映射表替换各字段类型；移除 `IBISCornerValue` / `ViPoint` / `IBISTableData` 与旧 serde 辅助（`de_opt_f64` / `de_vi_points`）。
4. **解释器**：`symbol_table_build.rs` 产出新结构化 toml；形态判定收敛。
5. **Emitter**：`toml.rs` 新序列化（§8）。
6. **Validation**：按 §9 推荐方案改造（或先降级结构校验）。
7. **黄金文件重生成**（§10）。
8. `cargo build --workspace` + `cargo test -p ibis2ibstoml` 全绿，零警告。

## 12. 决策点 / 待确认

- **D1**：`Table.matrix` 元素类型 —— 采用 `Cell = Quantity | Text`（本设计默认）还是「纯 Quantity + 行结构体」？
- **D2**：Pin 等含标识符的行 —— 保留行结构体（推荐）还是泛化为 `Table`？
- **D3**：`Scalar::Ratio` 的 TOML 渲染（serde 标签）细节，实施期定。
- **D4**：数值字段是否保留原数字面量的尾零（如 `1.0000k` → `value = 1.0`，尾零丢失）——默认接受（保留数值与单位语义即可）。
- **D5**：validation 采用「按需解析数值比较」（推荐）还是「先结构校验」。
