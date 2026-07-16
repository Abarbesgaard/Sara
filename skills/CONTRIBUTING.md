# Forge a new rite — contributing to the Adeptus register

The register is not a fixed list. It is a **law and a shape**: any contributor
may forge a **new Via** (a kind of work) or a **new sub-faction** (a discipline),
so long as it inherits the creed whole and overrides none of it. This document
is the way to do that.

Read the creed first — [`docs/factions/adeptus-machinae.md`](../docs/factions/adeptus-machinae.md).
Everything below assumes it.

---

## The law you may not break

Whatever you forge, these hold. They are what make the register worth adopting;
a contribution that weakens them is not accepted.

1. **The order of authority is fixed** — `Lex > Edictum > Mos > Sententia`. A new
   rite may add a *Mos* (a default) or lean on a *Sententia* (a maxim); it may
   **never** contradict a *Lex*.
2. **The five Leges are invariant** — Loci, Itineris, Nexus, Recordi, Termini. A
   rite inherits them; it does not re-legislate them.
3. **One charge, one Via** (Iter Unum). A rite governs exactly one kind of work.
   If your rite seems to hold two, it is two rites.
4. **Every rite ends at a gate, not a hope** (Lex Termini). Its Testes are real,
   demonstrable witnesses of completion — not "looks done".
5. **The work runs through sara** (Lex Recordi + Itineris). A rite drives the
   charge with sara: `recall` → found → walk the Ritus with `step_done` and
   evidence → `learn`. Work not logged did not happen.

---

## Anatomy of a Via skill

A Via is a *kind of work*. Its skill is the executable form of one row of the
Viae table in the creed. Use [`skills/via_emendatio/SKILL.md`](via_emendatio/SKILL.md)
as the template. Every `via_*` skill has the same five parts:

```
skills/via_<name>/SKILL.md
```

**1 — Frontmatter (YAML).** Three fields, no more:

```yaml
---
name: via_<name>
description: >-
  <One sentence naming the kind of work>, under the Adeptus rite of
  Via <Name>. Invoke when <trigger>: <Ritus as a → chain>.
  Completion requires <the Testes, restated>.
argument-hint: <what to build/fix/prove, or a sara task id>
allowed-tools: Bash(sara:*), Bash(git:*), Bash(gh:*), Read, Edit, Write, Glob, Grep, <build/test runners>
---
```

The `description` is the *only* part always in the model's context — it must
name the trigger and the completion gate so the model knows when to invoke and
when it may stop. Keep `allowed-tools` to what the rite actually needs
(Exploratio, for instance, grants no `Edit`/`Write` — it changes no code).

**2 — Bind to the creed.** The body opens by inheriting the law:

```markdown
You are bound by the `adeptus` creed and the `legio` discipline. The charge is:

$ARGUMENTS

Declare the Via aloud (**"I take Via <Name>"**), then walk its Ritus in order.
```

**3 — Ritus.** The ordered acts, numbered, walked one at a time. Each act names
the sara tool it drives (`recall`, `add`, `annotate`, `step_done`, `verify`,
`learn`). Copy the Ritus verbatim from the creed's Viae table — do not invent a
new procedure here.

**4 — Testes.** The witnesses of completion, as a short list. These must match
the creed's Testes for that Via exactly. This is the gate the model may not
walk past.

**5 — Ending (Lex Termini).** How the rite ends: open + `link` a PR and refuse
`done` until merged (for code rites), or record the finding (for Exploratio). A
failed gate is mended and re-walked, never abandoned.

---

## Anatomy of a sub-faction skill

A sub-faction is a **discipline**, not a kind of work — it sharpens *how* every
Via is walked. Use [`skills/legio/SKILL.md`](legio/SKILL.md) as the template. It:

- inherits the creed and all six Viae whole;
- adds a small set of `Lex <Name>` leges scoped to one concern (Legio's is
  execution & completion);
- **overrides nothing** — every lex it adds is consistent with the five Leges
  above it.

If your idea changes what "done" means, or forbids a step the creed allows, it
is not a sub-faction — it is a fork, and it does not belong in the register.

---

## Keeping the three faces in lockstep

The same law lives in three places. A change to one is a change to all three —
they must never drift:

| Face | File | Audience |
|---|---|---|
| **Download** | `docs/factions/*.md` | pasted into an agent's `AGENTS.md` |
| **Skill** | `skills/*/SKILL.md` | invoked by the agent as `/…` |
| **Register card** | `docs/skills.html` | the human choosing, before adopting |

When you add a Via: add its row to the creed's Viae table in
`docs/factions/adeptus-machinae.md`, add its `skills/via_<name>/SKILL.md`, and
add its card to the "six Viae" grid in `docs/skills.html`.

---

## Install, test, and submit

1. **Install reversibly** — symlink your skill into a skill root:
   ```bash
   ln -s "$PWD/skills/via_<name>" "$HOME/.agents/skills/via_<name>"
   ```
   Verify it appears under Skills in `/env`; remove the symlink to revert.
2. **Prove the rite** — run it against a real (or seeded) charge. The rite must
   drive sara, hit its Testes, and refuse to declare `done` before its gate.
   Walking the rite *is* the acceptance test for the rite.
3. **Keep the three faces in lockstep** (above).
4. **Open a PR** and let the rite you added judge itself: it is `via_publicatio`
   — Testes green, PR opened and linked, nothing `done` before merge.

> **ADEPTUS MACHINAE. NON VAGA. ITER UNUM. AD FINEM.** A contribution that
> wanders, assumes, or stops short of its gate is not of the order.
