//! `sara mcp` — a stdio JSON-RPC MCP server exposing sara's agent loop as typed
//! tools, so any MCP client (Claude, Codex, Copilot, …) can drive sara without
//! the CLI's flag-ordering / UUID / TUI footguns.
//!
//! Design (see task 13024f4f):
//! - A THIN adapter: every tool calls the same `commands::*` value functions the
//!   CLI uses, so there is one source of truth. Nothing here re-implements DB logic.
//! - Folder-awareness: sara derives "the project" from the current git folder, but
//!   a long-running server has no per-call cwd. Every tool therefore takes an
//!   optional `project_path`; [`server::SaraServer::with_project`] sets the process
//!   cwd to it (guarded by the connection mutex, so concurrent calls can't race on
//!   cwd or the thread-local undo batch) before calling the underlying function.
//! - stdout is the JSON-RPC channel: tools MUST never print. That is why the
//!   adapter calls the print-free `*_value` cores, not the CLI's `run` functions.
//! - The async runtime is built ONLY here; the rest of the CLI stays synchronous.
//!
//! Vertical slice: [`server`] holds the runtime (`SaraServer`, cwd guard, helpers,
//! `ServerHandler`, entry point); [`params`] the tool argument schemas; and the
//! `#[tool]` methods are grouped by capability into [`read`], [`guide`], and
//! [`lifecycle`], each contributing a named `ToolRouter` that `SaraServer::new`
//! composes into one.

mod params;
mod server;

mod guide;
mod lifecycle;
mod read;

pub use server::run;

#[cfg(test)]
mod tests;
