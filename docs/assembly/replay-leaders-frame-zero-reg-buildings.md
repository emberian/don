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

## Activation high-water and registries

The same exhaustive transaction now owns another 642 walked bytes. `Build::activate`
`0x00624ba4..0x00624c16` calls `LeaderData::get_buildings` `0x006e0680` along the current
type's `BuildTypeData::to +0x2e0` chain, then writes the count to
`high_buildings[BuildTypeData::basic_type()]`. `basic_type` `0x00639970` recursively follows
`TypeData::from +0x3c`. Both links are extracted from the digest-bound replay Rules section;
out-of-range links and cycles refuse, and callers cannot supply a convenient root or count.
The one-Village census makes the recursive count one and every other high-water slot zero.

`reg_cities[64]` is independently recomputed with retail wrapping `u16` addition over the
Village, Town, Metropolis, and Forbidden City cells of `reg_buildings`. Its single nonzero cell
must agree with the constructor's `leader_region_cities_delta`. `reg_forts` and `reg_docks`
remain constructor-zero because the complete setup contains only Villages and never reaches
the Fort/Dock registry stores at `0x0073eb5f` and `0x00740b17`. No detached naval default is
used as evidence.

## Coverage and temporal boundary

Per active row:

```text
previous unique canonical walked bytes                 6,498
frame-zero reg_buildings plane                         16,512
frame-zero last_building_finished history                 516
pre-plan regional strategy histories                      256
high_buildings history                                    258
regional City/Fort/Dock registries                        384
canonical type-owner masks                               117
setup-surviving Leader::init scalars                       48
pre-plan strategy scratch and regional census           2,558
pre-gather rare-resource fixed history                    180
lifetime-invariant residual BitMask headers                56
setup-surviving stat/action histories                     104
setup-surviving diplomacy/CTW/repair stamps                24
starting-Village Leader counter history                    16
human-only zero Personality child                          92
canonical init-teams chat status                           32
frame-zero unique canonical walked bytes               27,651
default empty-child transcript                          28,428
frame-zero residual                                        777
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

The 642-byte activation cohort is historical too. Later upgrades, captures, destruction, and
Fort/Dock lifecycles require complete live maintainers; current counts cannot reconstruct a
high-water mark. It therefore expires at precisely the same setup boundary.

The canonical BHS type owner supplies another 117 dynamic bytes: the eight-byte `tech` mask
header and the complete 109-byte `obs_flags` header/payload. Its current `tech` payload is
duplicate-checked but not counted twice because the production-tech join already owns it. The
type owner is not yet mounted directly on `Sim`; this join therefore remains nested under and
expires with the setup receipt rather than claiming later-frame survival.

The same constructor boundary admits 48 fixed-body bytes written by `Leader::init`: `gov = -1`
at `0x006e3b0f` and eleven zero dwords spanning the gather, support, nuke, flock, weapon-use,
and technology-frame stamps at `0x006e3ba4..0x006e3bf9`. The ordinary setup receipt reaches no
gather/support action, weapon use, or technology completion before publication, so the exact
initialized bytes survive to that boundary. This is a setup-only historical claim, not a live
maintainer; it expires before the first world turn.

The constructor also clears a disjoint 2,558-byte strategy cohort which the first
`Leader::plan_strategy` pass immediately clears or recomputes. It contains the two 64-dword
rare-region arrays `[0x4d4,0x6d4)`, `filled_gather_slots[6]` `[0x8bc,0x8d4)`, 28 individually
enumerated plan-entry scratch dwords, `strategy` through `reg_attack` `[0xa68,0xe62)`, and the
naval/transport/peasant/gather regional census `[0xee2,0x125e)`. The entry list deliberately
excludes scalars that have another canonical owner or are not cleared there; the array ranges
exclude the already-owned `reg_pop` and City/Fort/Dock registries, and the unported territory
arrays. The 28 scratch and regional stores are at `0x006e49c8..0x006e4ba2`;
`filled_gather_slots` inherits the fresh negative-init bulk zero at `0x006e398c..0x006e3995`.
The first plan clears/recomputes are at `0x006b97cc..0x006b9c62`, with the strategy array
written later in that same pass at `0x006bbba1..0x006bbe70`. No setup action reaches these
writers between construction and the published receipt. Like every claim in this continuation,
it expires before the first plan.

Another 180 constructor-zero bytes survive until the first `Leader::calc_gather`: the
`known_rares` dword and `rares_collected[44]` at `[0x6d4,0x788)`. `Leader::init` clears them at
`0x006e4aba..0x006e4acb`; `calc_gather` first clears and recomputes them at
`0x006cef42..0x006cef6c`. Setup rare discovery mutates the separate dynamic `new_rares` list,
not this fixed history, and ordinary setup never calls `Leader::gather`. This claim therefore
expires before the first gather pass and cannot serve as a live maintainer.

Seven remaining fixed-size dynamic masks contribute 56 constructor-owned header bytes:
`tech_at_start`, three conquest masks, and three rare masks. Each visitor header is exactly
`{bits:i32,size:i32}`; the flags byte is not walked. Constructor stores at
`0x006d75b5..0x006d76ec` bind the fixed shapes (806/101, 17/3, 24/3, or 44/6), and every
runtime mutation touches only payload/dirty state. Even `Leader::close` clears payloads using
the retained bit counts without changing the headers. These 56 values are lifetime-stable, but
the composed receipt is still nested under setup-only authorities and remains uninstalled.

Three more disjoint fixed ranges contribute 104 setup-surviving history bytes. The 36-byte
`gather_slots_high`/trade/Fort/bribe range, 32-byte best-stat/war range, and 36-byte
garrison/nuke/attack range retain their exact `Leader::init` values through ordinary setup.
Every byte is zero except the `attacked_by` and `gov_hero_frame` sentinels, which are `-1`.
The complete setup transaction reaches no plan, gather-high update, trade, bribe, Fort build,
war census, garrison order, missile launch, combat, or government-Hero action. These are
historical constructor claims, not maintainers: the owner expires before the first relevant
plan/process/action writer.

The same fresh negative-init bulk clear owns the six contiguous dwords at `[0x1f4,0x20c)`:
three attrition/diplomacy stamps, two Conquer-the-World Hero stamps, and the repair stamp.
Ordinary non-scenario setup reaches no diplomacy action, CTW Hero action, or repair order, so
all 24 zero bytes survive the published receipt. The owner expires before any such action.

The complete starting-City chronology owns another 16 bytes. The fresh Village activation
increments `city_mine` and the setup caller increments `cities_built`; both are exactly one
per active row. The adjacent `village_num` and `village_mine` dwords retain constructor zero
because the exhaustive receipt reaches no writer for them. These counters remain historical
until a later City lifecycle event supplies a live maintainer.

All-human admission also closes 92 bytes of the raw 96-byte `Personality` child.
`Personality::init` at `0x006d8640` jumps to the complete zeroing body at `0x006d8650`.
For an active human row, `Leader::init` tests `flags & 0x0c == 4` at
`0x006e4cb9..0x006e4cc5` and jumps around the AI personality-selection body to
`0x006e4df1`. The already-owned `raid` dword is duplicate-checked rather than counted twice;
the other 92 exact zero bytes are newly canonical. This proof expires if an AI personality is
selected or any later personality mutator runs.

The canonical `Game::init_teams` transaction closes `chat_status[8]` without assuming a zero
row. The starting receipt retains both the complete pre-state and the exact post-state from
`0x0058c020..0x0058c184`; re-executing the deterministic product seam must reproduce its
ordered receipt byte-for-byte. Active rows therefore own the exact 32-byte roster/team-style
dependent result. Later diplomacy/team mutation expires this claim.

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
walk remains red and the global scoreboard remains uninstalled and non-substantive. It also
mutates the independent `high_buildings` tail and replay-source identity, proving that neither
the historical row nor its Rules provenance can be substituted. A tail mutation in the
canonical type owner's observation mask also refuses against the already-bound dynamic child.
The final byte of `tech_cat_frame[4]` is independently mutated as well, proving the entire
44-byte initialized stamp block must agree before the additional 48-byte cohort is admitted.
Independent mutations of the `attack` scratch dword and the final `reg_gather_slots` byte prove
both the sparse-scalar and contiguous-array halves of the 2,558-byte pre-plan cohort fail closed.
The final byte of `rares_collected[44]` independently refuses the 180-byte pre-gather join.
Existing malformed-mask tests change header bits/size and refuse before the lifetime header
receipt, while the focused census verifies all seven exact constructor shapes and byte counts.
Separate mutations of the final `gather_slots_high` byte and the `gov_hero_frame = -1`
sentinel refuse the 104-byte stat-history join. A nonzero `repair_stamp` independently refuses
the 24-byte action-stamp join. Clearing either the receipt-owned `city_mine` or `cities_built`
dword refuses the 16-byte City-counter join.
Mutating the final `alliance_ai` personality dword independently refuses the 92-byte human
Personality join.
Changing either the retained init-teams post-state or an independently imaged `chat_status`
byte refuses the 32-byte chat join.
