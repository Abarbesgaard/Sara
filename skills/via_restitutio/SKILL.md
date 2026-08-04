---
name: via_restitutio
description: >-
  Invoke when the red signal is a COMMAND — a failing build, restore, install,
  or CI job — and the mend is mechanical (version, manifest, lockfile, config),
  not logic: restore a broken build/deps/CI to a consistent state under the
  Adeptus rite of Via Restitutio. Reproduce the red command → locate the
  offending manifest/version/config → align (smallest mend) → witness the same
  command green and the suite still green → record the cause. Completion
  requires the red command green and the full suite still green, with NO new
  test authored — the command itself is the Testis.
argument-hint: <the broken build/deps/config, the failing command, or a sara task id>
allowed-tools: Bash(sara:*), Bash(git:*), Bash(gh:*), Read, Edit, Write, Glob, Grep, Bash(python3:*), Bash(pytest:*), Bash(cargo:*), Bash(dotnet:*), Bash(npm:*), Bash(pnpm:*), Bash(make:*)
---

# Via Restitutio — the rite of restoration

You are bound by the `adeptus` creed and, as a rite of the **Legio** faction, its execution leges. The charge is:

$ARGUMENTS

Declare the Via aloud (**"I take Via Restitutio"**), then walk its Ritus in order.
The red signal here is a **command**, not a written test — a build, restore,
install, or CI job. Do not fabricate a unit test to satisfy Emendatio's gate: the
failing command *is* the Testis. The mend is a mechanical alignment (version,
manifest, lockfile, config), never new logic — if the fix needs new behaviour or a
regression test, this is not Restitutio; it is Emendatio.

Open **each phase** with its Praeco line (creed § *The Praeco*), naming the phase
and the Lex that binds it, before you walk it.

## Ritus — walk in order, one phase at a time

1. **Recall** (Lex Recordi). `sara recall --tag <topic>` / `--file <path>` for prior fixes to this dependency, toolchain, or config fault before deriving anything.
2. **Found the charge** (unless one exists). `sara add`; set `assignment`/`rationale`. Register the **failing command** as the acceptance criterion and its `--verify`: `sara check <id> "<command> is green" --kind acceptance --verify "<the command>"` (e.g. `dotnet build`, `npm ci`, `cargo build`, the CI job).
3. **Reproduce.** Run the command and **witness it red** with your own eyes — capture the build/restore/CI failure output. `sara step_done` this phase with the red output as its `result`.
4. **Locate.** Find the offending manifest, version, lockfile, or config — the point of disagreement. `sara annotate <id> --kind finding "cause: …"` once found.
5. **Mend.** Make the **smallest alignment** that brings things into agreement — a version bump, a lockfile regen, a manifest or CI-config edit. Introduce no new logic and author no test.
6. **Witness (Testes).** Run the once-red command again — it must now be **green** — **and** run the existing suite to prove **no regression**. The Testis is the command, explicitly, not a new test. `sara verify <id>`; tick acceptance via `sara step_done <id> <N> --kind acceptance --result "<command now green + suite green>"`.
7. **Record.** `sara learn --auto-files --tag <topic> "<the root cause and the alignment>"` so the next Adept starts from knowledge.

## Testes — no completion without these

- The **command that was red is now green** (build / restore / install / CI) — the command itself is the witness.
- The **existing suite still green** (no regression) — run it, do not assume it.
- **No new test authored** — a Restitutio that writes a unit test to prove itself has mistaken its Via.
- The **cause recorded** as memory (Lex Recordi).

## Ending (Lex Termini)

Do **not** open a PR and do **not** `sara done` here. When the red command is green
and the suite still passes, the build is restored but **not yet released** —
opening the PR is the sole office of `via_publicatio`, invoked explicitly. Leave
the work committed — scratch and throwaway probes removed first (Lex Munditiae) —
and the charge green and ready; stop there. On a failed gate, mend and re-walk
(Lex Emendationis) — do not abandon the charge or raise a new question.
