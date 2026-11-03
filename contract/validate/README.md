# validate

Why `sara validate` refuses to stamp a task. Validation records the project's
HEAD commit on the task, so it needs a project folder that still exists and a
git repo with at least one commit.

## Obligations

- **A1** When the project is a git repo with no commits, validate refuses and says the project has no commits yet, with or without `--no-run`.
- **A2** When the acceptance gate passed but there is no commit to stamp, the refusal says the gate is green; with `--no-run` it doesn't mention the gate.
- **A3** When the project folder no longer exists, validate refuses and says to re-run `sara init` in the new location.
- **A4** When the project folder is not in a git repo, validate refuses and says so.

```covers
src/commands/shared/project.rs::require_project_head
src/commands/guide/handlers/validate.rs::validate_value
```

```surface
cli begin
cli validate
```
