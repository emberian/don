# `Cities::capture_city` prefix: counters, center swap, and continuation state

Implementation tranche:
`crates/don-sim/src/systems/combat/cities_capture_prefix.rs`.

Fidelity tier: **C**. This is an instruction-bounded recovery of the first 969 bytes of
the retail outer city-capture driver. Nested object operations are identity-bound typed
receipts. No retail differential run has promoted the tranche.

## Provenance and exact boundary

- `ron-bin/riseofnations.exe` SHA-256:
  `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`
- `ron-bin/sbl/rise.pdb` SHA-256:
  `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`
- PDB procedure: `int Cities::capture_city(int new_owner, int old_city, int old_owner)`,
  VA `0x00733380`, size 7,998 (`0x1F3E`), exclusive end `0x007352BE`, source
  `cities.cpp`.
- This tranche: `0x00733380..0x00733749`, 969 (`0x3C9`) bytes.

The exclusive end is the first load used to branch on the center `swap_team` result.
A negative result jumps to the old-member census at `0x00733CE2`. A nonnegative result
continues into `City::capture` and member reassignment at `0x00733755`. The receipt names
both instruction addresses; it does not pretend that either downstream path completed.

The caller is the recovered `Build::check_capture` city-center arm. Its arguments compose
directly with `CaptureCityBoundaryRequest`: selected/re-homed `new_owner`, the old city
index, and the old owner. The inner `City::capture` model in `tech_cities` is downstream
of this prefix and is not substituted for this outer transaction.

## Retail mutation order

Before changing the center object, retail performs the following sequence:

1. If `old_owner.LeaderData::dip[new_owner].agree == 1`, call
   `old_owner.action_clear_all(new_owner)`.
2. If the old owner has Persian tribe bonus 23 (`0x17`) and the old city has capital flag
   `0x10`, call `LeaderData::find_capital`. Valid nonnegative outputs whose owner is the
   old owner set a boolean retained for the later capital phase. This lookup is mandatory
   preflight data for that branch.
3. Increment old-owner `LeaderData::cities_lost` (`+0x828`), then new-owner
   `cities_captured` (`+0x824`). These happen even if the later center swap fails.
4. Optionally call `IStatsAndAchievements::UnlockAchievement` for `ACH_CONQUEROR`
   (table index `0x0B`) with value 1. Every gate is required:
   the new owner is `Game +0x77`, `Game +0x820` lacks bit `0x10`, both mode fields
   `+0x69C/+0x6A8` differ from 1, and the selected setup-player byte `+0xCB` is zero.
5. OR leader flag `0x02000000` into the new owner first and the old owner second.

Retail resolves the old center type's leader radius, clamps only values greater than 64,
and computes the later circle-table ring with the signed sequence
`(clamped + 3 + (((clamped + 3) >> 31) & 3)) >> 2`. The old center object is the first
entry in the old-object SimpleArray.

It then calls the center object's `swap_team(new_owner)` boundary. On success, the new
center is immediately activated with `(1,1,0)`, its object index becomes the first entry
of the new-object SimpleArray, and `BuildData::city` selects the returned new city record.
The initial plunder accumulator is:

```text
CITY_PLUNDER_PER_LEVEL * (new_city.get_level() - 1)
```

All arithmetic is wrapping 32-bit integer arithmetic. This corrects the older unverified
helper/comment in `tech_cities`, which guessed `constant * level`; that shared file is not
edited by this lane. A failed swap leaves the accumulator zero.

Finally, both swap outcomes emit `Achieve::add_event` in order:

1. `ACHIEVEEVENT_CITY_CAPTURED` (6) for the new owner;
2. `ACHIEVEEVENT_CITY_LOST` (7) for the old owner.

Both receive the old `CityData::name` String at offset `0x90`. Thus failure does not roll
back diplomacy cleanup, counters, achievement unlock, leader flags, or these events.

## Typed boundary and validation receipts

`SwapCityCenterReceipt` distinguishes a negative retail return from a successful object
identity. Success must bind the new center and new city to `new_owner`, supply the new city
level, and affirm both ownership and object-copy effects before activation is allowed.

The external mutation-sensitive specs freeze:

- exact address and byte-count receipts;
- mandatory Persian capital lookup before mutation;
- failed-swap mutation ordering and its `0x00733CE2` continuation;
- successful activation before events, plus `(level - 1)` plunder;
- every platform-achievement gate and successful-swap identity checks;
- signed radius-ring arithmetic and the 64-unit high clamp.

No Cargo, compiler, test runner, formatter, or remote validation was run in this token-only
wave. Static validation is limited to source/decomp/disassembly comparison and
`git diff --check`.

## Frozen residual

The exact residual is `0x00733749..0x007352BE`, 7,029 bytes. Its first forks are:

- success `0x00733755`: `City::capture`, `Armies::update_city`, member enumeration and
  selective member `swap_team/activate` operations;
- failure `0x00733CE2`: old member census without a new city record;
- both later converge into `City::find_buildings`, same-team nearby-object reassignment,
  plunder/resource transfer, capital/recapture diplomacy, score/presentation, cleanup,
  and the final returned new city index.

The next coherent tranche should consume `CitiesCapturePrefixReceipt`, preserve both
SimpleArray orders, and converge the two swap-result arms before claiming economy,
diplomacy, score, or returned-city completion.

Shared integration handoff only:

```diff
 pub mod build_check_capture;
+pub mod cities_capture_prefix;
```
