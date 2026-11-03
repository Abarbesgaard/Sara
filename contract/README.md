# The Contract

What sara promises, written down as numbered obligations, each proved by a
contract test. [impish](https://github.com/Abarbesgaard/impish) holds it; its
[contract/README.md](https://github.com/Abarbesgaard/impish/blob/main/contract/README.md)
is the convention: obligation blocks, the `#[impish::contract]` attribute,
`covers` and `surface` blocks, and what fails the build.

```
contract/<primitive>/README.md   purpose, obligations, covers block
tests/contract/<primitive>.rs    the tests that prove them
```

sara's surface is its CLI commands and MCP tools, one `cli <command>` or
`mcp <tool>` per line; `scripts/surface.sh` prints it. List only what a
primitive's tests actually call, so coverage counts what is proved.

CI runs `cargo impish check` and writes `contract/coverage.json` to the job
summary. After changing a primitive, refresh it:

```sh
cargo install --git https://github.com/Abarbesgaard/impish --tag v0.4.0 --locked cargo-impish
cargo impish check --write
```
