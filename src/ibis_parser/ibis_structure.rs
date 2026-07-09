//! IBIS 7.0 semantic AST structures (ground-truth type definitions).
//!
//! This module defines the complete set of strongly-typed data structures
//! representing a fully parsed IBIS file, as specified by the IBIS 7.0 standard.
//! All types are intended to be constructed by the second-pass semantic analysis
//! phase (not yet implemented).
//!
//! # Organisation
//!
//! | Section | Types |
//! |---------|-------|
//! | [File Header](IBIS_FileHeader) | `IBIS_FileHeader` |
//! | [Component](IBIS_Component) | `IBIS_Component`, `PinInfo`, `DiffPin`, etc. |
//! | [Model](IBIS_Model) | `IBIS_Model`, `Ramp`, `WaveformFixture`, etc. |
//! | Package Model | `IBIS_DefinePackageModel`, `PackagePinNumbers` |
//! | Other | `IBIS_Submodel`, `IBIS_ExternalCircuit`, `IBIS_TestData`, etc. |

#![allow(non_camel_case_types)]

use std::collections::HashMap;

// -----------------------------------------------------------------------------
// Core Type Definitions & Tables
// -----------------------------------------------------------------------------

/// A generic typical/min/max corner-value container.
///
/// Many IBIS parameters are specified as a triplet of values representing
/// typical, minimum, and maximum operating corners.
///
/// # Parameters
///
/// * `typ` — The typical (nominal) value.
/// * `min` — The minimum value, if specified.
/// * `max` — The maximum value, if specified.
#[derive(Debug)]
pub struct Triplet<T> {
    pub typ: T,
    pub min: Option<T>,
    pub max: Option<T>,
}

/// Shorthand for a floating-point [`Triplet<f64>`].
///
/// Used throughout the IBIS structure for electrical parameters such as
/// resistances, capacitances, voltages, and timings.
pub type IBIS_CornerValue = Triplet<f64>;

/// Tabular data wrapper for curves, wave tables, and RLGC matrices
#[derive(Debug)]
pub struct IBIS_TableData {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<f64>>,
}



// -----------------------------------------------------------------------------
// Root AST Complete File Structure
// -----------------------------------------------------------------------------

/// Ground-truth Abstract Syntax Tree root for parsed IBIS models
#[derive(Debug)]
pub struct IBIS_File {
    pub header: IBIS_FileHeader,
    pub components: Vec<IBIS_Component>,
    pub model_selectors: Vec<IBIS_ModelSelector>,
    pub models: HashMap<String, IBIS_Model>,
    pub submodels: HashMap<String, IBIS_Submodel>,
    pub external_circuits: Vec<IBIS_ExternalCircuit>,
    pub test_data: Vec<IBIS_TestData>,
    pub test_loads: HashMap<String, IBIS_TestLoad>,
    pub package_models: HashMap<String, IBIS_DefinePackageModel>,
    pub interconnect_model_sets: Vec<IBIS_InterconnectModelSet>,
}



// -----------------------------------------------------------------------------
// File Header Section
// -----------------------------------------------------------------------------

/// Parsing destination for the file header properties
#[derive(Debug, Default)]
pub struct IBIS_FileHeader {
    pub ibis_ver: String,
    pub comment_char: Option<char>,
    pub file_name: String,
    pub file_rev: String,
    pub date: Option<String>,
    pub source: Option<String>,
    pub notes: Option<String>,
    pub disclaimer: Option<String>,
    pub copyright: Option<String>,
}



// -----------------------------------------------------------------------------
// Component Section Sub-structures
// -----------------------------------------------------------------------------

/// Default global pin parasitic parameters
#[derive(Debug)]
pub struct ComponentPackage {
    pub r_pkg: IBIS_CornerValue,
    pub l_pkg: IBIS_CornerValue,
    pub c_pkg: IBIS_CornerValue,
}

