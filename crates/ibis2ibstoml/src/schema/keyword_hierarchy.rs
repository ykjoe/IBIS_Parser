//! keyword_hierarchy — IBIS 强类型结构体（保留「数值 + 单位」）+ validator 声明式校验。
//!
//! 分为三个内联子模块：
//! - [`data_struct`]：基础值类型（Quantity / Ratio / Cell / Table）；
//! - [`file_hierarchy`]：keyword 层级强类型领域模型；
//! - [`valid_function`]：声明式校验函数（validator custom）。
//!
//! 子模块内部用 `use super::xxx::*` 互相引用；模块顶层 `pub use ...::*` 统一对外导出，
//! 保持 `crate::schema::keyword_hierarchy::*` 路径兼容。

#![allow(non_camel_case_types)]

pub mod data_struct {

use serde::de::Deserializer;
use serde::{Deserialize, Serialize};

use crate::backend::rules::parse_quantity;

// -----------------------------------------------------------------------------
// 基础值类型（quantity / ratio / cell / table 抽象）
// -----------------------------------------------------------------------------

/// 比值（分式）：分子 / 分母，各自为带单位的量；保留原文不计算。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ratio {
    pub numerator: Quantity,
    pub denominator: Quantity,
}

/// 量的数值形态：普通数或比值。
///
/// `Ratio` 经 `Box` 间接以断开 `Ratio → Quantity → Scalar → Ratio` 的递归类型。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Scalar {
    /// 普通十进制数值。
    Number(f64),
    /// 分式（如 `1.9/597p`），保留分子 / 分母，不计算。
    Ratio(Box<Ratio>),
}

impl Default for Scalar {
    fn default() -> Self {
        Scalar::Number(0.0)
    }
}

/// 带单位的量：保留原始数值与单位（不做归一化换算）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Quantity {
    pub value: Scalar,
    /// 原始单位（原样保留：`"p"` / `"mA"` / `"nH"` / `"V"` / `"k"` ...）；无单位 → None。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

impl<'de> Deserialize<'de> for Quantity {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // 解释器产出原始文本；`NA` / 非法在此为 Err（供 `Cell` 回落为文本）。
        let text = String::deserialize(d)?;
        parse_quantity(&text).map_err(serde::de::Error::custom)
    }
}

impl Default for Quantity {
    fn default() -> Self {
        Quantity { value: Scalar::default(), unit: None }
    }
}

/// 表格单元格：数值量或文本（用于含标识符列的混合表，如 Pin）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Cell {
    Quantity(Quantity),
    Text(String),
}

/// 表：列头 + 真矩阵（每个元素为 [`Cell`]）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Table {
    #[serde(default)]
    pub col_header: Vec<String>,
    #[serde(default)]
    pub matrix: Vec<Vec<Cell>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::rules::de_corner_table;

    #[test]
    fn test_quantity_deserialize_from_text() {
        let v = toml::Value::String("2.69p".to_string());
        let q: Quantity = v.try_into().unwrap();
        assert_eq!(q.value, Scalar::Number(2.69));
        assert_eq!(q.unit.as_deref(), Some("p"));
    }

    #[test]
    fn test_corner_table_deserialize_from_text() {
        let v = toml::Value::String("2.69p 2.29p 3.07p".to_string());
        let corner = de_corner_table(v).unwrap();
        assert_eq!(corner.col_header, vec!["typ", "min", "max"]);
        let row = &corner.matrix[0];
        let Cell::Quantity(typ) = &row[0] else { panic!("expected quantity cell") };
        assert_eq!(typ.value, Scalar::Number(2.69));
        assert_eq!(typ.unit.as_deref(), Some("p"));
        let Cell::Quantity(min) = &row[1] else { panic!("expected quantity cell") };
        assert!(matches!(min.value, Scalar::Number(2.29)));
        let Cell::Quantity(max) = &row[2] else { panic!("expected quantity cell") };
        assert!(matches!(max.value, Scalar::Number(3.07)));
    }

    #[test]
    fn test_ratio_deserialize_from_text() {
        // 分式 1.9/597p：保留分子 / 分母，不计算。
        let v = toml::Value::String("1.9/597p".to_string());
        let q: Quantity = v.try_into().unwrap();
        let Scalar::Ratio(r) = q.value else { panic!("expected ratio") };
        assert_eq!(r.numerator.value, Scalar::Number(1.9));
        assert_eq!(r.numerator.unit, None);
        assert_eq!(r.denominator.value, Scalar::Number(597.0));
        assert_eq!(r.denominator.unit.as_deref(), Some("p"));
    }
}
}

