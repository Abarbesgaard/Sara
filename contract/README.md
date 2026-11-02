# The Contract

What sara promises, written down as numbered obligations, each proved by a
contract test. It follows the sibling layout used by Impatient Programming's
[flinch](https://github.com/tools4imps/flinch) and
[covenant](https://github.com/tools4imps/covenant).

```
contract/<primitive>/README.md   purpose, obligations, covers block
tests/contract/<primitive>.rs    the tests that prove them
```

## Obligations

An obligation is one line, numbered `A1`, `A2`, … and never renumbered:

```markdown
- **A1** One sentence a test can prove.
```

## Covers

A fenced `covers` block lists the code the primitive is about, one
`<file>::<function>` per line.

## Surface

An optional fenced `surface` block lists the commands and MCP tools the
primitive's tests drive, one `cli <command>` or `mcp <tool>` per line:

```surface
cli done
mcp validate
```

List only what the tests actually call, so coverage counts what is proved.

## Contract tests

Every `#[test]` in `tests/contract/<primitive>.rs` names the obligations it
proves, in a comment above it:

```rust
// Contract: ids/A1, ids/A2
#[test]
fn ...
```

## What fails the build

`tests/contract/obligations.rs` runs with the contract suite and fails when:

- an obligation has no tagged test, or two obligations share a number
- a test in a primitive's file has no tag, or a tag names an obligation that
  doesn't exist
- a tag appears outside its primitive's test file
- a `covers` line names a file or function that doesn't exist
- a `surface` line names a command or tool sara doesn't have
- `contract/coverage.json` is out of date

The tagged tests run in the same suite, so an obligation only holds while its
tests pass.

## Coverage

`contract/coverage.json` counts the obligations and how many of sara's CLI
commands and MCP tools some primitive's surface covers, and lists the rest.
After changing a primitive, refresh it:

```sh
SARA_UPDATE_CONTRACT=1 cargo test --test contract obligations
```

CI writes it to the job summary, and the Pages site shows it at
[`/contract.html`](https://abarbesgaard.github.io/Sara/contract.html).
