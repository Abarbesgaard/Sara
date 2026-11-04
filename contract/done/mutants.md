# Mutants done leaves alone

## The notes after "Done:" are not promised yet

After the `Done:` line, `run` prints notes for what closing set off elsewhere:
the task's next recurrence and memory hygiene (memories auto-archived, or
waiting for review). The Contract promises the close itself (A1–A3), not these
notes. An obligation for recurrence or memory hygiene will catch these mutants,
and this declaration then has to go.

```unpromised
src/commands/done/mod.rs: delete ! in run
src/commands/done/mod.rs: replace > with == in run
src/commands/done/mod.rs: replace > with < in run
src/commands/done/mod.rs: replace > with >= in run
```
