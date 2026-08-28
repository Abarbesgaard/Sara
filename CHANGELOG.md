# Changelog

## [Unreleased]

## [1.1.0] - 2026-08-28

Hardens the MCP server and completes its memory surface.

### Features

- **Memory maintenance over MCP** — `consolidate`, `reflect`,
  `diagnose_memories` and `reindex_embeddings` are now MCP tools (40 -> 44).
  They were the last non-interactive, `--json`-capable commands still CLI-only,
  so an agent could `learn`/`recall`/`forget`/`promote`/`prune` but could not
  maintain the memory graph it depends on. Defaults mirror the CLI's, so the two
  interfaces cannot silently diverge.
- **Richer validate failures over MCP** — a red acceptance gate now returns the
  failing command, its exit code and its captured output, instead of only the
  criterion's text.

### Fixes

- **`validate` no longer corrupts the MCP JSON-RPC stream** — the acceptance
  gate printed progress and let verify commands inherit stdout, which the MCP
  stdio transport reserves for JSON-RPC. Calling the tool injected raw text into
  the stream and crashed conformant clients; any project running `cargo test` or
  `pytest` flooded it. The CLI keeps its live streaming output unchanged.
- **Co-activation no longer loses genuine co-firings** — recall events were
  bucketed on a fixed epoch grid, so two recalls 100ms apart fell in different
  buckets whenever they straddled a boundary (exactly 5% of the time), silently
  dropping Hebbian reinforcement and making a test flaky. Bursts are now cut by
  single linkage, on the gap between consecutive events.
- **A relative `project_path` is rejected** — the MCP server is long-running and
  its working directory is whatever the previous call left, so `".."` silently
  targeted an unrelated repository.

### Documentation

- README: corrected the MCP tool count (twenty-six -> forty-four) and added the
  eleven tool rows that were missing from the table.

## [1.0.0] - 2026-08-25

First stable release. Consolidates the memory system's canonical/derived model
and brings CI to the `nightly` trunk.

### Features

- **Canonical memories now act on the graph** — the `derived_from` relationship
  went from a passive label to a first-class signal across the whole memory
  lifecycle:
  - **Strength boost** — a canonical memory's strength rises with its
    derived-child count (capped), so the memory more work relies on outranks its
    offshoots in `recall`/`dream`.
  - **Smarter overlap warnings** — `sara learn` detects when a near-duplicate or
    partial-tag match is itself canonical and suggests `--derived-from`/`relearn`
    instead of a generic warning.
  - **Cascade on forget** — `sara forget` warns and lists a canonical's derived
    children; `--cascade` archives them too. The same warning fires on
    `sara learn --supersedes`.
  - **Visible labels** — `sara memories` and `sara dream` now show
    `[canonical, N derived]` / `[derived from: mN]` in plain and JSON output.
  - **Cross-project reach** — a canonical memory surfaces under any project one
    of its derived children belongs to, even if it lives in a different project.

### Fixes

