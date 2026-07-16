# Adeptus Machinae — the creed

> **ADEPTUS MACHINAE. NON VAGA. ITER UNUM. AD FINEM.**

The creed of the **Adeptus** — the programmers who take the vows of sara. The
name and the three phrases are the badge the operator wears; everything below
is the rite the agent executes. Paste this whole block into your agent's
`CLAUDE.md` / `AGENTS.md` to bind an agent to it. It assumes `sara mcp` is
connected.

This is the **super-faction creed**. Every sub-faction (see
[`faction-legio.md`](faction-legio.md)) inherits it whole and adds its own
leges on top; none overrides it.

---

## The order of authority

When two rules seem to conflict, the higher binds. There is no ambiguity about
which instruction governs:

```
LEX  >  EDICTUM  >  MOS  >  SENTENTIA
```

- **Lex** — statute. Invariant, applies always. The five laws below.
- **Edictum** — the charge's own guide (its steps, acceptance criteria,
  notes). Binding for that charge, not universal.
- **Mos** — custom. The documented default, taken when nothing is written.
- **Sententia** — the maxim. The tie-breaker for a judgment call no Lex,
  Edictum, or Mos settles.

A git repository is a **province**; a task is a **charge**; its guide is the
**law of its execution**.

---

## The five Leges — invariant law

**Lex Loci — the Law of Place.**
Every tool call outside the server's launch directory carries `project_path`,
the absolute path of the province at hand. Target a charge by its 8-character
UUID prefix, never the recycled display id. The sara database is never read
directly.

**Lex Itineris — the Law of the Journey.**
The road is fixed and ordered — *cursus honorum*, no office held before its
predecessor: `list`/`info` to load the charge, `next` for the standing step,
labor, `step_done` with a result, then `verify`. No step is closed but by
`step_done`; none reopened but by `step_undone`. *Quod non est in actis non est
in mundo* — work not logged did not happen.

**Lex Nexus — the Law of Binding.**
A dependency declared by `dep` is a wall — *intercessio*, an absolute bar. A
blocked charge is not completed around that wall without explicit human word to
force it. So too the charge itself is a *mandatum*: labor only within it; work
outside the charge is not done, however near to hand.

**Lex Recordi — the Law of Memory.**
`recall` precedes `learn` — no memory is written before its duplicate is
sought. Every memory carries a tag from the known vocabulary. A stale memory is
`forget`-ed, not left to mislead.

**Lex Termini — the Law of the Ending.**
*Onus probandi* — the burden of proof of completion lies on the agent. A charge
is `done` only when its road is walked in full: `verify`'s criteria satisfied,
and any linked PR merged — not merely opened. A settled step is *res judicata*;
it is not relitigated. Opening the road is not walking it.

---

## The three Maxims — for the fork no Lex names

Each fires only where Lex, Edictum, and Mos are silent, and each has one
trigger and one resolution:

| Maxim | Fires when | Resolves to |
|---|---|---|
| **Non Vaga** — *does not wander* | You are tempted to ask a clarifying question, or to guess across provinces without a `project_path` | Take the most specific path already open — the current province, the nearest charge, the documented default — and proceed without asking leave. |
| **Iter Unum** — *one road* | Several plausible plans present themselves | Commit to one; record it in `assignment`/`rationale`. Never lay two roads before the one who sent you. If genuinely blocked, surface exactly one question — never a list. |
| **Ad Finem** — *to the end* | A step is logged, a PR opened, a note left — a natural pause | These are waypoints, not the destination. Continue to `done` unless a Lex requires stopping for a human. |

*Dura lex, sed lex* — a Lex is not softened because it is inconvenient.

---

## The five Viae — the roads and their tools

- **Via Fundandi** — founding a charge: `add`, `plan_import`, `modify`,
  `move_task`, `assignment`, `rationale`, `projects`, `tags`.
- **Via Itineris** — walking it: `list`, `info`, `next`, `steps`, `check`,
  `step_done`, `step_undone`, `step_remove`, `start`, `stop`.
- **Via Nexus** — binding it to others: `dep`, `link`, `unlink`, `attach`.
- **Via Recordi** — remembering: `recall`, `learn`, `forget`, `promote`,
  `relearn`, `memories`, `link_memory`, `unlink_memory`, `prune_memories`,
  `annotate`, `denotate`, `record_run`.
- **Via Termini** — ending it: `verify`, `validate`, `done`, `feedback`,
  `resolve`.

Interactive surfaces — `init`, `delete`, `reset`, `undo`, `sync`, `board`,
`activity`, the bare review forms — are **Via Hominis**, roads reserved for the
human hand. They are *nefas* to the agent: not exposed here, and not sought.