pub mod file_hierarchy {
#![allow(non_camel_case_types)]

use indexmap::IndexMap;
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use validator::Validate;

use super::data_struct::*;
use super::valid_function::{
    validate_corner, validate_corner_opt, validate_not_empty, validate_vi_points,
};
use crate::backend::rules::{de_corner_table, de_opt_corner_table, de_opt_quantity};

// -----------------------------------------------------------------------------
// File Header
// -----------------------------------------------------------------------------

/// 文件头属性（虚拟容器，其 children 为文件头字段）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct IBIS_File_Header {
    #[validate(custom(function = "validate_not_empty"))]
    #[serde(default)]
    pub ibis_ver: String,
    pub comment_char: Option<String>,
    #[validate(custom(function = "validate_not_empty"))]
    #[serde(default)]
    pub file_name: String,
    #[validate(custom(function = "validate_not_empty"))]
    #[serde(default)]
    pub file_rev: String,
    pub date: Option<String>,
    pub source: Option<String>,
    pub notes: Option<String>,
    pub disclaimer: Option<String>,
    pub copyright: Option<String>,
}

// -----------------------------------------------------------------------------
// Component section 子结构
// -----------------------------------------------------------------------------

/// [Package] — 默认全局引脚寄生参数（保留数值 + 单位；corner 用 `Table` 表示）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Component_Package {
    #[validate(custom(function = "validate_corner"))]
    #[serde(default, deserialize_with = "de_corner_table")]
    pub r_pkg: Table,
    #[validate(custom(function = "validate_corner"))]
    #[serde(default, deserialize_with = "de_corner_table")]
    pub l_pkg: Table,
    #[validate(custom(function = "validate_corner"))]
    #[serde(default, deserialize_with = "de_corner_table")]
    pub c_pkg: Table,
}

/// [Pin] — 引脚表（`pin` 为 [`Table`]；列 = signal_name / model_name / R_pin / L_pin / C_pin）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Validate)]
pub struct Component_Pin {
    #[serde(default)]
    pub pin: Table,
}

impl<'de> Deserialize<'de> for Component_Pin {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // `[Pin]` 节段被解释为 Table（col_header + matrix），此处包一层 `pin` 字段。
        let table = Table::deserialize(d)?;
        Ok(Component_Pin { pin: table })
    }
}

/// [Alternate Package Models] — 备用封装模型名列表。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Component_Alternate_Package_Models {
    pub alternate_package_models: Vec<String>,
}

/// [Interconnect Model Group] 条目。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Component_Interconnect_Model_Group {
    #[serde(default)]
    pub interconnect_model_group: String,
}

/// [Package Model] 选择器。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Component_Package_Model {
    #[serde(default)]
    pub package_model: String,
    #[serde(default, rename = "alternate_package_models")]
    pub alternates: Option<Component_Alternate_Package_Models>,
}

/// [Pin Mapping] 行（电源 / 地参考映射）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Component_Pin_Mapping {
    #[serde(default, rename = "pin_mapping")]
    pub pin_name: String,
    #[serde(default)]
    pub pulldown_ref: String,
    #[serde(default)]
    pub pullup_ref: String,
    pub gnd_clamp_ref: Option<String>,
    pub power_clamp_ref: Option<String>,
    pub ext_ref: Option<String>,
}

/// [Bus Label] 行。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Component_Bus_Label {
    #[serde(default)]
    pub bus_label: String,
    #[serde(default)]
    pub signal_name: String,
}

/// [Die Supply Pads] 行。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Component_Die_Supply_Pads {
    #[serde(default, rename = "die_supply_pads")]
    pub pad_name: String,
    #[serde(default)]
    pub signal_name: String,
    pub bus_label: Option<String>,
}

/// [Diff Pin] 行（`vdiff` / `tdelay_*` 为 `Quantity`）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Component_Diff_Pin {
    #[serde(default, rename = "diff_pin")]
    pub pin_name: String,
    #[serde(default)]
    pub inv_pin: String,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vdiff: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub tdelay_typ: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub tdelay_min: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub tdelay_max: Option<Quantity>,
}

/// [Repeater Pin] 行。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Component_Repeater_Pin {
    #[serde(default)]
    pub tx_non_inv_pin: String,
}

/// [Series Pin Mapping] 行。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Component_Series_Pin_Mapping {
    #[serde(default, rename = "series_pin_mapping")]
    pub pin_1: String,
    #[serde(default)]
    pub pin_2: String,
    #[serde(default)]
    pub model_name: String,
    pub function_table_group: Option<String>,
}

