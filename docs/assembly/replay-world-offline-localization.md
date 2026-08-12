# Replay World turn-2 offline localization

## Result

`don-world-localize` now sees a coherent byte-owner ledger after the reconstructed
continent, common post-continent and fertility stages. The owner transition is a model
provenance result, not a retail checksum match: every transition compares exact before/after
walk images and admits only changed bytes in the stage's allowed sections.

On the local corpus at 2026-08-11:

| fact | result |
|---|---:|
| recordings opened | 62 |
| recordings carrying `CheckSumsCommand` `0x39` World checkpoints | 21 |
| first checksummed turn | 2 in 21/21 |
| first checkpoints with at least two agreeing same-group peers | 21/21 |
| all-turn same-group World peer comparisons | 265,619/265,619 identical |
| first-turn retail/model matches | 0/21 |
| distinct first-turn values | 21 retail, 21 model |
| current model images with a coherent, transitioned owner ledger | **21/21** |

No recorded Adler-32 value was used as state input. An actual first differing retail byte
remains unavailable without a captured retail walk image whose Adler-32 equals the
peer-agreed recorded value.

## Exact ownership after transitions

The replay/static prefix still owns the same 76 bytes per recording:

| section | exact range | bytes | source |
|---:|---:|---:|---|
| 1 `Dims` | `0..8` | 8 | replay map-size selector through the shipped size table |
| 4 `Scalars` | `0..40` | 40 | derived `World::init` dimension words |
| 4 `Scalars` | `48..72` | 24 | independently projected replay Rules territory limits |
| 4 `Scalars` | `116..120` | 4 | replay `GameInfo::seed` |

The executed wipe and generator then add receipt-bound last-writer ownership:

| stage | admitted walked sections | rule |
|---|---|---|
| `World::wipe` | 6 `TDataAndFog`, 7 `WCoordSeen` | all explicit written ranges, including equal zero rewrites |
| map-style continent virtual | 2 `StartArrays`, 5 `WData` | changed or newly walked bytes only |
| common regions/diagonal/coastline chain | 5 `WData` | changed bytes only |
| `TerrainGroups::fill_fertile` | 5 `WData` | changed bytes only |
| owned `place_all` prefix | 5 `WData`, 6 `TDataAndFog` | receipt-proven region/oil/mountain writes only |

Across the 21 recordings the wipe contributes 7,564,500 written bytes, while the later
transitions contribute 350,995 changed generator bytes: 4,274 in section 2 and 346,721 in
section 5. Together with the 1,596 replay/static prefix bytes, exact coverage is 7,917,091
bytes. Outside explicitly receipted writes, unchanged zeroes and unchanged neighbours remain
unknown even when an exact routine visited them.

The section-2 growth rule is deliberately strict. A proved continent receipt may grow an
unowned section and the ledger rebases later owners by section-local offset. Growth in a
forbidden section, shrinkage, or reshaping a section which already has any owned byte is
rejected transactionally.

## Section/range census

The current model walks 13,119,476 bytes over the 21 checksum-bearing recordings. The owner
snapshot now equals each current model image, so `model delta` is zero by construction; that
column no longer stands in for missing provenance.

| section | walked | exact owner | unknown |
|---:|---:|---:|---:|
| 1 `Dims` | 168 | 168 | 0 |
| 2 `StartArrays` | 4,484 | 4,274 | 210 |
| 3 `OilArrays` | 168 | 0 | 168 |
| 4 `Scalars` | 2,520 | 1,428 | 1,092 |
| 5 `WData` | 3,530,100 | 346,721 | 3,183,379 |
| 6 `TDataAndFog` | 7,396,400 | 7,396,400 | 0 |
| 7 `WCoordSeen` | 168,100 | 168,100 | 0 |
| 8 `Danger` | 1,344,800 | 0 | 1,344,800 |
| 9 `CollBlocks` | 672,400 | 0 | 672,400 |
| 10–13 terrain arrays | 336 | 0 | 336 |
| **total** | **13,119,476** | **7,917,091** | **5,202,385** |

The earliest lawful unknown is section 2 offset 1 in all 21 recordings. The two East Meets
West stops now contain and own the changed bytes from all four exact start appends and the
complete post-loop `Map::check_player_land` body. Those offsets are exclusion boundaries
only, not observed retail/model differences.

## Source-stage correlation

The analyzer derives each endpoint by executing the reconstruction; it does not read model
bytes from `schema/replay-validation.json`:

| current exact stop | recordings |
|---|---:|
| `place_all_mountains_add_mountain` | 19 |
| `map_team_continent_game_log_say_checksum` | 2 |

Every recording crosses and receipts its executed continent prefix. The 19 recordings whose
style virtual completes also receipt the common post-continent and fertility stages. This
establishes model last-writer provenance only; it does not establish retail equality for any
transitioned byte.

## Transaction contract

`InitialItemReconstruction` stages World, generation regions, cached checksum and the owner
ledger together. It commits only after every reached stage's receipt and allowed-section
diff validate. A late owner error exposes none of the generated state. Tests poison a real
Mediterranean replay with prior StartArrays ownership and prove that the later shape refusal
rolls back World, regions, checksum, ledger and reconstruction plan.

The ledger transition itself also rejects stale input/output checksums, forbidden section
mutations, forbidden shape changes and already-owned section reshaping without changing its
snapshot or owners.

## Ranked blockers

1. **Continue the measured generator stops.** The shipped mountain range-list inputs to
   `Mountains::randomize_mountains` `0x0089ca70` block 19 files; the two East Meets West
   recordings have completed `Map::check_player_land` and the exact caller-local
   `String::close`, both centroid `SimpleArray<int>` frees, and the complete
   `MapEastMeetsWest::make_continents` epilogue. They also execute the exact
   first common `Regions::clear_all` and now stop at `Regions::find_all`
   `0x0067eff0` (caller `0x0068be3c`).
2. **Replace the wipe baseline with later TData and visibility producers.** The baseline is
   exact, but starting objects, terrain footprints, LOS and detector passes can overwrite it
   before the turn-2 checkpoint.
3. **Prove unchanged WData values independently.** The exact transitions own changed bytes;
   they intentionally do not promote the 3,183,379 unchanged bytes.
4. **Resolve the remaining StartArrays bytes only from real producers.** Zero header bytes
   which happened not to change are still unknown.
5. **Acquire a checksum-bound retail byte image only when live work resumes.** Offline work
   can shrink the candidate set, but the checksum alone cannot name a retail byte.

## Commands and claims not made

```sh
cargo test -p don-replay --test world_owner_frontier
cargo test -p don-replay --test replay_world_owner_transitions
cargo run -p don-replay --bin don-world-localize -- --corpus
cargo run -p don-replay --bin don-world-localize -- --ranges path/to/recording.rcx
```

This work does not claim a channel-12 match, an actual first-difference offset, correctness
of an unknown zero byte, ownership from a successful function call alone, or any byte inferred
from Adler-32.