/// Detailed per-pin linkage mapping
#[derive(Debug)]
pub struct PinInfo {
    pub pin_name: String,
    pub signal_name: String,
    pub model_name: String,
    pub r_pin: Option<f64>,
    pub l_pin: Option<f64>,
    pub c_pin: Option<f64>,
}

#[derive(Debug)]
pub struct AlternatePackageModels {
    pub alternate_package_models: Vec<String>,
}

#[derive(Debug)]
pub struct PackageModelSelector {
    pub package_model: String,
    pub alternates: Option<AlternatePackageModels>,
}

/// Supply and reference rail routing maps
#[derive(Debug)]
pub struct PinMapping {
    pub pin_name: String,
    pub pulldown_ref: String,
    pub pullup_ref: String,
    pub gnd_clamp_ref: Option<String>,
    pub power_clamp_ref: Option<String>,
    pub ext_ref: Option<String>,
}

#[derive(Debug)]
pub struct BusLabel {
    pub bus_label: String,
    pub signal_name: String,
}

#[derive(Debug)]
pub struct DieSupplyPads {
    pub pad_name: String,
    pub signal_name: String,
    pub bus_label: Option<String>,
}

/// Differential signaling configuration and timing parameters
#[derive(Debug)]
pub struct DiffPin {
    pub pin_name: String,
    pub inv_pin: String,
    pub vdiff: f64,
    pub tdelay_typ: f64,
    pub tdelay_min: Option<f64>,
    pub tdelay_max: Option<f64>,
}

#[derive(Debug)]
pub struct RepeaterPin {
    pub tx_non_inv_pin: String,
}

#[derive(Debug)]
pub struct SeriesPinMapping {
    pub pin_1: String,
    pub pin_2: String,
    pub model_name: String,
    pub function_table_group: Option<String>,
}

#[derive(Debug)]
pub struct SeriesSwitchGroups {
    pub on: String,
    pub off: String,
}

#[derive(Debug)]
pub struct CircuitCall {
    pub signal_pin: String,
    pub diff_signal_pins: Option<String>,
    pub series_pins: Option<String>,
    pub port_map: Option<String>,
    pub converter_parameters: Option<String>,
    pub parameters: Option<String>,
}

#[derive(Debug)]
pub struct PinEmi {
    pub domain_name: String,
    pub clock_div: String,
}

#[derive(Debug)]
pub struct PinDomainEmi {
    pub percentage: f64,
}

/// Component level EMI allocation parameters
#[derive(Debug)]
pub struct BeginEmiComponent {
    pub domain: String,
    pub cpd: f64, 
    pub c_heatsink_gnd: f64,
    pub c_heatsink_float: f64,
    pub pin_emi: Vec<PinEmi>,
    pub pin_domain_emi: Vec<PinDomainEmi>,
}

/// Representation of the physical chip or board module
#[derive(Debug)]
pub struct IBIS_Component {
    pub component: String,
    pub si_location: Option<String>,     
    pub timing_location: Option<String>, 
    pub manufacturer: String,
    pub package: Option<ComponentPackage>,
    pub pins: Vec<PinInfo>,
    pub package_model: Option<PackageModelSelector>,
    pub interconnect_model_groups: Vec<String>,
    pub pin_mappings: Vec<PinMapping>,
    pub bus_labels: Vec<BusLabel>,
    pub die_supply_pads: Vec<DieSupplyPads>,
    pub diff_pins: Vec<DiffPin>,
    pub repeater_pins: Vec<RepeaterPin>,
    pub series_pin_mappings: Vec<SeriesPinMapping>,
    pub series_switch_groups: Vec<SeriesSwitchGroups>,
    pub node_declarations: Vec<String>,
    pub circuit_calls: Vec<CircuitCall>,
    pub emi: Option<BeginEmiComponent>,
}



// -----------------------------------------------------------------------------
// Model Selector Section
// -----------------------------------------------------------------------------

/// Multi-model grouping structure
#[derive(Debug)]
pub struct IBIS_ModelSelector {
    pub model_selector: String,
    pub models: Vec<String>,
}



