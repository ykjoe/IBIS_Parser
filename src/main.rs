//! IBIS Parser — command-line entry point.
//!
//! Parses a hard-coded sample IBIS file (`tests/examples/f103c8.ibs`) and writes
//! the TOML output to `<input>.ibs.toml`.  This is a development/debugging binary;
//! production use should call the library API [`ibis2ibstoml::ibs2ibstoml`]
//! directly.

use ibis2ibstoml::ibs2ibstoml;

fn main() {
    let path = "tests/examples/f103c8.ibs";
    match ibs2ibstoml(path) {
        Ok(toml_str) => {
            // Write output to .ibs.toml
            let out_path = format!("{}.toml", path);
            std::fs::write(&out_path, &toml_str)
                .expect("Failed to write output file");
            println!("TOML output written to: {}", out_path);
            println!("\n--- Preview ---");
            // Print first 80 lines as preview
            for line in toml_str.lines().take(80) {
                println!("{}", line);
            }
        }
        Err(e) => eprintln!("Error: {}", e),
    }
}
