# Architecture

Sara uses a **vertical slice** layout: every command lives in its own self-contained
directory under `commands/`; the domain model lives in `domain/`, and cross-cutting
concerns live under `infrastructure/`. Those two are the only layers a command may depend on.

## Layout

```
src/
  main.rs                   # CLI dispatch — match on Command enum, call slice entry-points
  cli.rs                    # Clap struct definitions for all commands
  domain/
    mod.rs                  # re-exports every entity
    task.rs                 # Task (+ Priority, Status) — one entity per file, named after it
    …
  infrastructure/
    mod.rs                  # re-exports all infrastructure modules
    db.rs                   # Database (open, migrations, all query helpers)
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

## Tiers

| Tier | Location | Rule |
|------|----------|------|
| **Domain** | `src/domain/` | Entities and value objects. Imports nothing from the crate. Pure: no persistence, presentation or I/O. |
| **Infrastructure** | `src/infrastructure/` | May be imported by anything. Never imports from `src/commands/`. |
| **Command slice** | `src/commands/<name>/mod.rs` | Imports only from `crate::domain` and `crate::infrastructure`. Never imports another command slice or `crate::cli`. |

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

## Enforced rules

`tests/architecture.rs` enforces these with [ArchUnitRust](https://github.com/LukasNiessen/ArchUnitRust)
(`archunit`), which analyzes every file under `src/` — including qualified paths such as
`crate::commands::add::f()`, not just `use` lines. Run them with `cargo test --test architecture`.

### Layers

| Layer | May depend on |
|-------|---------------|
| `domain/` | nothing |
| `infrastructure/` | `domain` |
| `commands/` | `infrastructure`, `domain` |
| `mcp/` | `commands`, `infrastructure`, `domain` |
| `completion.rs` | `infrastructure`, `domain` |
| `cli.rs` | `completion` |
| `main.rs`, `dispatch.rs` | anything (composition root) |

### Domain

- External crates are limited to `std`, `anyhow`, `chrono`, `serde` and `uuid`.
- No I/O: no `std::fs`, `std::io`, `std::net`, `std::process`, `std::env` or printing.
- One entity per file: each file defines exactly one `pub struct` and is named after it
  (`BranchRecord` → `branch_record.rs`). Enums and helpers only that entity uses live alongside it.
- Read models and query results (`ProjectStats`, `SearchHit`, …) are not domain; they stay in `db`.

### Infrastructure

Inside `infrastructure/`, dependencies point inwards (every module may also use `domain`):

| Module | May depend on |
|--------|---------------|
| `util`, `config`, `git` | nothing |
| `db` | `util`, `config` |
| `memory` | `util`, `db` |
| `project` | `config`, `db` |
| `telemetry` | `config` |
| `tui` | `util` — presentation never touches persistence |

### Slices

- A slice never depends on another slice; shared code goes in `commands::shared`. Existing
  violations are frozen in `KNOWN_CROSS_SLICE_DEBT` — a ratchet that fails on new edges and on
  entries that have been paid off.
- `mod.rs` is a slice's only public surface: code outside a slice may not reach its other files
  (`shared` excepted).
- Every slice directory has a `mod.rs` and a matching `pub mod` in `commands/mod.rs`.

### Frameworks stay at the edges

- `clap` / `clap_complete`: only `main.rs`, `cli.rs`, `dispatch.rs`, `completion.rs`.
- `rmcp`, `tokio`, `schemars`: only `mcp/`.
- `ureq`, `libc`: only `infrastructure/telemetry.rs`.
- `rusqlite_migration`, `Migrations::new`, `M::up`: only `infrastructure/db/`.
- Raw SQL: never in `commands/`.
- `ratatui`, `ratatui_textarea`, `crossterm`: only `infrastructure/tui/` and a slice's presentation
  files (`render.rs`, `render/`, `edit.rs`, `interactive/`). `init_terminal()` lives in
  `infrastructure/tui/` only.
- Presentation files (`render*`, `infrastructure/tui/`) never use `rusqlite` or `infrastructure::db`;
  the slice loads data and hands it to the renderer.
- `infrastructure/` (except `tui/`) and `mcp/` never print to stdout — in `sara mcp`, stdout is the
  JSON-RPC transport. The acceptance gate routes output through `mode.say(...)`.

### Size

No source file exceeds 400 non-blank lines; split by concern (see the intra-slice convention above).

### Tests live in `tests/`

`src/` contains production code only: no `#[test]`, `proptest!`, test-only helpers, fixtures or
`tests/` directories. Unit tests that need private access live under `tests/unit/` and are
compiled as a child module:

```rust
#[cfg(test)]
#[path = "../../tests/unit/commands/guide/mod.rs"]
mod tests;
```

`#[cfg(test)]` may appear in `src/` only directly above such a `#[path = ".../tests/..."]`.

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