// -----------------------------------------------------------------------------
// Model Section Sub-structures
// -----------------------------------------------------------------------------

/// Static and dynamic limits for signal overshoot
#[derive(Debug)]
pub struct ModelSpec {
    pub vinh: Option<f64>,
    pub vinl: Option<f64>,
    pub vinh_plus: Option<f64>,
    pub vinh_minus: Option<f64>,
    pub vinl_plus: Option<f64>,
    pub vinl_minus: Option<f64>,
    pub s_overshoot_high: Option<f64>,
    pub s_overshoot_low: Option<f64>,
    pub d_overshoot_high: Option<f64>,
    pub d_overshoot_low: Option<f64>,
    pub d_overshoot_time: Option<f64>,
    pub d_overshoot_area_h: Option<f64>,
    pub d_overshoot_area_l: Option<f64>,
    pub d_overshoot_ampl_h: Option<f64>,
    pub d_overshoot_ampl_l: Option<f64>,
    pub pulse_high: Option<f64>,
    pub pulse_low: Option<f64>,
    pub pulse_time: Option<f64>,
    pub weak_r: Option<f64>,
    pub weak_i: Option<f64>,
    pub weak_v: Option<f64>,
}

/// AC and DC receiver verification thresholds
#[derive(Debug)]
pub struct ReceiverThresholds {
    pub vth: f64,
    pub vth_min: Option<f64>,
    pub vth_max: Option<f64>,
    pub vinh_ac: Option<f64>,
    pub vinh_dc: Option<f64>,
    pub vinl_ac: Option<f64>,
    pub vinl_dc: Option<f64>,
    pub threshold_sensitivity: Option<String>,
    pub reference_supply: Option<String>,
    pub vcross_low: Option<f64>,
    pub vcross_high: Option<f64>,
    pub vdiff_ac: Option<f64>,
    pub vdiff_dc: Option<f64>,
    pub tslew_ac: Option<f64>,
    pub tdiffslew_ac: Option<f64>,
}

/// Temperature-dependent die capacitance parameters
#[derive(Debug)]
pub struct CCompCorner {
    pub c_comp: IBIS_CornerValue,
    pub c_comp_pullup: Option<IBIS_CornerValue>,
    pub c_comp_pulldown: Option<IBIS_CornerValue>,
    pub c_comp_power_clamp: Option<IBIS_CornerValue>,
    pub c_comp_gnd_clamp: Option<IBIS_CornerValue>,
}

/// Slew rate transition metrics
#[derive(Debug)]
pub struct Ramp {
    pub dv_dt_r: IBIS_CornerValue,
    pub dv_dt_f: IBIS_CornerValue,
    pub r_load: Option<f64>,
}

/// Simulation testbench loading fixture parameters
#[derive(Debug)]
pub struct WaveformFixture {
    pub r_fixture: f64,
    pub v_fixture: f64,
    pub v_fixture_min: Option<f64>,
    pub v_fixture_max: Option<f64>,
    pub c_fixture: Option<f64>,
    pub l_fixture: Option<f64>,
    pub r_dut: Option<f64>,
    pub l_dut: Option<f64>,
    pub c_dut: Option<f64>,
    pub composite_current: Option<IBIS_TableData>,
}

#[derive(Debug)]
pub struct ExternalModel {
    pub language: String,
    pub corner: String,
    pub parameters: Option<String>,
    pub converter_parameters: Option<String>,
    pub ports: Option<String>,
    pub d_to_a: Option<String>,
    pub a_to_d: Option<String>,
}

#[derive(Debug)]
pub struct AlgorithmicModel {
    pub executable: String,
    pub executable_rx: Option<String>,
    pub executable_tx: Option<String>,
}

#[derive(Debug)]
pub struct BeginEmiModel {
    pub model_emi_type: String,
    pub model_domain: String,
}

