// =============================================================================
// rules — 数值解析相关规则（IBIS 3.2 §8）
//
// 集中存放 SI 数值 / 单位解析规则：
// `parse_quantity`（保留「数值 + 单位」，不换算）、`quantity_to_f64`（按需归一化，
// 仅供数值比较）、`corner_text_to_table`（角点三元组 → `Table`）。
//
// 这是 backend 数值化解析与 `schema::keyword_hierarchy` 强类型字段的公共规则层：
// schema 只提供类型，解析规则由本模块提供。
// =============================================================================

use serde::de::Deserializer;
use serde::Deserialize;

use crate::schema::keyword_hierarchy::{Cell, Quantity, Ratio, Scalar, Table};

// -----------------------------------------------------------------------------
// 数值解析原语
// -----------------------------------------------------------------------------

/// 把原始文本拆为「数值 + 原始单位后缀」（不做换算）。
///
/// - `"2.69p"` → `(2.69, Some("p"))`
/// - `"1.0000k"` → `(1.0, Some("k"))`
/// - `"3.3"` → `(3.3, None)`
fn split_number_unit(text: &str) -> Result<(f64, Option<String>), String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("empty numeric value".to_string());
    }

    let bytes = trimmed.as_bytes();
    let mut index = 0;

    // 可选符号。
    if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') {
        index += 1;
    }

    // 尾数（整数部分 + 可选小数部分）。
    let mut has_digits = false;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
        has_digits = true;
    }
    if index < bytes.len() && bytes[index] == b'.' {
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
            has_digits = true;
        }
    }
    if !has_digits {
        return Err(format!("'{trimmed}' has no numeric mantissa"));
    }

    // 可选指数（科学计数法）。
    if index < bytes.len() && (bytes[index] == b'e' || bytes[index] == b'E') {
        index += 1;
        if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') {
            index += 1;
        }
        let exponent_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == exponent_start {
            return Err(format!("'{trimmed}' has an empty exponent"));
        }
    }

    let numeric_part = &trimmed[..index];
    let suffix = &trimmed[index..];

    let value: f64 = numeric_part
        .parse()
        .map_err(|_| format!("'{trimmed}' is not a valid number"))?;
    let unit = if suffix.is_empty() { None } else { Some(suffix.to_string()) };
    Ok((value, unit))
}

/// SI 缩放因子（3.2 §8）：T / G / M / k / m / u / n / p / f。
const SCALING_FACTORS: &[(char, &str, f64)] = &[
    ('T', "tera", 1e12),
    ('G', "giga", 1e9),
    ('M', "mega", 1e6),
    ('k', "kilo", 1e3),
    ('m', "milli", 1e-3),
    ('u', "micro", 1e-6),
    ('n', "nano", 1e-9),
    ('p', "pico", 1e-12),
    ('f', "femto", 1e-15),
];

/// 基准单位（3.2 §8）：伏特 / 安培 / 欧姆 / 法拉 / 亨利 / 秒（含扩展）。
const BASE_UNITS: &[&str] = &["V", "A", "ohm", "Ohm", "F", "H", "W", "Hz", "s"];

/// 解析数值后的后缀：空 / 单字符前缀(+可选单位) / 单位。
fn scaling_multiplier(suffix: &str) -> Result<f64, String> {
    if suffix.is_empty() {
        return Ok(1.0);
    }
    let first_char = suffix.chars().next().expect("suffix is non-empty");
    if let Some((_, _, multiplier)) = SCALING_FACTORS.iter().find(|(symbol, _, _)| *symbol == first_char)
    {
        let unit_part = &suffix[first_char.len_utf8()..];
        if !unit_part.is_empty() && !is_valid_unit(unit_part) {
            return Err(format!("unknown unit '{unit_part}'"));
        }
        Ok(*multiplier)
    } else if is_valid_unit(suffix) {
        Ok(1.0)
    } else {
        Err(format!("unknown unit suffix '{suffix}'"))
    }
}

/// 是否是合法单位缩写。
fn is_valid_unit(unit: &str) -> bool {
    BASE_UNITS.contains(&unit)
}

// -----------------------------------------------------------------------------
// Quantity 解析（保留「数值 + 单位」，不换算）
// -----------------------------------------------------------------------------

/// 从原始文本解析 `Quantity`，不做归一化换算：
///
/// - `"2.69p"` → `Quantity { value: Number(2.69), unit: Some("p") }`
/// - `"1.0000k"` → `Quantity { value: Number(1.0), unit: Some("k") }`
/// - `"3.3"` → `Quantity { value: Number(3.3), unit: None }`
/// - `"1.9/597p"` → `Quantity { value: Ratio(Ratio{ numerator: Q(1.9), denominator: Q(597, "p") }) }`
pub fn parse_quantity(text: &str) -> Result<Quantity, String> {
    let trimmed = text.trim();
    if let Some((numerator, denominator)) = trimmed.split_once('/') {
        let numerator = parse_quantity(numerator)?;
        let denominator = parse_quantity(denominator)?;
        Ok(Quantity {
            value: Scalar::Ratio(Box::new(Ratio { numerator, denominator })),
            unit: None,
        })
    } else {
        let (value, unit) = split_number_unit(trimmed)?;
        Ok(Quantity { value: Scalar::Number(value), unit })
    }
}

