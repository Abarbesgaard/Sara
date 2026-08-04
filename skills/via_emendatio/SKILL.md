---
name: via_emendatio
description: >-
  Invoke when a defect can be reproduced by a test — repair a bug under the
  Adeptus rite of Via Emendatio. Reproduce → locate → prove with a FAILING test
  → mend → witness the test passes and none regress → record the cause.
  Completion requires a test that failed before and passes after, with the full
  suite green. A broken build/config is via_restitutio; a static-analysis alert
  is via_purgatio.
argument-hint: <bug description or sara task id>
allowed-tools: Bash(sara:*), Bash(git:*), Bash(gh:*), Read, Edit, Write, Glob, Grep, Bash(python3:*), Bash(pytest:*), Bash(cargo:*), Bash(dotnet:*), Bash(npm:*), Bash(pnpm:*), Bash(make:*)
---

# Via Emendatio — the rite of repair

You are bound by the `adeptus` creed and, as a rite of the **Legio** faction, its execution leges. The charge is:

$ARGUMENTS

Declare the Via aloud (**"I take Via Emendatio"**), then walk its Ritus in order.
Do not jump to a patch — the failing test comes before the mend.

Open **each phase** with its Praeco line (creed § *The Praeco*), naming the phase
and the Lex that binds it, before you walk it.

## Ritus — walk in order, one phase at a time

1. **Recall** (Lex Recordi). `sara recall --tag <topic>` and `sara recall --file <path>` for prior work on this fault before deriving anything.
2. **Found the charge** (unless one exists). `sara add` with `--annotation` for the report; set `sara assignment` (the report verbatim) and `sara rationale`. Write the Testes below as acceptance criteria: `sara check <id> "<criterion>" --kind acceptance --verify "<test cmd>"`.
3. **Reproduce.** Trigger the fault and observe the failure with your own eyes. `sara annotate <id> --kind finding "root cause: …"` once located.
4. **Locate.** Find the defect in the source. Record it.
5. **Prove.** Write a test that exercises the fault and **run it — confirm it FAILS (red).** A fix you cannot first make fail is not proven. `sara step_done` this phase with the failing output as its `result`.
6. **Mend.** Fix the defect — the smallest change that turns the test green.
7. **Witness (Testes).** Run the new test (now passes) **and the full suite** (no regressions). `sara verify <id>`; tick acceptance via `sara step_done <id> <N> --kind acceptance --result "<evidence>"`.
8. **Record.** `sara learn --auto-files --tag <topic> "<the cause and the fix>"` so the next Adept starts from knowledge.

## Testes — no completion without these

- A test that **failed before the mend and passes after**.
- The **full suite green** (no regression).
- The cause recorded as memory.

## Ending (Lex Termini)

Do **not** open a PR and do **not** `sara done` here. When the new test passes
and the full suite is green the charge is mended but **not yet released** —
opening the PR is the sole office of `via_publicatio`, invoked explicitly. Leave
the work committed — scratch and throwaway probes removed first (Lex Munditiae) —
and the charge green and ready; stop there. On a failed gate,
mend and re-walk (Lex Emendationis) — do not abandon the charge or raise a new
question.
