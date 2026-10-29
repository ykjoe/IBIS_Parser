//! Re-export of the shared IBIS 7.0 structural specification.
//!
//! The schema itself — `ibis_schema.toml` and the code that turns it into a
//! [`SectionSpec`] tree — lives in the standalone [`ibstoml_schema`] crate, so that
//! both `ibis2ibstoml` and `ibstoml_extrator` read the very same structure data.
//! This module only re-exports it, which keeps the historical
//! `ibis2ibstoml::schema::…` / `crate::schema::…` paths working inside the
//! conversion pipeline.
//!
//! `[X]` vs `[[X]]` gives the **single / multiple** occurrence ([`Occurrence`]),
//! `__schema__.format` gives **how header / params / text body are organized**
//! ([`SectionFormat`]), `__header__.format` gives **how the text right after the
//! keyword is organized** ([`HeaderFormat`]), and `__param__` gives the **named
//! sub-parameters with their value types** ([`FieldSpec`] / [`ParamType`]).
//!
//! # Examples
//!
//! ```rust
//! use ibis2ibstoml::schema::{find_child, find_level, find_root, Occurrence};
//!
//! let component = find_root("Component").expect("Component is registered");
//! assert_eq!(component.occurrence, Occurrence::Multiple);
//!
//! let pin = find_child(component, "Pin").expect("Component.Pin is registered");
//! // `col0` names the leading pin-name column that the manual's header line omits.
//! assert_eq!(pin.fields.len(), 6);
//! assert_eq!(pin.fields[0].key, "col0");
//!
//! // The nesting level of a keyword is a lookup, not a field of the spec.
//! assert_eq!(find_level("Pin"), Some(2));
//! ```

// =============================================================================
// schema — re-export of the shared `ibstoml_schema` crate
//
// The IBIS structure data (`ibis_schema.toml`), its loader and its lookup
// primitives were moved into the `ibstoml_schema` crate so that `ibis2ibstoml`
// and `ibstoml_extrator` share one source of truth. This module keeps the public
// path `ibis2ibstoml::schema::…` stable for the frontend, backend and emitter.
// =============================================================================

pub use ibstoml_schema::*;