/// [Series Switch Groups] 行。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Component_Series_Switch_Groups {
    #[serde(default)]
    pub on: String,
    #[serde(default)]
    pub off: String,
}

/// [Circuit Call] 行。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Component_Circuit_Call {
    #[serde(default, rename = "circuit_call")]
    pub signal_pin: String,
    pub diff_signal_pins: Option<String>,
    pub series_pins: Option<String>,
    pub port_map: Option<String>,
    pub converter_parameters: Option<String>,
    pub parameters: Option<String>,
}

/// [Pin EMI] 行。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Component_Begin_EMI_Component_Pin_EMI {
    #[serde(default)]
    pub domain_name: String,
    #[serde(default)]
    pub clock_div: String,
}

/// [Pin Domain EMI] 行（`percentage` 为 `Quantity`）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Component_Begin_EMI_Component_Pin_Domain_EMI {
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub percentage: Option<Quantity>,
}

/// [Begin EMI Component] — 组件级 EMI 分配参数。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Component_Begin_EMI_Component {
    pub domain: String,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub cpd: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub c_heatsink_gnd: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub c_heatsink_float: Option<Quantity>,
    #[serde(default)]
    pub pin_emi: Vec<Component_Begin_EMI_Component_Pin_EMI>,
    #[serde(default)]
    pub pin_domain_emi: Vec<Component_Begin_EMI_Component_Pin_Domain_EMI>,
}

/// [Component] — 物理芯片 / 板级模块。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct IBIS_Component {
    #[serde(default)]
    pub component: String,
    pub si_location: Option<String>,
    pub timing_location: Option<String>,
    #[validate(custom(function = "validate_not_empty"))]
    #[serde(default)]
    pub manufacturer: String,
    #[serde(default)]
    pub package: Option<Component_Package>,
    #[serde(default)]
    pub pin: Option<Component_Pin>,
    #[serde(default)]
    pub package_model: Option<Component_Package_Model>,
    #[serde(default, rename = "interconnect_model_group")]
    pub interconnect_model_groups: Vec<Component_Interconnect_Model_Group>,
    #[serde(default, rename = "pin_mapping")]
    pub pin_mappings: Vec<Component_Pin_Mapping>,
    #[serde(default, rename = "bus_label")]
    pub bus_labels: Vec<Component_Bus_Label>,
    #[serde(default)]
    pub die_supply_pads: Vec<Component_Die_Supply_Pads>,
    #[serde(default, rename = "diff_pin")]
    pub diff_pins: Vec<Component_Diff_Pin>,
    #[serde(default, rename = "repeater_pin")]
    pub repeater_pins: Vec<Component_Repeater_Pin>,
    #[serde(default, rename = "series_pin_mapping")]
    pub series_pin_mappings: Vec<Component_Series_Pin_Mapping>,
    #[serde(default)]
    pub series_switch_groups: Vec<Component_Series_Switch_Groups>,
    #[serde(default)]
    pub node_declarations: Vec<String>,
    #[serde(default, rename = "circuit_call")]
    pub circuit_calls: Vec<Component_Circuit_Call>,
    #[serde(default, rename = "begin_emi_component")]
    pub emi: Option<Component_Begin_EMI_Component>,
}

// -----------------------------------------------------------------------------
// Model Selector
// -----------------------------------------------------------------------------

/// [Model Selector] — 多模型分组。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct IBIS_Model_Selector {
    pub model_selector: String,
    pub models: Vec<String>,
}

// -----------------------------------------------------------------------------
// Model section 子结构
// -----------------------------------------------------------------------------

/// [Model Spec] — 信号过冲静态 / 动态限值（`Quantity`）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Model_Model_Spec {
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vinh: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vinl: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vinh_plus: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vinh_minus: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vinl_plus: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vinl_minus: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub s_overshoot_high: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub s_overshoot_low: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub d_overshoot_high: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub d_overshoot_low: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub d_overshoot_time: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub d_overshoot_area_h: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub d_overshoot_area_l: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub d_overshoot_ampl_h: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub d_overshoot_ampl_l: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub pulse_high: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub pulse_low: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub pulse_time: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub weak_r: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub weak_i: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub weak_v: Option<Quantity>,
}

