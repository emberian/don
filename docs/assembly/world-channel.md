# Lane report — `world-channel`

Wave: assembly. Ranked item #5 — reconcile channel 12 (`world`) and make the checksum
primitive singular. 2026-08-08.

---

## What now runs that did not before

**One walker owns channel 12, and the code that writes territory and fog writes into it.**

`crates/don-sim/src/systems/map_terrain.rs` and `crates/don-sim/src/systems/borders_fog.rs`
both declared themselves the `world` channel and each kept its own copy of the state. That
is why neither could be validated: `borders_fog::check_borders` wrote territory into a
private `WDataPlane`, `map_terrain::World::checksum` hashed a different `Vec<WData>`, and no
harness could read one and see the other. **`map_terrain::World` won.** `borders_fog`'s
duplicate storage and duplicate walker are deleted; every function there that touches
checksummed state now takes `&World` / `&mut World`.

Concretely, these are now true and are pinned by tests:

* A border pass moves the `world` channel. `check_borders(&mut regions, &mut world, …)`
  writes `WData::who` / `who2` and `World::checksum()` changes — and changes in **section 5
  only** (`a_border_pass_moves_section_5_of_the_world_channel`).
* A fog stamp moves the `world` channel. `update_seen(&fog, &mut world, …)` moves sections
  5, 6 and 7 and nothing else, with byte counts unchanged
  (`a_fog_stamp_moves_sections_5_6_and_7`).
* The walker takes the engine's own section argument. `World::walk_section(w, sec)` models
  `World::walk_data(DataWalk*, int)` `0x006b5cf0`'s thirteen `if (sec < 0 || sec == N)`
  guards, and `walk(w)` is `walk_section(w, -1)` — the value `check_all` passes.
* Per-section digests come off that same traversal, not a parallel one.
  `World::checksum_sections()` returns 13 `SectionDigest{adler, bytes}` plus `full`, and
  `differing_sections()` names where two worlds diverge. Proven identical to the whole-channel
  walk byte-for-byte by `the_thirteen_sections_concatenate_to_the_whole_channel`.
* **`don_sim::checksum::adler32` is differentially tested against retail.** The oracle case
  `adler32` used to point at `oracle::models::adler32`, a copy transcribed into the oracle,
  so it proved the copy. It now points at the single function every checksum walker in the
  workspace calls.

## Measured

| what | number | method |
|---|---:|---|
| `adler32` implementations of the arithmetic, before | **16** | enumerated: 9 free `pub fn adler32` in `don-sim/systems/{ammo, borders_fog, economy, groups_guys, items, movement, production, tech_cities, victory_score}`, 2 streaming structs (`map_terrain::Adler32`, `combat::Adler32`), 4 in `don-replay` (`checksum.rs` + `rules_channel` / `scenario_channel` / `script_channel` streaming), 1 in `oracle::models` |
| after | **1** | `crates/don-sim/src/checksum.rs:64`. `grep -rn '^pub fn adler32' crates/` returns that line plus `adler32_or_null`, which is the null-pointer arm delegating to it. Everything else is a `pub use` or a one-line call |
| retail differential trials on the shipped primitive | **100,013 trials, 0 mismatches** | `tools/oracle-regress.sh`, `adler32` case at `0x00a46830`, model `don_sim::checksum::adler32`, 12 boundary lengths straddling the 16-byte unroll and `NMAX = 5552` + 100,000 uniform + the `NULL` case |
| full oracle suite after the repoint | **13 pass, 0 fail, 0 skipped, 16,479,445 trials, 58.3 s, exit 0** | same run, `schema/oracle-regression.json` refreshed |
| `world` channel sections walked | **6 → 13 of 13** | `borders_fog` emitted §1, 4, 5, 6, 7, 8 and stubbed §2, 3, 9; `map_terrain` emits all thirteen |
| tests in the three files | **78 passing, 0 failing** (`checksum::tests` 6, `borders_fog::tests` 46, `map_terrain::tests` 26) | `cargo test -p don-sim --lib -- checksum::tests systems::borders_fog::tests systems::map_terrain::tests` |
| `cargo test --workspace` | **1,249 passing, 0 failing** | the wave's hard gate, run at the end of this lane |
| `don-replay` | 5 suites green | includes the rules-channel corpus gate, which would have caught any drift from the `adler32` delegation |

## Why `map_terrain` won

Decided against `World::walk_data` `0x006b5cf0` itself, not by preference. Evidence:

* `re/decomp-all/006b5cf0.c` is thirteen guarded sections. `borders_fog::world_checksum`
  modelled six of them and emitted §2, §3 and §9 as empty, and its own doc comment said so
  ("`full` therefore is not yet the complete channel-12 digest").
* Sections 2 and 3 are six `SimpleArray<WCoord>::walk_data` `0x0047c660` calls, whose header
  is `len`, then `capacity`, `increment` and `flags & !0x40` — the growable-container fields
  CODEX flags as checksummed. `borders_fog` had no array model at all; `map_terrain` has
  `WalkedArray<T>` with exactly those fields.
