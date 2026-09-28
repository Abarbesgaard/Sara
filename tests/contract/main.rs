//! Black-box contract tests for the `sara` binary (issue #169).
//!
//! Everything here spawns the REAL compiled `sara` executable against a
//! throwaway data directory and asserts its public contracts:
//!   - the stable `--json` CLI surfaces (`recall`, `list`, `info`), and
//!   - the MCP stdio tool-response envelopes (`begin`, `recall`, `learn`).
//!
//! This is one integration-test binary (per matklad's "one test crate"
//! guidance): `tests/contract.rs` is the crate root and the feature modules
//! live in the sibling `tests/contract/` directory, so they compile and link
//! once instead of as many separate crates.
//!
//! Unix-only: the harness isolates sara's data dir via `HOME`/`XDG_*`, which
//! the `directories` crate honours on Linux/macOS. On Windows it resolves the
//! data dir through the Known Folder API instead, so env-var isolation would
//! leak into the real profile — these platform-independent contract checks run
//! on Linux and macOS CI, which is sufficient.
#![cfg(unix)]

mod harness;

mod cli_errors;
mod cli_json;
mod mcp;