/// [Receiver Thresholds] — AC / DC 接收机验证阈值（引用类字段保留字符串）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Model_Receiver_Thresholds {
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vth: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vth_min: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vth_max: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vinh_ac: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vinh_dc: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vinl_ac: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vinl_dc: Option<Quantity>,
    pub threshold_sensitivity: Option<String>,
    pub reference_supply: Option<String>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vcross_low: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vcross_high: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vdiff_ac: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vdiff_dc: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub tslew_ac: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub tdiffslew_ac: Option<Quantity>,
}

/// [C Comp Corner] — 温度相关 die 电容参数（corner 用 `Table` 表示）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Model_C_Comp_Corner {
    #[validate(custom(function = "validate_corner"))]
    #[serde(default, deserialize_with = "de_corner_table")]
    pub c_comp: Table,
    #[validate(custom(function = "validate_corner_opt"))]
    #[serde(default, deserialize_with = "de_opt_corner_table")]
    pub c_comp_pullup: Option<Table>,
    #[validate(custom(function = "validate_corner_opt"))]
    #[serde(default, deserialize_with = "de_opt_corner_table")]
    pub c_comp_pulldown: Option<Table>,
    #[validate(custom(function = "validate_corner_opt"))]
    #[serde(default, deserialize_with = "de_opt_corner_table")]
    pub c_comp_power_clamp: Option<Table>,
    #[validate(custom(function = "validate_corner_opt"))]
    #[serde(default, deserialize_with = "de_opt_corner_table")]
    pub c_comp_gnd_clamp: Option<Table>,
}

/// [Ramp] — 压摆率过渡指标（`dv/dt` 保留比值分式；corner 用 `Table` 表示）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Model_Ramp {
    #[validate(custom(function = "validate_corner"))]
    #[serde(default, deserialize_with = "de_corner_table")]
    pub dv_dt_r: Table,
    #[validate(custom(function = "validate_corner"))]
    #[serde(default, deserialize_with = "de_corner_table")]
    pub dv_dt_f: Table,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub r_load: Option<Quantity>,
}

/// [Rising Waveform] / [Falling Waveform] — 仿真夹具参数。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Model_Waveform {
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub r_fixture: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub v_fixture: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub v_fixture_min: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub v_fixture_max: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub c_fixture: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub l_fixture: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub r_dut: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub l_dut: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub c_dut: Option<Quantity>,
    #[serde(default)]
    pub composite_current: Option<Table>,
}

/// [External Model]。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Model_External_Model {
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub corner: String,
    pub parameters: Option<String>,
    pub converter_parameters: Option<String>,
    pub ports: Option<String>,
    pub d_to_a: Option<String>,
    pub a_to_d: Option<String>,
}

/// [Algorithmic Model]。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Model_Algorithmic_Model {
    pub executable: Option<String>,
    pub executable_rx: Option<String>,
    pub executable_tx: Option<String>,
}

/// [Begin EMI Model]。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Model_Begin_EMI_Model {
    #[serde(default)]
    pub model_emi_type: String,
    #[serde(default)]
    pub model_domain: String,
}

