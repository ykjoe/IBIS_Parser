//! Integration tests — parse every sample `.ibs` file and compare the generated
//! TOML output against the reference `.ibs.toml` file (when one exists).
//!
//! The sample files live in the workspace root's `tests/examples/` directory.
//! Each `X.ibs` may have a reference `X.ibs.toml` produced by the same emitter
//! pipeline (see the root crate's `src/main.rs`); when present, the generated
//! output must match it exactly.

use std::fs;
use std::path::{Path, PathBuf};

use ibis2ibstoml::parse_to_toml;

/// Absolute path to the workspace-level example directory.
fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/examples")
}

#[test]
fn test_examples_match_reference_toml() {
    let examples = examples_dir();
    let mut parsed_count = 0;
    let mut compared_count = 0;

    for entry in fs::read_dir(&examples).expect("examples dir missing") {
        let entry = entry.expect("read entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ibs") {
            continue;
        }

        let generated = parse_to_toml(&fs::read_to_string(&path).expect("read .ibs"))
            .unwrap_or_else(|e| panic!("failed to parse {}: {}", path.display(), e));
        parsed_count += 1;

        // Reference file: `<name>.ibs.toml`.
        let reference = path.with_extension("ibs.toml");
        if reference.exists() {
            compared_count += 1;
            let expected = fs::read_to_string(&reference).expect("read reference");
            assert_eq!(
                generated, expected,
                "TOML output for {} does not match reference {}",
                path.display(),
                reference.display()
            );
        }
    }

    assert!(
        parsed_count > 0,
        "no .ibs examples found under {}",
        examples.display()
    );
    assert!(
        compared_count > 0,
        "no reference .ibs.toml found under {}",
        examples.display()
    );
}
