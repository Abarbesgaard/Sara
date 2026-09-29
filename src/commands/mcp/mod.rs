mod params;
mod server;
mod transport;

mod guide;
mod lifecycle;
mod read;

pub use server::run;

#[cfg(test)]
#[path = "../../../tests/unit/commands/mcp/tests.rs"]
mod tests;
