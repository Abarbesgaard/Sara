mod annotations;
mod files;
mod links;

pub use annotations::{annotate, annotate_value, denotate};
pub use files::{attach, attach_value};
pub use links::{link, link_value, unlink};
