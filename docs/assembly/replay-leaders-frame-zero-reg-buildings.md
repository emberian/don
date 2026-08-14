# Replay Leaders frame-zero regional Building producer

The largest exact default Leaders residual was the 16,512-byte
`LeaderData::reg_buildings[64][129]` plane. This tranche derives the whole plane from the
canonical ordinary setup transaction, joins it to the complete conditional Leaders transcript,
and keeps the channel red. It is a frame-zero source receipt, not a claim that the current Sim
maintains the matrix after arbitrary Building mutations.

## Retail mapping

The PDB declares the walked field at `LeaderData +0x14de` as 8,256 `u16` values. The executable
initializes it as 64 consecutive region slices of 129 Building types. `Wall::increment_stats`
at `0x00643270` pins the live write after its active-Wall gate:

```text
006432d2  owner = WallData::who
006432df  type  = current ObjectData::ptype TypeIndex
006432e2  ++num_buildings[type]
006432ea  if region >= 64: skip regional store
006432f9  index = region * 129
00643301  index += type
00643307  ++reg_buildings[region][type - 414]
```

The final address uses a base 414 types before the PDB field, so raw Building TypeIndex 414
lands at field slot zero. The executable's actual checksum/storage order is therefore:

```text
flat index = region * 129 + (TypeIndex - 414)
byte offset = 0x14de + flat index * 2
```

The binder exposes the executable order rather than relying on the visually ambiguous PDB
declaration `unsigned short[129][64]`.

## Complete source census

`StartingSetupState` admits only ordinary, human, unteamed, starting-town-one games whose City
center positions and regions are source-proven. For the currently admitted all-land Old World
and Himalayas setups, the canonical transaction contains exactly one active Village Build per
active owner:

- the first replay Camera command binds the center coordinate to `Player::play` and Leader;
- `spawn_canonical_build` owns the Build row, object id 2000, current ptype and registry entry;
- the City constructor reads region 1 from the produced WData cell and links that exact Build;
- the staged Sim Build count equals the receipt count, with no unaccounted Build rows;
- starting-town two is refused before its Woodcutter/Farm/Library continuation.

The producer rechecks all of those joins. It also requires the active roster to agree across
step 8, victory state, setup receipts, and the conditional transcript. Each active row therefore
has one at `region 1, Village slot 0` and source-backed zeroes in every other matrix cell. An
extra/missing Build, changed owner/type/object/city/position/region, duplicate owner or stale
conditional byte refuses the receipt.

Source-backed zeroes are justified by a complete census, not by allocating a zero-filled
sidecar. The distinction matters: the same binder refuses as soon as the staged Sim contains a
Build which the setup receipts do not enumerate.

## Coverage and temporal boundary

Per active row:

```text
previous unique canonical walked bytes                 6,498
frame-zero reg_buildings plane                         16,512
frame-zero last_building_finished history                 516
pre-plan regional strategy histories                      256
frame-zero unique canonical walked bytes               23,782
default empty-child transcript                          28,428
frame-zero residual                                      4,646
```

Inactive rows still walk and own only their eight-byte header. Dynamic child payloads extend
the denominator and residual. After the setup boundary expires, this producer expires too and
the general same-frame lower bound returns to 6,498 bytes with a 21,930-byte default residual.
Promotion beyond frame zero requires the canonical Sim to execute every increment/decrement,
capture/type-change and destruction path which maintains both aggregate and regional Building
counts. The adjacent 516-byte `last_building_finished[129]` setup receipt is narrower: retail
initializes all entries to `-1`, and ordinary setup's `Build::activate(0, 0, 0)` bypasses the
only activation-time completion-id store. Later completed Buildings still require a live owner,
so that history promotion expires at the same boundary.

The same temporal proof owns another 256 bytes. `Leader::init` clears the four 64-byte
`reg_attacked`, `reg_wars`, `reg_neutrals`, and `reg_allies` arrays; their first writers are
the later `Leader::plan_strategy` pass. The constructor receipt's explicit pre-first-checkpoint
state therefore promotes all four zeroed arrays and expires before that pass.

`checksum()` consequently still returns the complete frontier as an error and
`installed_in_scoreboard()` is false. The replay scoreboard remains:

```text
leaders compares              222,938
leaders matches                     0
leaders substantive compares        0
leaders best survived turns          0
installed                         false
```

No Cities scalar census is duplicated here. City ownership is used only to validate the
center-Build identity and region join; the Cities channel's frame-zero scalar continuation stays
in its own producer lane.

## Mutation gates

The focused test derives the census from a real admitted setup, checks the exact PE addresses,
layout, one-hot rows and byte accounting, then binds all 16,512 bytes per active row. Clearing
the independent conditional Village cell refuses at the first differing byte. Mutating the
canonical Build type while leaving its setup receipt stale refuses during census derivation, as
does appending an unreceipted Build row. Mutating the independent final byte of
`last_building_finished` refuses the history join. Dense and sparse object-registry views are
rechecked by the producer before these mutation gates. Mutating the independent tail of
`reg_allies` likewise refuses the pre-strategy join. The same test verifies the complete Leaders
walk remains red and the global scoreboard remains uninstalled and non-substantive.
