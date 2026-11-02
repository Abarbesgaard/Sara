mod brief;
mod check;
mod gate;
mod handlers;
mod support;
mod types;

pub use brief::{assignment, assignment_value, rationale, rationale_value};
pub use check::check_value;
pub use handlers::feedback::{feedback, feedback_value, resolve, resolve_value};
pub use handlers::next::{next, next_value};
pub use handlers::record_run::{record_run, record_run_value};
pub use handlers::step_done::{
    step_done, step_done_by_id_value, step_done_current_value, step_done_value,
};
pub use handlers::step_remove::{step_remove, step_remove_by_id_value, step_remove_value};
pub use handlers::step_undone::{step_undone, step_undone_by_id_value, step_undone_value};
pub use handlers::steps::{steps, steps_value};
pub use handlers::validate::{validate, validate_value};
pub use handlers::verify::{verify, verify_value};
pub use types::GateOutput;

#[cfg(test)]
#[path = "../../../tests/unit/commands/guide/mod.rs"]
mod tests;
