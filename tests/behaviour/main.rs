#![cfg(unix)]
//! Behaviour the Contract does not specify yet. A test here moves to
//! tests/contract/<primitive>.rs when an obligation claims it.

#[path = "../contract/harness.rs"]
mod harness;

mod cite;
mod cli_errors;
mod cli_json;
mod completion;
mod doctor;
mod links;
mod mcp;
mod mcp_args;
mod provenance;
mod recall;
mod reflect;
mod shorthand;
mod tag_parsing;
mod telemetry;