/// [Model] — 完整 core 缓冲器驱动 / 接收机模型属性（符号表条目）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct IBIS_Model {
    #[serde(default)]
    pub model: String,                   // 模型名 = 符号表 Key
    #[validate(custom(function = "validate_not_empty"))]
    #[serde(default)]
    pub model_type: String,
    pub polarity: Option<String>,
    pub enable: Option<String>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vinl: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vinh: Option<Quantity>,
    #[validate(custom(function = "validate_corner_opt"))]
    #[serde(default, deserialize_with = "de_opt_corner_table")]
    pub c_comp: Option<Table>,
    #[validate(custom(function = "validate_corner_opt"))]
    #[serde(default, deserialize_with = "de_opt_corner_table")]
    pub c_comp_pullup: Option<Table>,
    #[validate(custom(function = "validate_corner_opt"))]
    #[serde(default, deserialize_with = "de_opt_corner_table")]
    pub c_comp_pulldown: Option<Table>,
    #[validate(custom(function = "validate_corner_opt"))]
    #[serde(default, deserialize_with = "de_opt_corner_table")]
    pub c_comp_power_clamp: Option<Table>,
    #[validate(custom(function = "validate_corner_opt"))]
    #[serde(default, deserialize_with = "de_opt_corner_table")]
    pub c_comp_gnd_clamp: Option<Table>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vmeas: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub cref: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub rref: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vref: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub rref_diff: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub cref_diff: Option<Quantity>,
    #[serde(default)]
    pub model_spec: Option<Model_Model_Spec>,
    #[serde(default)]
    pub receiver_thresholds: Option<Model_Receiver_Thresholds>,
    pub add_submodel: Option<String>,
    pub driver_schedule: Option<String>,
    #[validate(custom(function = "validate_corner_opt"))]
    #[serde(default, deserialize_with = "de_opt_corner_table")]
    pub temperature_range: Option<Table>,
    #[validate(custom(function = "validate_corner_opt"))]
    #[serde(default, deserialize_with = "de_opt_corner_table")]
    pub voltage_range: Option<Table>,
    #[validate(custom(function = "validate_corner_opt"))]
    #[serde(default, deserialize_with = "de_opt_corner_table")]
    pub pullup_reference: Option<Table>,
    #[validate(custom(function = "validate_corner_opt"))]
    #[serde(default, deserialize_with = "de_opt_corner_table")]
    pub pulldown_reference: Option<Table>,
    #[validate(custom(function = "validate_corner_opt"))]
    #[serde(default, deserialize_with = "de_opt_corner_table")]
    pub power_clamp_reference: Option<Table>,
    #[validate(custom(function = "validate_corner_opt"))]
    #[serde(default, deserialize_with = "de_opt_corner_table")]
    pub gnd_clamp_reference: Option<Table>,
    #[validate(custom(function = "validate_corner_opt"))]
    #[serde(default, deserialize_with = "de_opt_corner_table")]
    pub external_reference: Option<Table>,
    #[serde(default)]
    pub c_comp_corner: Option<Model_C_Comp_Corner>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub ttgnd: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub ttpower: Option<Quantity>,
    #[validate(custom(function = "validate_vi_points"))]
    #[serde(default)]
    pub pulldown: Option<Table>,
    #[validate(custom(function = "validate_vi_points"))]
    #[serde(default)]
    pub pullup: Option<Table>,
    #[validate(custom(function = "validate_vi_points"))]
    #[serde(default)]
    pub gnd_clamp: Option<Table>,
    #[validate(custom(function = "validate_vi_points"))]
    #[serde(default)]
    pub power_clamp: Option<Table>,
    #[validate(custom(function = "validate_vi_points"))]
    #[serde(default, rename = "power_table")]
    pub isso_pu: Option<Table>,
    #[validate(custom(function = "validate_vi_points"))]
    #[serde(default, rename = "gnd_table")]
    pub isso_pd: Option<Table>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub rgnd: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub rpower: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub rac: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub cac: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub on: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub off: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub r_series: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub l_series: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub rl_series: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub c_series: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub lc_series: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub rc_series: Option<Quantity>,
    #[validate(custom(function = "validate_vi_points"))]
    #[serde(default)]
    pub series_current: Option<Table>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub vds: Option<Quantity>,
    #[serde(default)]
    pub ramp: Option<Model_Ramp>,
    #[serde(default, rename = "rising_waveform")]
    pub rising_waveforms: Vec<Model_Waveform>,
    #[serde(default, rename = "falling_waveform")]
    pub falling_waveforms: Vec<Model_Waveform>,
    #[serde(default)]
    pub initial_delay_vt: Option<Table>,
    #[serde(default)]
    pub initial_delay_it: Option<Table>,
    #[serde(default)]
    pub external_model: Option<Model_External_Model>,
    #[serde(default)]
    pub algorithmic_model: Option<Model_Algorithmic_Model>,
    #[serde(default, rename = "begin_emi_model")]
    pub emi: Option<Model_Begin_EMI_Model>,
}

// -----------------------------------------------------------------------------
// Submodel section
// -----------------------------------------------------------------------------

/// [Submodel Spec]。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Submodel_Submodel_Spec {
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub v_trigger_r: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub v_trigger_f: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub off_delay: Option<Quantity>,
}

/// [Submodel] — 独立可编程 / 次级逻辑块子模型。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct IBIS_Submodel {
    #[serde(default)]
    pub submodel: String,
    #[validate(custom(function = "validate_not_empty"))]
    #[serde(default)]
    pub submodel_type: String,
    #[serde(default)]
    pub submodel_spec: Option<Submodel_Submodel_Spec>,
    #[validate(custom(function = "validate_vi_points"))]
    #[serde(default)]
    pub power_pulse_table: Option<Table>,
    #[validate(custom(function = "validate_vi_points"))]
    #[serde(default)]
    pub gnd_pulse_table: Option<Table>,
    #[validate(custom(function = "validate_vi_points"))]
    #[serde(default)]
    pub pulldown: Option<Table>,
    #[validate(custom(function = "validate_vi_points"))]
    #[serde(default)]
    pub pullup: Option<Table>,
    #[validate(custom(function = "validate_vi_points"))]
    #[serde(default)]
    pub gnd_clamp: Option<Table>,
    #[validate(custom(function = "validate_vi_points"))]
    #[serde(default)]
    pub power_clamp: Option<Table>,
    #[serde(default)]
    pub ramp: Option<Model_Ramp>,
    #[serde(default, rename = "rising_waveform")]
    pub rising_waveforms: Vec<Model_Waveform>,
    #[serde(default, rename = "falling_waveform")]
    pub falling_waveforms: Vec<Model_Waveform>,
    #[serde(default)]
    pub initial_delay_vt: Option<Table>,
    #[serde(default)]
    pub initial_delay_it: Option<Table>,
}