- **Windows file-link resolution** — `resolve_file_link` treated a POSIX-style
  absolute path (`/repo/x`) as relative on Windows (no drive letter) and leaked
  native `\` separators into stored links. Paths are now judged absolute
  platform-independently and always stored forward-slash, so links resolve
  identically across operating systems.
- **Security** — bumped `crossbeam-epoch` to 0.9.20 (RUSTSEC-2026-0204) and
  `anyhow` to 1.0.104 (RUSTSEC-2026-0190 unsoundness).

### CI

- Rust CI (test matrix, fmt, clippy, build) and the security audit now run on
  the `nightly` trunk, not just `main`/`master`, so PRs targeting `nightly` get
  full feedback.

### Features (memory & execution, carried from Unreleased)

- **`add` surfaces the actual knowledge, not a pointer** — the pre-create similarity check now returns **full memory bodies** for matching learned memories (previously it silently dropped every memory hit, because it ran `resolve_task` on the item's own uuid, and only ever showed 160-char task snippets). It adds a **tag-exact pass** (`confidence: "canonical"`) and a **semantic embedding pass** (`confidence: "semantic"`) alongside the existing phrase/token passes, so the canonical fix for a recurring fault lands in context the instant the charge is created — no second `recall` round-trip.
- **Fail-closed `validate`** — `sara validate` now **runs every acceptance criterion's `verify` command and refuses to stamp** unless every criterion has a command and all exit 0. "Validated" now means *proven green by a command*, never asserted in prose. CLI `--no-run` stamps without running (with a loud warning) for environments where the checks genuinely can't run locally. The MCP `validate` tool is fail-closed with no escape hatch.
- **Branch guard against recycled display ids** — `add` auto-ties the new task to the current git branch, and `done`/`validate` **refuse a bare numeric display-id mutation when the project is on a different branch than the task's tie** (the concurrent-agent recompaction hazard), printing the stable UUID to use instead. Positive-evidence only: silent for uuid inputs, untied tasks, detached HEAD, or `--force`.
- **Finding-resurface (reconsider prompt)** — `step_done` (with a result) and `annotate --kind finding` now embed the new text and surface the task's **own prior findings that are semantically close** (cosine ≥ 0.55, top 2), so an agent is reconnected to what it already concluded exactly when it records something that contradicts it. Emitted as `related_findings` in the JSON/MCP path and a `⟳ reconsider` prompt on the CLI.


## [0.9.2] - 2026-08-17

### Changes

- **Semantic recall is now always on** — embedding-based ranking previously required the opt-in `--semantic` flag or `[recall] semantic = true`. Recall now always matches by meaning as well as keyword, so a paraphrase surfaces relevant memories even when it shares no literal term. The `--semantic` flag and `[recall] semantic` config field are retained as no-ops for backward compatibility. Cost stays bounded: the semantic merge only runs for non-empty queries, so bare `sara recall` and tag/project/file-only lookups still skip embeddings.

## [0.9.1] - 2026-08-17

### Features

- **`sara verify --tick-on-pass`** — runs each step/acceptance criterion's own stored verify command and marks it done **only** when it exits 0, recording the pass/fail as the item's execution result. Collapses "run the check", "read the output", and "tick the box" into a single call (`sara verify <id> --tick-on-pass`), instead of running the test by hand and then ticking manually.
- **`--json` on `sara step done` / `step undone` / `step remove`** — these write commands now emit the same structured record the MCP tools return (task, uuid, kind, index, commit, `activated`) when passed `--json`, so agents can act on fields instead of parsing human text.
- **Auto-transition to active on first work** — the first `sara step done` or `sara verify --run`/`--tick-on-pass` against an idle task now starts its timer automatically (`ensure_started`), so a task's active state reflects reality without remembering a separate `sara start`. `step done --json` reports whether it `activated` the task.
- **`sara recall` with no arguments returns recent memories** — a bare `sara recall` (no query, `--tag`, `--project`, or `--file`) now surfaces the most recent memories with `confidence: "recent"` instead of erroring, so the cheap exploratory "what do I know?" call just works.

### Changes

- **Learn memory size limit raised to 4000 chars and made configurable** — the guard against pasting a raw conversation into `sara learn` now defaults to 4000 characters (was 2000) and honours the `SARA_MEMORY_CHAR_LIMIT` environment variable to raise (or lower) it without editing the binary. `--force` still bypasses the check entirely.

### Bug Fixes

- **`sara relearn` now refreshes the semantic embedding** — editing a memory's body re-indexed FTS (via the `items` update trigger) but left the semantic embedding stale, so `recall --semantic` kept matching the old text. `relearn` now re-embeds when the body changes, but only for memories that were already indexed (never fabricating an embedding for one that had none).
- **Semantic recall respects exact `--tag`/`--project`/`--file` filters** — `merge_semantic_hits` scored the whole embedding corpus and appended matches regardless of the active exact filter, leaking memories outside the requested set. Semantic candidates are now restricted to the exact-filter allowlist, preserving AND semantics.
- **`sara dream` back-navigation no longer loses breadcrumbs** — `Esc`/`Backspace` popped the breadcrumb before confirming the target still loaded, so a since-forgotten crumb silently failed and was consumed. Back-nav now skips dead crumbs and only lands on a memory that still exists.

## [0.9.0] - 2026-07-06

### Features

- **Dependency tree panel in `sara info`** — replaces the old linear "feature chain"/depth-1 graph with a real recursive tree (blockers above, dependents below), each node showing id/status/description, expandable/collapsible with `d`, capped by depth and node count with a "+N more" row on overflow. Moved to a fixed-width panel on the *left* of the screen so the main content stays on the right.
- **Collapsed AI notes in `sara info`** — findings/constraints/assumptions/open-questions/non-goals/decisions/patterns ("the AI's execution workpaper") now collapse into a one-line count-by-kind summary by default, toggled open with `n`. **Risks** are the exception — always shown in full, since they're the one AI-authored note kind that's actually human-relevant.
- **Decluttered `sara info`** — Status only shows when it isn't the boring default, Age merges into Entered, the UUID row is dropped from the main view, the urgency breakdown is hidden behind a `u` toggle, long assignment/rationale/comment text truncates behind a `v` (verbose) toggle, checklist items collapse to one line unless selected or verbose, repeated per-section hints are merged into a single legend line, and the unused project-stats panel is gone from the layout.
- **`sara init --setup-cmd/--test-cmd/--lint-cmd/--run-cmd`** — set per-project verification commands. These now show up in `sara info`'s Verification section *and* are actually executed by `sara verify --run` (previously the section existed but nothing could populate it, and `verify` silently ignored project-level commands even when set).
- **`sara modify --estimate`/`--clear-estimate` and `--every`/`--recur`/`--clear-recur`** (+ MCP `modify`) — set or clear a task's time estimate and recurrence interval non-interactively, without opening the review-form TUI. Previously these fields were only editable from inside the `sara info` TUI.
- **`sara record-run`** (+ MCP tool `record_run`) — record an AI/LLM interaction against a task (kind/model/provider/prompt/response), populating `sara info`'s "AI activity" section, which previously had no way to ever be populated.
- **`sara resolve --run <run-id>`** (+ MCP `resolve` `run_id`) — link a resolved feedback item to the AI run that addressed it.
- **Capped "Related tasks (shared tags)" in `sara info`** — shows the top 3 by urgency instead of every tag-overlapping task in the project, with a "+N more" summary.

### Bug Fixes

- **`--kind`/`--author`/etc. silently swallowed by `annotate`, `add`, and `recall`** — these commands used a `trailing_var_arg` positional for their free-text argument, which (per clap's documented behavior) consumes everything after it verbatim, including flags that come later on the command line. `sara annotate 19 "text" --kind risk --author ai` silently saved as a plain comment with `risk ai` appended to the text, both `kind`/`author` defaulting to `comment`/`human`. Flags are now parsed correctly regardless of position.
- **`sara verify`/`verify_value` ignored project-level commands** — only read task-level `verify_cmd`s and a `meta_json` grab-bag that nothing could ever populate; now also pulls `setup`/`test`/`lint` from the project profile (matching what `sara info` already displayed).
- **Wrong command hint in `sara info`'s Verification section** — said `sara guide <id> --run`, a subcommand that doesn't exist; now says `sara verify <id> --run`.

## [0.8.0] - 2026-07-06

### Features

- **Issue-tree board** — `sara board` now groups tasks under the GitHub issue they link to (collapsed by default, expand/collapse with `o`/`Space`/`Enter`/`h`/`l`) instead of the old dependency-chain "Feature" grouping. Tasks with no linked issue show as flat top-level rows, and `sara board --finished` opts into seeing completed tasks (hidden by default, along with any issue whose tasks are all done). ([#73](https://github.com/Abarbesgaard/Sara/pull/73))
- **Board visual redesign** — bordered rounded-corner task rows, priority shown as a colored background chip instead of dim text, a fixed-width badge slot so descriptions stay aligned, and deeper indent for nested tasks so hierarchy reads at a glance. Long comment bodies in `sara info` now wrap onto indented lines instead of running on unbroken, with blank-line separation between comments.
- **PR/issue badges + group-by-issue toggle on the board** — board rows now surface the same PR/`ISS` badges `sara list` already computes, and `i` toggles between the dependency-chain view and grouping by issue; the mode persists across navigating into a task and back. ([#72](https://github.com/Abarbesgaard/Sara/pull/72))
- **`sara list --by-issue`** — groups pending tasks by the GitHub issue they trace back to (e.g. `Issue #47 · owner/repo`), with a trailing bucket for tasks with no issue link; issue-linked tasks also get a distinct `ISS` badge so they're no longer indistinguishable from generic links. ([#63](https://github.com/Abarbesgaard/Sara/pull/63))
- **Dependency graph view in `sara info`** — `d` toggles a depth-1 dependency graph panel (cycling chain panel → depth-1 graph → expanded graph → off; capital `D` expands to the full transitive-blocker closure), showing id/status/badge for up to 4 neighbors per side with a "+N more" row. ([#68](https://github.com/Abarbesgaard/Sara/pull/68))
- **Edit description/comments in `$EDITOR`** — `Ctrl+E` in `sara info` opens the Description field or a new comment in your `$VISUAL`/`$EDITOR` instead of the in-TUI textarea. ([#67](https://github.com/Abarbesgaard/Sara/pull/67))
- **`?` keybinding help overlay** — `board`, `info`, and the add/edit review form now show a help overlay listing active keybindings, sourced from the same dispatch table the keymap actually uses so it can't drift out of sync. ([#71](https://github.com/Abarbesgaard/Sara/pull/71))
- **Shared TUI keymap** — `board`, `info`, and the review form now share one `Mode`/`Action`/`KeyDispatcher` (`src/infrastructure/tui/keymap.rs`), so `hjkl` navigation and `gg`/`G` (top/bottom) behave identically across screens instead of each hand-rolling its own key loop. ([#66](https://github.com/Abarbesgaard/Sara/pull/66))

### Bug Fixes

- **Issue badges no longer over-applied** — the `ISS` badge on a task row now only appears when the task carries actual `sara sync` provenance, not for every task that merely links back to an issue.
- **`attach` URL results tagged as `link`** — `attach_value`'s URL branch now reports `kind: "link"` like the standalone `link` tool, and the in-memory test database enforces `foreign_keys=ON` so FK/cascade behavior matches production. ([#62](https://github.com/Abarbesgaard/Sara/pull/62))

### Removed

- **GitHub activity heatmap panel** — removed from `sara info`'s side panel for now; the underlying code is left in place (unused) so it can be re-enabled without a re-implementation. ([#69](https://github.com/Abarbesgaard/Sara/pull/69))

## [0.7.0] - 2026-07-02

### Features

- **`sara mcp` — MCP server** — a stdio JSON-RPC [Model Context Protocol](https://modelcontextprotocol.io) server (built on the official `rmcp` SDK) exposing sara's agent loop as twenty-six typed tools. Any MCP client (Claude, Codex, Copilot, …) can drive sara with structured JSON instead of the CLI's flag-ordering / UUID / TUI footguns. Every tool takes an optional `project_path` so a long-running server stays folder-aware. Implemented as a thin adapter over the existing command/db layer (new print-free `*_value` cores shared with the CLI's `--json` paths — one serializer), with the async runtime confined to this subcommand so the rest of the CLI stays synchronous.
  - Read: `list`, `info`, `next`, `steps`, `verify` (read-only), `recall`, `feedback`, `plan_show`.
  - Create / guide: `add`, `step_done`, `annotate`, `plan_import`, `check`, `step_undone`, `step_remove`, `assignment`, `rationale`, `attach`.
  - Completion / edit / lifecycle: `done`, `link`, `dep` (on/off/list), `validate`, `modify` (non-interactive setters — requires ≥1 field, never opens the review form), `resolve`, `start`, `stop`.
  - Interactive-only surfaces (bare `add`/`modify` review form, `board`, `activity`, `projects`) and niche/destructive/setup commands (`init`, `move`, `delete`, `reset`, `undo`, `sync`, `export`/`import`) remain CLI-only by design; no tool opens a TUI or reads stdin.

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
