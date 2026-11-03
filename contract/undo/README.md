# undo

How `sara undo` takes back a mistake. Each changing command is recorded, so
the most recent one can be reverted.

## Obligations

- **A1** `undo` reverts only the most recent command; changes made before it stay.

```covers
src/commands/undo/mod.rs::run
```

```surface
cli add
cli list
cli undo
```