/// 按需归一化为 f64（仅用于数值比较 / 校验，不改变存储形态）。
///
/// - `Number` 乘以其单位缩放因子（`"p"` → 1e-12）；
/// - `Ratio` 为分子 / 分母的商（分母为 0 或不可解析 → `None`）。
pub fn quantity_to_f64(quantity: &Quantity) -> Option<f64> {
    match &quantity.value {
        Scalar::Number(n) => {
            let multiplier = match &quantity.unit {
                Some(unit) => scaling_multiplier(unit).ok()?,
                None => 1.0,
            };
            Some(n * multiplier)
        }
        Scalar::Ratio(ratio) => {
            let numerator = quantity_to_f64(&ratio.numerator)?;
            let denominator = quantity_to_f64(&ratio.denominator)?;
            if denominator == 0.0 {
                None
            } else {
                Some(numerator / denominator)
            }
        }
    }
}

// -----------------------------------------------------------------------------
// 角点三元组解析（保留「数值 + 单位」，不换算）
// -----------------------------------------------------------------------------

/// 解析单个 token 为 `Quantity`；空 / `NA` / 非法 → `None`（宽松）。
fn parse_opt_quantity_token(token: &str) -> Option<Quantity> {
    let token = token.trim();
    if token.is_empty() || token.eq_ignore_ascii_case("NA") {
        return None;
    }
    parse_quantity(token).ok()
}

/// 角点三元组文本（`"typ min max"`，token 按位置 → typ / min / max）→ `Table`。
///
/// 统一用 `Table` 表示 corner（`col_header = ["typ", "min", "max"]`，单行
/// `matrix = [[typ, min, max]]`），替代已删除的 `Triplet`；`min` / `max` 缺失或
/// `NA` 时行内省略对应单元格。
pub fn corner_text_to_table(text: &str) -> Table {
    let mut row: Vec<Cell> = Vec::new();
    for token in text.split_whitespace().take(3) {
        if let Some(q) = parse_opt_quantity_token(token) {
            row.push(Cell::Quantity(q));
        }
    }
    Table {
        col_header: vec!["typ".into(), "min".into(), "max".into()],
        matrix: vec![row],
    }
}

// -----------------------------------------------------------------------------
// serde 反序列化辅助（挂载到 schema 强类型字段，作为"槽位类型 → 解析方式"的声明）
// -----------------------------------------------------------------------------

/// 从解释器产出的文本解析 `Option<Quantity>`：空 / `NA` → `None`；非法 → `None`（宽松）。
pub fn de_opt_quantity<'de, D>(d: D) -> Result<Option<Quantity>, D::Error>
where
    D: Deserializer<'de>,
{
    let text: Option<String> = Option::deserialize(d)?;
    Ok(text.as_deref().and_then(parse_opt_quantity_token))
}

/// 从解释器产出的文本解析 `Table`（corner 三元组，必填槽位）。
pub fn de_corner_table<'de, D>(d: D) -> Result<Table, D::Error>
where
    D: Deserializer<'de>,
{
    let text = String::deserialize(d)?;
    Ok(corner_text_to_table(&text))
}

/// 从解释器产出的文本解析 `Option<Table>`（corner 三元组）：空 / `NA` → `None`。
pub fn de_opt_corner_table<'de, D>(d: D) -> Result<Option<Table>, D::Error>
where
    D: Deserializer<'de>,
{
    let text: Option<String> = Option::deserialize(d)?;
    let trimmed = text.as_deref().map(str::trim);
    match trimmed {
        Some(t) if !t.is_empty() && !t.eq_ignore_ascii_case("NA") => Ok(Some(corner_text_to_table(t))),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_quantity_preserves_value_and_unit() {
        let q = parse_quantity("2.69p").unwrap();
        assert_eq!(q.value, Scalar::Number(2.69));
        assert_eq!(q.unit.as_deref(), Some("p"));
        assert_eq!(quantity_to_f64(&q), Some(2.69e-12));

        let q = parse_quantity("1.0000k").unwrap();
        assert_eq!(q.value, Scalar::Number(1.0));
        assert_eq!(q.unit.as_deref(), Some("k"));
        assert_eq!(quantity_to_f64(&q), Some(1000.0));

        let q = parse_quantity("3.3").unwrap();
        assert_eq!(q.unit, None);
        assert_eq!(quantity_to_f64(&q), Some(3.3));
    }

    #[test]
    fn test_parse_quantity_ratio_preserved() {
        let q = parse_quantity("1.9/597p").unwrap();
        let Scalar::Ratio(ratio) = &q.value else { panic!("expected ratio") };
        assert_eq!(ratio.numerator.value, Scalar::Number(1.9));
        assert_eq!(ratio.numerator.unit, None);
        assert_eq!(ratio.denominator.value, Scalar::Number(597.0));
        assert_eq!(ratio.denominator.unit.as_deref(), Some("p"));
        // 按需比较：1.9 / (597e-12)
        let f = quantity_to_f64(&q).unwrap();
        assert!((f - 1.9 / 5.97e-10).abs() < 1e-6, "ratio = {f}");
    }

    #[test]
    fn test_quantity_preserves_unknown_unit() {
        // 保留式解析：未知单位后缀仍保留为 unit（value + unit），仅无数值尾数才报错。
        let q = parse_quantity("1x").unwrap();
        assert_eq!(q.value, Scalar::Number(1.0));
        assert_eq!(q.unit.as_deref(), Some("x"));
        assert!(parse_quantity("abc").is_err());
    }
}
