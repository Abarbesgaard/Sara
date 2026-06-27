# Copilot instructions for Sara

## Commands

- Build: `cargo build`
- Release build: `cargo build --release`
- Test: `cargo test`
- Test all features: `cargo test --all-features`
- Format check: `cargo fmt -- --check`
- Clippy: `cargo clippy --all-targets --all-features -- -D warnings`
- Single test: `cargo test cli::tests::cli_name_is_sara` (replace with any test name or module path substring)

## Architecture

- Sara is a single Rust binary (`src/main.rs`) with a Clap command enum in `src/cli.rs`; `main.rs` normalizes taskwarrior-style shorthands and dispatches to `src/commands/*`.
- Task data lives in SQLite under the user config/data directories, not inside the repository. `src/config.rs` owns config/data paths and legacy `tk` migration; `src/db.rs` owns schema, migrations, undo batching, and task persistence.
- A Git repository is treated as a project by default. `src/project.rs` resolves the current project from the git root when possible, otherwise from the folder name. `sara init` records project metadata and optional LLM-seeded starter tasks.
- The main user flows are `list`, `info`, `add`, `done`, dependency management, time tracking, checklist/guide editing, and branch linkage. `list` and `info` are built on the same task model and guide data.
- Optional LLM enrichment lives in `src/enrich.rs` and is wired into task creation and project initialization. Provider selection is configured in `src/config.rs` and `src/commands/provider.rs`.

## Key conventions

- Most commands accept either the small recycled display ID or a UUID prefix. Display IDs are not stable; UUIDs are.
- `sara add` parses inline Taskwarrior-style tokens only at the edges of the description (`project:foo`, `+tag`, `pri:H`, `every:daily`). Flags must come before the trailing description words.
- `--yes` skips interactive forms/prompts; `--no-llm` disables enrichment. In non-interactive mode, Sara accepts the current values directly.
- `sara list` defaults to the current project and prints urgency-ranked pending tasks. `-a` shows all projects; `-p/--project` filters explicitly.
- Dependency direction matters: `sara dep A on B` means A is blocked by B. The UI uses `⊘` for blocked tasks and `⛓` for tasks blocking others.
- A task’s guide is broader than a plain todo: steps/checklist items, acceptance criteria, notes, links, file anchors, history, and validation metadata are all persisted in SQLite.
- `sara undo` reverts the most recent command batch; mutating commands are wrapped in an undo batch unless the command itself is `undo`.
- `sara project init` is deprecated; use `sara init`.
- `sara addbranch` stores the current git branch for a task; `sara stop` can snapshot changed files when a branch is tied.

## Repo-specific notes

- `NO_COLOR=1` disables colored output.
- Config lives at `~/Library/Application Support/sara/config.toml` on macOS and `~/.config/sara/config.toml` on Linux.
- The schema and guide view in `src/db.rs` are the source of truth for what `sara info` can render.