/// Complete core buffer driver/receiver model properties
#[derive(Debug)]
pub struct IBIS_Model {
    pub model: String,
    pub model_type: String,
    pub polarity: Option<String>,
    pub enable: Option<String>,
    pub vinl: Option<f64>,
    pub vinh: Option<f64>,
    pub c_comp: Option<IBIS_CornerValue>,
    pub c_comp_pullup: Option<IBIS_CornerValue>,
    pub c_comp_pulldown: Option<IBIS_CornerValue>,
    pub c_comp_power_clamp: Option<IBIS_CornerValue>,
    pub c_comp_gnd_clamp: Option<IBIS_CornerValue>,
    pub vmeas: Option<f64>,
    pub cref: Option<f64>,
    pub rref: Option<f64>,
    pub vref: Option<f64>,
    pub rref_diff: Option<f64>,
    pub cref_diff: Option<f64>,

    pub model_spec: Option<ModelSpec>,
    pub receiver_thresholds: Option<ReceiverThresholds>,
    pub add_submodel: Option<String>,
    pub driver_schedule: Option<String>,
    
    pub temperature_range: Option<IBIS_CornerValue>,
    pub voltage_range: Option<IBIS_CornerValue>,
    pub pullup_reference: Option<IBIS_CornerValue>,
    pub pulldown_reference: Option<IBIS_CornerValue>,
    pub power_clamp_reference: Option<IBIS_CornerValue>,
    pub gnd_clamp_reference: Option<IBIS_CornerValue>,
    pub external_reference: Option<IBIS_CornerValue>,
    
    pub c_comp_corner: Option<CCompCorner>,
    pub ttgnd: Option<f64>,
    pub ttpower: Option<f64>,

    pub pulldown: Option<IBIS_TableData>,
    pub pullup: Option<IBIS_TableData>,
    pub gnd_clamp: Option<IBIS_TableData>,
    pub power_clamp: Option<IBIS_TableData>,
    pub isso_pu: Option<IBIS_TableData>,
    pub isso_pd: Option<IBIS_TableData>,

    pub rgnd: Option<f64>,
    pub rpower: Option<f64>,
    pub rac: Option<f64>,
    pub cac: Option<f64>,
    pub on: Option<f64>,
    pub off: Option<f64>,

    pub r_series: Option<f64>,
    pub l_series: Option<f64>,
    pub rl_series: Option<f64>,
    pub c_series: Option<f64>,
    pub lc_series: Option<f64>,
    pub rc_series: Option<f64>,
    pub series_current: Option<IBIS_TableData>,
    pub vds: Option<f64>,

    pub ramp: Option<Ramp>,
    pub rising_waveforms: Vec<WaveformFixture>,
    pub falling_waveforms: Vec<WaveformFixture>,
    pub initial_delay_vt: Option<IBIS_TableData>,
    pub initial_delay_it: Option<IBIS_TableData>,
    pub external_model: Option<ExternalModel>,
    pub algorithmic_model: Option<AlgorithmicModel>,
    pub emi: Option<BeginEmiModel>,
}



// -----------------------------------------------------------------------------
// Submodel Section
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct SubmodelSpec {
    pub v_trigger_r: Option<f64>,
    pub v_trigger_f: Option<f64>,
    pub off_delay: Option<f64>,
}

/// Standalone programmable or secondary logic block submodels
#[derive(Debug)]
pub struct IBIS_Submodel {
    pub submodel: String,
    pub submodel_type: String,
    pub submodel_spec: Option<SubmodelSpec>,
    pub power_pulse_table: Option<IBIS_TableData>,
    pub gnd_pulse_table: Option<IBIS_TableData>,
    pub pulldown: Option<IBIS_TableData>,
    pub pullup: Option<IBIS_TableData>,
    pub gnd_clamp: Option<IBIS_TableData>,
    pub power_clamp: Option<IBIS_TableData>,
    pub ramp: Option<Ramp>,
    pub rising_waveforms: Vec<WaveformFixture>,
    pub falling_waveforms: Vec<WaveformFixture>,
    pub initial_delay_vt: Option<IBIS_TableData>,
    pub initial_delay_it: Option<IBIS_TableData>,
}



