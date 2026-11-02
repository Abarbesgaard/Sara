# ids

How a task id typed by a person or an agent is read. Display ids are renumbered
when tasks complete; uuid prefixes never change.

## Obligations

- **A1** An all-digit id shorter than 8 characters is a display id; when no pending task has it, the refusal says display ids are renumbered.
- **A2** An id of 8 or more characters that is no current display id is read as a uuid prefix, even when it is all digits.
- **A3** Acting by display id on a task tied to a branch other than the checked-out one is refused; acting by uuid prefix is not.
- **A4** A branch refusal suggests an id that is read as a uuid prefix, so re-running with it succeeds.
- **A5** `--force` acts on the task whatever branch is checked out.

```covers
src/infrastructure/db/tasks.rs::resolve_task_matched
src/commands/shared/project.rs::guard_branch_mutation
src/commands/shared/project.rs::stable_id
```

```surface
cli done
cli info
cli validate
```
