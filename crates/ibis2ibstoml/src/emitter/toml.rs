//! TOML output — serialize the strongly-typed [`IBIS_File`] into a TOML string.
//!
//! Serialization rules (see the architecture book, section 4):
//!
//! - String fields → `key = "value"`; `Option<None>` keys are omitted.
//! - **Quantities → `key = "value+unit"`**（如 `key = "1.65V"`、`key = "0.8"`）—
//!   保留原始「数值 + 单位」原文，不做归一化换算，且为合法 TOML 字符串。
//! - corner（角点三元组）→ 内联表 `key = { col_header = ["typ", "min", "max"], data = [...] }`
//!   （以 `Table` 统一表示，替代已删除的 `Triplet`；单元格为字符串）。
//! - I-V 曲线 / 表格 keyword（`[Pulldown]` / `[Pullup]` / `[Ramp]` / `[GND Clamp]` 等在 IBIS
//!   中以 `[]` 括起的**子级 keyword**）→ **TOML section** `[Model.xxx]`（对应 IBIS 的 `[]`
//!   括起形式）；section 内再用**同名字段**承载内联表：
//!   `[Model.Pulldown]` + `pulldown = { col_header = [...], data = [[...]] }`。
//! - `Model` / `Submodel` 中非 `[]` 括起的 inline 量（如 `C_comp`）→ `[[Model]]` 元素上的
//!   内联表字段 `key = { col_header, data }`。
//! - `Component` 的 `[Pin]`（内部仅一个 `pin` 字段，类型为 Table）→ TOML section
//!   `[Component.Pin]` + `Pin = { col_header = [...], data = [[...]] }`（列 =
//!   signal_name / model_name / R_pin / L_pin / C_pin）。
//! - `Component` 的其余**行式表**子级 keyword（`[Pin Mapping]` / `[Bus Label]` /
//!   `[Diff Pin]` / `[Circuit Call]`）→ `[[Component.xxx]]` array-of-tables。
//! - `Vec<T>` / `IndexMap<K, T>` fields → `[[...]]` array-of-tables。

use std::fmt::Write as FmtWrite;

use crate::schema::keyword_hierarchy::*;

// -----------------------------------------------------------------------------
// Value emitters
// -----------------------------------------------------------------------------

/// Escape and wrap a raw string value for TOML output.
fn emit_string(buf: &mut String, key: &str, value: &str) {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    let _ = writeln!(buf, "{} = \"{}\"", key, escaped);
}

/// Emit an `Option<String>` field (omitted when `None`).
fn emit_opt_string(buf: &mut String, key: &str, value: &Option<String>) {
    if let Some(value) = value {
        emit_string(buf, key, value);
    }
}

/// 紧凑表示一个 `Quantity` 的裸文本（不加引号）：`-2mA`、`3.3`、`1.9/597p`。
fn quantity_bare(q: &Quantity) -> String {
    match &q.value {
        Scalar::Number(n) => {
            let num = format!("{n}");
            match &q.unit {
                Some(unit) => format!("{num}{unit}"),
                None => num,
            }
        }
        Scalar::Ratio(r) => {
            format!("{}/{}", quantity_bare(&r.numerator), quantity_bare(&r.denominator))
        }
    }
}

