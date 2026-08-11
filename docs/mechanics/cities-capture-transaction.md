# Cities capture transaction braid and first Sim owner

## Scope and evidence

`Cities::capture_city` is now registered and braided as one contiguous public body:

| tranche | address range | bytes |
|---|---:|---:|
| prefix | `0x00733380..0x00733749` | 969 |
| center-swap fork | `0x00733749..0x00733D67` | 1,566 |
| plunder gate | `0x00733D67..0x00733FAC` | 581 |
| capital-plunder award | `0x00733FAC..0x00734152` | 422 |
| local award notification | `0x00734152..0x0073432D` | 475 |
| residual | `0x0073432D..0x007352BE` | 3,985 |

The sum is the PDB procedure extent, 7,998 bytes. The braid in
`cities_capture_transaction.rs` adds no mechanics. It applies the six existing plan/apply
contracts in address order and routes only their typed continuations:

```text
prefix -> swap -> gate
                  | SkipPlunder -> residual
                  ` EnterPlunder -> award
                                     | LocalAward -> local UI -> residual
                                     ` refund/alternate ----------> residual
```

The gate's `old_city`, `new_owner`, and `captured_own_capital` inputs are derived from the
retained prefix receipt, not accepted again from a caller. Success-only swap facts are
requested from the combined host after the prefix receipt fixes the center-swap branch.
This prevents a host from taking the later snapshot before prefix mutations.

The retail failed-center-swap arm remains deliberately fail-closed in the public braid.
The swap tranche can describe its `0x00733D67` convergence, but it has no new City or
center; the following retail code dereferences those values without a safe language-level
equivalent. The plunder-gate planner therefore returns `MissingNewCity`/`MissingNewCenter`
instead of inventing corrupt-state behavior.

## Canonical ownership audit

A complete `Sim` host cannot yet be joined honestly. Relevant live owners are split but
real:

- City state: `tech_cities::CityPool`, including checksum-exact `CityRecord` and eight
  `city_mark` values.
- Build state and identities: `Sim::builds`, `World::objects`, and
  `LiveProductionRuntime::build_types`.
- diplomacy, capital timers, type availability, score, and population cap:
  `Sim::vic_leaders`;
- economy buckets: `Sim::leaders[*].econ` (mirrored into the exact step-8 owner at its
  scheduled boundary);
- tribe-bonus and exact step-8 leader queries: `Sim::step8`;
- console identity: `Sim::players` (`lifecycle_host::PlayerTable`);
- army city targets: `Sim::armies`.

The first missing prefix owner was also recovered without adding a parallel leader model:
the PDB-backed `cities_captured`/`cities_lost` fields at `LeaderData +0x824/+0x828` now
live on the existing `victory_score::LeaderState`. The Sim owner applies the exact wrapping
increments there (`inc dword` at `0x0073353F`/`0x00733530`). DoNSave v11 predates these
fields and explicitly refuses nonzero values.

The first missing City join was likewise concrete and pre-existing: `CityPool` existed but
`Sim` did not own one. `Sim::cities` now mounts that exact type and `Sim::channel_digest`
includes `tech_cities::check_cities`. Save serialization refuses a non-pristine pool until
a Cities chunk is added; it never silently drops live cities.

`cities_capture_sim_owner.rs` implements the first nested mutation against those canonical
stores: `City::capture` `0x00736C40`. It resolves both centers through the live Build band,
reads the old checksum record, derives `CaptureContext` from `vic_leaders`, writes the
reserved new-owner City slot, and commits the new center's `damage = hits(0) - 10`.
The ECX residue passed to `Armies::update_city` is also recovered rather than guessed:
`0x0073701F` computes `hits - 10` in ECX, `0x00737026` stores it, and the callee returns;
`Cities::capture_city` pushes ECX at `0x00733781`.

## Remaining host blockers

No closure promotion is made. The first successful real-Sim owner test reaches the public
combat City-record call and mutates `Sim::cities` plus `Sim::builds`, but there is not yet a
full Build-check/damage-path test. The next authoritative joins are:

1. `Build::swap_team`, `Build::close`, `Wall::mask_me`, and dependent Unit swaps have no
   one live adapter spanning `ObjectRegistry`, `BuildData`, sparse identity, and type facts;
2. `LeaderData::{find,lost,recapture}_capital` have typed call sites but no shared body;
3. local capture presentation has command/product owners, but no `Sim` presentation sink
   with the String/TextBubble/MessageWin transaction represented by the local tranche;
4. Cities and capture-counter save/load persistence is not yet implemented, so save rejects
   either live owner.

Filling these owners, then running `Build::check_capture -> Cities::capture_city` through a
real `Sim`, is the condition for promoting the body from public assembly to closure. A
test-only shadow city/resource/presentation store is not an acceptable substitute.

## Validation

The public library compiles with all six modules registered simultaneously. An external
public-import test pins every contiguous seam and the 7,998-byte sum, avoiding the private
path-import type copies used by the older tranche tests. Focused tests cover those tranches
plus the new real-Sim owners. The Sim tests prove wrapping leader counters, a hostile
capture's checksum record, center coordinates, race preservation, `hits - 10` damage/ECX
residue, digest visibility, pre-mutation refusal of an unreserved destination, and explicit
save refusal for both not-yet-serialized owner additions.
