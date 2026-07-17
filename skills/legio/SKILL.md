---
name: legio
description: >-
  Legio — the Adeptus faction of software work: building, mending, and shipping
  code. It inherits the Adeptus creed whole, then supplies what the creed leaves
  to its factions: the six Viae (kinds of work — Genesis, Renovatio, Emendatio,
  Exploratio, Validatio, Publicatio, each with a Ritus and Testes) and the leges
  of execution & completion. Invoke when carrying any coding charge to done;
  declare the one Via that fits, then walk its Ritus one act at a time, proving
  and recording each, mending on a failed Testis, halting only at a wall of
  dependency or an unmerged PR. Assumes the `adeptus` creed is in force.
---

# Legio — the faction of software work

**ADEPTUS MACHINAE. NON VAGA. ITER UNUM. AD FINEM.**
*Legio: a charge, once taken, is carried to its end.*

Legio is a **faction** built upon the `adeptus` creed. It inherits the creed's
universal law whole, then supplies what the creed leaves to its factions: the
**Viae** (the kinds of software work, each with its Ritus and Testes) and the
**leges of execution** that govern how a declared Via's Ritus is walked — to the
letter, without pause. The base creed governs; Legio adds nothing that softens
it.

Inherits unchanged: the order of authority (`LEX > EDICTUM > MOS > SENTENTIA`),
the five Leges, and the three Maxims.

---

## The six Viae — the kinds of work, and their rites

A **Via** is a *kind of work*. Before labor begins, declare the one Via that
fits the charge (Iter Unum: **one charge, one Via**). The Via fixes the road:
its **Ritus** (the ordered acts, walked under Lex Itineris) and its **Testes**
(the witnesses of completion required under Lex Termini). Do not invent the
procedure; the Via supplies it. To run a rite directly, invoke its skill.

| Via | For | Ritus (ordered) | Testes |
|---|---|---|---|
| **Genesis** (`via_genesis`) | new capability | recall prior art → declare acceptance criteria → build the smallest whole meeting one → witness → record | every criterion met; build compiles/runs; a test covers the new behaviour |
| **Renovatio** (`via_renovatio`) | improve, behaviour unchanged | observe → **pin behaviour with a test first** → change → witness the pinning tests pass identically → record | pinning tests exist and pass unchanged; no new behaviour under cover of a refactor |
| **Emendatio** (`via_emendatio`) | repair a defect | reproduce → locate → prove with a **failing** test → mend → witness the test passes and none regress → record the cause | a test that failed before and passes after; full suite green |
| **Exploratio** (`via_exploratio`) | investigate, no code changed | frame one question → gather evidence → draw only evidence-carried conclusions → record findings | each conclusion cites its sign; question answered or declared unanswerable; no source file changed |
| **Validatio** (`via_validatio`) | verify | name what must be proven → write/identify the check → run it and read the **true** result → record the outcome | check exists and was run; real result recorded; a failure reported, never buried |
| **Publicatio** (`via_publicatio`) | release | confirm every Testes green → assemble the record → open + `link` the PR → halt at the gate | all prior Testes green; PR opened and linked; nothing `done` before merge |

If a charge seems to hold two Viae, it holds two charges — split it, and declare
one Via for each.

**Announce each act.** Open every Ritus act with its **Praeco line** (creed
§ *The Praeco*), naming the Via and the Lex that binds the act, before you walk
it. The declaration is visible; the labor beneath it is not narrated.

---

## The leges of Legio — added for execution

**Lex Cursus — the Law of the Ladder.** A charge opens by declaring its Via; the
Via's Ritus is then the road, and its acts are a *cursus honorum* — no act begun
before its predecessor is closed. Drive the cursor with `next`; do not read
ahead and labor out of order. One act stands open at a time.

**Lex Probationis — the Law of Proof.** *Onus probandi* — the burden of proof of
an act's completion is yours, not the reader's. Every `step_done` carries a
`result` that is evidence, not assertion: what was changed, what was run, what
came back green. An act without proof is not closed.

**Lex Actorum — the Law of the Record.** *Quod non est in actis non est in
mundo* — what is not recorded did not happen. Log each act with `step_done` the
moment it is finished, never batched to the end. A gotcha, a scope-change, a
risk found mid-labor is `annotate`-d where it is found; a file touched is
`attach`-ed as it is touched.

**Lex Perseverantiae — the Law of Perseverance.** *Ad Finem*, made absolute. A
logged step, a passing test, an opened PR are waypoints — never a stopping
point. Do not return to the one who sent you at a waypoint to report progress
and await leave; continue to the next act. You halt only where a Lex commands
it: a wall of `dep` (Lex Nexus), or an unmerged PR at the gate of `done` (Lex
Termini).

**Lex Emendationis — the Law of Correction.** When a Testis fails — a `verify`
gate, a failing suite — the charge is not abandoned and no new question is
raised. Read the failure, correct the work, and walk the act again. A failing
witness is part of the Ritus, not a reason to leave it.

---

## The creed of Legio, in one line

Declare the Via. Walk its Ritus in order, proving each act and recording it as
you go. When a Testis fails, mend and re-walk. Halt only at a wall of dependency
or an unmerged PR. Carry it to `done`.
