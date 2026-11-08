# add

How `sara add` records a task. The words after `add` become the task's
description.

## Obligations

### A1 — An added task is pending
- **must:** save the words as a new pending task that `list` shows
- **kind:** functional
- **status:** active

### A2 — An empty description is refused
- **must:** refuse to add a task whose description is empty or only whitespace, exit non-zero, and create no task
- **kind:** functional
- **status:** active

### A3 — Leading and trailing modifiers set the task's fields
- **must:** read `project:<name>`, `+<tag>` and `pri:<level>` words at either end of the description as the task's project, tags and priority, and leave them out of the description
- **kind:** functional
- **status:** active

### A4 — A matching open task is named, not hidden
- **must:** when the description matches a pending task's in the same project, ignoring case and surrounding whitespace, warn and name that task, and still add the new one
- **why:** a duplicate is often a sign the work already exists; the warning lets the caller reuse it without `add` second-guessing them
- **kind:** functional
- **status:** active

### A5 — Flags win over inline modifiers
- **must:** take `--project` and `--priority` over `project:` and `pri:` in the words, and add `--tag` values to the `+tag` words
- **why:** a script or agent passes flags on purpose; words typed into a description must not overrule them
- **kind:** functional
- **status:** active

### A6 — An unknown priority is refused
- **must:** refuse a `pri:` word or `--priority` value that is not a known priority (H, M, L, high, med, medium or low, in any case); name the bad value, exit 1, and create no task
- **why:** a typo in a priority must not quietly become a task with no priority
- **kind:** functional
- **status:** active

```covers
src/commands/add/mod.rs::run
src/commands/add/input.rs::resolve
src/infrastructure/project.rs::parse_add_tokens
src/commands/add/duplicate.rs::find_duplicate_open_task
src/domain/value_objects/priority.rs::from_str
```
