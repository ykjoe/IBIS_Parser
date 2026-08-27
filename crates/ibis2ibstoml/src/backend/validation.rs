// =============================================================================
// validation — 第三步：物理与逻辑数据校验
//
// 针对第二步 `symbol_table_build` 已构建好的**数值化强类型符号表与结构体**
// 执行业务 / 物理规则校验。语义校验（corner 范围、IV 单调性、必填）由
// `schema::keyword_hierarchy` 的 **validator 声明式挂载**承担（`#[validate(custom(...))]`），
// 本模块负责：
//
//   1. 对 `IBIS_File` 调用 `.validate()`（递归覆盖 header / components 等），
//      并把 `ValidationErrors` 映射为 `SemanticError`；
//   2. 对 `IndexMap` 符号表字段（models / submodels / test_loads /
//      package_models）手动遍历校验（validator nested 需 &'static 字段名）；
//   3. **符号引用检查**：`[Component]` 中 `Component_Pin` 引用的 `model_name` 是否在
//      `[Model]` / `[Submodel]` / `[Model Selector]` 符号表中真实存在。
//
// 校验问题写入 [`ValidationCollector`]（严格模式报错 / 宽松模式收集）。
// =============================================================================

use std::collections::HashSet;

use crate::backend::{SemanticError, ValidationCollector};
use crate::schema::keyword_hierarchy::*;
use validator::{Validate, ValidationErrors, ValidationErrorsKind};

/// 运行第三步的全部物理 / 逻辑校验。
pub fn data_valid(file: &IBIS_File, collector: &mut ValidationCollector) {
    // 1. 结构体级 validator 校验（header / components / model_selectors /
    //    external_circuits / test_data / interconnect_model_sets 等 nested 字段）。
    if let Err(errors) = file.validate() {
        reporting::collect_validation_errors(&errors, "(file)", collector);
    }
    // 2. IndexMap 符号表字段手动遍历校验。
    for (name, model) in &file.models {
        if let Err(errors) = model.validate() {
            reporting::collect_validation_errors(&errors, &format!("Model.{name}"), collector);
        }
    }
    for (name, submodel) in &file.submodels {
        if let Err(errors) = submodel.validate() {
            reporting::collect_validation_errors(&errors, &format!("Submodel.{name}"), collector);
        }
    }
    for (name, load) in &file.test_loads {
        if let Err(errors) = load.validate() {
            reporting::collect_validation_errors(&errors, &format!("Test Load.{name}"), collector);
        }
    }
    for (name, package) in &file.package_models {
        if let Err(errors) = package.validate() {
            reporting::collect_validation_errors(&errors, &format!("Define Package Model.{name}"), collector);
        }
    }
    // 3. 符号引用检查。
    references::validate_symbol_references(file, collector);
}

/// validator 错误 → `SemanticError::ValidationFailed` 映射。
mod reporting {
    use super::*;

    /// 把 validator 的 `ValidationErrors` 映射为 `SemanticError::ValidationFailed`
    /// 并写入 collector。
    ///
    /// `field_errors()` 不递归 nested 结构体的错误，故用 `errors()` 递归遍历
    /// `ValidationErrorsKind`（Field / Struct / List / Map）。
    pub(super) fn collect_validation_errors(
        errors: &ValidationErrors,
        scope: &str,
        collector: &mut ValidationCollector,
    ) {
        for (field, kind) in errors.errors() {
            collect_validation_kind(kind, scope, field, collector);
        }
    }

    /// 递归遍历一个字段的校验结果种类。
    pub(super) fn collect_validation_kind(
        kind: &ValidationErrorsKind,
        scope: &str,
        field: &str,
        collector: &mut ValidationCollector,
    ) {
        match kind {
            ValidationErrorsKind::Field(field_errors) => {
                for error in field_errors {
                    let message = error
                        .message
                        .as_deref()
                        .map(str::to_string)
                        .unwrap_or_else(|| "validator 校验未通过".to_string());
                    collector.error(SemanticError::ValidationFailed {
                        section: scope.to_string(),
                        field: field.to_string(),
                        message,
                    });
                }
            }
            ValidationErrorsKind::Struct(inner) => {
                let nested_scope = format!("{scope}.{field}");
                collect_validation_errors(inner, &nested_scope, collector);
            }
            ValidationErrorsKind::List(map) => {
                for inner in map.values() {
                    collect_validation_errors(inner, scope, collector);
                }
            }
        }
    }
}

