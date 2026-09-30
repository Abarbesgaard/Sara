mod annotation;
mod color;
pub mod insight;
mod json;
mod memory_graph;
mod project;
mod prompt;
mod text;
mod time;

pub use annotation::annotation_target;
pub use color::heat_color;
pub use json::{json_strs, print_json};
pub use memory_graph::{
    canonical_labels, derived_children, derived_count, derived_from_suffix, item_label,
    item_snippet, memory_handle, short_handle,
};
pub use project::{guard_branch_mutation, parse_due, project_head, project_path};
pub use prompt::{print_cancelled, prompt_line};
pub use text::{plural, strength_label, summarize, truncate};
pub use time::{month_abbr, parse_duration_mins, rel_time};

#[cfg(test)]
#[path = "../../../tests/unit/commands/shared/mod.rs"]
mod tests;
