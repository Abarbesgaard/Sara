# Changelog

## [0.5.6] - 2026-07-01

### Features

- **Echo UUID on `sara add`** — creation output now includes an 8-char UUID prefix, so agents/scripts creating tasks don't need a follow-up `sara info` lookup. ([#56](https://github.com/Abarbesgaard/Sara/pull/56))
- **`--annotation`, `--link`, `--check` flags on `sara add`** — attach notes, URLs, and checklist steps inline at task creation instead of separate follow-up commands. ([#56](https://github.com/Abarbesgaard/Sara/pull/56))
- **`--depends-on` flag on `sara add`** — wire a dependency at creation time without a separate `sara dep` call. ([#56](https://github.com/Abarbesgaard/Sara/pull/56))
- **`sara dep chain <id1> <id2> ...`** — wire a full linear dependency sequence in one command. ([#56](https://github.com/Abarbesgaard/Sara/pull/56))

### Internal

- **More vertical-slice splits** — `add`, `annotate`, and `board` were broken into focused sub-modules (input/persist, annotations/files/links, types/state), continuing the pattern from 0.5.5. ([#51](https://github.com/Abarbesgaard/Sara/pull/51), [#52](https://github.com/Abarbesgaard/Sara/pull/52), [#53](https://github.com/Abarbesgaard/Sara/pull/53))
- **Simplified board graph algorithms** — replaced union-find with a plain BFS and Kahn's-algorithm/heap with a simple indegree-drain loop, and precomputes board stats instead of recounting every frame (124 lines net removed). ([#54](https://github.com/Abarbesgaard/Sara/pull/54))
- **`activity` command split** — `mod.rs` slimmed to a thin orchestrator; `render.rs` split into focused zone functions (`render_stats`, `render_month_labels`, `render_heatmap`, `render_legend`). ([#55](https://github.com/Abarbesgaard/Sara/pull/55))

## [0.5.5] - 2026-06-29

### Features

- **Project-browser TUI** — `sara` now ships a project-browser TUI so you can switch between projects without leaving the terminal. ([#35](https://github.com/Abarbesgaard/Sara/pull/35))

### Internal

- **Vertical-slice architecture** — all 23 commands were migrated to isolated subdirectory slices (`src/commands/<cmd>/`), each owning its own handler, types, and SQL. Shared plumbing lives in a new `src/infrastructure/` module. ([#39](https://github.com/Abarbesgaard/Sara/pull/39), [#41](https://github.com/Abarbesgaard/Sara/pull/41), [#42](https://github.com/Abarbesgaard/Sara/pull/42))
- **Architecture enforcement tests** — a suite of compile-time invariant tests catches cross-slice coupling, naming-convention drift, and SQL leaking out of the infrastructure layer. ([#43](https://github.com/Abarbesgaard/Sara/pull/43), [#44](https://github.com/Abarbesgaard/Sara/pull/44))
- **`info` command split** — the 3 600-line `info/mod.rs` was broken into 5 focused sub-modules (render, input, state, actions, layout). ([#46](https://github.com/Abarbesgaard/Sara/pull/46))

## [0.5.0] - 2026-06-27

### Features

- **Checklist editing from the TUI** — `sara info` can now add new checklist steps inline (`a`) and reorder them with `Shift+Up` / `Shift+Down` (or `K` / `J`). No more dropping out to `sara check` just to extend a list. ([#24](https://github.com/Abarbesgaard/Sara/pull/24))
- **`gh auth token` fallback for sync** — `sara sync` now resolves a GitHub token in precedence order: `GH_TOKEN` → `GITHUB_TOKEN` → `gh auth token`. If you authenticated with `gh auth login`, Sara picks it up automatically — no manual token export needed. ([#26](https://github.com/Abarbesgaard/Sara/pull/26))

### Bug Fixes

- **File picker hides `.git/`** — the fzf file/folder picker no longer descends into `.git/`, trimming hundreds of noise entries from the candidate list. Genuinely useful dotfiles like `.github/` remain. ([#29](https://github.com/Abarbesgaard/Sara/pull/29))
- **Database migration fix** — backfills `github_owner` and `github_repo` columns in the `projects` table for databases that were upgraded mid-sequence and missed the migration. ([#25](https://github.com/Abarbesgaard/Sara/pull/25))

## [0.4.1] - 2026-06-27

### Bug Fixes

- **Dynamic GitHub remote detection** — `sara sync` no longer requires the remote to be named `origin`. Sara now searches all configured remotes for a GitHub URL and uses the first one found.

## [0.4.0] - 2026-06-27

### Features

- Initial release of GitHub sync (`sara sync`).

## [0.3.0] - 2026-06-26

### Features

- Task export/import (`sara export` / `sara import`) — share a task and its full dependency closure as a portable blob.
- Full history panel in `sara info` with `--history` flag.
- `sara undo` to revert the most recent mutating command.
- `sara reset` to wipe a project's tasks and profile.
- Shell completions with dynamic task-id and project-name suggestions.
- `--md` / `--plain` / `--json` output modes on `sara info` for agent-friendly output.

## [0.2.2] - 2026-06-26

### Bug Fixes

- Minor stability fixes.

## [0.2.0] - 2026-06-26

### Features

- Initial public release with folder-aware task management, urgency scoring, TUI detail view, dependencies, time tracking, git branch linkage, and recurring tasks.
