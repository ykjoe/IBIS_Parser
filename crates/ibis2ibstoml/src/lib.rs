//! ibis2ibstoml — First-pass IBIS-to-TOML conversion pipeline.
//!
//! 该 crate 将原始 IBIS 文本转换为 TOML 表示，采用三段式 Pipeline：
//!
//! 1. **frontend** — 唯一公开接口 [`frontend::parse`]：IBIS 文本 → 抽象语法树
//!    （内部为词法 [`frontend::tokenizer`] → 语法 [`frontend::syntax_analysis`]
//!    → AST [`frontend::ast_builder`]，失败时由 [`frontend::recovery`] 容错回退）。
//! 2. **backend** — 预留：后端语义处理 / 数据转换接口。
//! 3. **emitter** — 将 [`SectionNode`](frontend::SectionNode) 树序列化为 TOML。
//!
//! 所有值均以原始字符串保留，不做数值转换或单位换算。
//!
//! # Examples
//!
//! ```rust
//! use ibis2ibstoml::parse_to_toml;
//!
//! let toml_output = parse_to_toml("[IBIS ver] 2.1\n[Component] MyChip\n[End]\n")
//!     .expect("parsing failed");
//! assert!(toml_output.contains("ibis_ver"));
//! ```

pub mod backend;
pub mod compat;
pub mod core;
pub mod emitter;
pub mod frontend;

use std::fs;
use std::path::Path;

// =============================================================================
// Public API — parse IBIS content and produce TOML
// =============================================================================

/// Parse IBIS content and produce TOML output in a single pass.
///
/// # Pipeline
///
/// 1. **Frontend** — [`frontend::parse`] 将 IBIS 文本解析为 [`SectionNode`](frontend::SectionNode) 树。
/// 2. **Emit** — [`emitter::toml::serialize_tree_to_string`] 递归序列化为 TOML。
///
/// # Parameters
///
/// * `content` — A string containing the full text of an IBIS file.
///
/// # Returns
///
/// * `Ok(String)` — The TOML representation of the IBIS content.
/// * `Err(String)` — A human-readable error message if parsing fails.
///
/// # Errors
///
/// If pest parsing fails and the fallback also fails, an error message is returned.
///
/// # Examples
///
/// ```rust
/// use ibis2ibstoml::parse_to_toml;
///
/// let toml_output = parse_to_toml("[IBIS ver] 2.1\n[Component] MyChip\n[End]\n")
///     .expect("parsing failed");
/// assert!(toml_output.contains("ibis_ver"));
/// ```
pub fn parse_to_toml(content: &str) -> Result<String, String> {
    // ── Phase 1: frontend 解析 → AST 树 ──
    let tree = frontend::parse(content)?;

    // ── Phase 2: emitter 序列化 → TOML ──
    Ok(emitter::toml::serialize_tree_to_string(&tree))
}

/// Read an IBIS file and produce a `.ibs.toml` representation.
///
/// The function uses a single-pass pipeline:
/// 1. **Frontend parsing** ([`parse_to_toml`]) — pest-based full parsing
///    (lexical + syntax) and direct TOML serialization.
///
/// # Parameters
///
/// * `path` — Path to an `.ibs` file. Accepts any type implementing [`AsRef<Path>`].
///
/// # Returns
///
/// * `Ok(String)` — The TOML representation of the IBIS content.
/// * `Err(String)` — A human-readable error message if the file cannot be read or conversion fails.
///
/// # Errors
///
/// Returns `Err` if the file cannot be read from disk, or if the content cannot be parsed.
///
/// # Panics
///
/// Does not panic under normal operation.
pub fn ibs2ibstoml<P: AsRef<Path>>(path: P) -> Result<String, String> {
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    parse_to_toml(&content)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_to_toml_simple() {
        let ibis_content = "\
[IBIS ver] 2.1
[Component] STM32F103
[Manufacturer] STMicro
[Package]
R_pkg 0.1
L_pkg 1nH
";
        let result = parse_to_toml(ibis_content).unwrap();
        assert!(result.contains("[File_Header.IBIS_ver]"), "Missing File_Header.IBIS_ver");
        assert!(result.contains("ibis_ver = \"2.1\""), "Missing ibis_ver");
        assert!(result.contains("[Component]"), "Missing [Component]");
        assert!(result.contains("component = \"STM32F103\""), "Missing component");
        assert!(result.contains("[Component.Manufacturer]"), "Missing Component.Manufacturer");
        assert!(result.contains("[Component.Package]"), "Missing Component.Package");
    }

    #[test]
    fn test_parse_to_toml_handles_comments() {
        let ibis_content = "\
| This is a comment
[IBIS ver] 2.1
| Another comment
[File name] test.ibs
";
        let result = parse_to_toml(ibis_content).unwrap();
        assert!(result.contains("[File_Header.IBIS_ver]"));
        assert!(result.contains("[File_Header.File_name]"));
    }

    #[test]
    fn test_parse_to_toml_multiple_models() {
        let ibis_content = "[Model] ModelA\n[Model] ModelB\n";
        let result = parse_to_toml(ibis_content).unwrap();
        assert_eq!(result.matches("[Model]").count(), 2);
        assert!(result.contains("model = \"ModelA\""));
        assert!(result.contains("model = \"ModelB\""));
    }

    #[test]
    fn test_parse_to_toml_end_is_skipped() {
        let ibis_content = "\
[Component] MyComp
[End]
[Other] val
";
        let result = parse_to_toml(ibis_content).unwrap();
        assert!(result.contains("[Component]"));
        assert!(result.contains("[Other]"));
        assert!(!result.contains("End"), "[End] should not appear in output");
    }
}
