//! Lookup primitives — query the cached schema tree by keyword.
//!
//! The primitives are grouped by what they reach into, each in its own inline
//! module:
//!
//! | Inline module | Responsibility |
//! |---------------|----------------|
//! | `schema` | the whole tree and the virtual `[File_Header]` container |
//! | `keyword` | sections reached by keyword, at a known scope or any depth |
//! | `param` | named sub-parameters inside one section |
//!
//! Every primitive normalizes its keyword before comparing, so matching is
//! case-insensitive and treats `_` as a space.

// =============================================================================
// lookup — query primitives over the cached schema
//
// Design constraints:
//   - Read-only: no primitive ever mutates the tree or writes anything back;
//   - Total: a miss is `None`, never an error — whether a keyword is registered
//     is a question about the data, not a failure;
//   - Breadth-first whenever a name may appear at several depths, so the
//     shallowest registration wins and the answer is deterministic.
// =============================================================================

pub use keyword::{find_child, find_descendant, find_level, find_root};
pub use param::find_field;
pub use schema::{file_header_section, load_schema};

/// Schema-level lookups — reach the whole tree and the virtual container.
mod schema {
    use crate::schema::loader::{file_header_container, top_level_sections};
    use crate::schema::spec::SectionSpec;

    /// Returns the top-level keyword sections of the schema.
    ///
    /// The nine file header entries are **not** part of this list; they are grouped
    /// under the virtual container returned by [`file_header_section`].
    ///
    /// # Returns
    ///
    /// * `&'static [SectionSpec]` — Top-level sections in `ibis_schema.toml` order.
    ///
    /// # Panics
    ///
    /// Panics on the first call if `ibis_schema.toml` cannot be parsed or violates
    /// the metadata convention (missing `__schema__` / `__header__`, unknown enum
    /// value). The schema is a compile-time asset, so such a failure is a
    /// programming error rather than a runtime condition.
    pub fn load_schema() -> &'static [SectionSpec] {
        top_level_sections()
    }

    /// Returns the virtual `[File_Header]` container.
    ///
    /// Its `fields` hold the names of the nine file header entries (used by the
    /// frontend to decide whether a keyword is a header field) and its `children`
    /// hold their full specifications.
    ///
    /// # Returns
    ///
    /// * `&'static SectionSpec` — Container named `File_Header`.
    ///
    /// # Panics
    ///
    /// Panics on the first call if `ibis_schema.toml` is malformed (see
    /// [`load_schema`]).
    pub fn file_header_section() -> &'static SectionSpec {
        file_header_container()
    }
}

/// Keyword lookups — find sections by keyword, at a known scope or any depth.
mod keyword {
    use crate::schema::spec::{normalize_keyword, SectionSpec};

