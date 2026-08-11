# Replay initial Goods prefix and oil adapter

## Result and boundary

`crates/don-replay/src/replay_goods_initial.rs` owns the earliest exact
`PtrArray<Good>` state used by procedural replay reconstruction and adapts typed
map-generation oil requests to the canonical `don-sim` Good owner. It retains
slot order, holes, `ObjectsData::good_mark`, engine capacity history, the
21-byte active-Good checksum rows, and the complete World/oil placement receipt.

This is a bounded prefix, not a channel-11 compatibility claim. A replay does
not serialize the initial object graph. `TerrainGroups::place_all` can create
type-5 Oil Goods, and the later `Map::place_resources` (`0x0068f4f0`) creates
non-oil Goods. Until both schedules complete, `check_goods` remains deliberately
uninstalled in the replay checksum bridge. No recorded checksum is an input and
no value was fitted.

Authority is the supported `ron-bin/riseofnations.exe`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`,
and its GUID-matched PDB. Names and layouts come from the PDB; the stores and
order below come from the PE32 instruction stream.

## The exact post-`Objects::init` state

The global `PtrArray<Good> goods` begins at `0x00c0a0e0`. Its relevant fields
are vftable `+0`, length `+4`, capacity `+8`, increment `+0xc`, list `+0x10`,
flags `+0x14`, and iterator cursor `+0x18`. The supported PE's cold `.data`
image at `0x00c0a0e0..0x00c0a0f8` has length 0, capacity 0, increment -1,
null list, zero flags, and zero cursor. The dynamic initializer at `0x00402610`
only installs the vftable and registers the destructor.

`Objects::init` (`0x0065ea80`, PDB size 2,292) then executes:

| instruction | exact store |
|---|---|
| `0x0065eb74` | goods length = 0 |
| `0x0065eb7e` | increment = -1 |
| `0x0065eb85` | flags = 0 |
| `0x0065eb6d..0x0065ebb3` | retain a nonnegative capacity; free/reset only a negative one |
| `0x0065f29f` | call `Objects::clear` (`0x0065d740`) |

`Objects::clear` stores `rare_mark = 0` at `0x0065d7c3` and `good_mark = 0`
at `0x0065d7c9`, then sets the Goods length to zero at `0x0065d867`. In this
specific caller the earlier `0x0065eb74` store already made its close/delete
loop empty. Retained allocation memory can therefore contain stale pointer
words outside the zero logical length, but they are neither rows nor holes and
the next append overwrites the applicable pointer slot. The post-init state has
no active logical rows and no inactive holes below `good_mark`, regardless of a
retained nonnegative capacity.

The `.rcx` carries neither an allocator-capacity nor iterator-cursor fact, and
`Objects::init` does not rewrite the cursor at `+0x18`. The runtime therefore
has two explicit constructors:

- `cold_process`, whose capacity 0 and cursor 0 come from the supported PE's
  cold image;
- `from_measured_retained_storage`, whose nonnegative capacity and cursor must
  come from a separate observation and are labelled as such.

Both start with length 0, increment -1, flags 0, `good_mark` 0, no holes, no
walked rows, and Goods Adler-32 value 1. Capacity and cursor do not enter
`CheckSums::check_goods`, but they change allocation and generic-save receipts,
so erasing their provenance would still be wrong.

## Oil placement composition

`ReplayInitialGoodsRuntime::resolve_oil_request` accepts only the exact
`DropTileExternalRequest::OilGoodMutation` produced by terrain placement. It
copies all six fields into `world_oil_goods::OilGoodMutation`, calls
`apply_world_set_oil_at`, retains that execution receipt, and returns the typed
`OilGoodsApplied` resolution only on success.

The underlying owner is documented in `docs/mechanics/world-oil-goods.md` and
derives from these functions:

| function | VA |
|---|---:|
| `World::set_oil_at` | `0x006b2a10` |
| `Objects::init_good` | `0x00653f30` |
| `Good::close` | `0x0066d860` |
| `Good::init` | `0x0066da20` |
| `CheckSums::check_goods` | `0x00937710` |

Each adapter receipt adds its ordinal and the inactive-hole count before and
after the transaction to the canonical owner's existing allocation, close,
World-section, checksum, walked-byte, scenario-row, and zero-RNG receipt. A
failed or non-oil request changes neither World, Goods, nor receipt history.

The hole rule is load-bearing. `Objects::init_good` scans the full logical
length from slot zero and reuses the first inactive row. `Good::close` does not
lower `good_mark` or compact storage. Thus disabling slot zero below a mark of
two creates one checksum-skipped hole; the next allocation reuses slot zero and
the active checksum order becomes the new slot-zero row followed by slot one.

The three bounds must not be conflated. `CheckSums::check_goods` compares its
cursor against `goods.length` at `0x00937713` and `0x00937774`, then skips rows
whose active bit is clear. `World::set_oil_at` compares against `good_mark` at
`0x006b2a46`/`0x006b2aab`, and scenario save does the same at
`0x009a7399`/`0x009a73c3` and `0x009a744b`/`0x009a7572`. The canonical runtime
enforces the initializer invariant that no active row exists at or above
`good_mark`, so scanning either bound yields the same active 21-byte rows in
this prefix; only the checksum walk itself is accurately described as the
logical-length scan.

## Corpus audit

The path-mounted test reopens the local replay corpus, selects recordings by
the presence of `CheckSumsCommand` `0x39`, and reads each first recorded Goods
word. The 2026-08-11 local run found 21 checksum-bearing recordings out of 61;
all first checkpoints were turn 2, all 21 Goods values were nonempty, and all
21 values were distinct. `WorldSim` installed no Goods producer and walked zero
Goods bytes in the independently regenerated validation report. These recorded
values are used only as a falsification boundary. The derived empty post-init
value and any oil-only prefix are therefore not promoted merely because their
bytes are exact.

## Integration hook and remaining work

No shared file is edited by this tranche. Integration needs:

1. one `pub mod replay_goods_initial;` in `crates/don-replay/src/lib.rs`;
2. one `ReplayInitialGoodsRuntime` created at the `Objects::init` boundary and
   retained alongside replay map generation;
3. its typed resolver passed through the canonical owned `place_all` path, so
   oil execution receipts replace echo-only acknowledgements;
4. the same sparse runtime extended through exact `Map::place_resources`
   `Objects::init_good` allocations;
5. only after the whole initial Good-producing schedule completes, a
   `SimState`/`check_all` installation using active rows in slot order.

Still red are complete `place_all`, the two large resource-placement bodies,
their non-oil Good initialization/footprints, later dynamic Good mutations, and
whole-channel replay agreement. The prefix is static/in-tree Tier C: instruction-
derived and mutation-tested, with no retail differential run.
