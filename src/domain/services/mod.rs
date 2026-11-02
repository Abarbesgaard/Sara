//! Domain services: business rules that span entities or need inputs from
//! outside (weights, the clock). Pure functions; the caller supplies the data.

pub mod fingerprint;
pub mod memory_strength;
pub mod urgency;
mod urgency_weights;

pub use urgency_weights::UrgencyWeights;