* Sections 10–13 are four `Terrain` arrays reached through the global at `0x00c06218`, not
  through `World` at `0x00c06188`. `map_terrain::TerrainSync` has them; `borders_fog` did not
  know they existed.
* Cross-checked against `schema/state-schema.json`'s `World` entry, whose 24 recovered ops
  line up one-for-one: op 1 is `[+0,+8)` on `0x00c06188`; ops 2–7 are the six
  `SimpleArray<WCoord>` sub-object walks at `+128/156/184/212/252/280`; op 8 is `[+8,+0x80)`;
  ops 10–14 are the five plane pointers at `+0x138/+0x15c/+0x160/+0x164/+0x168`; ops 20–23 are
  `Array<WCoordData>` + three `SimpleArray<int>` on the `0x00c06218` object.

**Merged from the loser** (its one real advantage): the per-section digest, now
`World::checksum_sections()` → `WorldChecksum` with `SectionDigest` and
`differing_sections()`, plus the `WorldSection` enum carrying the engine's own 1..=13
numbering.

## Territory ownership — the answer

> `map_terrain::World` owns the storage. `borders_fog` owns the algorithm and writes through.

* **Storage**: `World::wdata: Vec<WData>`. `WData::who` is `+0x0f`, `who2` is `+0x10`, both
  inside the 21 bytes section 5 walks. `World::get_who` / `get_who2` are the readers.
* **Algorithm**: `borders_fog::{leader_border_params, claim_tile, check_borders}` — the radius
  caps, the `(k + wx) & 7` leader-scan rotation, the 256-tile-per-frame budget. Unchanged
  arithmetic; only the destination moved.
* **Tightened while moving it**: `check_borders` no longer takes the two territory-limit
  triples as arguments. It reads them from `World +0x38..0x4c` — the same dwords section 4 of
  the checksum walks — so a caller can no longer run the border computation with limits that
  differ from the ones on the wire.
* Same rule for fog: `Fog` is now policy only (`leaders: [FogLeader; 8]`, `option`). The
  `seen` / `seen2` / `seen3` / `wcoord_seen` planes are `World` fields, because they are
  sections 6 and 7.

## The singular checksum primitive

`crates/don-sim/src/checksum.rs` — new, 327 lines. `don-sim` has no dependencies, so it is
the only crate every other one can call.

* `adler32(adler, &[u8])` — `NMAX` chunking **and** the 16-way unroll, both transcribed from
  `re/decomp-all/00a46830.c`. The unroll is arithmetically invisible and is kept because it
  is the second structural boundary the oracle case's boundary lengths probe.
* `adler32_or_null(adler, Option<&[u8]>)` — the null arm (`test edx,edx` → `return 1`), which
  a Rust slice cannot express. `tech_cities` is the module that needed it.
* `Adler32` — the `CheckSum` accumulator: `+0x10` running adler seeded to 1, `+0x14` byte
  count. `DataWalk` — the two-slot visitor, `walk_tag` defaulted to the no-op `CheckSum` uses.
* `ByteSink` — not an engine object. Walks the traversal into a `Vec<u8>` and reports
  `first_difference`, because "the checksums differ" is not a debuggable statement.

Everything else in the tree is a `pub use` of it or a one-line call. The two remaining
hand-written adler loops (`economy` and `tech_cities` test modules) are deliberately
independent reference implementations inside `#[cfg(test)]`; keeping those is the point.

