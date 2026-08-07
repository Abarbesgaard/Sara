---
name: via_renovatio
description: >-
  Invoke to improve existing code WITHOUT changing its behaviour — refactors and
  internal improvements — under the Adeptus rite of Via Renovatio. Observe → PIN
  behaviour with a test FIRST → change → witness the pinning tests pass
  identically → record. Completion requires behaviour-pinning tests that pass
  unchanged before and after, with no new behaviour. New behaviour is
  via_genesis; a bug fix is via_emendatio.
argument-hint: <what to improve, or a sara task id>
allowed-tools: Bash(sara:*), Bash(herdr:*), Bash(git:status), Bash(git:log), Bash(git:diff), Bash(git:show), Bash(git:branch), Bash(git:fetch), Bash(git:ls-files), Bash(git:stash), Bash(gh:*), Read, Edit, Write, Glob, Grep, Bash(python3:*), Bash(pytest:*), Bash(cargo:*), Bash(dotnet:*), Bash(npm:*), Bash(pnpm:*), Bash(make:*)
---

# Via Renovatio — the rite of renewal

You are bound by the `adeptus` creed and, as a rite of the **Legio** faction, its execution leges. The charge is:

$ARGUMENTS

Declare the Via aloud (**"I take Via Renovatio"**), then walk its Ritus in order.

> **STOP — this rite is delegated. You do NOT edit any file yourself.** You are the **Praefectus**: you drive sara —
> recall, the record, the witness — and you do **not** write the code yourself.
> Raise a **Miles** through herdr in its own worktree to write the pinning test and
> the **change** (`herdr worktree create` → `herdr agent start … --kind copilot` →
> `herdr agent prompt … --wait` → watch with `herdr agent wait`/`read`). Brief it with the behaviour to preserve + province
> path/branch; record every phase against the charge UUID. The Miles never touches your cwd, and it leaves its changes **uncommitted** — `via_publicatio` alone commits.
Behaviour must not change; pin it before you touch it.

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

2. **Recall** (Lex Recordi). `sara recall --tag <topic>` / `--file <path>` for prior work.
3. **Found the charge.** `sara add`; set `assignment`/`rationale`. Acceptance criteria as below.
4. **Observe** the standing behaviour — what must remain true.
5. **Pin it FIRST.** Write (or confirm) tests that fix the current behaviour in place, and **run them green** *before a line is changed*. **If coverage already exists, running it green IS the pin** — do not author redundant tests; only write new ones when the behaviour you are about to touch is genuinely unpinned. `sara step_done` with the passing baseline as `result`. *(If the flaw you are fixing is one a functional test cannot see — a static-analysis finding like an undisposed `IDisposable`, where the real witness is an analyzer re-scan, not the suite — this is not Renovatio; take `via_purgatio`.)*
6. **Change.** Refactor. Introduce no new behaviour under cover of the change. **This edit is the Miles's hand, not yours (Lex Delegationis)** — brief it, then witness and record its result.
7. **Witness (Testes).** Run the pinning tests — they must pass **identically**, before and after. `sara verify <id>`; tick acceptance.
8. **Record.** `sara learn --auto-files --tag <topic> "<what changed and what stayed fixed>"`.

## Testes — no completion without these

- **Behaviour-pinning tests exist** and pass **unchanged** across the work.
- **No new behaviour** introduced under cover of a refactor.
- A **memory recorded** for what changed and what stayed fixed (Lex Recordi).

## Ending (Lex Termini)

Do **not** open a PR and do **not** `sara done` here. When the pinning tests pass
identically the charge is proven but **not yet released** — opening the PR is the
sole office of `via_publicatio`, invoked explicitly. Leave the work uncommitted — committing and pushing are `via_publicatio`'s office alone;
scratch and throwaway probes removed first (Lex Munditiae) — and the charge green
and ready; stop there. On a failed gate, mend and re-walk.
