//! Regenerate the golden `.ibs.toml` example files from the corresponding
//! `.ibs` sources, using the current emitter.
//!
//! Run from the workspace root:
//!
//! ```sh
//! cargo run -p ibis2ibstoml --example regen_golden
//! ```
//!
//! Golden files live in the workspace root's `tests/examples/` directory and
//! are the expected serialized output for each sample `.ibs` file.

use std::fs;
use std::path::PathBuf;

use ibis2ibstoml::parse_to_toml_lenient;

fn main() {
    let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/examples");
    let mut count = 0;

    for entry in fs::read_dir(&examples).expect("examples dir missing") {
        let entry = entry.expect("read entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ibs") {
            continue;
        }

        let source = fs::read_to_string(&path).expect("read .ibs");
        let (generated, report) = parse_to_toml_lenient(&source)
            .unwrap_or_else(|e| panic!("failed to parse {}: {}", path.display(), e));

        // 守护：golden 输出必须是合法 TOML。
        toml::from_str::<toml::Value>(&generated)
            .unwrap_or_else(|e| panic!("{} 输出不是合法 TOML: {}", path.display(), e));

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("utf-8 file name");
        let out_path = examples.join(format!("{name}.toml"));
        fs::write(&out_path, generated).expect("write golden toml");
        count += 1;

        if !report.errors.is_empty() {
            println!("note: {} — {} issue(s) collected", name, report.errors.len());
        }
        println!("regen {}", out_path.display());
    }

    println!("Regenerated {count} golden files under {}", examples.display());
}
