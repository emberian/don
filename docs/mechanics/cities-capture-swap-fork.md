# `Cities::capture_city` swap fork: city members and allied units

Implementation tranche:
`crates/don-sim/src/systems/combat/cities_capture_swap_fork.rs`.

Fidelity tier: **C**. This is an instruction-bounded recovery of the next 1,566 bytes
of the retail outer city-capture driver. Nested mutations use identity-bound receipts.
No retail differential run has promoted it.

## Provenance and exact boundary

- `ron-bin/riseofnations.exe` SHA-256:
  `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`
- `ron-bin/sbl/rise.pdb` SHA-256:
  `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`
- PDB procedure: `int Cities::capture_city(int new_owner, int old_city, int old_owner)`,
  VA `0x00733380`, size 7,998 (`0x1F3E`), exclusive end `0x007352BE`, source
  `cities.cpp`.
- This tranche: `0x00733749..0x00733D67`, 1,566 (`0x61E`) bytes.
- Prior prefix plus this tranche: `0x00733380..0x00733D67`, 2,535 bytes.
- Exact residual: `0x00733D67..0x007352BE`, 5,463 (`0x1557`) bytes.

The start is the load and compare of the center `swap_team` result. A result of `-1`
enters the failed census at `0x00733CE2`; every other value enters the successful city
record/member path at `0x00733755`. Both arms converge at `0x00733D64`; `0x00733D67`
is the first instruction of the next common phase.

Named nested calls are fixed by the PDB:

| VA | PDB procedure | Role in this tranche |
|---|---|---|
| `0x00736C40` | `void City::capture(City*, int, int, int)` | copy/close the old city record into the new city |
| `0x006F2D70` | `void Armies::update_city(int,int,int,int,int)` | rewrite army city owner/object pairs |
| `0x007384C0` | `void City::find_buildings()` | rebuild the captured city's building bookkeeping |
| `0x0046CFF0` | `int vector_dist(int,int)` | test nearby-unit locality |
| `0x0065BA90` | `ObjectsData::find_city(...) const` | prove which city owns a nearby unit's location |

## Successful center-swap arm

Retail first calls `new_city.capture(old_city, new_city_index, new_owner, new_center)`.
It immediately calls `Armies::update_city(old_center, old_owner, new_center, new_owner,
ECX-residue)`. Disassembly of `Armies::update_city` shows that its fifth stack argument
(`[EBP+0x18]`) is never read. The model nevertheless retains it as the opaque typed
`CityCaptureEcxResidue`; it is not assigned a fabricated meaning.

The army routine scans players in ascending order and each army list in array order. For
every entry whose city pair is exactly `(old_center, old_owner)`, it writes
`(new_center, new_owner)`. When the new owner equals the currently scanned player it also
ORs army flag `0x80`. The receipt attests completion of that nested synchronous tail.

Only after both calls does retail walk `old_center.get_build()->city_down`, following each
captured building's next `city_down` link. For every member, in chain order:

1. If `BuildTypeData::build_flags +0x2C0` has bit `0x2000`, skip the member completely.
2. Otherwise append the old member identity to the old-object SimpleArray.
3. Ask `ObjectData::is(FARM=0x1A1, 0)`, then, only when false,
   `ObjectData::is(GRANARY=0x1A7, 0)`.
4. A Farm or Granary is not swapped when the old owner has Lakota bonus 19 (`0x13`).
5. All other candidates must satisfy the virtual active-build predicate (the known Build
   vtable fast path is object `flags & 4`).
6. Call `swap_team(new_owner)`. A negative result retains only the old-array append.
7. A successful new identity is activated with `(1,1,0)`, appended to the new-object
   SimpleArray, and adds exactly 25 to the wrapping plunder accumulator.

Thus member activation occurs before the new-array append and before the 25-point plunder
addition. Old-array append occurs before every capture predicate and remains committed on
swap failure. The `0x2000` member flag is not the failed-arm flag below.

