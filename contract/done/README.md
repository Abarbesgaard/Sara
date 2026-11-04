# done

How `sara done` closes a task. Closing is the end of the task loop, so it
records that the work is finished and takes the task off the list.

## Obligations

### A1 — Done closes exactly the named task
- **must:** `done <id>` marks that task completed, with an end time, and takes it off `list`; other tasks stay pending. A task with no acceptance criteria still closes, with a warning that nothing proved it done.
- **kind:** functional
- **status:** active

```covers
src/commands/done/mod.rs::done_value
src/commands/done/mod.rs::done_gate
src/commands/done/mod.rs::run
```

```surface
cli add
cli done
cli list
```
