//! Sara's domain: the entities and value objects the rest of the application is
//! built around. Pure Rust — no persistence, presentation or I/O. Everything
//! else may depend on this module; it depends on nothing in the crate.
//!
//! Laid out by building block: `entities/` depend on `value_objects/`, and
//! `services/` (urgency, strength, fingerprints) on both. Each entity and value
//! object lives in its own file, named after it.

mod entities;
mod error;
pub mod services;
mod value_objects;

pub use entities::*;
pub use error::*;
pub use value_objects::*;
