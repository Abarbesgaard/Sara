#!/bin/sh
# Prints sara's surface for `cargo impish check`: one `cli <command>` per
# subcommand and one `mcp <tool>` per MCP tool, run against a throwaway home.
set -eu
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}" CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
home=$(mktemp -d)
trap 'rm -rf "$home"' EXIT
export HOME="$home" XDG_DATA_HOME="$home/data" XDG_CONFIG_HOME="$home/config"
export SARA_NO_TELEMETRY=1 NO_COLOR=1
cargo run --quiet -- --help | awk '
  /^Commands:/ { on = 1; next }
  on && /^  [a-z]/ { if ($1 != "help") print "cli " $1; next }
  on { exit }
'
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"impish","version":"0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' |
  cargo run --quiet -- mcp |
  jq -r 'select(.id == 2) | .result.tools[].name | "mcp " + .'
