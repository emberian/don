# `Cities::capture_city` plunder gate

Implementation tranche:
`crates/don-sim/src/systems/combat/cities_capture_plunder_gate.rs`.

Fidelity tier: **C**. This is an instruction-bounded recovery of 581 bytes of the
retail outer city-capture driver. It has static mutation/query proofs but no retail
differential promotion.

## Provenance and exact boundary

- `ron-bin/riseofnations.exe` SHA-256:
  `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`
- `ron-bin/sbl/rise.pdb` SHA-256:
  `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`
- PDB procedure: `int Cities::capture_city(int new_owner, int old_city, int old_owner)`,
  VA `0x00733380`, size 7,998 (`0x1F3E`), exclusive end `0x007352BE`, source
  `cities.cpp:87..733`.
- This tranche: `0x00733D67..0x00733FAC`, 581 (`0x245`) bytes.
- Cumulative recovered prefix: `0x00733380..0x00733FAC`, 3,116 (`0xC2C`) bytes.
- Exact residual: `0x00733FAC..0x007352BE`, 4,882 (`0x1312`) bytes.

`0x00733D67` is the first common instruction after the center-swap fork. The end is
the `test leader_flags,0x00400000` at `0x00733FAC`: the next CFG owns the first
persistent write (`leader_flags |= 0x00400000` at `0x00733FDC`) and the resource
transfer/presentation forks. No persistent leader, city, object, or UI mutation is
claimed here.

## Exact local and query order

Retail first writes three stack locals, in this order:

1. Russian plunder-steal gate = 0 (`[EBP-0x28]`).
2. notification-raised gate = 0 (`[EBP-0x34]`).
3. qualifying-government-general gate = 0 (`[EBP-0x18]`).

The model retains those writes as ordered local effects. It then resolves the new city
center and executes this conditional vtable/query sequence:

1. `SubObjectData::is_unit()` at vtable `+0x18`.
2. Only for a Unit, `ObjectData::is_hero()` at `+0xC4`.
3. Only for a Unit Hero, `ObjectData::is(THEDESPOT=0x160, 0)`.
4. Unless the center itself is The Despot, `SubObjectData::is_build()` at `+0x20`.
5. On that fallback, compute search extent as
   `(BuildTypeData[+0x234] + BuildTypeData[+0x238]) * 96` for a Build, otherwise 0.
6. If the center owner's Despot count is nonzero, call
   `HeroesData::find_hero(center.x, center.y, owner.who, 0, THEDESPOT, extent)` at
   `0x0073A1B0`.
7. Independently, if the new owner's Spitamenes count is nonzero, call
   `center.has_general(0, SPITAMENES=0x16D)` at `0x00646B00`.

Both named hero calls return an object index. Retail projects every nonnegative return
to true; they are deliberately not modeled as Boolean host calls. The center itself,
the Despot search, and the Spitamenes search are ORed into the one qualifying-general
local.

The PDB exposes `LeaderData::num_units` as 352 entries beginning at `+0x5762` while
unit TypeIndex values begin at 50. Consequently the raw prechecks have exact identities:

| Instruction field | Array slot | Effective TypeIndex |
|---|---:|---:|
| `LeaderData +0x59BE` | `num_units[302]` | `302 + 50 = 352`, `THEDESPOT` |
| `LeaderData +0x59D8` | `num_units[315]` | `315 + 50 = 365`, `SPITAMENES` |

The typed center receipt preserves every conditional query shape and attests the exact
query order. A missing predicate, an eager predicate from a branch retail skipped, a
changed type/mask/zero argument, or an incorrect wrapping search extent is rejected
before later host reads.

## Russian plunder-steal local

After the general queries, retail restores the old-owner `LeaderData*` and calls
`LeaderData::has_tribe_bonus(13)` (`0x006E1370`) on that old owner. It reads
`Constants::russian_plunder_steal` at `+0x770` only when that succeeds, and reads both
version words only when the rule is nonzero.

The packed versions are not compared as one numeric `u32`. Retail compares four
unsigned bytes from least significant to most significant. When the game version is
at most the shipped/global version, the Russian local becomes true. For a later game
version it becomes `!captured_own_capital`. The receipt's optional fields freeze those
short-circuit reads and the helper preserves the bytewise comparison.

## Plunder admission and cooldown

Retail next reads the old `CityData::city_flags` at `+0x04`:

1. When accumulated plunder is zero, flag `0x0010` (capital) is required.
2. Flag `0x0100` always skips the plunder CFG.
3. Otherwise retail reads `CityData::capture_stamp` at `+0x20`.
4. A zero stamp admits plunder without reading the game frame.
5. A nonzero stamp reads `Game::frame` at `+0x550`; signed wrapping
   `frame - capture_stamp <= 0x1194` (4,500) skips plunder.

The comparison is signed and inclusive. Both early flag exits skip the stamp and frame
reads. The typed owner/city identities supplied by the prior tranche prove the compiled
late `new_owner < 0` defense false; accepting a negative owner would already have made
the preceding global-array lookups invalid, so this seam uses `u8` rather than inventing
safe semantics for that corrupt state.

The two exact continuations are:

- `EnterPlunder0x00733fac`; or
- `SkipPlunder0x00734a3c`.

The latter is a forward edge outside this tranche, not a claim that the intervening
resource-transfer CFG has been modeled.

The handoff carries the prior swap receipt plus `old_city`, `new_owner`,
`captured_own_capital`, both derived plunder locals, the still-false notification local,
the city flags, and the optional capture stamp. A failed center-swap receipt has no valid
new city/center identity and is rejected at planning rather than turning retail's
unguarded `-1` array dereference into invented safe behavior.

## Validation and frozen residual

Focused tests privately path-import both this file and the frozen swap-fork file; no
shared combat export is added. They freeze the byte range, host-call order, three local
initializers, direct and fallback general paths, exact `find_hero` arguments, receipt
mutation rejection, Russian optional-field shape, bytewise version ordering, protected
city and zero-plunder early exits, and the inclusive signed cooldown edge.

Root convergence formatted the isolated Rust files and persvati job
`city-capture-plunder-20260809T231440Z-8229-17503-5f896c0277b5` passed all seven focused tests.
Retail was not run; the validation remains an isolated proof rather than a shared combat export.

The exact remaining body is `0x00733FAC..0x007352BE`, 4,882 bytes. It begins with the
old-owner leader flag/capital-capture fork and owns plunder sizing, good availability
classification, resource bucket mutations, localized notifications, capital and
recapture diplomacy, score/presentation effects, array cleanup, and the returned city
index. None of those effects are claimed here.
