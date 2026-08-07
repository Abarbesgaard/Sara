---
name: via_publicatio
description: >-
  Invoke to release finished, green work to the gate — the Adeptus rite of Via
  Publicatio, the SOLE rite that opens a PR. Confirm every charge's Testes are
  already green → assemble the record from the Acta → open and link the PR →
  halt at the gate. Completion is the merge, not the opening — nothing is called
  done before it.
argument-hint: <what to release, or a sara task id>
allowed-tools: Bash(sara:*), Bash(git:*), Bash(gh:*), Read, Glob, Grep, Bash(cargo:*), Bash(dotnet:*), Bash(npm:*), Bash(pnpm:*), Bash(make:*)
---

# Via Publicatio — the rite of release

You are bound by the `adeptus` creed and, as a rite of the **Legio** faction, its execution leges. The charge is:

$ARGUMENTS

Declare the Via aloud (**"I take Via Publicatio"**), then walk its Ritus in
order. This rite opens the road; it does not walk another's.

> **Delegation (Lex Delegationis).** Publicatio is the **Praefectus's own gate
> office** and the **sole committer**: staging, `git commit`, `git push`, and
> `gh pr create` happen **here and nowhere else** — no rite and no Miles commits
> before this gate. The Miles left its changes **uncommitted** in the worktree;
> you commit them now. Dismiss any lingering Miles agent, but close its workspace
> (`herdr workspace close`) only **after** you have committed and pushed from its
> worktree — never before, or the uncommitted work is lost.

Open **each phase** with its Praeco line (creed § *The Praeco*), naming the phase
and the Lex that binds it, before you walk it.

## Ritus — walk in order, one phase at a time

1. **Confirm the Testes are already green** (Lex Termini). Every charge being released must have its own Ritus complete and its Testes satisfied. `sara verify <id>` for each; if any gate is red, **halt** — release is not the place to do the work. Send it back to its Via.
2. **Assemble the record from the Acta.** Build the PR body / changelog from the charges' steps, results, and annotations (`sara info <id> --md`). The record is faithful to what was actually done.
3. **Open the PR.** In the Miles's worktree (where the uncommitted changes live), stage and commit **only the work in scope** — never the scratch, ad-hoc scripts, or throwaway fixtures used to probe along the way (Lex Munditiae); delete or `git restore`/exclude them first. This is the **first and only commit** of the charge. Push, `gh pr create`. `sara link <id> <url>` on each charge released.
4. **Halt at the gate.** The rite ends at the opened, linked PR.

## Testes — no completion without these

- **All prior Viae's Testes green** before release begins.
- The **PR is opened and linked**.
- **Nothing is `sara done`** until the PR is **merged** — opening is not walking.

## Ending (Lex Termini)

Do not `sara done` on merge-of-hope. When the PR is truly merged, `sara validate`
then `sara done`; review the provisional memory sara synthesises and
`promote`/`relearn`/`forget` it.
