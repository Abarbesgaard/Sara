# Sara — a personal assistant (built on tk)

`Sara` is a personal assistant with a folder-aware task manager at its core. She knows
which Git project you're standing in, ranks your work with a transparent urgency
model, tracks time, links tasks to branches, and (optionally) uses an LLM to
enrich new tasks with a priority, due date, tags, dependencies, and relevant
files.

Task data lives in a SQLite database in your home directory — **nothing
is ever written into your repositories.** Sara's private knowledge store (notes,
links, and her learned profile) lives in a `Sara/` folder you don't need to browse.

> **Migrating from tk:** On first run, Sara imports your existing tk config and tasks automatically.

```text
     ID  PRI   PROJECT           DUE              URG  DEPS              DESCRIPTION
────────────────────────────────────────────────────────────────────────────────────
⛓    1  H     pling-backend     2026-06-15      28.0  blocks 1 task     Get an overview of the solution
     2  M     pling-backend     -                5.9                    Align the api folder
⊘    3  M     pling-backend     -                0.9  blocked by 1      Align the acceptance tests
```

---

## Table of contents

- [Highlights](#highlights)
- [Installation](#installation)
- [Quick start](#quick-start)
- [Core concepts](#core-concepts)
- [The task list](#the-task-list)
- [The detail view (`tk info`)](#the-detail-view-tk-info)
- [Working with tasks](#working-with-tasks)
  - [Adding tasks](#adding-tasks)
  - [Dependencies](#dependencies)
  - [Time tracking](#time-tracking)
  - [Recurring tasks](#recurring-tasks)
  - [Checklists](#checklists)
  - [Notes, comments & links](#notes-comments--links)
  - [Git branch linkage](#git-branch-linkage)
  - [History & undo](#history--undo)
- [The urgency model](#the-urgency-model)
- [LLM setup](#llm-setup)
- [Provider profiles](#provider-profiles)
- [Configuration](#configuration)
- [Inline Taskwarrior-style tokens](#inline-taskwarrior-style-tokens)
- [Due dates](#due-dates)
- [Shell completions](#shell-completions)
- [File locations](#file-locations)
- [Command reference](#command-reference)
- [Uninstall](#uninstall)

---

## Highlights

- **Folder-aware** — `sara` auto-detects the current Git project and scopes `sara list` to it by default.
- **Transparent urgency** — a Taskwarrior-style scoring model decides ordering; `sara info` shows the exact breakdown.
- **Interactive TUI** — a ratatui review form for adding/editing, and a rich detail view for everything else.
- **Dependencies** — block tasks on each other, with cycle detection and an at-a-glance `DEPS` column.
- **Time tracking** — `sara start` / `sara stop` accumulate active time, with optional estimates.
- **Git integration** — tie a task to a branch and snapshot the files it touched.
- **Full history** — every change (field edits, deps, files, checklist, links, comments, timer) is recorded.
- **Optional LLM** — enrich new tasks locally with Ollama, or via OpenAI / Anthropic / Azure / MLX.
- **Single SQLite file** — easy to back up; Sara's markdown store is separate.

---

## Installation

### 1 — Prerequisites

**Rust** (if not already installed):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Restart your shell, or:
source "$HOME/.cargo/env"
```

**Ollama** (optional — for local LLM enrichment):

```bash
# macOS
brew install ollama
ollama serve &        # start the server
ollama pull qwen2.5   # default model (good structured-output quality)
```

`sara` works fine without any LLM — enrichment is opt-in (`--ai`), so you only pay
the latency when you ask for it.

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
cd ~/my-project
sara init

# Add a task (opens the interactive review form)
sara add "implement user authentication"

# Add quickly, no form, with an inline priority token
sara add "fix login bug" pri:H --yes

# Capture a note or link into Sara's store
sara add --note "meeting notes from standup"
sara add --link https://example.com

# See what to work on (current project, ranked by urgency)
sara list --items   # include notes & links

# Inspect / edit a task interactively
sara info 1
sara info n1        # view note #1

# Start the clock, do the work, stop it
sara start 1
sara stop 1

# Complete it
sara done 1
```

---

## Core concepts

**Projects.** Every task belongs to a project. Inside a Git repo, `tk` uses the
repo as the project (run `tk init` once to record its goal/stack). Outside a
repo, tasks land in `inbox` (configurable via `default_project`).

**IDs vs UUIDs.** Each task has a small, recycled display **ID** (the `1`, `2`,
`3` you type) and a stable **UUID** that never changes. Most commands accept
either the ID or a UUID prefix. When a task is completed, pending IDs are
repacked to stay small — so today's `4` may be tomorrow's `3`.

**Urgency.** Tasks are ordered by a computed urgency score (see
[The urgency model](#the-urgency-model)). It rewards priority, due dates,
active timers, tags, and tasks that block others — and penalizes blocked tasks.

---

## The task list

`tk list` prints the pending tasks for the current project, highest urgency
first.

```bash
tk list                      # current project
tk list -a                   # all projects
tk list --project backend    # a specific project from anywhere
```

Each row has a small marker gutter, columns, and a dependency column:

```text
⛓    1  H     pling-backend     2026-06-15      28.0  blocks 1 task     Get an overview
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

## The detail view (`tk info`)

`tk info <id>` opens a full-screen, interactive view of a single task: all
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
| `PgUp` / `PgDn`| Scroll                                            |
| `Esc`          | Cancel an edit                                    |
| `q` / `Esc`    | Close the view                                    |

**Editable fields:** Description, Project, Priority, Due, Tags, Estimate,
Recur, and **Depends on**.

To change dependencies, select **Depends on**, press `Enter`, and type the task
IDs it should wait on (space- or comma-separated), e.g. `7 9`. `tk` reconciles
the set — adding and removing edges — and rejects self-references and cycles
with an inline error. The change is reflected immediately in the "Blocked by"
section and the History panel.

---

## Working with tasks

### Adding tasks

```bash
tk add "write integration tests"            # opens the review form
tk add "write tests" --yes                  # skip the form, save immediately
tk add "write tests" --ai                   # enrich with the LLM first
tk add "write tests" -p backend --priority H -t testing
```

By default `tk add` opens an interactive review form so you can confirm the
fields before saving. `--yes` saves immediately; `--ai` (alias `--llm`) asks the
configured LLM to propose a priority, due date, tags, dependencies, and relevant
files first. See [inline tokens](#inline-taskwarrior-style-tokens) for the
`project:` / `+tag` / `pri:` shorthand.

```bash
tk modify 2        # edit via the review form
tk done 1          # mark complete (use --force to complete a blocked task)
tk delete 3        # soft-delete (prompts; -y to skip)
```

### Dependencies

A dependency means "this task is blocked until that task is done." Blocked tasks
sink in urgency; blocking tasks rise.

```bash
tk dep 4 on 5      # task 4 now depends on (is blocked by) task 5
tk dep 4 off 5     # remove that dependency
tk dep 4 list      # show what 4 is blocked by / blocking
```

You can also edit dependencies interactively in the **Depends on** field of
`tk info` (see above). Dependencies are shown in `tk list` via the `⊘`/`⛓`
gutter markers and the `DEPS` column. Cycles are prevented automatically.

### Time tracking

```bash
tk start 1     # begin working — marks the task active (●) and starts the clock
tk stop 1      # stop — accumulates elapsed time into "time spent"
```

Set an estimate (in the `Estimate` field of `tk info`) to see a progress
percentage against time spent. If a task is tied to a git branch, `tk stop`
snapshots the files changed on that branch.

### Recurring tasks

```bash
tk add "weekly review" --every weekly
tk add "rotate secrets" --recur 2w     # --recur is an alias for --every
```

Supported intervals: `daily`, `weekly`, `monthly`, `yearly`, or `Nd` / `Nw` /
`Nm` (e.g. `3d`, `2w`, `1m`). Recurring tasks show a `♺` marker in the list.

### Checklists

Break a task into sub-steps without creating separate tasks:

```bash
tk check 1 "draft the schema"
tk check 1 "write the migration"
```

Toggle items with `Space` in `tk info`.

### Notes, comments & links

```bash
tk annotate 1 "spoke with design, going with option B"   # alias: tk comment
tk denotate 4                                             # remove comment #4 (alias: uncomment)

tk link 1 https://github.com/org/repo/pull/123           # auto-labels GitHub PRs/issues
tk link 1 https://example.com --label "Spec"
tk unlink 2                                               # remove link #2

tk attach 1 src/auth/login.rs                            # attach a file path (alias: tk pr)
```

Linked PRs/URLs surface as a badge in `tk list` and are openable from `tk info`.

### Git branch linkage

```bash
tk addbranch 1            # tie task 1 to the *currently checked-out* branch
tk addbranch 1 --clear    # remove the tie
```

> Note: `addbranch` takes the **task ID**, not a branch name — the branch is read
> from the repo you're standing in. The task's project must have been `tk init`'d
> inside that repo. Run `tk stop` afterwards to snapshot the changed files.

### History & undo

Every mutating action is recorded and shown in the History panel of `tk info`:
field edits (description, project, priority, due, tags, estimate, recur, status),
timer start/stop, dependencies, attached files, checklist items, links, comments,
and branch ties. Additions show `+`, removals show `−`, and value changes show
`old → new`.

```bash
tk undo     # revert the most recent command
```

---

## The urgency model

Urgency is a sum of weighted components, recomputed whenever a task changes.
`tk info` displays the exact breakdown, e.g.
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
| `project`   | `1.0`   | The task is not in `inbox`                     |
| `age`       | `2.0`   | Scaled by age, capped at `age_max` days        |
| `age_max`   | `365.0` | Age in days at which the age bonus maxes out   |

All coefficients are configurable under `[urgency]` in the config file.

---

## LLM setup

Enrichment is opt-in per command (`tk add --ai`). The default provider is local
Ollama; no API key required.

### Ollama (default — local & private)

```toml
# config.toml
[llm]
provider = "ollama"
model    = "qwen2.5"        # or llama3.1, mistral-nemo, etc.
# base_url = "http://localhost:11434"
```

### OpenAI

```toml
[llm]
provider = "openai"
model    = "gpt-4o"
api_key  = "sk-..."
```

### Anthropic

```toml
[llm]
provider = "anthropic"
model    = "claude-opus-4-5"
api_key  = "sk-ant-..."
```

Azure and MLX are also supported (see `tk provider add --type`).

---

## Provider profiles

Switch LLM backends on the fly without editing the config by hand:

```bash
tk provider list                  # show profiles and the active one
tk provider add gpt4o --type openai --model gpt-4o --key sk-...
tk provider add local --type ollama --model qwen2.5 --url http://localhost:11434
tk provider use gpt4o             # activate a profile
tk provider use default           # revert to the [llm] block
tk provider remove gpt4o
```

The active profile overrides the `[llm]` block for all enrichment.

---

## Configuration

A config file is created with sensible defaults on first run.

| OS    | Path                                            |
|-------|-------------------------------------------------|
| macOS | `~/Library/Application Support/sara/config.toml`  |
| Linux | `~/.config/sara/config.toml`                      |

Full example:

```toml
default_project = "inbox"   # project used when not inside a Git repo
date_dialect    = "uk"      # "uk" or "us" — affects "next friday" parsing

[llm]
provider     = "ollama"
model        = "qwen2.5"
timeout_secs = 60
# base_url = "http://localhost:11434"
# api_key  = ""

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
tk paths
```

---

## Inline Taskwarrior-style tokens

Leading and trailing tokens on `tk add` are parsed as attributes:

```bash
tk add "fix login bug" project:backend +auth pri:H
tk add project:api "redesign rate limiting" +backend
```

Tokens in the middle of a description stay as literal text. Explicit flags are
always unambiguous and win over inline tokens:

```bash
tk add "fix the project:foo reference in docs" --project backend
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
| `2026-06-20`  | ISO date           |
| `today`       | Today              |
| `tomorrow`    | Tomorrow           |
| `friday`      | This coming Friday |
| `next friday` | Friday next week   |
| `+3d`         | 3 days from now    |
| `+2w`         | 2 weeks from now   |

The `date_dialect` config setting (`uk` vs `us`) affects ambiguous phrasing.

---

## Shell completions

```bash
# Zsh
tk completions zsh > ~/.zsh/completions/_tk
# ensure in ~/.zshrc:  fpath=(~/.zsh/completions $fpath) && autoload -U compinit && compinit

# Bash
tk completions bash >> ~/.bashrc

# Fish
tk completions fish > ~/.config/fish/completions/tk.fish
```

---

## File locations

| What     | macOS                                          | Linux                        |
|----------|------------------------------------------------|------------------------------|
| Database | `~/Library/Application Support/sara/tasks.db`     | `~/.local/share/sara/tasks.db` |
| Config   | `~/Library/Application Support/sara/config.toml`  | `~/.config/sara/config.toml`   |

Run `tk paths` to see the exact locations on your machine.

---

## Command reference

| Command                         | Description                                              |
|---------------------------------|----------------------------------------------------------|
| `tk init`                       | Create/update the current project's profile              |
| `tk add <desc> [tokens]`        | Add a task (`--yes`, `--ai`, `-p`, `--priority`, `-t`, `--every`) |
| `tk list`                       | List tasks (`-a` all, `--project <name>`)                |
| `tk info <id>`                  | Open the interactive detail view                         |
| `tk modify <id>`                | Edit via the review form (`--no-llm`)                    |
| `tk done <id>`                  | Complete a task (`--force` if blocked)                   |
| `tk delete <id>`                | Soft-delete a task (`-y` to skip confirmation)           |
| `tk start <id>` / `tk stop <id>`| Start / stop the timer                                   |
| `tk dep <id> on\|off\|list`      | Manage dependencies                                      |
| `tk check <id> <text>`          | Add a checklist item                                     |
| `tk annotate <id> <text>`       | Add a comment (alias `comment`); `tk denotate <n>` removes |
| `tk link <id> <url>`            | Add a link; `tk unlink <n>` removes                      |
| `tk attach <id> <path>`         | Attach a file path (alias `pr`)                          |
| `tk addbranch <id>`             | Tie the current git branch to a task (`--clear`)         |
| `tk activity`                   | GitHub-style activity heatmap (`--project`, `-a`)        |
| `tk provider …`                 | Manage LLM provider profiles                             |
| `tk undo`                       | Revert the most recent command                           |
| `tk reset`                      | Delete a project's tasks and profile (`-p`, `-y`)        |
| `tk paths`                      | Print config and data paths                              |
| `tk completions <shell>`        | Generate shell completions                               |

Run `tk help` or `tk <command> --help` for full options.

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