/// 符号引用检查：`Pin.model_name` ∈ 符号表（models / submodels / model_selectors）。
mod references {
    use super::*;

    pub(super) fn validate_symbol_references(file: &IBIS_File, collector: &mut ValidationCollector) {
        let mut targets: HashSet<String> = HashSet::new();
        targets.extend(file.models.keys().cloned());
        targets.extend(file.submodels.keys().cloned());
        for selector in &file.model_selectors {
            targets.extend(selector.models.iter().cloned());
        }

        for component in &file.components {
            let Some(pin) = &component.pin else { continue };
            for row in &pin.pin.matrix {
                // 列：signal_name(0) / model_name(1) / R_pin(2) / L_pin(3) / C_pin(4)
                let Some(Cell::Text(model_name)) = row.get(1) else { continue };
                let target = model_name.trim();
                if !target.is_empty() && !targets.contains(target) {
                    let from = match row.first() {
                        Some(Cell::Text(sig)) => sig.clone(),
                        _ => "<pin>".into(),
                    };
                    collector.error(SemanticError::ReferenceNotFound {
                        from,
                        target: model_name.clone(),
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::field_reassign_with_default)]
    use super::*;

    fn with_error(file: &IBIS_File) -> ValidationCollector {
        let mut collector = ValidationCollector::new();
        data_valid(file, &mut collector);
        collector
    }

    /// 构造 `[Pin]` Table（列 = signal_name / model_name）。
    fn pin_table(rows: &[(&str, &str)]) -> Component_Pin {
        Component_Pin {
            pin: Table {
                col_header: vec!["signal_name".into(), "model_name".into()],
                matrix: rows
                    .iter()
                    .map(|(sig, mdl)| {
                        vec![Cell::Text(sig.to_string()), Cell::Text(mdl.to_string())]
                    })
                    .collect(),
            },
        }
    }

    /// 构造一个 header 合法的空 `IBIS_File`（避免默认 header 触发必填校验）。
    fn valid_file_with_header() -> IBIS_File {
        let mut file = IBIS_File::default();
        file.header.ibis_ver = "2.1".into();
        file.header.file_name = "test.ibs".into();
        file.header.file_rev = "1.0".into();
        file
    }

    /// 构造 corner 表（col_header = ["typ", "min", "max"]，单行）。
    fn corner_table(cells: &[Quantity]) -> Table {
        Table {
            col_header: vec!["typ".into(), "min".into(), "max".into()],
            matrix: vec![cells.iter().cloned().map(Cell::Quantity).collect()],
        }
    }

    /// 构造 IV 曲线 Table（voltage / i_typ / i_min / i_max）。
    fn vi_table(points: &[(f64, f64)]) -> Table {
        Table {
            col_header: vec!["voltage".into(), "i_typ".into(), "i_min".into(), "i_max".into()],
            matrix: points
                .iter()
                .map(|(v, i)| {
                    vec![
                        Cell::Quantity(Quantity { value: Scalar::Number(*v), unit: None }),
                        Cell::Quantity(Quantity { value: Scalar::Number(*i), unit: Some("mA".into()) }),
                        Cell::Quantity(Quantity { value: Scalar::Number(*i), unit: Some("mA".into()) }),
                        Cell::Quantity(Quantity { value: Scalar::Number(*i), unit: Some("mA".into()) }),
                    ]
                })
                .collect(),
        }
    }

    #[test]
    fn test_non_monotonic_voltage_reported() {
        let mut file = IBIS_File::default();
        let mut model = IBIS_Model::default();
        model.model = "M1".into();
        model.model_type = "I/O".into();
        model.pulldown = Some(vi_table(&[(0.0, 0.0), (-1.0, 1.0)]));
        file.models.insert("M1".into(), model);

        let collector = with_error(&file);
        assert!(collector.errors.iter().any(|e| matches!(e, SemanticError::ValidationFailed { .. })));
    }

    #[test]
    fn test_monotonic_voltage_passes() {
        let mut file = valid_file_with_header();
        let mut model = IBIS_Model::default();
        model.model = "M1".into();
        model.model_type = "I/O".into();
        model.pulldown = Some(vi_table(&[(-3.3, -2.0e-3), (-3.1, -1.0e-3), (0.0, 0.0)]));
        file.models.insert("M1".into(), model);

        let collector = with_error(&file);
        assert!(!collector.errors.iter().any(|e| matches!(e, SemanticError::ValidationFailed { .. })));
    }

    #[test]
    fn test_corner_range_violation_reported() {
        let mut file = IBIS_File::default();
        let mut model = IBIS_Model::default();
        model.model = "M1".into();
        model.model_type = "I/O".into();
        // min(3.3) > typ(2.0)
        model.voltage_range = Some(corner_table(&[
            Quantity { value: Scalar::Number(2.0), unit: Some("V".into()) },
            Quantity { value: Scalar::Number(3.3), unit: Some("V".into()) },
            Quantity { value: Scalar::Number(3.6), unit: Some("V".into()) },
        ]));
        file.models.insert("M1".into(), model);

        let collector = with_error(&file);
        assert!(collector.errors.iter().any(|e| matches!(e, SemanticError::ValidationFailed { field, .. } if field == "voltage_range")));
    }

    #[test]
    fn test_corner_range_valid_passes() {
        let mut file = valid_file_with_header();
        let mut model = IBIS_Model::default();
        model.model = "M1".into();
        model.model_type = "I/O".into();
        model.temperature_range = Some(corner_table(&[
            Quantity { value: Scalar::Number(27.0), unit: Some("C".into()) },
            Quantity { value: Scalar::Number(-40.0), unit: Some("C".into()) },
            Quantity { value: Scalar::Number(125.0), unit: Some("C".into()) },
        ]));
        model.voltage_range = Some(corner_table(&[
            Quantity { value: Scalar::Number(3.3), unit: Some("V".into()) },
            Quantity { value: Scalar::Number(2.0), unit: Some("V".into()) },
            Quantity { value: Scalar::Number(3.6), unit: Some("V".into()) },
        ]));
        file.models.insert("M1".into(), model);

        let collector = with_error(&file);
        assert!(!collector.errors.iter().any(|e| matches!(e, SemanticError::ValidationFailed { .. })));
    }

    #[test]
    fn test_symbol_reference_not_found() {
        let mut file = IBIS_File::default();
        let mut component = IBIS_Component::default();
        component.component = "Chip".into();
        component.pin = Some(pin_table(&[("SIG", "NONEXISTENT")]));
        file.components.push(component);

        let collector = with_error(&file);
        assert!(collector.errors.iter().any(|e| matches!(
            e,
            SemanticError::ReferenceNotFound { target, .. } if target == "NONEXISTENT"
        )));
    }

    #[test]
    fn test_symbol_reference_valid() {
        let mut file = IBIS_File::default();
        file.models.insert("M1".into(), IBIS_Model { model: "M1".into(), model_type: "I/O".into(), ..Default::default() });
        let mut component = IBIS_Component::default();
        component.pin = Some(pin_table(&[("SIG", "M1")]));
        file.components.push(component);

        let collector = with_error(&file);
        assert!(!collector.errors.iter().any(|e| matches!(e, SemanticError::ReferenceNotFound { .. })));
    }

    #[test]
    fn test_missing_header_field_reported() {
        let file = IBIS_File::default();
        let collector = with_error(&file);
        // 缺 IBIS ver → validator 必填校验报 ValidationFailed。
        assert!(collector.errors.iter().any(|e| matches!(e, SemanticError::ValidationFailed { .. })));
    }
}
