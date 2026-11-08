# The Contract

What sara promises, written down as numbered obligations, each proved by a
contract test. [impish](https://github.com/Abarbesgaard/impish) holds it; its
[contract/README.md](https://github.com/Abarbesgaard/impish/blob/main/contract/README.md)
is the convention: obligation blocks, the `#[impish::contract]` attribute, and
what fails the build.

```
contract/<primitive>/README.md   purpose, obligations, covers block
tests/contract/<primitive>.rs    the tests that prove them
```

Every contract test says both things it is: a test, and proof of an
obligation.

```rust
#[test]
#[impish::contract("add/A1")]
fn an_added_task_is_pending() { … }
```

The attribute alone would also make the function a test, but sara always
writes `#[test]` (or `#[tokio::test]`) above it; the test still runs once.

A `covers` block lists the functions a primitive's obligations are about, as
`<file>::<function>`. It is sara's own: impish doesn't read it, but CI's New
tests job (`scripts/new-tests.sh`) breaks those files to check that a new
contract test catches something.

CI runs `cargo impish check`, which fails when an obligation has no test or a
contract test names no obligation, and writes the result to the job summary.
Run it locally after changing a primitive:

```sh
cargo install cargo-impish --version 0.7.2 --locked
cargo impish check
```
