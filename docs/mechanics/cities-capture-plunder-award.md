# `Cities::capture_city` capital-plunder award prefix

Implementation tranche:
`crates/don-sim/src/systems/combat/cities_capture_plunder_award.rs`.

Fidelity tier: **C**. This is an instruction-bounded recovery of 422 bytes of the
retail outer city-capture driver. Nested mutations remain request-bound host receipts;
no retail differential has promoted it.

## Provenance and exact boundary

- `ron-bin/riseofnations.exe` SHA-256:
  `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`
- `ron-bin/sbl/rise.pdb` SHA-256:
  `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`
- PDB procedure: `int Cities::capture_city(int new_owner, int old_city, int old_owner)`,
  VA `0x00733380`, size 7,998 (`0x1F3E`), exclusive end `0x007352BE`, source
  `cities.cpp:87..733`.
- This tranche: `0x00733FAC..0x00734152`, 422 (`0x1A6`) bytes.
- Cumulative recovered prefix: `0x00733380..0x00734152`, 3,538 (`0xDD2`) bytes.
- Exact residual: `0x00734152..0x007352BE`, 4,460 (`0x116C`) bytes.

The start is the old-owner `leader_flags & 0x00400000` test. The exclusive end is the
first localized-notification initializer at `0x00734152`. Thus the tranche includes
the complete first six-good availability/mutation loop and the local-player decision,
but owns none of the following String, event allocation, coordinate, or UI calls.

## Capital-capture admission and the one persistent flag mutation

Input must be the prior gate's `EnterPlunder0x00733fac` receipt. Retail reads the old
owner's leader flags, then takes the untouched forward edge `0x00734547` when any of
these is true:

1. old-owner leader flag `0x00400000` is already set;
2. the old city lacks capital flag `0x0010`;
3. the prefix proved this was `captured_own_capital`; or
4. the first `GameInfo::team_style` read equals 3,
   `BARBARIANS_AT_THE_GATES`.

The first three predicates short-circuit the team-style read. Only the admitted arm
executes the persistent mutation
`old_owner.leader_flags |= 0x00400000` at `0x00733FDC`. Its typed receipt is bound to
the old owner, the exact mask, the flags observed by the prior read, and the exact OR
result. A missing effect or altered before/after image is rejected at that commit point.

Retail reads `team_style` again after the OR; the proof retains both reads as distinct
phases rather than assuming the second value.

## Exact capital-plunder sizing

For second-read team styles 1 (`SURVIVAL`), 2 (`ASSASSIN`), and 9
(`SURVIVAL_COOPERATIVE`), retail ignores the accumulated plunder and computes:

```text
flagged = count(player[0..7].leader_flags & 3 == 1)
remaining = wrapping(Game::num_nations - flagged)
multiplier = max(wrapping(Game::num_nations - remaining), 1)
sized_plunder = wrapping(Constants::capital_plunder_assassin * multiplier)
```

The two subtractions reduce algebraically to `flagged`, but the proof retains both
wrapping operations and the otherwise-cancelled `num_nations` read. PDB names fix
`Game::num_nations` at `+0x6A0` and
`Constants::capital_plunder_assassin` at `+0x30C`. The receipt attests the compiled read
order: `num_nations`, player flags 0 through 7, then the rule.

For every other admitted style, retail reads `Constants::capital_plunder` at `+0x308`
and computes `max(accumulated_plunder, capital_plunder)` with a signed comparison. This
is a floor, not a cap: the `jg 0x007340AC` arm preserves an already larger value.

## Russian/Despot split

At `0x007340AF` retail initializes the old-owner refund local to zero. It then consumes
the prior gate's two exact locals:

- With old-owner Russian plunder-steal true, the refund becomes the full sized amount.
  The new owner also receives the full amount only when the qualifying-general local is
  true; otherwise its award becomes zero.
- Without Russian plunder-steal but with a qualifying Despot/Spitamenes probe, retail
  reads `Constants::thedespot_plunder` at `+0xC70` and replaces the new-owner award with
  `wrapping(sized * percent) / 100`. The division is signed and truncates toward zero.
- Otherwise the new-owner award remains the sized amount and the refund remains zero.

The local-effect receipt retains initialization, conditional rule read, refund write,
and award write in compiled order. A zero new-owner award jumps directly to the later
old-owner refund fork at `0x0073432D` without any resource-availability calls.

## Ordered availability and bucket mutations

For a nonzero new-owner award, retail walks TypeIndex values 0 through 5 in ascending
order. For each index:

1. call `new_owner.type_avail(type_index, 1)` at `0x006E33A0`;
2. only when nonzero, call `old_owner.type_avail(type_index, 1)`;
3. only when both are nonzero and the index is not 3, call
   `new_owner.bucket_add(type_index, award)` at `0x0043ED10`.

The PDB fixes `type_avail(TypeIndex,int)` and its raw result is the 0/2/4 availability
classification. The model retains that raw value and rejects any other value rather
than relabeling it Boolean. Type 3 is excluded only after both availability calls, so
both queries still occur for it.

Disassembly of `LeaderData::bucket_add` is a 41-byte encrypted-bucket tail: decode with
XOR `0x8221`, wrapping-add the amount, update `LeaderDataEncrypt::scratch`, re-encode,
and store. Each receipt binds owner, bucket, amount, decoded before/after balances, and
synchronous completion; `after == wrapping(before + amount)` is enforced. Mutations
therefore occur in exact bucket order 0, 1, 2, 4, 5 when all six types are available.

After the loop retail reads `Console::who` at `+0x298`. Equality with the new owner
enters localized award notification construction at `0x00734152`; every other value
goes to `0x0073432D`.

## Typed continuations and validation

The exact exits are:

- `LocalAwardNotification0x00734152` after all bucket mutations when the new owner is
  the console player;
- `OldOwnerRefund0x0073432d` for a zero award or a nonlocal new owner; and
- `AlternatePlunder0x00734547` before sizing/mutation when the capital-capture admission
  predicates fail.

The alternate receipt deliberately leaves sized award/refund values absent. The target
CFG begins by performing its own split on the untouched accumulated plunder; precomputing
those locals here would merge two distinct retail cones.

Focused private path-import tests freeze addresses, both team-style reads, the capital
floor, elimination multiplier, Russian split, Despot signed percentage, availability
short-circuit order, excluded type 3 queries, bucket order and wrapping mutation images,
the console fork, and corruption rejection for both mutation receipt kinds. No shared
combat export is added.

Root convergence formatted the isolated Rust files and persvati job
`city-capture-award-20260809T232812Z-30742-30991-cfb2a38cb8f5` passed all seven focused tests.
Retail was not run; the proof remains outside the shared combat module.

The exact residual is `0x00734152..0x007352BE`, 4,460 bytes. It starts with localized
new-owner award presentation. Forward cones still own the old-owner refund resource
loop and message, alternate non-capital plunder allocation, later capital/recapture
diplomacy, scoring/presentation, SimpleArray cleanup, and returned city index. None of
those effects are claimed here.
