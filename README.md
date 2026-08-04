<!-- cargo-rdme start -->

IBIS Parser — Parse IBIS chip model description files and convert them to TOML.

This library provides a complete IBIS 7.0 parsing pipeline, including lexical
analysis, syntax analysis, semantic construction, and TOML serialization.

# Architecture

The pipeline is split across two crates in a Cargo workspace:

- [`ibis2ibstoml`](https://docs.rs/ibis2ibstoml/latest/ibis2ibstoml/) — First-pass format reshaping layer (standalone crate). Tokenises IBIS
  text via PEST grammar, classifies keywords, builds a section tree, and serialises
  to TOML. All values remain as raw strings; no numerical conversion or unit scaling
  is performed.
- [`ibis_parser`](https://docs.rs/ibis_parser/latest/ibis_parser/ibis_parser/) — (Planned) Second-pass semantic parsing layer. Converts the TOML
  string into strongly-typed AST structures

# Quick Start

```rust
use ibis2ibstoml::ibs2ibstoml;

let toml_output = ibs2ibstoml("tests/examples/f103c8.ibs")
    .expect("parsing failed");
assert!(toml_output.contains("[File_Header]"));
println!("{}", toml_output);
```

<!-- cargo-rdme end -->
