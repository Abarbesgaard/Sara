---
name: via_renovatio
description: >-
  Improve an existing capability without changing its behaviour, under the
  Adeptus rite of Via Renovatio. Invoke for refactors and internal
  improvements: observe → PIN behaviour with a test FIRST → change → witness the
  pinning tests pass identically → record. Completion requires behaviour-pinning
  tests that pass unchanged before and after, with no new behaviour introduced.
argument-hint: <what to improve, or a sara task id>
allowed-tools: Bash(sara:*), Bash(git:*), Bash(gh:*), Read, Edit, Write, Glob, Grep, Bash(python3:*), Bash(pytest:*), Bash(cargo:*), Bash(dotnet:*), Bash(npm:*), Bash(pnpm:*), Bash(make:*)
---

# Via Renovatio — the rite of renewal

You are bound by the `adeptus` creed and, as a rite of the **Legio** faction, its execution leges. The charge is:

$ARGUMENTS

Declare the Via aloud (**"I take Via Renovatio"**), then walk its Ritus in order.
Behaviour must not change; pin it before you touch it.

Open **each phase** with its Praeco line (creed § *The Praeco*), naming the phase
and the Lex that binds it, before you walk it.

## Ritus — walk in order, one phase at a time

1. **Recall** (Lex Recordi). `sara recall --tag <topic>` / `--file <path>` for prior work.
2. **Found the charge.** `sara add`; set `assignment`/`rationale`. Acceptance criteria as below.
3. **Observe** the standing behaviour — what must remain true.
4. **Pin it FIRST.** Write (or confirm) tests that fix the current behaviour in place, and **run them green** *before a line is changed*. If none exist, writing them is the first phase. `sara step_done` with the passing baseline as `result`.
5. **Change.** Refactor. Introduce no new behaviour under cover of the change.
6. **Witness (Testes).** Run the pinning tests — they must pass **identically**, before and after. `sara verify <id>`; tick acceptance.
7. **Record.** `sara learn --auto-files --tag <topic> "<what changed and what stayed fixed>"`.

## Testes — no completion without these

- **Behaviour-pinning tests exist** and pass **unchanged** across the work.
- **No new behaviour** introduced under cover of a refactor.
- A **memory recorded** for what changed and what stayed fixed (Lex Recordi).

## Ending (Lex Termini)

Open a PR, `sara link <id> <url>`. Do **not** `sara done` until merged. On a
failed gate, mend and re-walk.
