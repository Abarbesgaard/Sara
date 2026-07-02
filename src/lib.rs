#![allow(dead_code)]
#![allow(clippy::too_many_arguments)]

//! Library half of the `sara` crate. Exists so integration tests under
//! `tests/` can reach crate internals (`db`, `commands`, ...) directly instead
//! of only through the CLI surface. Not intended for use as a dependency.

pub mod cli;
pub mod commands;
pub mod completion;
pub mod infrastructure;
