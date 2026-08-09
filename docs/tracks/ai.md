# `Ai`: observation-only improved-edition player

`Ai` is the neutral public identifier for the observation-only candidate policy. It receives
the same `Obs` and issues the same `Cmd` values as Marshal. It has no income bonus, map
reveal, production multiplier, combat multiplier, or direct access to authoritative enemy
entities. The identifier describes its role; it is not a character or product name.

## Decision model

The policy keeps only beliefs that can be derived from observations:

- the last observed enemy building location;
- the maximum observed count of each enemy military type;
- a recent threat location when an own civilian or building takes damage;
- its own push-start value and health for deterministic retreat decisions.

Remembered enemies inform composition and movement. Focus-fire commands require a target
on a currently visible tile; otherwise the army moves toward the remembered location or a
public symmetric-start hypothesis. A source regression test forbids this module from
reaching through `Obs` to the arena `World`.

Its current strategy combines:

- a food reservation that researches Art of War before speculative citizen growth;
- shared legal construction placement with a protected same-tick command actor;
- bounded assistance for a visible unfinished critical site;
- a twelve-soldier bootstrap reserve, then bounded interleaving of citizen and military
  production;
- one citizen scouting an opposite-start hypothesis and then a deterministic wide circuit;
- counter selection from observed enemy composition, mixed with ranged and durable units;
- concentrated attacks on visible soft targets;
- retreat after measured army-value/health attrition or a visible local disadvantage;
- economy/base damage alarms that do not turn ordinary front-line contact into a false
  defensive recall.

These rules are arena policy, not claims about hidden retail AI implementation. They must
be re-evaluated as movement and gathering fidelity improves.

## Evaluation protocol

RoNEval evaluates `Ai` against Marshal in both seats on every seed. Development and
certification seed blocks are disjoint through `--seed-offset`. The certification gate uses
one paired sign-test sample per map seed, because its two seat assignments share terrain
and are correlated. It also requires positive aggregate raw damage and retains city,
resource, and army-value deltas without collapsing them into a tuned scalar.

See [RoNEval policy evaluation](roneval.md) for the exact command and gate. A passing arena
result means only that `Ai` beats Marshal under this documented arena model. It does not
promote arena fidelity, prove strength against a skilled human, or certify the absent
naval/air/diplomacy/supply game.

## Readiness

`Ai` is experimental and the current evidence is inconclusive. On the eight-seed
development block (offsets 0–7, twenty-minute horizon, eight requested threads), the
worktree candidate atop `10d96cd` produced identical serial and threaded checksums
`0xf493df96e8346941`. Its direct match record was 8–8, while the correlated seed-pair
record was only 1–1–6 (exact one-sided sign-test probability 0.75). Aggregate
`Ai`-minus-Marshal deltas were -1 city, +61,469 damage, -5,922 resources, and +2,320
army value. That seat-sensitive result does not establish superiority.

The sixteen-pair holdout block at offsets 8–23 has deliberately not been consumed. A
future frozen candidate should use it only after development evidence warrants the test.
The eventual report must include the exact commit, seed offset/count, horizon, thread
count, checksums, paired result, one-sided probability, raw deltas, and known model
blockers. Neither the current development snapshot nor a future arena pass is a release or
retail-fidelity claim.
