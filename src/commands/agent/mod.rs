//! Agent-facing helpers behind the MCP server: what to call next, a compact
//! task briefing, closing a task off a merged PR, and memory maintenance.

mod finish;
mod maintain;
mod next_actions;
mod resume;
mod scope;

pub use finish::{FinishRequest, finish_value};
pub use maintain::{MaintainRequest, maintain_value};
#[cfg(test)]
pub use next_actions::next_actions;
pub use next_actions::with_next_actions;
pub use resume::resume_value;
pub use scope::{Scope, memories_value, tags_value};