// -----------------------------------------------------------------------------
// External Circuit / Test Data / Test Load
// -----------------------------------------------------------------------------

/// [External Circuit]。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct IBIS_External_Circuit {
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub corner: String,
    pub parameters: Option<String>,
    pub converter_parameters: Option<String>,
    pub ports: Option<String>,
    pub d_to_a: Option<String>,
    pub a_to_d: Option<String>,
}

/// [Test Data]。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct IBIS_Test_Data {
    #[serde(default)]
    pub test_data: String,
    #[serde(default)]
    pub test_data_type: String,
    #[serde(default)]
    pub driver_model: String,
    pub driver_model_inv: Option<String>,
    #[serde(default)]
    pub test_load: String,
    #[serde(default)]
    pub rising_waveform_near: Option<Table>,
    #[serde(default)]
    pub falling_waveform_near: Option<Table>,
    #[serde(default)]
    pub rising_waveform_far: Option<Table>,
    #[serde(default)]
    pub falling_waveform_far: Option<Table>,
    #[serde(default)]
    pub diff_rising_waveform_near: Option<Table>,
    #[serde(default)]
    pub diff_falling_waveform_near: Option<Table>,
    #[serde(default)]
    pub diff_rising_waveform_far: Option<Table>,
    #[serde(default)]
    pub diff_falling_waveform_far: Option<Table>,
}

/// [Test Load]（数值字段为 `Quantity`）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct IBIS_Test_Load {
    #[serde(default)]
    pub test_load: String,
    #[serde(default)]
    pub test_load_type: String,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub c1_near: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub rs_near: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub ls_near: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub c2_near: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub rp1_near: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub rp2_near: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub td: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub zo: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub rp1_far: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub rp2_far: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub c2_far: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub ls_far: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub rs_far: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub c1_far: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub v_term1: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub v_term2: Option<Quantity>,
    pub receiver_model: Option<String>,
    pub receiver_model_inv: Option<String>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub r_diff_near: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub r_diff_far: Option<Quantity>,
}

// -----------------------------------------------------------------------------
// Define Package Model section
// -----------------------------------------------------------------------------

/// RLGC 矩阵：表格位于节段自身内容，`[Bandwidth]` 子节段提供带宽。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Define_Package_Model_Matrix {
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub bandwidth: Option<Quantity>,
    #[serde(default)]
    pub row: Table,
}

/// [Pin Numbers] 行。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Define_Package_Model_Pin_Numbers {
    #[serde(default, rename = "pin_numbers")]
    pub len: String,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub l: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub r: Option<Quantity>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub c: Option<Quantity>,
    pub fork: Option<String>,
    pub endfork: Option<String>,
}

/// [Model Data] 内部容器（电阻 / 电感 / 电容矩阵）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Define_Package_Model_Model_Data {
    #[serde(default)]
    pub resistance_matrix: Option<Define_Package_Model_Matrix>,
    #[serde(default)]
    pub inductance_matrix: Option<Define_Package_Model_Matrix>,
    #[serde(default)]
    pub capacitance_matrix: Option<Define_Package_Model_Matrix>,
}

/// [Define Package Model]。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct IBIS_Define_Package_Model {
    #[serde(default)]
    pub define_package_model: String,
    #[serde(default)]
    pub manufacturer: String,
    #[serde(default)]
    pub oem: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub number_of_sections: String,
    #[serde(default)]
    pub number_of_pins: String,
    #[serde(default)]
    pub pin_numbers: Vec<Define_Package_Model_Pin_Numbers>,
    #[serde(default)]
    pub merged_pins: Vec<String>,
    #[serde(default)]
    pub resistance_matrix: Option<Define_Package_Model_Matrix>,
    #[serde(default)]
    pub inductance_matrix: Option<Define_Package_Model_Matrix>,
    #[serde(default)]
    pub capacitance_matrix: Option<Define_Package_Model_Matrix>,
}

