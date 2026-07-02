# Sara — a folder-aware task manager

`Sara` is what plan mode would be if it kept its memory. She's built primarily
as a tool for an LLM agent, not the human at the keyboard: instead of a plan
that disappears when the conversation ends, an agent can persist it in Sara
and pick up where it left off in the next session. She knows which Git
project the agent is standing in, ranks work with a transparent urgency
model, tracks time, and links tasks to branches. She's LLM-agnostic — any
agent can drive her through a plain local CLI, and a human can still poke
around by hand.

Task data lives in a single SQLite database in your home directory — **nothing
is ever written into your repositories.**


```bash
     ID  PRI   PROJECT      DUE           URG  DEPS             DESCRIPTION
──────────────────────────────────────────────────────────────────────────────
⛓    1  H     web-app      2026-07-01   28.0  blocks 1 task    Design the auth flow
     2  M     web-app      -             5.9                   Wire up the login form
⊘    3  M     web-app      -             0.9  blocked by 1     Write auth integration tests
```

---

## Table of contents

- [Highlights](#highlights)
- [Installation](#installation)
- [Quick start](#quick-start)
- [Core concepts](#core-concepts)
- [The task list](#the-task-list)
- [The detail view (`sara info`)](#the-detail-view-sara-info)
- [MCP server (`sara mcp`)](#mcp-server-sara-mcp)
- [Working with tasks](#working-with-tasks)
  - [Adding tasks](#adding-tasks)
  - [Dependencies](#dependencies)
  - [Time tracking](#time-tracking)
  - [Recurring tasks](#recurring-tasks)
  - [Checklists](#checklists)
  - [Notes, comments & links](#notes-comments--links)
  - [Git branch linkage](#git-branch-linkage)
  - [Sharing tasks](#sharing-tasks)
  - [History & undo](#history--undo)
- [The urgency model](#the-urgency-model)
- [Configuration](#configuration)
- [Inline Taskwarrior-style tokens](#inline-taskwarrior-style-tokens)
- [Due dates](#due-dates)
- [Shell completions](#shell-completions)
- [File locations](#file-locations)
- [Command reference](#command-reference)
- [Uninstall](#uninstall)

---

## Highlights

- **Folder-aware** — `sara` auto-detects the current project (a Git repo, or any
  folder you run `sara init` in) and scopes `sara list` to it by default.
- **Transparent urgency** — a Taskwarrior-style scoring model decides ordering; `sara info` shows the exact breakdown.
- **Interactive TUI** — a ratatui review form for adding/editing, and a rich detail view for everything else.
- **Dependencies** — block tasks on each other, with cycle detection and an at-a-glance `DEPS` column.
- **Time tracking** — `sara start` / `sara stop` accumulate active time, with optional estimates.
- **Git integration** — tie a task to a branch and snapshot the files it touched.
- **Full history** — every change (field edits, deps, files, checklist, links, comments, timer) is recorded.
- **Single SQLite file** — easy to back up, and nothing is written into your repos.

---

## Installation

The fastest paths use prebuilt packages and need **no Rust toolchain**. Prefer
one of these; the from-source build further down is for development.

### Quick install (Linux & macOS)

```bash
curl -fsSL https://raw.githubusercontent.com/Abarbesgaard/Sara/main/scripts/install.sh | sh
```

Downloads the right binary from the latest
[GitHub Release](https://github.com/Abarbesgaard/Sara/releases) into
`~/.local/bin` (override with `SARA_INSTALL_DIR`).

### crates.io

Published as **`sara-tasks`** (`sara` and `sara-cli` were taken); the installed
binary is still `sara`.

```bash
cargo install sara-tasks          # compiles from source
cargo binstall sara-tasks         # or grabs the prebuilt binary, no compile
```

### Debian / Ubuntu (apt)

One-off `.deb` from a release:

```bash
curl -fsSLO https://github.com/Abarbesgaard/Sara/releases/latest/download/sara_amd64.deb
sudo apt install ./sara_amd64.deb
```

Or add the apt repository so `sudo apt install sara` and future upgrades work:

```bash
echo "deb [trusted=yes] https://abarbesgaard.github.io/Sara/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/sara.list
sudo apt update
sudo apt install sara
```

---

### Build from source

### 1 — Prerequisites

**Rust** (if not already installed):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Restart your shell, or:
source "$HOME/.cargo/env"
```

### 2 — Build & install

```bash
git clone https://github.com/Abarbesgaard/Sara
cd Sara
cargo install --path .
```

This compiles the binary and places it at `~/.cargo/bin/sara`. Make sure
`~/.cargo/bin` is on your `PATH` (the Rust installer usually handles this):

```bash
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc   # or ~/.bashrc
source ~/.zshrc
sara --version
```

---

## Quick start

```bash
# Initialize the current folder as a Sara project.
# A git repo becomes its own project (named after the repo root); any other
# folder is initialized in place (named after the folder).
cd ~/web-app
sara init

# Add a task (opens the interactive review form)
sara add "add password reset flow"

# Add quickly, no form, with an inline priority token
sara add "fix logout redirect" pri:H --yes

# See what to work on (current project, ranked by urgency)
sara list

# Inspect / edit a task interactively
sara info 1

# Start the clock, do the work, stop it
sara start 1
sara stop 1

# Complete it
sara done 1
```

---

## Core concepts

**Projects.** Every task belongs to a project. Inside a Git repo, `sara` uses
the repo as the project (run `sara init` once to record its goal/stack). In any
other folder, `sara init` registers that folder as the project, named after the
directory. The configurable `default_project` (`inbox`) is only used as a
last-resort fallback when a folder has no usable name.

**IDs vs UUIDs.** Each task has a small, recycled display **ID** (the `1`, `2`,
`3` you type) and a stable **UUID** that never changes. Most commands accept
either the ID or a UUID prefix. When a task is completed, pending IDs are
repacked to stay small — so today's `4` may be tomorrow's `3`.

**Urgency.** Tasks are ordered by a computed urgency score (see
[The urgency model](#the-urgency-model)). It rewards priority, due dates,
active timers, tags, and tasks that block others — and penalizes blocked tasks.

---

## The task list

`sara list` prints the pending tasks for the current project, highest urgency
first.

```bash
sara list                      # current project
sara list -a                   # all projects
sara list -p web-app           # a specific project from anywhere (`--project` also works)
```

Each row has a small marker gutter, columns, and a dependency column:

```text
⛓    1  H     web-app      2026-07-01   28.0  blocks 1 task    Design the auth flow
```

**Gutter markers** (left edge):

| Marker | Meaning                                   |
|:------:|-------------------------------------------|
| `●`    | Timer is running (task is active)         |
| `♺`    | Recurring task                            |
| `⊘`    | Blocked — waiting on an unfinished task   |
| `⛓`    | Blocking — other tasks depend on this one |

**Columns:** `ID`, `PRI` (H/M/L, color-coded), `PROJECT`, `DUE` (red overdue,
yellow soon), `URG` (urgency score), `DEPS`, and `DESCRIPTION`. A `PR` or `↗`
badge appears before the description when the task has a linked pull request or
URL.

The **`DEPS`** column spells out the relationship the gutter hints at:
`blocked by 3` (red) or `blocks 2 tasks` (gray).

> Tip: set `NO_COLOR=1` to disable colors (e.g. for piping or screenshots).

---

## The detail view (`sara info`)

`sara info <id>` opens a full-screen, interactive view of a single task: all
fields, dependencies, attached files, links, comments, a checklist, the urgency
breakdown, a git panel, a project activity heatmap, and a live history log.

It's also where you **edit** a task inline.

**Keys**

| Key            | Action                                            |
|----------------|---------------------------------------------------|
| `↑` / `↓` (or `k` / `j`) | Move between fields and items            |
| `Enter` / `e`  | Edit the selected field, or open the selected file/link |
| `←` / `→`      | Cycle priority (when Priority is selected)        |
| `Space`        | Toggle the selected checklist item                |
| `a`            | Add a new checklist step (Enter saves, Esc cancels) |
| `⇧↑` / `⇧↓` (or `K` / `J`) | Reorder the selected checklist step within its kind |
| `PgUp` / `PgDn`| Scroll                                            |
| `Esc`          | Cancel an edit                                    |
| `q` / `Esc`    | Close the view                                    |

**Editable fields:** Description, Project, Priority, Due, Tags, Estimate,
Recur, and **Depends on**.

To change dependencies, select **Depends on**, press `Enter`, and type the task
IDs it should wait on (space- or comma-separated), e.g. `7 9`. `sara` reconciles
the set — adding and removing edges — and rejects self-references and cycles
with an inline error. The change is reflected immediately in the "Blocked by"
section and the History panel.

### Non-interactive & agent-friendly output

`sara info` detects whether stdout is a terminal: in a TTY it opens the
interactive view; when piped it prints a readable plain-text digest instead.
You can force a specific format regardless of TTY:

| Flag        | Output                                                                 |
|-------------|------------------------------------------------------------------------|
| `--md`      | **Markdown digest** — LLM-native: `##` headings and `- [ ]`/`- [x]` checkboxes for steps & acceptance. Ideal for agent context or a PR body. |
| `--plain`   | The readable plain-text digest, forced (no TUI).                       |
| `--json`    | The full structured guide (every field) for scripts.                   |
| `--history` | Include the full History log in `--plain`/`--md` (collapsed to a one-line summary by default to keep output lean). |

```bash
sara info 7 --md                 # paste-ready Markdown for an agent / PR
sara info 7 --md --history       # …including the full change log
sara info 7 --plain              # readable text, History collapsed
sara info 7 --json | jq .steps   # structured access
```

The Markdown digest is the recommended way to feed a task to an AI agent: it's
stable, omits the unbounded History log by default, and needs no reshaping.

---

## MCP server (`sara mcp`)

`sara mcp` runs a [Model Context Protocol](https://modelcontextprotocol.io)
server over stdio, exposing sara's agent loop as typed tools. Any MCP client
(Claude Code / Claude Desktop, OpenAI Codex, GitHub Copilot, Cursor, …) can then
drive sara with structured JSON in and out — no flag-ordering, UUID-juggling, or
TUI pitfalls. It's a thin adapter over the same code the CLI uses, so there is a
single source of truth.

The server exposes twenty-six tools — the non-interactive agent loop end to end,
from reading and planning a task through to completing it:

| Tool | Purpose |
|------|---------|
| `list` | Pending tasks for a project (or all) |
| `info` | Full task guide: steps, acceptance, notes, links, freshness, feedback |
| `add` | Create a task (never opens the review form) |
| `next` | The execution cursor — first not-done step |
| `steps` | Ordered steps (optionally up to step N) |
| `step_done` | Mark a step done, recording result + commit |
| `verify` | Read-only: the verification commands + acceptance criteria (does **not** run them) |
| `recall` | Cross-task keyword search |
| `annotate` | Add a comment / finding / decision |
| `plan_import` | Bulk-ingest a task graph from an inline JSON plan |
| `plan_show` | Dependency-ordered briefing (the task plus everything it is blocked by) |
| `check` | Add a checklist step or acceptance criterion (with optional intent / verify command) |
| `step_undone` | Reopen a completed step / acceptance criterion |
| `step_remove` | Delete step N (remaining items renumber) |
| `dep` | Manage dependencies — `action` = `on` / `off` / `list` |
| `link` | Attach a URL (e.g. a PR) to a task |
| `attach` | Attach a file or code anchor (`reason`, `symbol`, `lines`, `source`); a URL becomes a link |
| `assignment` | Set the task's assignment (the originating prompt / what to build) |
| `rationale` | Set the task's rationale (why it exists) |
| `modify` | Set task fields non-interactively (never opens the review form; at least one field required) |
| `validate` | Stamp the guide as validated against the project's current git HEAD |
| `feedback` | List a task's open human feedback |
| `resolve` | Resolve a feedback item by its id |
| `start` / `stop` | Time tracking (`stop` snapshots a tied branch's changed files) |
| `done` | Complete a task (errors if blocked unless `force`; spawns the next recurrence) |

Interactive-only surfaces (the bare `add`/`modify` review form, `board`,
`activity`, `projects`) stay CLI-only by design — the server never opens a TUI or
blocks on stdin. So do a few niche/destructive/setup commands (`init`, `move`,
`delete`, `reset`, `undo`, `sync`, `export`/`import`).

**Folder-awareness:** the CLI derives "the project" from the current git folder,
but a long-running server has no per-call working directory. So **every tool
takes an optional `project_path`** — set it to the absolute path of the target
repo and the tool operates on that project. Omit it to use the directory the
server was launched in.

### Client configuration

The server is launched as a subprocess over stdio — every client needs the same
two ingredients: **`command: sara`, `args: ["mcp"]`**. If `sara` isn't on the
client's `PATH`, point `command` at the absolute path (e.g. `~/.cargo/bin/sara`).

**Quickest — Claude Code** (`--scope user` makes sara available in every project):

```bash
claude mcp add --scope user --transport stdio sara -- sara mcp
# repo-local instead:  claude mcp add --scope project --transport stdio sara -- sara mcp
```

For any other client, drop in the same `command` + `args`:

**Claude Desktop** (`claude_desktop_config.json`):

```json
{ "mcpServers": { "sara": { "command": "sara", "args": ["mcp"] } } }
```

**OpenAI Codex** (`~/.codex/config.toml`):

```toml
[mcp_servers.sara]
command = "sara"
args = ["mcp"]
```

**GitHub Copilot / VS Code** (`.vscode/mcp.json`):

```json
{ "servers": { "sara": { "type": "stdio", "command": "sara", "args": ["mcp"] } } }
```

> On stdio transport, stdout is the JSON-RPC channel: run `sara mcp` directly (no
> wrapper that writes to stdout). Diagnostics go to stderr.

### Instructing your agent to use it

Once connected, the tools show up in the client automatically, and the server
sends usage **`instructions`** on `initialize` — the `project_path` model, UUID
targeting, and the execution loop through to the PR/completion discipline. Clients
like Claude Code surface those to the model, so often no extra prompting is needed.

For stronger, always-on steering, add a short rule to your agent's own persistent
instructions (Claude Code's `CLAUDE.md`, an `AGENTS.md`, Cursor rules, …):

> Use the **sara MCP tools** for task management (prefer them over the `sara` CLI).
> Pass **`project_path`** — the absolute path of the repo you're working in — on
> every call. Target tasks by their **8-char UUID prefix**, not the recycled
> display id. Mirror multi-step work into sara: tick steps with `step_done` as you
> finish them, `link` the PR when you open it, and call `done` only once that PR
> has merged.

Keep it short — the tool descriptions and the server's `instructions` carry the
mechanics; your rule just says *prefer sara, pass `project_path`, follow the loop*.

---

## Working with tasks

### Adding tasks

```bash
sara add "write integration tests"            # opens the review form
sara add "write tests" --yes                  # skip the form, save immediately
sara add "write tests" -p web-app --priority H -t testing
```

By default `sara add` opens an interactive review form so you can confirm the
fields before saving. `--yes` saves immediately without the form. See
[inline tokens](#inline-taskwarrior-style-tokens) for the `project:` / `+tag` /
`pri:` shorthand.

Creation output echoes the new task's UUID prefix, so scripts/agents never need
a follow-up lookup: `Created task 5 [Sara] (3f458474): write tests`.

Attach notes, links, checklist steps, and a dependency **inline at creation**
instead of separate follow-up commands (all repeatable except `--depends-on`,
which chains onto an existing task by uuid prefix):

```bash
sara add --priority H -t api --annotation "amounts in minor units" \
  --link "https://…/Controller.cs" --check "Add model" --check "Wire route" \
  --depends-on 3f458474 --yes "Implement payment client"
```

```bash
sara modify 2        # edit via the review form
# …or set fields non-interactively (no TUI):
sara modify 2 --description "new text" --priority H --due 2026-12-31
sara modify 2 --tag api --tag urgent   # replace tags (--clear-tags / --clear-due to unset)
sara done 1          # mark complete (use --force to complete a blocked task)
sara delete 3        # soft-delete (prompts; -y to skip)
```

### Dependencies

A dependency means "this task is blocked until that task is done." Blocked tasks
sink in urgency; blocking tasks rise.

```bash
sara dep 4 on 5      # task 4 now depends on (is blocked by) task 5
sara dep 4 off 5     # remove that dependency
sara dep 4 list      # show what 4 is blocked by / blocking
sara dep chain 1 2 3 # wire a linear sequence in one command: 1 → 2 → 3
```

You can also edit dependencies interactively in the **Depends on** field of
`sara info` (see above). Dependencies are shown in `sara list` via the `⊘`/`⛓`
gutter markers and the `DEPS` column. Cycles are prevented automatically.

### Time tracking

```bash
sara start 1     # begin working — marks the task active (●) and starts the clock
sara stop 1      # stop — accumulates elapsed time into "time spent"
```

Set an estimate (in the `Estimate` field of `sara info`) to see a progress
percentage against time spent. If a task is tied to a git branch, `sara stop`
snapshots the files changed on that branch.

### Recurring tasks

```bash
sara add "weekly review" --every weekly
sara add "rotate secrets" --recur 2w     # --recur is an alias for --every
```

Supported intervals: `daily`, `weekly`, `monthly`, `yearly`, or `Nd` / `Nw` /
`Nm` (e.g. `3d`, `2w`, `1m`). Recurring tasks show a `♺` marker in the list.

### Checklists

Break a task into sub-steps without creating separate tasks:

```bash
sara check 1 "draft the schema"
sara check 1 "write the migration"

sara step done 1 1                  # tick step 1 (records the commit, if any)
sara step undone 1 1               # reopen it
sara step remove 1 2              # delete step 2 (alias: sara step rm); later steps shift up
```

Add `--kind acceptance` to any `sara step …` command to act on the task's
acceptance criteria instead of its steps. Toggle items with `Space` in `sara info`.

### Notes, comments & links

```bash
sara annotate 1 "spoke with design, going with option B"   # alias: sara comment
sara denotate 4                                             # remove comment #4 (alias: uncomment)

sara link 1 https://github.com/org/repo/pull/123           # auto-labels GitHub PRs/issues
sara link 1 https://example.com --label "Spec"
sara unlink 2                                               # remove link #2

sara attach 1 src/auth/login.rs                             # attach a file path (alias: sara pr)
```

Linked PRs/URLs surface as a badge in `sara list` and are openable from `sara info`.

### Git branch linkage

```bash
sara addbranch 1            # tie task 1 to the *currently checked-out* branch
sara addbranch 1 --clear    # remove the tie
```

> Note: `addbranch` takes the **task ID**, not a branch name — the branch is read
> from the repo you're standing in. The task's project must have been `sara init`'d
> inside that repo. Run `sara stop` afterwards to snapshot the changed files.

### Sharing tasks

Export a task — together with its full dependency closure (the task plus every
task it transitively depends on) — to a single copy-pasteable blob, then import
it into another user's Sara on a different machine.

```bash
sara export 1                     # print a `sara-task-v1:…` blob to stdout
sara export 1 -o task.blob        # …or write it to a file
sara 1 export                     # the usual id-first shorthand

sara import task.blob             # read a blob from a file
sara import "sara-task-v1:…"      # …or pass the blob string directly
pbpaste | sara import             # …or pipe it in on stdin
sara import task.blob -p backlog  # reassign every imported task to a project
```

What travels: the description, project, status, priority, due date, tags,
estimate, recurrence, comments, checklist/steps, links and attached file paths —
plus the dependency edges between the exported tasks. On import every task gets a
**fresh** uuid and display id (so importing into a DB that already has the task
never collides), dependency edges are remapped within the bundle, the timer is
reset and urgency is recomputed. History and time-tracking do not travel.

A bundle carries each task's project **name**, not the project *profile* (its
goal, stack, conventions and setup/test/lint commands). Importing a task whose
project doesn't exist locally is fine — it's created under that name and shows up
in `sara list`/`-p` and tab-completion straight away; only the profile metadata
is absent. Run `sara init` in that project's folder to attach a profile, or use
`-p`/`--project` on import to drop everything into an existing local project
instead of the bundle's original name.

The blob tolerates being line-wrapped by email or chat clients, so a pasted
`sara-task-v1:…` token still imports even if it picked up newlines.

### History & undo

Every mutating action is recorded and shown in the History panel of `sara info`:
field edits (description, project, priority, due, tags, estimate, recur, status),
timer start/stop, dependencies, attached files, checklist items, links, comments,
and branch ties. Additions show `+`, removals show `−`, and value changes show
`old → new`.

```bash
sara undo     # revert the most recent command
```

---

## The urgency model

Urgency is a sum of weighted components, recomputed whenever a task changes.
`sara info` displays the exact breakdown, e.g.
`28.0 (pri 6.0 + due 12.0 + blocking 8.0 + age 2.0)`.

| Component   | Default | Applies when…                                  |
|-------------|--------:|------------------------------------------------|
| `priority_h`| `6.0`   | Priority is High                               |
| `priority_m`| `3.9`   | Priority is Medium                             |
| `priority_l`| `1.8`   | Priority is Low                                |
| `due`       | `12.0`  | Scaled by closeness (overdue = full, 7+ days out = 0) |
| `blocking`  | `8.0`   | The task blocks at least one other task        |
| `blocked`   | `-5.0`  | The task is blocked (penalty)                  |
| `active`    | `4.0`   | A timer is currently running                   |
| `has_tags`  | `1.0`   | The task has any tags                          |
| `project`   | `1.0`   | The task is not in the fallback project (`inbox`) |
| `age`       | `2.0`   | Scaled by age, capped at `age_max` days        |
| `age_max`   | `365.0` | Age in days at which the age bonus maxes out   |

All coefficients are configurable under `[urgency]` in the config file.

---

## Configuration

A config file is created with sensible defaults on first run.

| OS    | Path                                            |
|-------|-------------------------------------------------|
| macOS | `~/Library/Application Support/sara/config.toml`  |
| Linux | `~/.config/sara/config.toml`                      |

Full example:

```toml
default_project = "inbox"   # last-resort fallback name when a folder has no usable name
date_dialect    = "uk"      # "uk" or "us" — affects "next friday" parsing

[urgency]                   # all optional; defaults shown
priority_h = 6.0
priority_m = 3.9
priority_l = 1.8
due        = 12.0
blocking   = 8.0
blocked    = -5.0
active     = 4.0
has_tags   = 1.0
project    = 1.0
age        = 2.0
age_max    = 365.0
```

Print the resolved config and database paths:

```bash
sara paths
```

---

## Inline Taskwarrior-style tokens

Leading and trailing tokens on `sara add` are parsed as attributes:

```bash
sara add "fix login bug" project:web-app +auth pri:H
sara add project:api "redesign rate limiting" +backend
```

Tokens in the middle of a description stay as literal text. Explicit flags are
always unambiguous and win over inline tokens:

```bash
sara add "fix the project:foo reference in docs" --project web-app
```

| Token         | Meaning            |
|---------------|--------------------|
| `project:x`   | Set the project    |
| `+tag`        | Add a tag          |
| `pri:H`       | Set priority (H/M/L) |

---

## Due dates

Natural-language dates work in the `Due` field of the review form and anywhere a
date is accepted:

| Input         | Meaning            |
|---------------|--------------------|
| `2026-07-01`  | ISO date           |
| `today`       | Today              |
| `tomorrow`    | Tomorrow           |
| `friday`      | This coming Friday |
| `next friday` | Friday next week   |
| `+3d`         | 3 days from now    |
| `+2w`         | 2 weeks from now   |

The `date_dialect` config setting (`uk` vs `us`) affects ambiguous phrasing.

---

## Shell completions

Sara ships **dynamic** completions: once registered, `<TAB>` completes real
pending task ids — annotated with their descriptions — for commands like
`sara done` / `info` / `start`, and known project names for `--project` / `-p`.

Register by having your shell evaluate `COMPLETE=<shell> sara` at startup
(no `fpath`/`compinit` setup needed). Re-run on upgrade so the generated shell
glue stays in sync with the binary:

```bash
# Bash — ~/.bashrc
echo 'source <(COMPLETE=bash sara)' >> ~/.bashrc

# Zsh — ~/.zshrc
echo 'source <(COMPLETE=zsh sara)' >> ~/.zshrc

# Fish
echo 'COMPLETE=fish sara | source' >> ~/.config/fish/completions/sara.fish

# Elvish
echo 'eval (E:COMPLETE=elvish sara | slurp)' >> ~/.elvish/rc.elv
```

Restart your shell (or `source` the file) afterwards. To disable, set
`COMPLETE=` or `COMPLETE=0`.

> Prefer a **static** completion script (command/flag structure only — no
> dynamic task-id/project values)? `sara completions <shell>` still emits one,
> e.g. `sara completions zsh > ~/.zsh/completions/_sara`.

---

## File locations

| What     | macOS                                          | Linux                        |
|----------|------------------------------------------------|------------------------------|
| Database | `~/Library/Application Support/sara/tasks.db`     | `~/.local/share/sara/tasks.db` |
| Config   | `~/Library/Application Support/sara/config.toml`  | `~/.config/sara/config.toml`   |

Run `sara paths` to see the exact locations on your machine.

---

## Command reference

| Command                            | Description                                              |
|------------------------------------|----------------------------------------------------------|
| `sara init`                        | Initialize/update the current folder as a project (`--goal`, `--stack`, `--conventions`, `--notes`, `-y`) |
| `sara add <desc> [tokens]`         | Add a task (`--yes`, `-p`, `--priority`, `-t`, `--every`, `--annotation`, `--link`, `--check`, `--depends-on`) |
| `sara list`                        | List tasks (`-a` all, `-p`/`--project <name>`)           |
| `sara modify <id>`                 | Edit via the review form, or set fields non-interactively (`--description`, `--priority`, `--due`/`--clear-due`, `--tag`/`--clear-tags`) |
| `sara info <id>`                   | Open the interactive detail view (`--md`/`--plain`/`--json`, `--history`) |
| `sara done <id>`                   | Complete a task (`--force` if blocked)                   |
| `sara delete <id>`                 | Soft-delete a task (`-y` to skip confirmation)           |
| `sara start <id>` / `sara stop <id>`| Start / stop the timer                                  |
| `sara dep <id> on\|off\|list` / `sara dep chain <id>...` | Manage dependencies, or wire a linear chain in one command |
| `sara check <id> <text>`           | Add a checklist item                                     |
| `sara step done\|undone\|remove <id> <n>` | Tick / reopen / delete step n (`--kind acceptance`)|
| `sara annotate <id> <text>`        | Add a comment (alias `comment`); `sara denotate <n>` removes |
| `sara link <id> <url>`             | Add a link; `sara unlink <n>` removes                    |
| `sara attach <id> <path>`          | Attach a file path (alias `pr`)                          |
| `sara addbranch <id>`              | Tie the current git branch to a task (`--clear`)         |
| `sara export <id>`                 | Export a task + its deps to a portable blob (`-o <file>`) |
| `sara import [src]`                | Import a task blob (file, arg, or stdin; `-p <project>`)  |
| `sara activity`                    | GitHub-style activity heatmap (`--project`, `-a`)        |
| `sara mcp`                         | Run a stdio MCP server exposing the agent loop as tools ([details](#mcp-server-sara-mcp)) |
| `sara undo`                        | Revert the most recent command                           |
| `sara reset`                       | Delete a project's tasks and profile (`-p`, `-y`)        |
| `sara paths`                       | Print config and data paths                              |
| `sara completions <shell>`         | Generate shell completions                               |

Run `sara help` or `sara <command> --help` for full options.

---

## Uninstall

```bash
cargo uninstall sara
```

Remove data and config:

```bash
# macOS
rm -rf ~/Library/Application\ Support/sara/

# Linux
rm -rf ~/.config/sara/ ~/.local/share/sara/
```