// -----------------------------------------------------------------------------
// External Circuit Section
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct IBIS_ExternalCircuit {
    pub language: String,
    pub corner: String,
    pub parameters: Option<String>,
    pub converter_parameters: Option<String>,
    pub ports: Option<String>,
    pub d_to_a: Option<String>,
    pub a_to_d: Option<String>,
}



// -----------------------------------------------------------------------------
// Test Data & Test Load Sections
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct IBIS_TestData {
    pub test_data: String,
    pub test_data_type: String,
    pub driver_model: String,
    pub driver_model_inv: Option<String>,
    pub test_load: String,
    pub rising_waveform_near: Option<IBIS_TableData>,
    pub falling_waveform_near: Option<IBIS_TableData>,
    pub rising_waveform_far: Option<IBIS_TableData>,
    pub falling_waveform_far: Option<IBIS_TableData>,
    pub diff_rising_waveform_near: Option<IBIS_TableData>,
    pub diff_falling_waveform_near: Option<IBIS_TableData>,
    pub diff_rising_waveform_far: Option<IBIS_TableData>,
    pub diff_falling_waveform_far: Option<IBIS_TableData>,
}

#[derive(Debug)]
pub struct IBIS_TestLoad {
    pub test_load: String,
    pub test_load_type: String,
    pub c1_near: Option<f64>,
    pub rs_near: Option<f64>,
    pub ls_near: Option<f64>,
    pub c2_near: Option<f64>,
    pub rp1_near: Option<f64>,
    pub rp2_near: Option<f64>,
    pub td: Option<f64>,             
    pub zo: Option<f64>,             
    pub rp1_far: Option<f64>,
    pub rp2_far: Option<f64>,
    pub c2_far: Option<f64>,
    pub ls_far: Option<f64>,
    pub rs_far: Option<f64>,
    pub c1_far: Option<f64>,
    pub v_term1: Option<f64>,
    pub v_term2: Option<f64>,
    pub receiver_model: Option<String>,
    pub receiver_model_inv: Option<String>,
    pub r_diff_near: Option<f64>,
    pub r_diff_far: Option<f64>,
}



// -----------------------------------------------------------------------------
// Define Package Model Section
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct PackageMatrix {
    pub bandwidth: Option<f64>,
    pub row: IBIS_TableData,
}

#[derive(Debug)]
pub struct PackagePinNumbers {
    pub len: f64,
    pub l: Option<f64>,
    pub r: Option<f64>,
    pub c: Option<f64>,
    pub fork: Option<String>,
    pub endfork: Option<String>,
}

/// Detailed multi-section pin package properties and parasitics
#[derive(Debug)]
pub struct IBIS_DefinePackageModel {
    pub define_package_model: String,
    pub manufacturer: String,
    pub oem: String,
    pub description: String,
    pub number_of_sections: usize,
    pub number_of_pins: usize,
    pub pin_numbers: Vec<PackagePinNumbers>,
    pub merged_pins: Vec<String>,
    pub resistance_matrix: Option<PackageMatrix>,
    pub inductance_matrix: Option<PackageMatrix>,
    pub capacitance_matrix: Option<PackageMatrix>,
}



// -----------------------------------------------------------------------------
// Interconnect Model Set Section
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct InterconnectModel {
    pub interconnect_model: String,
    pub param: Option<String>,
    pub file_ts: Option<String>,     
    pub file_ibis_iss: Option<String>, 
    pub unused_port_termination: Option<f64>,
    pub number_of_terminals: Option<usize>,
}

#[derive(Debug)]
pub struct IBIS_InterconnectModelSet {
    pub interconnect_model_set: String,
    pub manufacturer: String,
    pub description: String,
    pub interconnect_models: Vec<InterconnectModel>,
}

