# add

How `sara add` records a task. The words after `add` become the task's
description.

## Obligations

### A1 — An added task is pending
- **must:** save the words as a new pending task that `list` shows
- **kind:** functional
- **status:** active

```covers
src/commands/add/mod.rs::run
```

```surface
cli add
cli list
```

### A2 — An empty description is refused
- **must:** refuse to add a task whose description is empty or only whitespace, exit non-zero, and create no task
- **kind:** functional
- **status:** active

```covers
src/commands/add/input.rs::resolve
```

```surface
cli add
```
