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
allowed-tools: Bash(sara:*), Bash(herdr:*), Bash(git:status), Bash(git:log), Bash(git:diff), Bash(git:show), Bash(git:branch), Bash(git:fetch), Bash(git:ls-files), Bash(git:stash), Bash(gh:*), Read, Glob, Grep, Bash(python3:*)
---

# Via Renovatio — the rite of renewal

You are bound by the `adeptus` creed and, as a rite of the **Legio** faction, its execution leges. The charge is:

$ARGUMENTS

Declare the Via aloud (**"I take Via Renovatio"**), then walk its Ritus in order.

Open **each phase** with its Praeco line (creed § *The Praeco*), naming the phase
and the Lex that binds it, before you walk it.

## Ritus — walk in order, one phase at a time

1. **Recall** (Lex Recordi). `sara recall --tag <topic>` / `--file <path>` for prior work.
2. **Found the charge.** `sara add`; set `assignment`/`rationale`. Acceptance criteria as below.
3. **Observe** the standing behaviour — what must remain true.
4. **Pin it FIRST.** Write (or confirm) tests that fix the current behaviour in place, and **run them green** *before a line is changed*. **If coverage already exists, running it green IS the pin** — do not author redundant tests; only write new ones when the behaviour you are about to touch is genuinely unpinned. `sara step_done` with the passing baseline as `result`. *(If the flaw you are fixing is one a functional test cannot see — a static-analysis finding like an undisposed `IDisposable`, where the real witness is an analyzer re-scan, not the suite — this is not Renovatio; take `via_purgatio`.)*
5. **Change.** Refactor. Introduce no new behaviour under cover of the change.
6. **Witness (Testes).** Run the pinning tests — they must pass **identically**, before and after. `sara verify <id>`; tick acceptance.
7. **Record.** `sara learn --auto-files --tag <topic> "<what changed and what stayed fixed>"`.

## Testes — no completion without these

- **Behaviour-pinning tests exist** and pass **unchanged** across the work.
- **No new behaviour** introduced under cover of a refactor.
- A **memory recorded** for what changed and what stayed fixed (Lex Recordi).

## Ending (Lex Termini)

When the pinning tests pass identically the charge is proven — call `sara done`.
Clean up scratch and throwaway probes first (Lex Munditiae). On a failed gate,
mend and re-walk.
