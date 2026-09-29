# Architecture

Sara uses a **vertical slice** layout: every command lives in its own self-contained
directory under `commands/`; cross-cutting concerns live under `infrastructure/` and
are the only layer a command may depend on.

## Layout

```
src/
  main.rs                   # CLI dispatch — match on Command enum, call slice entry-points
  cli.rs                    # Clap struct definitions for all commands
  infrastructure/
    mod.rs                  # re-exports all infrastructure modules
    db.rs                   # Database (open, migrations, all query helpers)
    model.rs                # Domain types: Task, Project, Priority, Status, …
    config.rs               # Config loading and paths
    project.rs              # Git-root detection, project-name resolution, token parsing
    tui/
      mod.rs                # Terminal setup — init_terminal() lives here and nowhere else
      fzf.rs                # Fuzzy-finder widget
      review_form.rs        # TUI review form
    dates.rs                # Date parsing helpers
    files.rs                # File-attachment helpers
    git.rs                  # Git helpers (branch detection, etc.)
    portable.rs             # Import/export serialisation
  commands/
    mod.rs                  # pub mod declarations — one entry per command slice
    activity/
      mod.rs                # `sara activity` — entry point
      render.rs             # TUI event loop and rendering
    add/
      mod.rs                # `sara add` — create a new task
    annotate/
      mod.rs                # `sara annotate` / `sara attach` / `sara link` — attach metadata
    board/
      mod.rs                # `sara board` — entry point + state builder
      render.rs             # TUI event loop and rendering
    branch/
      mod.rs                # `sara addbranch` — tie a git branch to a task
    delete/
      mod.rs                # `sara delete` — remove a task
    dep/
      mod.rs                # `sara dep` — manage task dependencies
    done/
      mod.rs                # `sara done` — mark a task complete
    export/
      mod.rs                # `sara export` — serialise a task to JSON/markdown
    guide/
      mod.rs                # `sara next` / `sara steps` / `sara step` / `sara verify` — execution cursor
    import/
      mod.rs                # `sara import` — deserialise tasks from a file
    info/
      mod.rs                # `sara info` — entry points (run, run_json)
      types.rs              # Detail, EditState, EditField, Focusable, and constants
      edit.rs               # Interactive TUI edit loop
      render.rs             # TUI rendering (render + panel helpers)
      plain.rs              # Plain-text and markdown output
    init/
      mod.rs                # `sara init` — initialise a project in the current repo
    list/
      mod.rs                # `sara list` — list tasks for the current project
    modify/
      mod.rs                # `sara modify` — edit task fields via TUI form
    move_task/
      mod.rs                # `sara move` — move a task to another project
    plan/
      mod.rs                # `sara plan` — import/show a structured plan
    projects/
      mod.rs                # `sara projects` — entry point + state builder
      render.rs             # TUI event loop and rendering
    recall/
      mod.rs                # `sara recall` — full-text search across tasks
    reset/
      mod.rs                # `sara reset` — wipe a project's tasks
    sync/
      mod.rs                # `sara sync` — entry point + token resolution
      github.rs             # GitHub REST API types and fetch functions
      import.rs             # Task creation / update / comment reconciliation
    undo/
      mod.rs                # `sara undo` — revert the last write command
```

## Two tiers

| Tier | Location | Rule |
|------|----------|------|
| **Infrastructure** | `src/infrastructure/` | May be imported by anything. Never imports from `src/commands/`. |
| **Command slice** | `src/commands/<name>/mod.rs` | Imports only from `crate::infrastructure`. Never imports another command slice or `crate::cli`. |

## Intra-slice file convention

Large command slices are split into focused sub-files within the same directory.
`mod.rs` is always the public entry point; the other files are private to the slice.

| File | Contents |
|------|----------|
| `mod.rs` | `pub fn` entry points + `mod` declarations — nothing else |
| `render.rs` | All TUI rendering and display functions |
| `handler.rs` | Business logic (for commands with no TUI) |
| `github.rs` | External API client code (e.g. GitHub REST calls) |
| `import.rs` | Data ingestion / reconciliation logic |
| `types.rs` | Command-specific structs, enums, and constants |
| `edit.rs` | Interactive edit loop and related helpers |

**Threshold:** Only split commands that are large enough to benefit — roughly > 200 lines
with at least two distinct concerns. Small commands (< ~200 lines or a single concern)
stay as a single `mod.rs`.

## The five invariants

These rules are enforced by the test suite in `tests/architecture.rs`.

### 1 — No cross-slice dependencies

A command slice must not import another command slice.

```rust
// FORBIDDEN inside src/commands/done/mod.rs
use crate::commands::list;
```

### 2 — Commands depend only on infrastructure

A command slice may import from:

```
crate::infrastructure::db
crate::infrastructure::model
crate::infrastructure::config
crate::infrastructure::project
crate::infrastructure::tui
crate::infrastructure::dates
crate::infrastructure::files
crate::infrastructure::git
crate::infrastructure::portable
```

```rust
// ALLOWED inside src/commands/done/mod.rs
use crate::infrastructure::db;
use crate::infrastructure::model::Status;

// FORBIDDEN — cli struct definitions are wired in main.rs, not in slices
use crate::cli::Command;
```

### 3 — DB migrations are centralised

All `Migrations::new(…)` / `M::up(…)` calls must live in `src/infrastructure/db.rs` only.
Adding a migration inside a command slice (e.g. `src/commands/sync/`) is forbidden.

### 4 — Command slices have a consistent structure

Every directory under `src/commands/` must contain a `mod.rs`. A new command that
adds only a bare directory without `mod.rs` will fail the structure test.

### 5 — TUI infrastructure is centralised

`init_terminal()` is defined in `src/infrastructure/tui/mod.rs` and must not be
duplicated inside any command slice. Commands that need a terminal call
`crate::infrastructure::tui::init_terminal()`.

## Adding a new command

1. Create `src/commands/<name>/mod.rs` with a `pub fn run(…)` entry-point.
2. Add `pub mod <name>;` to `src/commands/mod.rs`.
3. Add the variant to the `Command` enum in `src/cli.rs`.
4. Add the dispatch arm to the `match cli.command` block in `src/main.rs`.
5. Run `cargo test` — the architecture tests will catch any invariant violations.

## Migration notes

### Command files → subdirectories

Rust resolves `pub mod add;` to **either** `src/commands/add.rs` **or**
`src/commands/add/mod.rs`. Converting a command flat-file to a subdirectory is a
rename only — no import changes are needed in `main.rs`, `cli.rs`, or anywhere else.

### Shared files → `infrastructure/`

Moving `src/db.rs` → `src/infrastructure/db.rs` is a real import change. Every
`use crate::db` across all command files and `main.rs` must become
`use crate::infrastructure::db`. The same applies to all other modules that move
into `infrastructure/`. This is a mechanical sed-style update but it must be done
atomically with the file moves to keep the codebase compiling.
