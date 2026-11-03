# ids

How a task id typed by a person or an agent is read. Display ids are renumbered
when tasks complete; uuid prefixes never change.

## Obligations

### A1 — A short all-digit id is a display id
- **must:** an all-digit id shorter than 8 characters is a display id; when no pending task has it, the refusal says display ids are renumbered
- **kind:** functional
- **status:** active

### A2 — A long id is a uuid prefix
- **must:** an id of 8 or more characters that is no current display id is read as a uuid prefix, even when it is all digits
- **kind:** functional
- **status:** active

### A3 — Display ids stay on their branch
- **must:** acting by display id on a task tied to a branch other than the checked-out one is refused; acting by uuid prefix is not
- **kind:** functional
- **status:** active

### A4 — A branch refusal suggests a working id
- **must:** a branch refusal suggests an id that is read as a uuid prefix, so re-running with it succeeds
- **kind:** functional
- **status:** active

### A5 — --force ignores the branch
- **must:** `--force` acts on the task whatever branch is checked out
- **kind:** functional
- **status:** active

### A6 — A blank id matches nothing
- **must:** a blank task id or memory handle matches nothing, even when only one task or memory exists
- **kind:** functional
- **status:** active

```covers
src/infrastructure/db/tasks.rs::resolve_task_matched
src/commands/shared/project.rs::guard_branch_mutation
src/commands/shared/project.rs::stable_id
src/infrastructure/db/tasks.rs::get_task_by_uuid_prefix
src/infrastructure/db/memory/items.rs::get_item_by_uuid_prefix
```

```surface
cli done
cli forget
cli info
cli validate
```
