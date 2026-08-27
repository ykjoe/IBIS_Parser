//! Integration tests — parse every sample `.ibs` file through the full
//! three-stage pipeline (frontend → backend → emitter) and verify that a
//! strongly-typed TOML string is produced.
//!
//! The sample files live in the workspace root's `tests/examples/` directory.
//!
//! > Real-world samples use **lenient** validation (`parse_to_toml_lenient`):
//! > the strongly-typed conversion always succeeds and collects issues into a
//! > [`ValidationReport`] (see the architecture book, section 5). Strict mode is
//! > covered by focused unit tests with crafted inputs.

use std::fs;
use std::path::{Path, PathBuf};

use ibis2ibstoml::{ValidationReport, parse_to_toml_lenient};

/// Absolute path to the workspace-level example directory.
fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/examples")
}

#[test]
fn test_examples_parse_to_strongly_typed_toml() {
    let examples = examples_dir();
    let mut parsed_count = 0;

    for entry in fs::read_dir(&examples).expect("examples dir missing") {
        let entry = entry.expect("read entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ibs") {
            continue;
        }

        // 宽松模式：真实样本全量转换并收集问题。
        let (generated, report) = parse_to_toml_lenient(&fs::read_to_string(&path).expect("read .ibs"))
            .unwrap_or_else(|e| panic!("failed to parse {}: {}", path.display(), e));
        parsed_count += 1;

        assert!(!generated.is_empty(), "empty TOML output for {}", path.display());
        assert!(matches!(report, ValidationReport { .. }), "no validation report for {}", path.display());

        // Strongly-typed output is expected to contain the file header table
        // and at least one section.
        assert!(
            generated.contains("[File_Header]"),
            "missing [File_Header] for {}",
            path.display()
        );
        assert!(
            generated.contains('['),
            "no TOML tables produced for {}",
            path.display()
        );
    }

    assert!(
        parsed_count > 0,
        "no .ibs examples found under {}",
        examples.display()
    );
}