After the linked walk, `City::find_buildings()` runs even if no member swapped.

## Guarded circle/unit pass

The circle pass exists only when either `old_leader.who == new_owner` or both diplomacy
directions equal 2. The ordinary capture case therefore requires a mutual relation. Its
host receipt must attest the exact retail `CIRCLE_COUNT[ring]`, signed X-offset, and signed
Y-offset table walk in circle-entry order, followed by each tile's `ObjectData::down`
chain order. Out-of-bounds coordinates are omitted without reordering later ordinals.

The ring is the prefix's old-owner value `(min(old_radius,64)+3)/4`. The separate distance
gate recomputes the center radius after `City::capture` under the new owner, clamps only
values above 64, and compares against:

```text
min(new_owner_center_radius, 64) * 3 * 64
```

The multiplication is wrapping signed arithmetic and there is no low clamp. Distance is
retail `vector_dist(abs(unit.x-center.x), abs(unit.y-center.y))`.

Candidates retain only old-owner objects whose virtual predicates say valid Unit and
on-map. The compiled CFG also admits type indices `0x32..0x35` (PEASANTS,
PEASANTSKOREAN, SCHOLARS, SCHOLARSKOREAN) when the relation predicate is false; the outer
relation gate makes that alternate arm unreachable in this tranche, but the model keeps
the predicate rather than deleting it. When `old_owner.city_num <= 1`, the relation gate
bypasses both distance and `find_city`. Otherwise distance must pass and the exact
nine-argument `ObjectsData::find_city` query must return the new city. Its typed receipt
freezes the raw argument tuple `(unit.x, unit.y, 1, new_owner, unit.y, 0, 0, 0, 0)`;
notably Y is pushed twice. Retail repeats the relation test immediately before virtual
`swap_team(new_owner)`.

Nearby unit swaps do not activate the result, append either city-object array, or change
the plunder accumulator. Their only effect in this tranche is the typed swap receipt.

## Failed center-swap arm

Failure performs no `City::capture`, army update, `find_buildings`, or circle pass. It
walks the old center's `BuildData::city_down` chain. For each member whose type flags lack
bit `0x10`, retail appends the old identity and then adds 25 plunder. Bit `0x10` skips both.
The arm then converges at the same `0x00733D67` continuation.

## Typed boundary and validation

`CitiesCapturePrefixReceipt` is consumed without recomputing its center result, object
arrays, city identity, ring, or initial `(level-1)` plunder. Success requires exactly one
new-center identity and one new city. Failure requires a distinct ordered census snapshot.

Every mutating nested call returns a request-bound receipt. Successful object swaps must
prove the new identity belongs to `new_owner` and affirm their ownership/copy effects.
`City::capture`, army update, and `City::find_buildings` must affirm synchronous completion.
The member-chain host receipt is deliberately requested after the army call on success
(and as the failed arm's first operation), so it does not precompute a chain that
`City::capture` could have changed.
The mutation receipt records local array/plunder operations among host calls so their
relative order is testable rather than merely described.

Root convergence validated the path-imported proof without adding a shared combat-module
export. The first compile exposed only the focused test's private module-resolution seam;
after that was corrected, both validation modes passed:

- hbox debug: `city-capture-swap-v2-20260809T224301Z-83636-30566-ef54ec2d1035`, 5/5 passed;
- persvati release: `city-capture-swap-release-v2-20260809T224303Z-83652-27096-ef54ec2d1035`, 5/5 passed.

The isolated module and focused test were normalized with `rustfmt`; neither validation
job ran the retail executable.

## Frozen residual

The exact remaining body is `0x00733D67..0x007352BE`, 5,463 bytes. It starts with a second
new-city member traversal and owns the later plunder/resource transfer, capital and
recapture diplomacy, score/presentation effects, array cleanup, and returned city index.
None of those effects are claimed here.

Shared integration handoff only:

```diff
 pub mod cities_capture_prefix;
+pub mod cities_capture_swap_fork;
```
