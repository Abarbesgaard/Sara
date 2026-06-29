# Architecture

Sara uses a **vertical slice** layout: every command lives in its own self-contained
directory; shared infrastructure lives at the `src/` root and is the only layer a
command may depend on.

## Layout

```
src/
  main.rs                   # CLI dispatch — match on Command enum, call slice entry-points
  cli.rs                    # Clap struct definitions for all commands
  db.rs                     # Database (open, migrations, all query helpers)
  model.rs                  # Domain types: Task, Project, Priority, Status, …
  config.rs                 # Config loading and paths
  project.rs                # Git-root detection, project-name resolution, token parsing
  tui/
    mod.rs                  # Terminal setup — init_terminal() lives here and nowhere else
    fzf.rs                  # Fuzzy-finder widget
    review_form.rs          # TUI review form
  dates.rs                  # Date parsing helpers
  files.rs                  # File-attachment helpers
  git.rs                    # Git helpers (branch detection, etc.)
  portable.rs               # Import/export serialisation
  commands/
    mod.rs                  # pub mod declarations — one entry per command slice
    add/
      mod.rs                # `sara add` — implementation only, no shared-layer logic
    list/
      mod.rs
    done/
      mod.rs
    …                       # one subdirectory per command
```

## Two tiers

| Tier | Location | Rule |
|------|----------|------|
| **Shared infrastructure** | `src/*.rs` and `src/tui/` | May be imported by anything. Never imports from `src/commands/`. |
| **Command slice** | `src/commands/<name>/mod.rs` | Imports only from the shared tier. Never imports another command slice or `crate::cli`. |

## The five invariants

These rules are enforced by the test suite in `tests/architecture.rs`.

### 1 — No cross-slice dependencies

A command slice must not import another command slice.

```
// FORBIDDEN inside src/commands/done/mod.rs
use crate::commands::list;
```

### 2 — Commands depend only on shared infrastructure

A command slice may import from:

```
crate::db
crate::model
crate::config
crate::project
crate::tui
crate::dates
crate::files
crate::git
crate::portable
```

Importing `crate::cli` from inside a command slice is forbidden — the CLI struct
definitions are wired in `main.rs`, not inside the slices.

### 3 — DB migrations are centralised

All `Migrations::new(…)` / `M::up(…)` calls must live in `src/db.rs` only.
Adding a migration inside a command slice (e.g. `src/commands/sync/`) is forbidden.

### 4 — Command slices have a consistent structure

Every directory under `src/commands/` must contain a `mod.rs`. A new command that
adds only a bare directory without `mod.rs` will fail the structure test.

### 5 — TUI infrastructure is centralised

`init_terminal()` is defined in `src/tui/mod.rs` and must not be duplicated inside
any command slice. Commands that need a terminal call `crate::tui::init_terminal()`.

## Adding a new command

1. Create `src/commands/<name>/mod.rs` with a `pub fn run(…)` entry-point.
2. Add `pub mod <name>;` to `src/commands/mod.rs`.
3. Add the variant to the `Command` enum in `src/cli.rs`.
4. Add the dispatch arm to the `match cli.command` block in `src/main.rs`.
5. Run `cargo test` — the architecture tests will catch any invariant violations.

## Rust module resolution note

Rust resolves `pub mod add;` to **either** `src/commands/add.rs` **or**
`src/commands/add/mod.rs`. Converting a flat file to a subdirectory is a rename
only — no import changes are needed anywhere else in the codebase.