    /// Finds a top-level section by keyword.
    ///
    /// Matching ignores case and treats `_` as a space, so both `"Model Selector"`
    /// and `"Model_Selector"` are accepted.
    ///
    /// # Parameters
    ///
    /// * `keyword` — Keyword to look up (e.g. `"Component"`, `"Model Selector"`).
    ///
    /// # Returns
    ///
    /// * `Some(&'static SectionSpec)` — The matching top-level section.
    /// * `None` — Not registered; file header entries are reached through
    ///   [`file_header_section`](super::schema::file_header_section) instead.
    ///
    /// # Panics
    ///
    /// Panics on the first call if `ibis_schema.toml` is malformed.
    pub fn find_root(keyword: &str) -> Option<&'static SectionSpec> {
        let normalized = normalize_keyword(keyword);
        let sections = super::schema::load_schema();
        sections
            .iter()
            .find(|section| normalize_keyword(&section.name) == normalized)
    }

    /// Finds the nesting level of a keyword in the schema tree.
    ///
    /// Levels are the depths of the schema tree: `1` for a top-level section
    /// (e.g. `Component`, `Model`, `Submodel`), `2` for its direct children
    /// (e.g. `Component.Manufacturer`, `Component.Package`), `3` for their
    /// children, and so on. When the same keyword name is registered at several
    /// depths, the **shallowest** registration wins.
    ///
    /// This is the frontend's only source for the level of a `[Keyword]`; the
    /// level is never hard-coded.
    ///
    /// # Parameters
    ///
    /// * `keyword` — Keyword to look up (e.g. `"Component"`, `"Pin"`).
    ///
    /// # Returns
    ///
    /// * `Some(usize)` — The depth of the shallowest matching section.
    /// * `None` — The keyword has no place in the schema tree. File header entries
    ///   are reached through
    ///   [`file_header_section`](super::schema::file_header_section), so they
    ///   answer `None` too.
    ///
    /// # Panics
    ///
    /// Panics on the first call if `ibis_schema.toml` is malformed (see
    /// [`load_schema`](super::schema::load_schema)).
    pub fn find_level(keyword: &str) -> Option<usize> {
        let normalized = normalize_keyword(keyword);
        let mut frontier: Vec<&'static SectionSpec> =
            super::schema::load_schema().iter().collect();
        let mut level = 1;
        while !frontier.is_empty() {
            let mut next_frontier: Vec<&'static SectionSpec> = Vec::new();
            for candidate in frontier {
                if normalize_keyword(&candidate.name) == normalized {
                    return Some(level);
                }
                next_frontier.extend(candidate.children.iter());
            }
            frontier = next_frontier;
            level += 1;
        }
        None
    }

    /// Finds a direct child section by keyword.
    ///
    /// # Parameters
    ///
    /// * `parent` — Parent section spec, usually obtained from [`find_root`].
    /// * `keyword` — Child keyword name (e.g. `"Pin"`).
    ///
    /// # Returns
    ///
    /// * `Some(&SectionSpec)` — The matching direct child.
    /// * `None` — No such child; deeper sections need [`find_descendant`].
    pub fn find_child<'a>(parent: &'a SectionSpec, keyword: &str) -> Option<&'a SectionSpec> {
        let normalized = normalize_keyword(keyword);
        parent
            .children
            .iter()
            .find(|section| normalize_keyword(&section.name) == normalized)
    }

    /// Finds a section among **all descendants** of `parent` (breadth-first, the
    /// parent itself excluded).
    ///
    /// The frontend only recurses on top-level sections (`level == 1`), so sections
    /// three levels deep are flattened into second-level siblings in the AST (for
    /// example `[Model Data]` and `[Resistance Matrix]` both appear directly under
    /// `[Define Package Model]`). `content_parse` therefore resolves a spec with a
    /// child → descendant → root fallback.
    ///
    /// # Parameters
    ///
    /// * `parent` — Parent section spec.
    /// * `keyword` — Keyword to look up at any depth below `parent`.
    ///
    /// # Returns
    ///
    /// * `Some(&SectionSpec)` — The shallowest matching descendant.
    /// * `None` — No such descendant.
    pub fn find_descendant<'a>(parent: &'a SectionSpec, keyword: &str) -> Option<&'a SectionSpec> {
        let normalized = normalize_keyword(keyword);
        let mut frontier: Vec<&SectionSpec> = parent.children.iter().collect();
        while !frontier.is_empty() {
            let mut next_frontier: Vec<&SectionSpec> = Vec::new();
            for candidate in frontier {
                let candidate_name = normalize_keyword(&candidate.name);
                if candidate_name == normalized {
                    return Some(candidate);
                }
                next_frontier.extend(candidate.children.iter());
            }
            frontier = next_frontier;
        }
        None
    }
}

/// Param lookups — find named sub-parameters inside one section.
mod param {
    use crate::schema::spec::{normalize_keyword, FieldSpec, SectionSpec};

