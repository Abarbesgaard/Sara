# Changelog

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
