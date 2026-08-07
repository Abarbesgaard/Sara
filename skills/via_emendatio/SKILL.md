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
allowed-tools: Bash(sara:*), Bash(herdr:*), Bash(git:*), Bash(gh:*), Read, Edit, Write, Glob, Grep, Bash(python3:*), Bash(pytest:*), Bash(cargo:*), Bash(dotnet:*), Bash(npm:*), Bash(pnpm:*), Bash(make:*)
---

# Via Emendatio — the rite of repair

You are bound by the `adeptus` creed and, as a rite of the **Legio** faction, its execution leges. The charge is:

$ARGUMENTS

Declare the Via aloud (**"I take Via Emendatio"**), then walk its Ritus in order.

> **STOP — this rite is delegated. You do NOT edit any file yourself.** You are the **Praefectus**: you drive sara —
> recall, the record, the witness — and you do **not** write the code yourself.
> Raise a **Miles** through herdr in its own worktree to write the failing test and
> the **mend** (`herdr worktree create` → `herdr agent start … --kind copilot` →
> `herdr agent prompt … --wait` → watch with `herdr agent wait`/`read`). Brief it with the repro + province path/branch; record every
> phase against the charge UUID. The Miles never touches your cwd, and it leaves its changes **uncommitted** — `via_publicatio` alone commits.
Do not jump to a patch — the failing test comes before the mend.

Open **each phase** with its Praeco line (creed § *The Praeco*), naming the phase
and the Lex that binds it, before you walk it.

## Ritus — walk in order, one phase at a time

1. **Delegatio (Lex Delegationis) — walk this FIRST, before anything else.** You
   are the **Praefectus**; you will **not** edit a single source file this charge.
   Raise your **Miles** now, and confirm it is live before you walk any further:
   - `herdr worktree create --branch <charge-branch> --base <base-ref> --label "<charge>" --focus` — from the JSON, read the new `workspace_id` and its pane id (e.g. `w25:p1`).
   - `herdr agent start copilot --kind copilot --pane <pane-id>` — wait for `ready`.
   - Hold that pane id. **Every** file-changing phase below is briefed to THIS Miles: `herdr agent prompt <pane-id> "<the phase + its acceptance criteria + the worktree path/branch>" --wait --until idle,done,blocked`, then `herdr agent read <pane-id>` for its evidence.
   - **Gate:** if you ever reach a phase that writes to a file and no Miles is running, you have walked the rite wrong — **STOP** and raise one before proceeding. The Praefectus's own hands touch only `sara`, `herdr`, and read-only `view`/`grep`.

2. **Recall** (Lex Recordi). `sara recall --tag <topic>` and `sara recall --file <path>` for prior work on this fault before deriving anything.
3. **Found the charge** (unless one exists). `sara add` with `--annotation` for the report; set `sara assignment` (the report verbatim) and `sara rationale`. Write the Testes below as acceptance criteria: `sara check <id> "<criterion>" --kind acceptance --verify "<test cmd>"`.
4. **Reproduce.** Trigger the fault and observe the failure with your own eyes. `sara annotate <id> --kind finding "root cause: …"` once located.
5. **Locate.** Find the defect in the source. Record it.
6. **Prove.** Write a test that exercises the fault and **run it — confirm it FAILS (red).** A fix you cannot first make fail is not proven. `sara step_done` this phase with the failing output as its `result`.
7. **Mend.** Fix the defect — the smallest change that turns the test green. **This edit is the Miles's hand, not yours (Lex Delegationis)** — brief it, then witness and record its result.
8. **Witness (Testes).** Run the new test (now passes) **and the full suite** (no regressions). `sara verify <id>`; tick acceptance via `sara step_done <id> <N> --kind acceptance --result "<evidence>"`.
9. **Record.** `sara learn --auto-files --tag <topic> "<the cause and the fix>"` so the next Adept starts from knowledge.

## Testes — no completion without these

- A test that **failed before the mend and passes after**.
- The **full suite green** (no regression).
- The cause recorded as memory.

## Ending (Lex Termini)

Do **not** open a PR and do **not** `sara done` here. When the new test passes
and the full suite is green the charge is mended but **not yet released** —
opening the PR is the sole office of `via_publicatio`, invoked explicitly. Leave
the work uncommitted — committing and pushing are `via_publicatio`'s office alone; scratch and throwaway probes removed first (Lex Munditiae) —
and the charge green and ready; stop there. On a failed gate,
mend and re-walk (Lex Emendationis) — do not abandon the charge or raise a new
question.