// -----------------------------------------------------------------------------
// Interconnect Model Set section
// -----------------------------------------------------------------------------

/// [Interconnect Model]。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct Interconnect_Model_Set_Interconnect_Model {
    #[serde(default)]
    pub interconnect_model: String,
    pub param: Option<String>,
    pub file_ts: Option<String>,
    pub file_ibis_iss: Option<String>,
    pub unused_port_termination: Option<String>,
    #[serde(default, deserialize_with = "de_opt_quantity")]
    pub number_of_terminals: Option<Quantity>,
}

/// [Interconnect Model Set]。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Validate)]
pub struct IBIS_Interconnect_Model_Set {
    #[serde(default)]
    pub interconnect_model_set: String,
    #[serde(default)]
    pub manufacturer: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "interconnect_model")]
    pub interconnect_models: Vec<Interconnect_Model_Set_Interconnect_Model>,
}

// -----------------------------------------------------------------------------
// 根领域模型（符号表容器）
// -----------------------------------------------------------------------------

/// 根领域模型：保留「数值 + 单位」后的符号表容器。
///
/// - `models` / `submodels` / `test_loads` / `package_models` 为 `IndexMap`，
///   Key 为各自名称字段，同时保证 O(1) 查找与源文件顺序。
#[derive(Debug, Clone, Default, Serialize, Deserialize, Validate)]
pub struct IBIS_File {
    #[validate(nested)]
    #[serde(default)]
    pub header: IBIS_File_Header,
    #[validate(nested)]
    #[serde(default)]
    pub components: Vec<IBIS_Component>,
    #[validate(nested)]
    #[serde(default)]
    pub model_selectors: Vec<IBIS_Model_Selector>,
    // IndexMap 字段的 key 为动态字符串，validator nested 需 &'static 字段名，
    // 故这些符号表字段在 data_valid 中手动遍历校验。
    #[serde(default)]
    pub models: IndexMap<String, IBIS_Model>,
    #[serde(default)]
    pub submodels: IndexMap<String, IBIS_Submodel>,
    #[validate(nested)]
    #[serde(default)]
    pub external_circuits: Vec<IBIS_External_Circuit>,
    #[validate(nested)]
    #[serde(default)]
    pub test_data: Vec<IBIS_Test_Data>,
    #[serde(default)]
    pub test_loads: IndexMap<String, IBIS_Test_Load>,
    #[serde(default)]
    pub package_models: IndexMap<String, IBIS_Define_Package_Model>,
    #[validate(nested)]
    #[serde(default)]
    pub interconnect_model_sets: Vec<IBIS_Interconnect_Model_Set>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_roundtrip_through_toml() {
        // 解释器契约：字段名用 snake；数值字段为原始文本（String），曲线为 Table（单元为文本）。
        let toml_str = r#"
            [model]
            model = "IO8FT"
            model_type = "I/O"
            c_comp = "1.12p 0.79p 1.15p"
            vref = "1.8V"
            voltage_range = "3.3V 2.0V 3.6V"
            [model.pulldown]
            col_header = ["voltage", "i_typ", "i_min", "i_max"]
            matrix = [
                ["-3.3", "-2mA", "-2mA", "-1mA"],
                ["0.0", "0mA", "0mA", "0mA"],
            ]
            [model.ramp]
            r_load = "1.0000k"
        "#;
        let value: toml::Value = toml::from_str(toml_str).unwrap();
        let model_value = value.get("model").cloned().unwrap();
        let model: IBIS_Model = model_value.try_into().unwrap();
        assert_eq!(model.model, "IO8FT");
        assert_eq!(model.model_type, "I/O");
        let c_comp = model.c_comp.as_ref().unwrap();
        assert_eq!(c_comp.col_header, vec!["typ", "min", "max"]);
        let Cell::Quantity(typ) = &c_comp.matrix[0][0] else { panic!("expected quantity cell") };
        assert_eq!(typ.value, Scalar::Number(1.12));
        assert_eq!(typ.unit.as_deref(), Some("p"));
        assert_eq!(model.vref.as_ref().unwrap().value, Scalar::Number(1.8));
        assert_eq!(model.vref.as_ref().unwrap().unit.as_deref(), Some("V"));
        let vr = model.voltage_range.as_ref().unwrap();
        let Cell::Quantity(vr_typ) = &vr.matrix[0][0] else { panic!("expected quantity cell") };
        assert_eq!(vr_typ.unit.as_deref(), Some("V"));
        let pd = model.pulldown.unwrap();
        assert_eq!(pd.col_header.len(), 4);
        assert_eq!(pd.matrix.len(), 2);
        let Cell::Quantity(first) = &pd.matrix[0][0] else { panic!("expected quantity cell") };
        assert_eq!(first.value, Scalar::Number(-3.3));
        let Cell::Quantity(i_typ) = &pd.matrix[0][1] else { panic!("expected quantity cell") };
        assert_eq!(i_typ.unit.as_deref(), Some("mA"));
        assert_eq!(model.ramp.as_ref().unwrap().r_load.as_ref().unwrap().value, Scalar::Number(1.0));
        assert_eq!(model.ramp.as_ref().unwrap().r_load.as_ref().unwrap().unit.as_deref(), Some("k"));
    }
}
}

