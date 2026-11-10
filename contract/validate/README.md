# validate

Why `sara validate` refuses to stamp a task. Validation records the project's
HEAD commit on the task, so it needs a project folder that still exists and a
git repo with at least one commit.

## Obligations

### A1 — A repo without commits is refused
- **must:** when the project is a git repo with no commits, validate refuses and says the project has no commits yet, with or without `--no-run`
- **kind:** functional
- **status:** active

### A2 — The refusal reports a green gate
- **must:** when the acceptance gate passed but there is no commit to stamp, the refusal says the gate is green; with `--no-run` it doesn't mention the gate
- **kind:** functional
- **status:** active

### A3 — A moved project folder is refused
- **must:** when the project folder no longer exists, validate refuses and says to re-run `sara init` in the new location
- **kind:** functional
- **status:** active

### A4 — A folder outside git is refused
- **must:** when the project folder is not in a git repo, validate refuses and says so
- **kind:** functional
- **status:** active

### A5 — A stamp names the steps still open
- **must:** when validate stamps a task, it reports the commit; while checklist steps are still open it says how many, and once all are ticked it doesn't mention open steps
- **why:** a green gate on an unfinished plan must not read as finished work
- **kind:** functional
- **status:** active

```covers
src/commands/shared/project.rs::require_project_head
src/commands/guide/handlers/validate.rs::validate_value
```
