# undo

How `sara undo` takes back a mistake. Each changing command is recorded, so
the most recent one can be reverted.

## Obligations

### A1 — Undo reverts only the last command
- **must:** `undo` reverts only the most recent command; changes made before it stay
- **kind:** functional
- **status:** active

```covers
src/commands/undo/mod.rs::run
```
