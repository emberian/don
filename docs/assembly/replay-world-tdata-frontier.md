# Replay World section-6 producer frontier

## Result

The earliest real producer of checksum-channel-12 section 6 is now isolated as an
executable replay source. `World::wipe` `0x006b2c00` writes **every** TData word to zero,
then clears `seen`, `seen2`, and `seen3`. It consumes no RNG. The new
`world_tdata_frontier.rs` adapter executes the existing `don-sim` port on a staged World,
checks the derived dimensions and all four plane lengths before mutation, checks the exact
section-6 walk image afterward, and returns a receipt with four explicit `written_ranges`
bound to the shipped producer VAs plus separate coalesced `changed_ranges`.

That distinction closes the source question. A zero which `World::wipe` explicitly rewrites
is sourced by the producer even though a before/after-difference ledger sees no change. No
recorded checksum is accepted as input, and this tranche does not register the source in the
shared owner ledger or initial replay schedule.

## Binary source chain

The supported executable is SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.

There are two zero producers, in schedule order:

1. `World::init(unsigned short,unsigned short)` `0x006b76f0` allocates and zeroes TData at
   `0x006b7854..0x006b7887`, then allocates and zeroes the three fog planes at
   `0x006b788a..0x006b78db`.
2. Every concrete shipped procedural map-style implementation enters through
   `World::wipe` `0x006b2c00` before its continent work. This later reset is the relevant
   last producer at map-generation entry and supersedes the allocation image.

The PDB gives `World::wipe` a 471-byte body ending at `0x006b2dd7`. Those exact 471 shipped
bytes hash to
`12f0886dfc5bff4dafb834a8e9d0f3743c4ba0ec8ba6024003712d81b01138f2`.
Capstone fixes the section-6 writes:

| output | instruction/call evidence | exact effect |
|---|---|---|
| `TData` | inner store `0x006b2d43`, loops bounded by `xs*4`, `ys*4` | write one zero `u16` for every tile |
| `seen` | call `0x006b2d5f` → `World::clear_seen` `0x006b2250` | zero `fog_size` bytes |
| `seen2` | call `0x006b2d66` → `World::clear_seen2` `0x006b2160` | zero `fog_size` bytes |
| `seen3` | inline `memset` setup at `0x006b2d75..0x006b2d80` | zero `fog_size` bytes |
| RNG | no call or access to `game_random` | zero draws |

A `.text` rel32 scan finds `World::wipe` called from 20 map-continent implementation
bodies, including every concrete shipped selector 6 through 22. The extra bodies are base,
grass, and conquest variants. This agrees with the already-receipted
`ContinentReceipt::world_wiped` boundary; it is not inferred from the resulting zeros.

## Exact walked-byte accounting

For an `edge × edge` World:

```text
tile_size = (4*edge)^2 = 16*edge^2 cells
TData     = 2*tile_size = 32*edge^2 bytes
fog_size  = (2*edge)^2 = 4*edge^2 bytes per plane
section 6 = TData + 3*fog_size = 44*edge^2 bytes
```

The receipt freezes four contiguous write ranges in the actual section walker order:
`TData`, `seen`, `seen2`, `seen3`. Each range carries the shipped store/call VA proving the
write. `section_bytes_written` is always the complete `44*edge^2`; `changed_ranges` may be
empty when the input was already blank. Ownership must come from the former, never from
equal zero values or the latter. Focused tests exercise both cases, malformed plane refusal,
derived-dimension refusal, the binary anchors, and all seven shipped map edges.

The current checksum-bearing corpus has 21 files distributed as two edge-60, five edge-70,
one edge-80, and thirteen edge-100 Worlds. The source contribution is therefore:

| plane | exact wipe-produced bytes |
|---|---:|
| `TData` | 5,379,200 |
| `seen` | 672,400 |
| `seen2` | 672,400 |
| `seen3` | 672,400 |
| **section 6** | **7,396,400** |

The distribution and total are independently reproducible from
`schema/replay-validation.json`:

```sh
jq -r '.files[] | select(.channels.world.compares > 0) |
  .initial.map_edge_world_cells' schema/replay-validation.json | sort -n | uniq -c
jq '[.files[] | select(.channels.world.compares > 0) |
  .initial.map_edge_world_cells as $e | 44*$e*$e] | add' schema/replay-validation.json
```

Against the 2026-08-11 localizer census, future ledger registration at the wipe boundary
would move exact World coverage from 345,647 to 7,742,047 bytes and reduce the current
model-boundary unknown count from 12,773,453 to 5,377,053. Those are source-coverage
quantities, not replay agreements. The retail checkpoint is later and contains subsequent
TData and fog writes.

## Remaining producer boundary

The wipe image is a baseline, not the completed initial World.

- The first later TData writes are group- and object-dependent. The current reconstruction
  reaches `TerrainGroups::place_all`; its active boundaries include
  `Mountains::add_mountain` `0x0089c2e0`, the Cliff placement path, forest/tree placement,
  and oil/resource object chains. Starting cities, resources, roads, rivers, and object
  footprints can also change TData before the first checksum. Each exact producer must
  replace the baseline owner for the bytes it writes.
- The ordinary fog planes stay at the wipe baseline until object/scenario visibility runs.
  The principal runtime producer is `Object::update_seen` `0x00651b80` through
  `GameDaemon::update_all_seen` `0x00732840`; it writes through `World::set_seen`
  `0x006b3c60` and `World::set_seen2` `0x006b4bb0`. Scenario `set_seen`/`set_explored` and
  incremental visibility paths are separate real producers. Reconstructing the turn-2
  checkpoint therefore still needs exact starting objects, LOS/detector facts, player masks,
  and scheduled fog execution.

The eventual shared hook is narrow but must be owned by convergence: register this new
module, replace the direct `world.wipe()` inside the continent transaction with an atomic
wipe adapter, and admit all bytes of `WorldSection::TDataAndFog` under a new producer-source
variant bound to the receipt and implementation identity. Existing changed-byte transitions
are insufficient because they intentionally ignore zero rewrites.

That integration remains deliberately red: the same retail call also clears section 7, but
the shared model does not yet do so. The adapter receipt exposes this as
`unreceipted_adjacent_write`, naming `WorldSection::WCoordSeen`, its exact byte count, and the
retail memset call. Convergence must either correct the shared `World::wipe` first or stage
and receipt sections 6 and 7 together in one atomic adapter; section 6 must not be attached
alone while the modeled call is missing an adjacent synchronized write.

## Adjacent finding: section 7 remains a separate gap

Retail `World::wipe` calls `World::clear_seen` at `0x006b2d5f`, and that callee also zeroes
`wcoord_seen` (`World +0x168`, checksum section 7) through the memset call at `0x006b22be`.
The current Rust `World::wipe` zeros the three section-6 fog planes but does not clear
`wcoord_seen`. This tranche does not edit the shared `map_terrain.rs` owner and makes no
whole-wipe claim; the section-7 correction needs its own focused test and must converge
atomically with section-6 owner registration.

## Claims not made

This is an isolated structural transcription, not a retail differential case and not a
fidelity-tier promotion. It does not claim a World-channel match, a first differing retail
byte, correctness after terrain/object/fog producers run, or completeness of the whole
`World::wipe` body. It does not use Adler-32 to invent bytes: the output bytes come from
explicit shipped stores, and Adler-32 is recorded only after the byte image exists.