pub mod valid_function {
use validator::ValidationError;

use super::data_struct::*;
use crate::backend::rules::quantity_to_f64;

// -----------------------------------------------------------------------------
// validator 校验函数（声明式挂载到字段）
// -----------------------------------------------------------------------------

/// 构造一个带消息的 `ValidationError`。
fn validation_error(message: &'static str) -> ValidationError {
    let mut error = ValidationError::new("custom");
    error.message = Some(std::borrow::Cow::Borrowed(message));
    error
}

/// 必填字符串非空校验（挂载到 `String` 必填字段）。
pub fn validate_not_empty(value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        Err(validation_error("该字段不能为空"))
    } else {
        Ok(())
    }
}

/// corner 范围校验：min ≤ typ ≤ max（挂载到 `Table` corner 必填字段）。
///
/// corner 表：`col_header = ["typ", "min", "max"]`，`matrix` 单行 = `[typ, min, max]`。
/// 数值比较按需解析（`quantity_to_f64`）；分式（Ratio）或不可解析时跳过该项。
pub fn validate_corner(corner: &Table) -> Result<(), ValidationError> {
    validate_corner_inner(corner)
}

/// corner 范围校验（挂载到 `Option<Table>` corner 字段）。
///
/// validator 0.19 对 `Option<T>` 字段先 `if let Some(ref v)` 解包再取引用，
/// 传给 custom 函数的参数类型为 `&&T`。
pub fn validate_corner_opt(corner: &&Table) -> Result<(), ValidationError> {
    validate_corner_inner(corner)
}

fn validate_corner_inner(corner: &Table) -> Result<(), ValidationError> {
    let Some(row) = corner.matrix.first() else { return Ok(()) };
    let Some(Cell::Quantity(typ_q)) = row.first() else { return Ok(()) };
    let Some(typ) = quantity_to_f64(typ_q) else { return Ok(()) };
    // 其余单元格为 min / max；兼容 `typ min max` 与 `typ max min` 两种书写顺序：
    // 按两个边界值的数值区间判定。
    let mut bounds: Vec<f64> = Vec::new();
    for cell in row.iter().skip(1) {
        if let Cell::Quantity(q) = cell
            && let Some(v) = quantity_to_f64(q)
        {
            bounds.push(v);
        }
    }
    let lower = bounds.iter().copied().reduce(f64::min);
    let upper = bounds.iter().copied().reduce(f64::max);
    if lower.is_some_and(|lo| typ < lo) {
        return Err(validation_error("角点顺序违规：typ 小于下界"));
    }
    if upper.is_some_and(|hi| typ > hi) {
        return Err(validation_error("角点顺序违规：typ 大于上界"));
    }
    Ok(())
}

/// IV/VT 曲线单调性校验：电压列严格递增（挂载到 `Option<Table>` 曲线字段）。
///
/// 电压列 = `col_header` 中 `voltage` 的下标（缺省第 0 列）；分式 / `NA` 行跳过。
pub fn validate_vi_points(table: &&Table) -> Result<(), ValidationError> {
    let voltage_col = table
        .col_header
        .iter()
        .position(|h| h.eq_ignore_ascii_case("voltage"))
        .unwrap_or(0);
    let mut previous: Option<f64> = None;
    for row in &table.matrix {
        let Some(cell) = row.get(voltage_col) else { continue };
        let Cell::Quantity(q) = cell else { continue };
        let Some(v) = quantity_to_f64(q) else { continue };
        if previous.is_some_and(|prev| v <= prev) {
            return Err(validation_error("IV 曲线电压未严格递增"));
        }
        previous = Some(v);
    }
    Ok(())
}
}

pub use data_struct::*;
pub use file_hierarchy::*;
pub use valid_function::*;