/// 内联序列化一个 `Quantity` 为合法 TOML 字符串：`"-2mA"`、`"3.3"`、`"1.9/597p"`。
fn quantity_string(q: &Quantity) -> String {
    let bare = quantity_bare(q);
    format!("\"{}\"", bare.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Emit a `Quantity` field as a string.
fn emit_quantity(buf: &mut String, key: &str, q: &Quantity) {
    let _ = writeln!(buf, "{} = {}", key, quantity_string(q));
}

/// Emit an `Option<Quantity>` field (omitted when `None`).
fn emit_opt_quantity(buf: &mut String, key: &str, q: &Option<Quantity>) {
    if let Some(q) = q {
        emit_quantity(buf, key, q);
    }
}

/// 内联序列化一个表格单元格（Quantity 或 Text）为合法 TOML 字符串。
fn cell_compact(cell: &Cell) -> String {
    match cell {
        Cell::Quantity(q) => quantity_string(q),
        Cell::Text(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
    }
}

/// corner 表内联字符串：
/// `{ col_header = ["typ", "min", "max"], data = ["typ", "min", "max"] }`。
fn corner_table_inline(table: &Table) -> String {
    let cells: Vec<String> = table
        .matrix
        .first()
        .map(|row| row.iter().map(cell_compact).collect())
        .unwrap_or_default();
    format!(
        "{{ col_header = [\"typ\", \"min\", \"max\"], data = [{}] }}",
        cells.join(", ")
    )
}

/// Emit 一个 corner（角点三元组）字段为内联表。
fn emit_corner_table(buf: &mut String, key: &str, table: &Table) {
    let _ = writeln!(buf, "{} = {}", key, corner_table_inline(table));
}

/// Emit an `Option<Table>` corner 字段（omitted when `None`）。
fn emit_opt_corner_table(buf: &mut String, key: &str, table: &Option<Table>) {
    if let Some(table) = table {
        emit_corner_table(buf, key, table);
    }
}

/// Emit a `Vec<String>` field as a TOML array.
fn emit_string_array(buf: &mut String, key: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    let items: Vec<String> = values
        .iter()
        .map(|s| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect();
    let _ = writeln!(buf, "{} = [{}]", key, items.join(", "));
}

/// 表格内联字符串：`{ col_header = [...], data = [\n    [...],\n] }`。
///
/// TOML 1.0 内联表 `{ }` 必须单行开合（不允许 `{` 后直接换行），但**数组值内部可跨行**：
/// 故 `col_header` 与 `{` 同行，仅 `data` 数组跨行，保证大表可读且合法。
fn table_inline_string(table: &Table) -> String {
    let cols: Vec<String> = table
        .col_header
        .iter()
        .map(|c| format!("\"{}\"", c.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect();
    let mut s = format!("{{ col_header = [{}], data = [", cols.join(", "));
    for row in &table.matrix {
        let cells: Vec<String> = row.iter().map(cell_compact).collect();
        s.push('\n');
        s.push_str(&format!("    [{}],", cells.join(", ")));
    }
    s.push_str("\n] }");
    s
}

/// Emit a `Table` 为内联表字段（用于 waveform 的 `composite_current` 等内嵌表）。
fn emit_table_inline(buf: &mut String, key: &str, table: &Table) {
    let _ = writeln!(buf, "{} = {}", key, table_inline_string(table));
}

/// Emit 一个 I-V 曲线 / 表格 keyword 为 TOML section + 同名字段内联表：
/// `[path]` 换行 `key = { col_header = [...], data = [[...]] }`。
fn emit_table_section(buf: &mut String, path: &str, key: &str, table: &Table) {
    let _ = writeln!(buf, "[{}]", path);
    let _ = writeln!(buf, "{} = {}", key, table_inline_string(table));
}

/// Emit an `Option<Table>` 曲线 / 表格 keyword section（omitted when `None`）。
fn emit_opt_table_section(buf: &mut String, path: &str, key: &str, table: &Option<Table>) {
    if let Some(table) = table {
        emit_table_section(buf, path, key, table);
    }
}

// -----------------------------------------------------------------------------
// Struct serializers
// -----------------------------------------------------------------------------

fn serialize_header(buf: &mut String, header: &IBIS_File_Header) {
    let _ = writeln!(buf, "[File_Header]");
    emit_string(buf, "ibis_ver", &header.ibis_ver);
    emit_opt_string(buf, "comment_char", &header.comment_char);
    emit_string(buf, "file_name", &header.file_name);
    emit_string(buf, "file_rev", &header.file_rev);
    emit_opt_string(buf, "date", &header.date);
    emit_opt_string(buf, "source", &header.source);
    emit_opt_string(buf, "notes", &header.notes);
    emit_opt_string(buf, "disclaimer", &header.disclaimer);
    emit_opt_string(buf, "copyright", &header.copyright);
    let _ = writeln!(buf);
}

fn serialize_package(buf: &mut String, path: &str, package: &Component_Package) {
    let _ = writeln!(buf, "[{}]", path);
    emit_corner_table(buf, "r_pkg", &package.r_pkg);
    emit_corner_table(buf, "l_pkg", &package.l_pkg);
    emit_corner_table(buf, "c_pkg", &package.c_pkg);
}

fn serialize_component(buf: &mut String, component: &IBIS_Component) {
    let _ = writeln!(buf, "[[Component]]");
    emit_string(buf, "component", &component.component);
    emit_opt_string(buf, "si_location", &component.si_location);
    emit_opt_string(buf, "timing_location", &component.timing_location);

    let _ = writeln!(buf, "[Component.Manufacturer]");
    emit_string(buf, "manufacturer", &component.manufacturer);

    if let Some(package) = &component.package {
        serialize_package(buf, "Component.Package", package);
    }

    // [Pin]：Table（列 = signal_name / model_name / R_pin / L_pin / C_pin）。
    if let Some(pin) = &component.pin {
        emit_table_section(buf, "Component.Pin", "Pin", &pin.pin);
    }

    for mapping in &component.pin_mappings {
        let _ = writeln!(buf, "[[Component.Pin_Mapping]]");
        emit_string(buf, "pin_name", &mapping.pin_name);
        emit_string(buf, "pulldown_ref", &mapping.pulldown_ref);
        emit_string(buf, "pullup_ref", &mapping.pullup_ref);
        emit_opt_string(buf, "gnd_clamp_ref", &mapping.gnd_clamp_ref);
        emit_opt_string(buf, "power_clamp_ref", &mapping.power_clamp_ref);
        emit_opt_string(buf, "ext_ref", &mapping.ext_ref);
    }

    for label in &component.bus_labels {
        let _ = writeln!(buf, "[[Component.Bus_Label]]");
        emit_string(buf, "bus_label", &label.bus_label);
        emit_string(buf, "signal_name", &label.signal_name);
    }

    for diff in &component.diff_pins {
        let _ = writeln!(buf, "[[Component.Diff_Pin]]");
        emit_string(buf, "pin_name", &diff.pin_name);
        emit_string(buf, "inv_pin", &diff.inv_pin);
        emit_opt_quantity(buf, "vdiff", &diff.vdiff);
        emit_opt_quantity(buf, "tdelay_typ", &diff.tdelay_typ);
        emit_opt_quantity(buf, "tdelay_min", &diff.tdelay_min);
        emit_opt_quantity(buf, "tdelay_max", &diff.tdelay_max);
    }

    emit_string_array(buf, "node_declarations", &component.node_declarations);

    for call in &component.circuit_calls {
        let _ = writeln!(buf, "[[Component.Circuit_Call]]");
        emit_string(buf, "signal_pin", &call.signal_pin);
        emit_opt_string(buf, "diff_signal_pins", &call.diff_signal_pins);
        emit_opt_string(buf, "series_pins", &call.series_pins);
        emit_opt_string(buf, "port_map", &call.port_map);
        emit_opt_string(buf, "converter_parameters", &call.converter_parameters);
        emit_opt_string(buf, "parameters", &call.parameters);
    }

    let _ = writeln!(buf);
}

fn serialize_model_selector(buf: &mut String, selector: &IBIS_Model_Selector) {
    let _ = writeln!(buf, "[[Model_Selector]]");
    emit_string(buf, "model_selector", &selector.model_selector);
    emit_string_array(buf, "models", &selector.models);
    let _ = writeln!(buf);
}

fn serialize_ramp(buf: &mut String, path: &str, ramp: &Model_Ramp) {
    let _ = writeln!(buf, "[{}]", path);
    // 分式保留原始 IBIS 关键字名（含 '/'，故需引号）与原表达方式。
    emit_corner_table(buf, "\"dv/dt_r\"", &ramp.dv_dt_r);
    emit_corner_table(buf, "\"dv/dt_f\"", &ramp.dv_dt_f);
    emit_opt_quantity(buf, "r_load", &ramp.r_load);
}

fn serialize_waveform(buf: &mut String, path: &str, waveform: &Model_Waveform) {
    let _ = writeln!(buf, "[[{}]]", path);
    emit_opt_quantity(buf, "r_fixture", &waveform.r_fixture);
    emit_opt_quantity(buf, "v_fixture", &waveform.v_fixture);
    emit_opt_quantity(buf, "v_fixture_min", &waveform.v_fixture_min);
    emit_opt_quantity(buf, "v_fixture_max", &waveform.v_fixture_max);
    emit_opt_quantity(buf, "c_fixture", &waveform.c_fixture);
    emit_opt_quantity(buf, "l_fixture", &waveform.l_fixture);
    if let Some(composite) = &waveform.composite_current {
        emit_table_inline(buf, "composite_current", composite);
    }
}

fn serialize_model(buf: &mut String, model: &IBIS_Model) {
    let _ = writeln!(buf, "[[Model]]");
    emit_string(buf, "model", &model.model);
    emit_string(buf, "model_type", &model.model_type);
    emit_opt_string(buf, "polarity", &model.polarity);
    emit_opt_string(buf, "enable", &model.enable);
    emit_opt_quantity(buf, "vinl", &model.vinl);
    emit_opt_quantity(buf, "vinh", &model.vinh);
    emit_opt_corner_table(buf, "c_comp", &model.c_comp);
    emit_opt_quantity(buf, "vmeas", &model.vmeas);
    emit_opt_quantity(buf, "cref", &model.cref);
    emit_opt_quantity(buf, "rref", &model.rref);
    emit_opt_quantity(buf, "vref", &model.vref);
    emit_opt_quantity(buf, "rref_diff", &model.rref_diff);
    emit_opt_quantity(buf, "cref_diff", &model.cref_diff);

    emit_opt_corner_table(buf, "temperature_range", &model.temperature_range);
    emit_opt_corner_table(buf, "voltage_range", &model.voltage_range);
    emit_opt_corner_table(buf, "pullup_reference", &model.pullup_reference);
    emit_opt_corner_table(buf, "pulldown_reference", &model.pulldown_reference);
    emit_opt_corner_table(buf, "power_clamp_reference", &model.power_clamp_reference);
    emit_opt_corner_table(buf, "gnd_clamp_reference", &model.gnd_clamp_reference);
    emit_opt_corner_table(buf, "external_reference", &model.external_reference);

    if let Some(spec) = &model.model_spec {
        let _ = writeln!(buf, "[Model.Model_Spec]");
        emit_opt_quantity(buf, "vinh", &spec.vinh);
        emit_opt_quantity(buf, "vinl", &spec.vinl);
    }

    if let Some(thresholds) = &model.receiver_thresholds {
        let _ = writeln!(buf, "[Model.Receiver_Thresholds]");
        emit_opt_quantity(buf, "vth", &thresholds.vth);
        emit_opt_quantity(buf, "vinh_ac", &thresholds.vinh_ac);
        emit_opt_quantity(buf, "vinl_ac", &thresholds.vinl_ac);
    }

    if let Some(ramp) = &model.ramp {
        serialize_ramp(buf, "Model.Ramp", ramp);
    }

    // I-V 曲线 / 表格 keyword（IBIS 中以 `[]` 括起的子级 keyword）→
    // TOML section `[Model.xxx]` + 同名字段内联表 `xxx = { col_header, data }`。
    emit_opt_table_section(buf, "Model.Pulldown", "pulldown", &model.pulldown);
    emit_opt_table_section(buf, "Model.Pullup", "pullup", &model.pullup);
    emit_opt_table_section(buf, "Model.GND_Clamp", "gnd_clamp", &model.gnd_clamp);
    emit_opt_table_section(buf, "Model.Power_Clamp", "power_clamp", &model.power_clamp);
    emit_opt_table_section(buf, "Model.POWER_Table", "power_table", &model.isso_pu);
    emit_opt_table_section(buf, "Model.GND_Table", "gnd_table", &model.isso_pd);
    emit_opt_table_section(buf, "Model.Series_Current", "series_current", &model.series_current);

    for waveform in &model.rising_waveforms {
        serialize_waveform(buf, "Model.Rising_Waveform", waveform);
    }
    for waveform in &model.falling_waveforms {
        serialize_waveform(buf, "Model.Falling_Waveform", waveform);
    }

    let _ = writeln!(buf);
}

fn serialize_submodel(buf: &mut String, submodel: &IBIS_Submodel) {
    let _ = writeln!(buf, "[[Submodel]]");
    emit_string(buf, "submodel", &submodel.submodel);
    emit_string(buf, "submodel_type", &submodel.submodel_type);

    if let Some(ramp) = &submodel.ramp {
        serialize_ramp(buf, "Submodel.Ramp", ramp);
    }

    // Submodel 的 I-V 曲线 / 表格 keyword → TOML section + 同名字段内联表。
    emit_opt_table_section(buf, "Submodel.Power_Pulse_Table", "power_pulse_table", &submodel.power_pulse_table);
    emit_opt_table_section(buf, "Submodel.GND_Pulse_Table", "gnd_pulse_table", &submodel.gnd_pulse_table);
    emit_opt_table_section(buf, "Submodel.Pulldown", "pulldown", &submodel.pulldown);
    emit_opt_table_section(buf, "Submodel.Pullup", "pullup", &submodel.pullup);
    emit_opt_table_section(buf, "Submodel.GND_Clamp", "gnd_clamp", &submodel.gnd_clamp);
    emit_opt_table_section(buf, "Submodel.Power_Clamp", "power_clamp", &submodel.power_clamp);

    for waveform in &submodel.rising_waveforms {
        serialize_waveform(buf, "Submodel.Rising_Waveform", waveform);
    }
    for waveform in &submodel.falling_waveforms {
        serialize_waveform(buf, "Submodel.Falling_Waveform", waveform);
    }

    let _ = writeln!(buf);
}

fn serialize_external_circuit(buf: &mut String, circuit: &IBIS_External_Circuit) {
    let _ = writeln!(buf, "[[External_Circuit]]");
    emit_string(buf, "language", &circuit.language);
    emit_string(buf, "corner", &circuit.corner);
    emit_opt_string(buf, "parameters", &circuit.parameters);
    emit_opt_string(buf, "ports", &circuit.ports);
    let _ = writeln!(buf);
}

fn serialize_test_data(buf: &mut String, data: &IBIS_Test_Data) {
    let _ = writeln!(buf, "[[Test_Data]]");
    emit_string(buf, "test_data", &data.test_data);
    emit_string(buf, "test_data_type", &data.test_data_type);
    emit_string(buf, "driver_model", &data.driver_model);
    emit_string(buf, "test_load", &data.test_load);
    let _ = writeln!(buf);
}

fn serialize_test_load(buf: &mut String, load: &IBIS_Test_Load) {
    let _ = writeln!(buf, "[[Test_Load]]");
    emit_string(buf, "test_load", &load.test_load);
    emit_string(buf, "test_load_type", &load.test_load_type);
    emit_opt_quantity(buf, "c1_near", &load.c1_near);
    emit_opt_quantity(buf, "rs_near", &load.rs_near);
    emit_opt_quantity(buf, "ls_near", &load.ls_near);
    emit_opt_quantity(buf, "c2_near", &load.c2_near);
    emit_opt_quantity(buf, "rp1_near", &load.rp1_near);
    emit_opt_quantity(buf, "td", &load.td);
    emit_opt_quantity(buf, "zo", &load.zo);
    emit_opt_quantity(buf, "rp1_far", &load.rp1_far);
    emit_opt_quantity(buf, "c2_far", &load.c2_far);
    emit_opt_quantity(buf, "ls_far", &load.ls_far);
    emit_opt_quantity(buf, "rs_far", &load.rs_far);
    emit_opt_quantity(buf, "c1_far", &load.c1_far);
    emit_opt_string(buf, "receiver_model", &load.receiver_model);
    let _ = writeln!(buf);
}

fn serialize_define_package_model(buf: &mut String, package: &IBIS_Define_Package_Model) {
    let _ = writeln!(buf, "[[Define_Package_Model]]");
    emit_string(buf, "define_package_model", &package.define_package_model);
    emit_string(buf, "manufacturer", &package.manufacturer);
    emit_string(buf, "oem", &package.oem);
    emit_string(buf, "description", &package.description);
    emit_string(buf, "number_of_sections", &package.number_of_sections);
    emit_string(buf, "number_of_pins", &package.number_of_pins);
    let _ = writeln!(buf);
}

fn serialize_interconnect_model_set(buf: &mut String, set: &IBIS_Interconnect_Model_Set) {
    let _ = writeln!(buf, "[[Interconnect_Model_Set]]");
    emit_string(buf, "interconnect_model_set", &set.interconnect_model_set);
    emit_string(buf, "manufacturer", &set.manufacturer);
    emit_string(buf, "description", &set.description);
    for model in &set.interconnect_models {
        let _ = writeln!(buf, "[[Interconnect_Model_Set.Interconnect_Model]]");
        emit_string(buf, "interconnect_model", &model.interconnect_model);
        emit_opt_string(buf, "file_ts", &model.file_ts);
        emit_opt_string(buf, "file_ibis_iss", &model.file_ibis_iss);
    }
    let _ = writeln!(buf);
}

// -----------------------------------------------------------------------------
// Entry point
// -----------------------------------------------------------------------------

/// Serialize the strongly-typed model [`IBIS_File`] into a TOML string
/// (including `[[array-of-tables]]`).
pub fn serialize_ibis_file(file: &IBIS_File) -> String {
    let mut buf = String::new();

    serialize_header(&mut buf, &file.header);

    for component in &file.components {
        serialize_component(&mut buf, component);
    }
    for selector in &file.model_selectors {
        serialize_model_selector(&mut buf, selector);
    }
    for model in file.models.values() {
        serialize_model(&mut buf, model);
    }
    for submodel in file.submodels.values() {
        serialize_submodel(&mut buf, submodel);
    }
    for circuit in &file.external_circuits {
        serialize_external_circuit(&mut buf, circuit);
    }
    for data in &file.test_data {
        serialize_test_data(&mut buf, data);
    }
    for load in file.test_loads.values() {
        serialize_test_load(&mut buf, load);
    }
    for package in file.package_models.values() {
        serialize_define_package_model(&mut buf, package);
    }
    for set in &file.interconnect_model_sets {
        serialize_interconnect_model_set(&mut buf, set);
    }

    buf
}

#[cfg(test)]
mod tests {
    #![allow(clippy::field_reassign_with_default)]
    use super::*;
    use indexmap::IndexMap;

    fn q(value: f64, unit: Option<&str>) -> Quantity {
        Quantity { value: Scalar::Number(value), unit: unit.map(str::to_string) }
    }

    /// 构造一个 corner 表（col_header = ["typ", "min", "max"]，单行）。
    fn corner_table(cells: &[Quantity]) -> Table {
        Table {
            col_header: vec!["typ".into(), "min".into(), "max".into()],
            matrix: vec![cells.iter().cloned().map(Cell::Quantity).collect()],
        }
    }

    fn sample_file() -> IBIS_File {
        let mut file = IBIS_File::default();
        file.header.ibis_ver = "2.1".into();
        file.header.file_name = "test.ibs".into();
        file.header.file_rev = "1.0".into();

        let mut component = IBIS_Component::default();
        component.component = "STM32F103".into();
        component.manufacturer = "STMicro".into();
        component.package = Some(Component_Package {
            r_pkg: corner_table(&[q(0.1, Some("ohm")), q(0.09, Some("ohm")), q(0.11, Some("ohm"))]),
            l_pkg: corner_table(&[q(1.0, Some("nH"))]),
            c_pkg: Table::default(),
        });
        component.pin = Some(Component_Pin {
            pin: Table {
                col_header: vec![
                    "signal_name".into(),
                    "model_name".into(),
                    "R_pin".into(),
                    "L_pin".into(),
                    "C_pin".into(),
                ],
                matrix: vec![vec![
                    Cell::Text("PC13".into()),
                    Cell::Text("M1".into()),
                    Cell::Quantity(q(0.5, Some("p"))),
                ]],
            },
        });
        file.components.push(component);

        let mut model = IBIS_Model::default();
        model.model = "M1".into();
        model.model_type = "I/O".into();
        model.c_comp = Some(corner_table(&[q(1.12, Some("p")), q(0.79, Some("p")), q(1.15, Some("p"))]));
        model.voltage_range = Some(corner_table(&[q(3.3, Some("V")), q(2.0, Some("V")), q(3.6, Some("V"))]));
        model.pulldown = Some(Table {
            col_header: vec!["voltage".into(), "i_typ".into(), "i_min".into(), "i_max".into()],
            matrix: vec![
                vec![Cell::Quantity(q(-3.3, None)), Cell::Quantity(q(-2.0, Some("mA"))), Cell::Quantity(q(-2.0, Some("mA"))), Cell::Quantity(q(-1.0, Some("mA")))],
                vec![Cell::Quantity(q(0.0, None)), Cell::Quantity(q(0.0, Some("mA"))), Cell::Quantity(q(0.0, Some("mA"))), Cell::Quantity(q(0.0, Some("mA")))],
            ],
        });
        let mut models = IndexMap::new();
        models.insert("M1".into(), model);
        file.models = models;

        file
    }

    #[test]
    fn test_serialize_header_and_quantity_fields() {
        let output = serialize_ibis_file(&sample_file());
        assert!(output.contains("[File_Header]"));
        assert!(output.contains("ibis_ver = \"2.1\""));
        assert!(output.contains("[[Component]]"));
        assert!(output.contains("r_pkg = { col_header = [\"typ\", \"min\", \"max\"], data = [\"0.1ohm\", \"0.09ohm\", \"0.11ohm\"] }"));
        assert!(output.contains("[Component.Pin]"));
        assert!(output.contains("Pin = { col_header = [\"signal_name\", \"model_name\", \"R_pin\", \"L_pin\", \"C_pin\"], data = ["));
        assert!(output.contains("[\"PC13\", \"M1\", \"0.5p\"]"));
    }

    #[test]
    fn test_serialize_table_matrix() {
        let output = serialize_ibis_file(&sample_file());
        assert!(output.contains("pulldown = {"));
        assert!(output.contains("col_header = [\"voltage\", \"i_typ\", \"i_min\", \"i_max\"]"));
        assert!(output.contains("[\"-3.3\", \"-2mA\", \"-2mA\", \"-1mA\"]"));
    }

    #[test]
    fn test_serialize_corner_and_range() {
        let output = serialize_ibis_file(&sample_file());
        assert!(output.contains("voltage_range = { col_header = [\"typ\", \"min\", \"max\"], data = [\"3.3V\", \"2V\", \"3.6V\"] }"));
        assert!(output.contains("c_comp = { col_header = [\"typ\", \"min\", \"max\"], data = [\"1.12p\", \"0.79p\", \"1.15p\"] }"));
    }

    #[test]
    fn test_option_none_is_omitted() {
        let output = serialize_ibis_file(&sample_file());
        assert!(!output.contains("l_pin ="), "l_pin (None) should be omitted");
    }
}