Lineage, and a correction: `CheckSum::walk_function` `0x00936ff0` is
`ecx = [edi+0x10]; call 0xa46830; [edi+0x10] = eax`, with `[edi+0x14] += end - begin`
[measured, disassembled this lane]. The binary has **two** `adler32` procedures —
`_adler32` `0x005089d0` (301 B, `__cdecl`, BHG's `main/basic` copy) and `adler32`
`0x00a46830` (295 B, `__fastcall`) — with structurally identical bodies. The checksum path
calls `0x00a46830`. `groups_guys` and `production` cited `0x005089d0`; corrected in place.

## Two real defects found by the merge

Neither would have been found without forcing the two modules together.

1. **`World::clear_seen` cleared one plane, not two.** `re/decomp-all/006b2250.c` ends in
   `memset(world+0x168, 0, world+0x08)` (`wcoord_seen`, `size` bytes) then
   `memset(world+0x15c, 0, world+0x14)` (`seen`, `fog_size` bytes). `map_terrain` cleared only
   `seen`; `borders_fog` had it right. Left as it was, `wcoord_seen` — section 7 — would have
   accumulated forever and diverged from retail on the second frame of any game. Fixed, and
   pinned by `clear_seen_clears_both_planes_and_spares_the_explored_one`.
2. **`borders_fog::div3` truncated toward zero.** It was `v / 3`. `init_coord_lookup_array`
   `0x00681db0` fills `t[j] = (j - 2) / 3` for `j < 0`, i.e. `floor(j/3)` on both sides of
   zero, which is what `map_terrain::div_3` already implemented. Every negative coordinate not
   divisible by 3 converted to the wrong cell. Now delegates.

## Files

Written (my lane):

* `/Users/ember/dev/don/crates/don-sim/src/checksum.rs` — **new**, the one primitive.
* `/Users/ember/dev/don/crates/don-sim/src/systems/map_terrain.rs` — winner. `walk_section`,
  `WorldSection`, `WorldChecksum` / `SectionDigest`, `checksum_sections`, `checksum_image`,
  `get_who2`, `valid_f`, `clear_seen` fix, `#[derive(Clone, Debug)]`, local `Adler32`/`DataWalk`
  replaced by re-exports.
* `/Users/ember/dev/don/crates/don-sim/src/systems/borders_fog.rs` — loser's storage removed
  (`Grids`, `WDataPlane`, `FogPlanes`, `WorldScalars`, `WorldWalkSection`, `WorldChecksum`,
  `world_checksum`, `adler32`); `Fog`, `update_seen`, `check_borders`, `get_who`, `get_who2`,
  `is_enemy_territory` retargeted at `World`.
* `/Users/ember/dev/don/docs/assembly/world-channel.md` — this report.

Shared files, smallest possible edit, each stated here as required:

* `crates/don-sim/src/lib.rs` — **one line**, `pub mod checksum;`.
* `crates/don-sim/src/systems/{ammo, economy, groups_guys, items, movement, production,
  tech_cities, victory_score}.rs` — each `pub fn adler32` body replaced by
  `pub use crate::checksum::adler32;` (`tech_cities` gets `adler32_or_null as adler32`, its
  signature). Doc comments kept; two stale `0x005089D0` citations corrected.
* `crates/don-sim/src/systems/combat.rs` — its `pub struct Adler32` replaced by a re-export.
* `crates/don-sim/src/systems/walls.rs` — its private `use …ammo::adler32` became
  `pub use crate::checksum::adler32`. This also fixed a live `E0603` where `tick.rs` called
  `walls::adler32` through that private import.
* `crates/don-replay/src/checksum.rs` — `adler32` body replaced by a one-line delegation.
* `crates/don-replay/src/{rules,scenario,script}_channel.rs` — each streaming `Adler32::update`
  body replaced by a call to the canonical function; the local `ADLER_BASE`/`ADLER_NMAX`
  removed. The structs and their `ByteSink` traits are untouched.
* `crates/oracle/src/registry.rs` — `adler32` case `model` repointed to
  `don_sim::checksum::adler32`; `caveat` records why.
* `crates/oracle/src/models.rs` — the `adler32` copy deleted, per that module's own rule
  ("when that symbol lands, the registry entry should be repointed and the copy here
  deleted").
* `schema/oracle-regression.json` — refreshed by the full-suite run.

**Not touched:** `crates/don-sim/src/tick.rs`. The tick lane was writing it live and adopted
this API on its own while this work was in flight — its `MapState` now holds a
`map_terrain::World`, calls `Fog::new()`, and calls the five-argument `check_borders`. No edit
from me was needed or made.

## Tree state, honestly

`cargo test --workspace`: **1,249 passing, 0 failing** at the close of this lane. It was red
in the middle of it — up to five failures in `crates/don-sim/src/systems/order_dispatch.rs`
and `target.rs`, files another lane created while this work was in flight, with no reference
to `borders_fog`, `map_terrain` or `crate::checksum`. That lane converged and the gate is
green. `cargo clippy -p don-sim` reports nothing in these three files, and
`rustfmt --check` is clean on every file I touched (three unrelated fmt diffs elsewhere in the
tree are other lanes' and were left alone).

## What this does not claim

* Nothing here is verified. The channel-12 walker is Tier C — transcribed from the decompiled
  structure and cross-checked against `schema/state-schema.json`, never compared against a
  retail `World::walk_data` byte stream. **No captured retail channel-12 value exists to
  compare against**, which is the next thing this needs.
* The 100,013-trial number is about `adler32` alone, over the stated length and value
  distribution. It says nothing about whether our `World` offers the right bytes.
* `World::checksum()` still hashes a world nothing populates from a replay. The walker being
  singular and complete is a precondition for the scoreboard to move, not the move itself.

## Next, in order

1. **Capture a retail channel-12 value** — one live read of `[0x00c06188]` mid-game plus the
   `issue_check_sums` record's `+0x2d` dword, and diff `World::checksum_image()` against the
   real walk with `ByteSink::first_difference`. Everything above is untested against retail
   until this exists.
2. **Section 9 on the load path.** `walk_data` *allocates* `CollBlock`s when `walker+4 != 0`
   and writes `flags = 2`. `World::walk_section` models the checksum path only; a save/load
   sink needs the other branch. `DataWalk::is_checksum()` is already there for it.
3. **`Array<T>` growth policy.** `WalkedArray` carries `capacity` and `increment` and they are
   checksummed, but nothing in this tree grows them the way retail does. Any populated
   `start_*` / `oil_*` array will desync on section 2 or 3 before section 5 is ever reached.
4. `docs/mechanics/COVERAGE.md` §1 still records "Channel 12 has two owners" and "`adler32` is
   implemented nine times". Both are now false. Left unedited — it is a generated status board
   another lane may be regenerating, and this report is the correction.
