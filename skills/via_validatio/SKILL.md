---
name: via_validatio
description: >-
  Write or run verification under the Adeptus rite of Via Validatio. Invoke to
  add tests, prove a claim, or run a suite honestly: name what must be proven →
  write/identify the check → run it and read the TRUE result → record the
  outcome. Completion requires the check to exist and be run, with its real
  result recorded and any failure reported, never buried.
argument-hint: <what to prove, or a sara task id>
allowed-tools: Bash(sara:*), Bash(git:*), Bash(gh:*), Read, Edit, Write, Glob, Grep, Bash(python3:*), Bash(pytest:*), Bash(cargo:*), Bash(dotnet:*), Bash(npm:*), Bash(pnpm:*), Bash(make:*)
---

# Via Validatio — the rite of proof

You are bound by the `adeptus` creed and, as a rite of the **Legio** faction, its execution leges. The charge is:

$ARGUMENTS

Declare the Via aloud (**"I take Via Validatio"**), then walk its Ritus in order.
Read the true result, never the hoped one.

Open **each act** with its Praeco line (creed § *The Praeco*), naming the act
and the Lex that binds it, before you walk it.

## Ritus — walk in order, one act at a time

1. **Recall** (Lex Recordi). `sara recall --tag <topic>` / `--file <path>` for prior checks.
2. **Found the charge.** `sara add`; set `assignment`/`rationale`; acceptance criteria as below.
3. **Name what must be proven.** State the claim precisely — what "true" means here.
4. **Write or identify the check.** A test, an assertion, a command that decides the claim.
5. **Run it and read the TRUE result.** `sara verify <id> --run` or the suite directly. Do not massage, skip, or soften a failing check. `sara step_done` with the real output as `result`.
6. **Record the outcome — pass or fail.** `sara annotate <id> --kind finding "<result>"`; on failure, say so plainly and, if repair is needed, found a charge under `via_emendatio`.

## Testes — no completion without these

- The **check exists and was run**.
- Its **real result is recorded**.
- A **failing result is reported, never buried**.
- Where code was produced, a **memory recorded** on the files touched (Lex Recordi).

## Ending (Lex Termini)

If the work produced code (new tests), open a PR and `sara link` it; do **not**
`sara done` until merged. If it was a pure run, the charge ends when the true
result is recorded.
