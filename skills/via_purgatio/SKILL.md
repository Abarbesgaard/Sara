---
name: via_purgatio
description: >-
  Cleanse a static-analysis alert (CodeQL/SAST) in code that already builds and
  runs — not a failing test or broken build. Invoke when a scanner flags a latent
  flaw (an undisposed IDisposable, a tainted flow, an unencoded output) that the
  functional tests are BLIND to, and the fix is behaviour-preserving (a mechanical
  mend, or a justified suppression of a false positive). The analyzer re-scan — not
  the unit suite — is the completion Testis; same-rule findings may be batched. If
  the diagnostic breaks the build use via_restitutio; if a failing test reproduces
  it use via_emendatio.
argument-hint: <the analyzer finding(s) — rule id + file:line — or a sara task id>
allowed-tools: Bash(sara:*), Bash(git:*), Bash(gh:*), Read, Edit, Write, Glob, Grep, Bash(python3:*), Bash(pytest:*), Bash(cargo:*), Bash(dotnet:*), Bash(npm:*), Bash(pnpm:*), Bash(make:*)
---

# Via Purgatio — the rite of cleansing

You are bound by the `adeptus` creed and, as a rite of the **Legio** faction, its execution leges. The charge is:

$ARGUMENTS

Declare the Via aloud (**"I take Via Purgatio"**), then walk its Ritus in order.
The red signal here is an **analyzer alert** — CodeQL, a SAST/security linter, a
compiler diagnostic — against code that already builds and runs. A diagnostic
that **breaks** the build (warnings-as-errors, a hard compile error) is *not*
Purgatio's — a red build is Via Restitutio's command; Purgatio takes only the
**non-fatal** alert against code that still compiles and runs. The flaw is
latent: your functional tests cannot see it, so they cannot prove its cure. The
mend is behaviour-preserving (like Renovatio), usually mechanical — a `using`/
`dispose`, a parameterised query, an encoded output — never new logic. Some
findings, though, are **false positives**: there the cleanse is not a code change
at all but a **justified suppression** (a dismissal in the analyzer, or a scoped
`query-filter` in `codeql-config.yml`) carrying a **recorded rationale** — never
contort correct code to placate a scanner. If the fix needs new behaviour, or a
failing functional test can reproduce it, this is not
Purgatio; it is Emendatio or Renovatio.

**The batching exception to Iter Unum.** Findings of the **same rule** from the
same analyzer share one cause and one fix pattern — they are *one kind of work*
and MAY be gathered under a single charge (register each site as its own
acceptance criterion). Findings from **different rules** are different charges.

Open **each phase** with its Praeco line (creed § *The Praeco*), naming the phase
and the Lex that binds it, before you walk it.

## Ritus — walk in order, one phase at a time

1. **Recall** (Lex Recordi). `sara recall --tag <rule-or-analyzer>` / `--file <path>` for prior cleansings of this rule, analyzer, or file before deriving anything.
2. **Found the charge** (unless one exists). `sara add`; set `assignment`/`rationale`. Register the finding as the acceptance criterion — one per site when batching a rule — naming the analyzer scan as its `--verify`: `sara check <id> "<rule> resolved at <file:line>" --kind acceptance --verify "<the scan command, e.g. codeql database analyze … / semgrep … / the CI security job>"`. **When the scan is CI-only there is no local command to run** — the `--verify` is then a **deferred descriptor** (e.g. `"the CI security job"`), a marker `sara verify` cannot execute; record it as such and prove it in the pipeline (step 6), not by a local run.
3. **Confirm the finding.** Cite the alert exactly — rule id, `file:line`, the analyzer's message — and state plainly that the functional suite is **blind** to it (so a passing suite is *not* proof of the cure). `sara step_done` this phase with the alert as its `result`. Where the analyzer runs only in CI, note that the true scan is deferred to the pipeline.
4. **Pin against regression.** The existing suite is the pin: **run it green before a line is changed** to fix the behaviour you must not disturb. Author a new pinning test ONLY when the site's behaviour is genuinely unpinned — never fabricate a test that pretends to see a flaw it cannot. `sara step_done` with the green baseline as `result`.
5. **Cleanse.** Make the **smallest behaviour-preserving mend** that removes the flaw — add the `using`/dispose, parameterise the query, encode the output. Introduce no new behaviour. When batching, apply the same fix pattern to every registered site. **Where the finding is a false positive**, cleanse it by a **justified suppression** instead of a code change — dismiss it in the analyzer, or add a **scoped** `query-filter` to `.github/codeql/codeql-config.yml` (the recorded global-suppression pattern) — and write the **rationale** into the charge (`sara annotate`). A bare suppression with no recorded reason is not a cleanse; and never reshape sound code to satisfy a wrong alert.
6. **Witness (Testes).** Re-run the analyzer — the finding(s) must be **gone**; the analyzer is the witness, not the unit tests — **and** run the existing suite to prove **no regression**. Tick each site: `sara step_done <id> <N> --kind acceptance --result "<rule clear at file:line + suite green>"`. Where the scan is CI-only, this witness is honestly **deferred to the pipeline**: the charge is proven when that scan comes back clean, and a green local suite alone does **not** close it.
7. **Record.** `sara learn --auto-files --tag <rule-or-analyzer> "<the rule, the fix pattern, and that functional tests were blind to it>"` so the next Adept cleanses from knowledge.

## Testes — no completion without these

- The **analyzer reports the finding(s) resolved** — the scan is the witness, explicitly, not the unit suite (which is blind to the flaw).
- A finding cleansed by **suppression** rather than a code mend carries a **recorded rationale** (a dismissal note, or the scoped `codeql-config.yml` filter) — a bare suppression is not a cleanse.
- The **existing suite still green** (behaviour unchanged, no regression) — run it, do not assume it.
- **No new behaviour** introduced under cover of the cleanse.
- Where the analyzer is **CI-only**, completion **waits on the pipeline scan** — a green local suite does not prove the cure, and the `--verify` stands as a deferred descriptor, not a runnable command.
- The **rule and its fix pattern recorded** as memory (Lex Recordi).

## Ending (Lex Termini)

Do **not** open a PR and do **not** `sara done` here. When the analyzer reports the
finding cleared and the suite still passes, the flaw is cleansed but **not yet
released** — opening the PR is the sole office of `via_publicatio`, invoked
explicitly. Leave the work committed — scratch and throwaway probes removed first
(Lex Munditiae) — and the charge green and ready; stop there.

**When the true scan is CI-only,** the analyzer that witnesses your cure runs on
the pipeline the PR triggers. The charge is proven only when that scan is clean;
so it is not `done` until the scan is green **and** the PR merged. Opening the PR
early to summon the scan is still `via_publicatio`'s office — the cleanse waits at
the gate, it does not open its own. On a failed scan, mend and re-walk (Lex
Emendationis) — do not abandon the charge or raise a new question.
