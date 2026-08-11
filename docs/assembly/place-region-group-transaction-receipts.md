# `TerrainGroup::place_region_group`: transaction receipts and owned leaves

This note freezes the replay-facing transaction at `0x006a2f60` against the
supported PE32 image (`ron-bin/riseofnations.exe`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`).
The shipped PDB names the function

```text
public: enum Liberr __thiscall TerrainGroup::place_region_group(...)
VA 0x006a2f60, code size 4,647, terraingroups.cpp:809..1318
```

The PDB establishes the identity and source signature, not the behavior below.
Behavior was read from the shipped instruction stream with LLVM objdump and
Capstone and cross-checked against `re/decomp-all/006a2f60.c`.

## What was already present

Before this tranche, the repository already contained and reached the complete
deterministic control around `drop_tile`:

- `terrain_region_placement.rs` owns the entry-region LFSR, all candidate
  filters, and the optional `Random::get(0, 0xffff)` at `0x006a2fe1`;
- `terrain_drop_tile.rs` owns the forest/rocks branches and produces typed
  requests for Mountains, oil Goods, and Cliffs;
- `terrain_region_continuation.rs` owns the resumed LFSR, helping-score update,
  growth loop, `randomize_orthogs`, clear/retry path, and type-6 oil tail;
- `terrain_region_patterns.rs` owns patterns 1--3 and their mountain cursor
  transactions.

This matters because deriving another prefix would not move replay. The first
unowned operation on the dominant Mediterranean path is inside the already
reachable `drop_tile` call.

## Instruction anchors

| VA | shipped operation | receipt consequence |
|---|---|---|
| `0x006a2fa7` | clear `TerrainGroup::tiles.length` | temporary group state; not channel 12 |
| `0x006a2fc9..0x006a2ff3` | draw a start cursor only when region length is greater than one | `prefix.region_cursor_draws` |
| `0x006a397a` | first `TerrainGroup::drop_tile` | first possible world/external mutation |
| `0x006a3cee` | growth base-index draw | one word only when tile-list length is greater than one |
| `0x006a3d15` | `randomize_orthogs` | exactly two words per visited base; the diagonal word is consumed even though this caller does not use it |
| `0x006a4091` | growth `drop_tile` | embeds the branch's reported draw count |
| `0x006a40b9` | type-6 `place_oil_deposits` | zero main-stream words |
| `0x006a4159` | failed-growth `clear_group` | zero main-stream words |

The enclosing exact draw count is therefore:

```text
region_cursor_draws
+ sum(entry/retry drop draws)
+ sum(for each growth pass:
       (base_count > 1)
       + 2 * visited_bases
       + embedded growth-drop draws)
```

Replaying that many LCG steps from the recorded entry word must produce the
recorded exit word. `PlaceRegionGroupReceipt::rng_receipt_is_coherent` checks
that invariant.

## The first external operation

`TerrainGroup::drop_tile` dispatches group type 5 to
`Mountains::add_mountain` `0x0089c2e0`. The request's nine source arguments are
preserved exactly: template, WCoord pair, verification pattern, four spacing
values, and `start_min`. `add_mountain` consumes **no main RNG word**; its
`Liberr` decides whether the tile is considered placed.

This is the first real stop for Mediterranean (map style 12), whose selected
group zero is a non-player mountain group. Great Lakes (style 14) first reaches
`World::set_oil_at` `0x006b2a10`, then reaches the same mountain leaf at group
two. A bare `DropTileExternalResolution` remains evidence of a caller answer,
not evidence that either external owner executed.

The exact owner implementations remain separate authorities:

- mountain geometry, verify-bit scratch, WData/TData writes, behind flags, and
  retained location arrays belong to `mountain_add_runtime`;
- Good slot closure/reuse/growth, `good_mark`, Goods checksum/save state, and
  the WData oil bit belong to `world_oil_goods`.

`apply_place_region_group_owned_audited` now calls both owners before replacing
a typed request with its internal resolution. The enclosing call clones Group,
World, RNG, and both owner states; it commits all four only on a native
`Returned(_)` outcome. A Cliff boundary or any typed owner refusal installs
none of the staged state. In particular, it cannot manufacture `liberr = 0` or
treat the void oil call as world-bit-only success.

The mountain adapter translates all nine request fields one for one into
`AddMountainCall`. Its leaf receipt carries the native `Liberr`, rejection,
zero-RNG assertion, exact channel-12 World delta, and before/after Adler plus
byte count for the four retained arrays walked by `Mountains::walk_data`.
The oil adapter translates all six request fields one for one into
`OilGoodMutation`. Its receipt retains closed/reused/appended slot identity,
PtrArray metadata and `good_mark`, active save rows, Goods-channel checksums,
World checksums/planes, and the zero-RNG assertion. The Goods checksum walks
the full PtrArray logical length; `good_mark` separately bounds oil lookup and
scenario-save rows.

The older recorded-resolution entry remains available for evidence replay. It
reports what a caller supplied but deliberately does not promote that row into
proof that either owner executed.

## Exact world receipt

Every exit from the explicit `apply_place_region_group_audited` entry point now
records:

- the complete channel-12 checksum before and after;
- the isolated `World::walk_data` sections whose digest or walked byte count
  changed;
- every changed byte as `(walk_offset, before, after)` in the exact channel-12
  stream;
- every physically changed WData and TData index;
- entry/exit RNG words and the exact draw count.

The ordinary `apply_place_region_group` path retains the cheap RNG receipt but
sets the World receipt to `None`; it does not clone and walk the entire World for
every clump in normal `place_all`. Replay localization opts into the audited
entry point at the boundary where those bytes are needed.

The walk-offset receipt is intentionally not a Rust-memory diff. Retail walks
only 21 of each WData record's 28 bytes and inserts `Array` metadata in other
sections, so a memory offset would be the wrong coordinate system for replay
localization.

## Gates and fidelity boundary

`map_core_region_group_transaction_receipts.rs` is mutation-sensitive to the
draw predicate/count, orthogonal two-word cost, changed walk sections, physical
plane indices, and the full byte-delta set. It also pins:

- the recorded Mediterranean mountain stop as zero hidden RNG/world mutation;
- an owned Mediterranean commit through World, TData, retained mountain arrays,
  and both enclosing/leaf walk receipts;
- a native mountain verification rejection as a committed `Returned(0)` with
  no owner mutation, distinct from a typed runtime refusal;
- an owned Great Lakes oil commit through a real Good slot, Goods checksum, and
  scenario row;
- complete rollback for a missing displacement template; and
- complete rollback when an oil-owner refusal happens after the rocks branch
  already mutated staged World state.

This is static/in-tree evidence (Tier C). Mode-4 `Mountains::add_mountain` and
mapgen `World::set_oil_at` are now locally owned, but the shipped displacement
template catalog producer is still a typed red boundary: production must not
substitute the one-cell fixture used only by mutation tests. Cliff positioning
also remains an explicit typed boundary. The oil owner deliberately refuses a
nonnegative WData `down` chain until the general object bands can participate.
Full `place_all` replay compatibility still requires those producers plus a
replay-produced checksum comparison; no agreement is fitted here.
