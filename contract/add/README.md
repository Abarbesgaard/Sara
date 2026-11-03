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
