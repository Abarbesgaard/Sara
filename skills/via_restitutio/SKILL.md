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
allowed-tools: Bash(sara:*), Bash(herdr:*), Bash(git:status), Bash(git:log), Bash(git:diff), Bash(git:show), Bash(git:branch), Bash(git:fetch), Bash(git:ls-files), Bash(git:stash), Bash(gh:*), Read, Glob, Grep, Bash(python3:*)
---

# Via Restitutio — the rite of restoration

You are bound by the `adeptus` creed and, as a rite of the **Legio** faction, its execution leges. The charge is:

$ARGUMENTS

Declare the Via aloud (**"I take Via Restitutio"**), then walk its Ritus in order.

> **STOP — this rite is delegated. You do NOT edit any file yourself.** You are the **Praefectus**: you drive sara —
> recall, the record, the witness — and you do **not** touch the manifests
> yourself. Raise a **Miles** through herdr in a new tab to carry the
> **align** phase and re-run the red command (`herdr worktree create` → `herdr
> agent start … --kind copilot` → `herdr agent prompt … --wait` → watch with
> `herdr agent wait`/`read`). Brief it with the red
> command + province path/branch; record every phase against the charge UUID. The Miles never touches your cwd, and it leaves its changes **uncommitted** — `via_publicatio` alone commits.
The red signal here is a **command**, not a written test — a build, restore,
install, or CI job. Do not fabricate a unit test to satisfy Emendatio's gate: the
failing command *is* the Testis. The mend is a mechanical alignment (version,
manifest, lockfile, config), never new logic — if the fix needs new behaviour or a
regression test, this is not Restitutio; it is Emendatio.

Open **each phase** with its Praeco line (creed § *The Praeco*), naming the phase
and the Lex that binds it, before you walk it.

## Ritus — walk in order, one phase at a time

1. **Delegatio (Lex Delegationis) — walk this FIRST, before anything else.** You
   are the **Praefectus**; you will **not** edit a single source file this charge.
   Raise your **Miles** now, and confirm it is live before you walk any further:
   - Raise the Miles in its own herdr workspace — **NEVER use the `task` tool** (that spawns a subagent inside Copilot, not a herdr pane):
     ```sh
   # Get the current workspace id
   WS=$(herdr pane current | python3 -c "import sys,json; print(json.load(sys.stdin)['result']['workspace_id'])")

   # Open a new tab in that workspace and grab its pane id
   PANE=$(herdr tab create --workspace "$WS" --cwd <province-path> --label "Miles: <charge>" --no-focus \
        | python3 -c "import sys,json; print(json.load(sys.stdin)['result']['pane_id'])")

   # Start Copilot — NEVER use the task tool (spawns inside Copilot, not a herdr pane)
   herdr agent start copilot --kind copilot --pane "$PANE"
   ```
   - Hold that pane id. **Every** file-changing phase below is briefed to THIS Miles: `herdr agent prompt <pane-id> "<the phase + its acceptance criteria + the province path/branch>" --wait`, then `herdr agent read <pane-id>` for its evidence.
   - **Gate:** if you ever reach a phase that writes to a file and no Miles is running, you have walked the rite wrong — **STOP** and raise one before proceeding. The Praefectus's own hands touch only `sara`, `herdr`, and read-only `view`/`grep`.
   - **Tool hard stop:** if you are about to call `Edit`, `Write`, or any shell command that writes a file (`tee`, `sed -i`, `echo … >`, etc.) — **STOP IMMEDIATELY**. You are the Praefectus; that call belongs to a Miles. Reaching for an edit tool is a signal you have skipped Phase 1; go back and raise the Miles now.

2. **Recall** (Lex Recordi). `sara recall --tag <topic>` / `--file <path>` for prior fixes to this dependency, toolchain, or config fault before deriving anything.
3. **Found the charge** (unless one exists). `sara add`; set `assignment`/`rationale`. Register the **failing command** as the acceptance criterion and its `--verify`: `sara check <id> "<command> is green" --kind acceptance --verify "<the command>"` (e.g. `dotnet build`, `npm ci`, `cargo build`, the CI job).
4. **Reproduce.** Run the command and **witness it red** with your own eyes — capture the build/restore/CI failure output. `sara step_done` this phase with the red output as its `result`.
5. **Locate.** Find the offending manifest, version, lockfile, or config — the point of disagreement. `sara annotate <id> --kind finding "cause: …"` once found.
6. **Mend.**

   ⛔ **HARD GATE — you have no Edit or build tools.** Brief your Miles: `herdr agent prompt "$PANE" "<exact changes + province path>" --wait`, then `herdr agent read "$PANE"`. The Miles edits and builds; you record the evidence. Make the **smallest alignment** that brings things into agreement — a version bump, a lockfile regen, a manifest or CI-config edit. Introduce no new logic and author no test.
   > **⛔ PRAEFECTUS HARD STOP.** You do NOT make this edit. You do not call `Edit`, `Write`, `sed -i`, `tee`, or any file-writing shell command. Brief your Miles with the exact change needed: `herdr agent prompt <pane-id> "align <file>: change <X> to <Y>; then re-run <the red command> and report the output" --wait`. Wait for its evidence, then record it.
7. **Witness (Testes).** Run the once-red command again — it must now be **green** — **and** run the existing suite to prove **no regression**. The Testis is the command, explicitly, not a new test. `sara verify <id>`; tick acceptance via `sara step_done <id> <N> --kind acceptance --result "<command now green + suite green>"`.
8. **Record.** `sara learn --auto-files --tag <topic> "<the root cause and the alignment>"` so the next Adept starts from knowledge.

## Testes — no completion without these

- The **command that was red is now green** (build / restore / install / CI) — the command itself is the witness.
- The **existing suite still green** (no regression) — run it, do not assume it.
- **No new test authored** — a Restitutio that writes a unit test to prove itself has mistaken its Via.
- The **cause recorded** as memory (Lex Recordi).

## Ending (Lex Termini)

When the red command is green and the suite still passes, the charge is
complete — do two things and call `sara done`:
1. **Lex Munditiae** — remove scratch probes, temp files, and test containers the Miles created that are not part of the fix (`docker compose down`, remove throwaway scripts, etc.).
2. **Record** — `sara learn` the root cause and the alignment made.

On a failed gate, mend and re-walk (Lex Emendationis) — do not abandon the charge or raise a new question.
