# done

How `sara done` closes a task. Closing is the end of the task loop, so it
records that the work is finished and takes the task off the list.

## Obligations

### A1 — Done closes exactly the named task
- **must:** `done <id>` marks that task completed, with an end time, and takes it off `list`; other tasks stay pending. A task with no acceptance criteria still closes, with a warning that nothing proved it done.
- **kind:** functional
- **status:** active

### A2 — A blocked task stays open
- **must:** `done <id>` refuses a task that a pending task blocks, naming the blocker and `--force`, and leaves it pending; `done <id> --force` closes it anyway
- **why:** closing work before what it depends on hides an unfinished dependency
- **kind:** functional
- **status:** active

### A3 — Unproven work stays open
- **must:** `done <id>` refuses a task with acceptance criteria that were never validated, or were validated at a commit other than the project's HEAD, and says why and to run `validate` or `done --force`; a task validated at HEAD closes without a warning, and `--force` closes an unproven one with a warning
- **why:** a task is done when its acceptance criteria pass on the code that ships, not on an older commit
- **kind:** functional
- **status:** active

```covers
src/commands/done/mod.rs::done_value
src/commands/done/mod.rs::done_gate
src/commands/done/mod.rs::run
```

```surface
cli add
cli begin
cli dep
cli done
cli list
cli validate
```
