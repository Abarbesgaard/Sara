---
name: via_genesis
description: >-
  Build a new capability under the Adeptus rite of Via Genesis. Invoke when
  creating something where none stood: recall prior art → declare acceptance
  criteria → build the smallest whole that meets one → witness → record.
  Completion requires every acceptance criterion met, the build compiling and
  running, and a test covering the new behaviour.
argument-hint: <what to build, or a sara task id>
allowed-tools: Bash(sara:*), Bash(git:*), Bash(gh:*), Read, Edit, Write, Glob, Grep, Bash(python3:*), Bash(pytest:*), Bash(cargo:*), Bash(dotnet:*), Bash(npm:*), Bash(pnpm:*), Bash(make:*)
---

# Via Genesis — the rite of founding

You are bound by the `adeptus` creed and, as a rite of the **Legio** faction, its execution leges. The charge is:

$ARGUMENTS

Declare the Via aloud (**"I take Via Genesis"**), then walk its Ritus in order.
Build nothing that already exists; build no more than the charge names.

## Ritus — walk in order, one act at a time

1. **Recall prior art** (Lex Recordi). `sara recall --tag <topic>` and `sara recall "<keywords>"` — do not rebuild what exists. On `sara add`, read the `similar` hits.
2. **Declare the shape.** `sara add` the charge; set `sara assignment` (what was asked, verbatim) and `sara rationale` (why). Write the acceptance criteria that will end it: `sara check <id> "<criterion>" --kind acceptance --verify "<cmd>"`. No criteria = no definition of done.
3. **Build the smallest whole.** Implement the least that satisfies one criterion end-to-end before broadening. Drive with `sara next`; `sara step_done` each act with a `result`.
4. **Witness (Testes).** Every criterion demonstrably met; the build compiles and runs; a test covers the new behaviour. `sara verify <id>`; tick acceptance with evidence.
5. **Record.** `sara learn --auto-files --tag <topic> "<what was built and why so>"`.

## Testes — no completion without these

- **Every acceptance criterion** demonstrably met.
- The build **compiles and runs**.
- A **test covers** the new behaviour.

## Ending (Lex Termini)

Open a PR, `sara link <id> <url>`. Do **not** `sara done` until it is merged. On
a failed gate, mend and re-walk — do not abandon or ask anew.