    /// Finds one named sub-parameter of a section.
    ///
    /// # Parameters
    ///
    /// * `section` — Section spec to inspect.
    /// * `key` — Param name (e.g. `"R_pin"`, `"dv/dt_r"`).
    ///
    /// # Returns
    ///
    /// * `Some(&FieldSpec)` — The declared param with its [`ParamType`](crate::schema::ParamType).
    /// * `None` — The section does not declare this param.
    pub fn find_field<'a>(section: &'a SectionSpec, key: &str) -> Option<&'a FieldSpec> {
        let normalized = normalize_keyword(key);
        section
            .fields
            .iter()
            .find(|field| normalize_keyword(&field.key) == normalized)
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::{
        file_header_section, find_child, find_descendant, find_level, find_root, normalize_keyword,
        to_snake_key, HeaderFormat, Occurrence, ParamType, SectionFormat, SectionSpec,
    };

    /// Whether the keyword is registered as a file header field.
    fn is_file_header_keyword(keyword: &str) -> bool {
        let normalized = normalize_keyword(keyword);
        let header = file_header_section();
        header
            .fields
            .iter()
            .any(|field| normalize_keyword(&field.key) == normalized)
    }

    /// Looks up a top-level section, panicking when it is missing.
    fn root(keyword: &str) -> &'static SectionSpec {
        find_root(keyword).unwrap_or_else(|| panic!("top-level section `{keyword}` is missing"))
    }

    #[test]
    fn test_file_header_container_holds_nine_entries() {
        let header = file_header_section();
        assert_eq!(header.name, "File_Header");
        assert_eq!(header.occurrence, Occurrence::Once);
        assert_eq!(header.children.len(), 9);
        assert_eq!(header.fields.len(), 9);

        let known_keywords = [
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
        for keyword in known_keywords {
            assert!(is_file_header_keyword(keyword), "`{keyword}` must be a header field");
        }
        assert!(!is_file_header_keyword("Component"));
        assert!(!is_file_header_keyword("Pin"));
    }

    #[test]
    fn test_file_header_entries_are_not_top_level_sections() {
        assert!(find_root("IBIS ver").is_none());
        assert!(find_root("Notes").is_none());
    }

    #[test]
    fn test_root_occurrence_follows_table_versus_array() {
        let multiple_keywords = [
            "Component",
            "Model",
            "Model Selector",
            "Submodel",
            "Test Data",
            "Test Load",
            "External Circuit",
            "Define Package Model",
            "Interconnect Model Set",
        ];
        for keyword in multiple_keywords {
            assert_eq!(root(keyword).occurrence, Occurrence::Multiple, "`[[{keyword}]]`");
        }
    }

    #[test]
    fn test_child_occurrence_follows_table_versus_array() {
        let component = root("Component");
        let pin = find_child(component, "Pin").expect("Component.Pin is registered");
        assert_eq!(pin.occurrence, Occurrence::Once);

        let pin_mapping =
            find_child(component, "Pin Mapping").expect("Component.Pin_Mapping is registered");
        assert_eq!(pin_mapping.occurrence, Occurrence::Multiple);
    }

    #[test]
    fn test_component_pin_fields_keep_declaration_order_and_types() {
        let pin = find_child(root("Component"), "Pin").expect("Component.Pin is registered");
        assert_eq!(pin.format, SectionFormat::Table);
        assert_eq!(pin.header_format, HeaderFormat::TableHeader);

        // `[Pin]` rows carry an unnamed leading pin-name column, declared as `col0`.
        let keys: Vec<&str> = pin.fields.iter().map(|field| field.key.as_str()).collect();
        assert_eq!(
            keys,
            vec!["col0", "signal_name", "model_name", "R_pin", "L_pin", "C_pin"]
        );

        let types: Vec<ParamType> =
            pin.fields.iter().map(|field| field.param_type.clone()).collect();
        let expected_types = vec![
            ParamType::Text,
            ParamType::Text,
            ParamType::Text,
            ParamType::Quantity,
            ParamType::Quantity,
            ParamType::Quantity,
        ];
        assert_eq!(types, expected_types);
    }

    #[test]
    fn test_param_keys_with_symbols_keep_original_spelling() {
        let ramp = find_child(root("Model"), "Ramp").expect("Model.Ramp is registered");
        let keys: Vec<&str> = ramp.fields.iter().map(|field| field.key.as_str()).collect();
        assert_eq!(keys, vec!["dv/dt_r", "dv/dt_f", "r_load"]);
        assert_eq!(ramp.fields[0].param_type, ParamType::Corner);
        assert_eq!(ramp.fields[2].param_type, ParamType::Quantity);

        let model_spec =
            find_child(root("Model"), "Model Spec").expect("Model.Model_Spec is registered");
        let has_signed_key = model_spec.fields.iter().any(|field| field.key == "vinh+");
        assert!(has_signed_key, "`vinh+` must keep its original spelling");
    }

    #[test]
    fn test_package_params_are_corner_triples() {
        let package =
            find_child(root("Component"), "Package").expect("Component.Package is registered");
        let keys: Vec<&str> = package.fields.iter().map(|field| field.key.as_str()).collect();
        assert_eq!(keys, vec!["r_pkg", "l_pkg", "c_pkg"]);
        let all_corner = package
            .fields
            .iter()
            .all(|field| field.param_type == ParamType::Corner);
        assert!(all_corner);
    }

    #[test]
    fn test_reference_sections_use_corner_header() {
        let model = root("Model");
        let corner_keywords = [
            "Temperature Range",
            "Voltage Range",
            "Pullup Reference",
            "Pulldown Reference",
            "Power Clamp Reference",
            "GND Clamp Reference",
            "External Reference",
        ];
        for keyword in corner_keywords {
            let section = find_child(model, keyword)
                .unwrap_or_else(|| panic!("Model.{keyword} must be registered"));
            assert_eq!(section.header_format, HeaderFormat::Corner);
            assert!(section.fields.is_empty());
        }
    }

    #[test]
    fn test_same_keyword_in_different_scopes_stays_isolated() {
        let component = root("Component");
        let component_mapping =
            find_child(component, "Pin Mapping").expect("Component.Pin_Mapping is registered");
        let component_keys: Vec<&str> =
            component_mapping.fields.iter().map(|field| field.key.as_str()).collect();
        assert_eq!(
            component_keys,
            vec![
                "col0",
                "pulldown_ref",
                "pullup_ref",
                "gnd_clamp_ref",
                "power_clamp_ref",
                "ext_ref",
            ]
        );

        let interconnect = root("Interconnect Model Set");
        let interconnect_mapping = find_child(interconnect, "Pin Mapping")
            .expect("Interconnect_Model_Set.Pin_Mapping is registered");
        let interconnect_keys: Vec<&str> =
            interconnect_mapping.fields.iter().map(|field| field.key.as_str()).collect();
        assert_eq!(
            interconnect_keys,
            vec![
                "col0",
                "pulldown_ref",
                "pullup_ref",
                "gnd_clamp_ref",
                "power_clamp_ref",
                "ext_ref",
            ]
        );
    }

    #[test]
    fn test_descendant_lookup_reaches_third_level_sections() {
        let package_model = root("Define Package Model");
        let model_data = find_child(package_model, "Model Data").expect("Model_Data");
        let resistance_matrix = find_child(model_data, "Resistance Matrix");
        assert!(resistance_matrix.is_some());

        // The AST flattens third-level sections into second-level siblings, so a
        // direct child lookup fails while a descendant lookup must succeed.
        assert!(find_child(package_model, "Resistance Matrix").is_none());
        assert!(find_descendant(package_model, "Resistance Matrix").is_some());
        assert!(find_descendant(package_model, "Capacitance Matrix").is_some());
    }

    #[test]
    fn test_find_level_counts_the_depth_from_the_top() {
        // Level 1: the top-level containers.
        assert_eq!(find_level("Component"), Some(1));
        assert_eq!(find_level("Model"), Some(1));
        assert_eq!(find_level("Submodel"), Some(1));

        // Level 2: their direct children.
        assert_eq!(find_level("Manufacturer"), Some(2));
        assert_eq!(find_level("Pin"), Some(2));
        assert_eq!(find_level("Ramp"), Some(2));

        // Level 3: their grandchildren.
        assert_eq!(find_level("Resistance Matrix"), Some(3));
        assert_eq!(find_level("Composite Current"), Some(3));

        // File header entries and unknown keywords have no place in the tree.
        assert!(find_level("IBIS ver").is_none());
        assert!(find_level("Mystery Section").is_none());
    }

    #[test]
    fn test_child_level_is_one_deeper_than_its_parent() {
        let component_level = find_level("Component").expect("Component");
        let pin_level = find_level("Pin").expect("Pin");
        assert_eq!(pin_level, component_level + 1);

        let package_model_level = find_level("Define Package Model").expect("Define Package Model");
        let model_data_level = find_level("Model Data").expect("Model Data");
        let resistance_matrix_level = find_level("Resistance Matrix").expect("Resistance Matrix");
        assert_eq!(model_data_level, package_model_level + 1);
        assert_eq!(resistance_matrix_level, model_data_level + 1);
    }

    #[test]
    fn test_required_flags_come_from_schema() {
        let header = file_header_section();
        let ibis_ver = find_child(header, "IBIS Ver").expect("IBIS_Ver is registered");
        assert!(ibis_ver.required);
        let date = find_child(header, "Date").expect("Date is registered");
        assert!(!date.required);

        assert!(root("Component").required);
        assert!(!root("Model Selector").required);

        let manufacturer =
            find_child(root("Component"), "Manufacturer").expect("Manufacturer is registered");
        assert!(manufacturer.required);
    }

    #[test]
    fn test_table_and_textline_formats() {
        let component = root("Component");
        let node_declarations =
            find_child(component, "Node Declarations").expect("Component.Node_Declarations");
        assert_eq!(node_declarations.format, SectionFormat::Table);
        assert_eq!(node_declarations.header_format, HeaderFormat::None);
        assert!(node_declarations.fields.is_empty());

        let notes = find_child(file_header_section(), "Notes").expect("Notes is registered");
        assert_eq!(notes.format, SectionFormat::FileHeader);
        assert_eq!(notes.header_format, HeaderFormat::TextLines);

        let pulldown = find_child(root("Model"), "Pulldown").expect("Model.Pulldown");
        assert_eq!(pulldown.format, SectionFormat::IvTable);
        assert_eq!(pulldown.header_format, HeaderFormat::TableHeader);
        assert!(pulldown.fields.is_empty());
    }

    #[test]
    fn test_iv_curve_sections_use_the_iv_table_format() {
        let iv_keywords = ["Pulldown", "Pullup", "GND Clamp", "Power Clamp"];

        let model = root("Model");
        for keyword in iv_keywords {
            let section = find_child(model, keyword)
                .unwrap_or_else(|| panic!("Model.{keyword} is registered"));
            assert_eq!(section.format, SectionFormat::IvTable, "Model.{keyword}");
        }

        let submodel = root("Submodel");
        for keyword in iv_keywords {
            let section = find_child(submodel, keyword)
                .unwrap_or_else(|| panic!("Submodel.{keyword} is registered"));
            assert_eq!(section.format, SectionFormat::IvTable, "Submodel.{keyword}");
        }

        let test_load = root("Test Load");
        for keyword in iv_keywords {
            let section = find_child(test_load, keyword)
                .unwrap_or_else(|| panic!("Test_Load.{keyword} is registered"));
            assert_eq!(section.format, SectionFormat::IvTable, "Test_Load.{keyword}");
        }

        // The remaining I-V style sections are marked the same way.
        for keyword in ["POWER Table", "GND Table", "Series Current"] {
            let section = find_child(root("Model"), keyword)
                .unwrap_or_else(|| panic!("Model.{keyword} is registered"));
            assert_eq!(section.format, SectionFormat::IvTable, "Model.{keyword}");
        }
        for keyword in ["Power Pulse Table", "GND Pulse Table"] {
            let section = find_child(root("Submodel"), keyword)
                .unwrap_or_else(|| panic!("Submodel.{keyword} is registered"));
            assert_eq!(section.format, SectionFormat::IvTable, "Submodel.{keyword}");
        }
        for waveform in ["Rising Waveform", "Falling Waveform"] {
            let waveform_section = find_child(root("Model"), waveform)
                .unwrap_or_else(|| panic!("Model.{waveform} is registered"));
            let composite_current = find_child(waveform_section, "Composite Current")
                .unwrap_or_else(|| panic!("Model.{waveform}.Composite_Current is registered"));
            assert_eq!(
                composite_current.format,
                SectionFormat::IvTable,
                "Model.{waveform}.Composite_Current"
            );
        }

        // Ordinary tables keep the plain table format.
        let pin = find_child(root("Component"), "Pin").expect("Component.Pin");
        assert_eq!(pin.format, SectionFormat::Table);
    }

    #[test]
    fn test_waveform_spec_and_submodel_shapes() {
        let model = root("Model");
        for keyword in ["Rising Waveform", "Falling Waveform"] {
            let section = find_child(model, keyword)
                .unwrap_or_else(|| panic!("Model.{keyword} is registered"));
            assert_eq!(section.format, SectionFormat::VtTable, "Model.{keyword}");
        }

        let submodel = root("Submodel");
        for keyword in ["Rising Waveform", "Falling Waveform"] {
            let section = find_child(submodel, keyword)
                .unwrap_or_else(|| panic!("Submodel.{keyword} is registered"));
            assert_eq!(section.format, SectionFormat::VtTable, "Submodel.{keyword}");
        }

        let test_data = root("Test Data");
        let waveform_keywords = [
            "Rising Waveform Near",
            "Falling Waveform Near",
            "Rising Waveform Far",
            "Falling Waveform Far",
            "Diff Rising Waveform Near",
            "Diff Falling Waveform Near",
            "Diff Rising Waveform Far",
            "Diff Falling Waveform Far",
        ];
        for keyword in waveform_keywords {
            let section = find_child(test_data, keyword)
                .unwrap_or_else(|| panic!("Test_Data.{keyword} is registered"));
            assert_eq!(section.format, SectionFormat::VtTable, "Test_Data.{keyword}");
        }

        // `[Model Spec]` sub-parameters are corner triples in IBIS 7.0, and
        // the list with its order must follow the manual.
        let model_spec = find_child(model, "Model Spec").expect("Model.Model_Spec");
        let keys: Vec<&str> = model_spec
            .fields
            .iter()
            .map(|field| field.key.as_str())
            .collect();
        assert_eq!(
            keys,
            vec![
                "vinh", "vinl", "vinh+", "vinh-", "vinl+", "vinl-",
                "s_overshoot_high", "s_overshoot_low",
                "d_overshoot_high", "d_overshoot_low",
                "d_overshoot_time", "d_overshoot_area_h", "d_overshoot_area_l",
                "d_overshoot_ampl_h", "d_overshoot_ampl_l",
                "pulse_high", "pulse_low", "pulse_time",
                "vmeas", "cref", "rref", "vref",
                "cref_rising", "cref_falling",
                "rref_rising", "rref_falling",
                "vref_rising", "vref_falling",
                "vmeas_rising", "vmeas_falling",
                "rref_diff", "cref_diff",
                "weak_r", "weak_i", "weak_v",
            ]
        );
        let all_corner = model_spec
            .fields
            .iter()
            .all(|field| field.param_type == ParamType::Corner);
        assert!(all_corner, "Model Spec params must be corners");

        // `[Add Submodel]` is its own keyword below `[Model]`.
        let add_submodel = find_child(model, "Add Submodel").expect("Model.Add_Submodel");
        let keys: Vec<&str> = add_submodel
            .fields
            .iter()
            .map(|field| field.key.as_str())
            .collect();
        assert_eq!(keys, vec!["submodel_name", "mode"]);
    }

    #[test]
    fn test_model_selector_and_submodel_shapes() {
        let selector = root("Model Selector");
        assert_eq!(selector.format, SectionFormat::Table);
        assert_eq!(selector.header_format, HeaderFormat::Text);
        assert!(selector.fields.is_empty());

        let submodel = root("Submodel");
        assert_eq!(submodel.header_format, HeaderFormat::Text);
        assert_eq!(submodel.fields.len(), 1);
        assert_eq!(to_snake_key(&submodel.fields[0].key), "submodel_type");
    }
}
