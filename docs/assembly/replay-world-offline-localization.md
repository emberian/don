# Replay World turn-2 offline localization

## Result

`don-world-localize` turns the current channel-12 mismatch into a repeatable section-local
report without treating Adler-32 as a byte oracle. On the tracked `b406017` replay/sim
baseline plus the analyzer (replay/sim byte-identical through `b906fb6`), the local corpus
establishes:

| fact | result |
|---|---:|
| recordings opened | 61 |
| recordings carrying `CheckSumsCommand` `0x39` World checkpoints | 21 |
| first checksummed turn | 2 in 21/21 |
| first checkpoints with at least two agreeing same-group peers | 21/21 |
| all-turn same-group World peer comparisons | 265,619/265,619 identical |
| first-turn retail/model matches | 0/21 |
| distinct first-turn values | 21 retail, 21 model |
| current model images with a coherent, transitioned owner ledger | 0/21 |

The lawful offline localization is therefore an exclusion boundary, not an observed retail
byte difference: section 1 is fully replay-bound, and the earliest byte not covered by the
existing owner ledger is **global walk offset 8, section 2 (`StartArrays`) offset 0**, in
21/21 recordings. An actual first differing byte remains unavailable without a captured
retail walk image whose Adler-32 equals the peer-agreed recorded value.

## Exact owner ranges

Every checksum-bearing recording has the same section-local ledger geometry:

| section | exact range | bytes | source |
|---:|---:|---:|---|
| 1 `Dims` | `0..8` | 8 | replay map-size selector through the shipped size table |
| 4 `Scalars` | `0..40` | 40 | derived `World::init` dimension words |
| 4 `Scalars` | `48..72` | 24 | independently projected replay Rules territory limits |
| 4 `Scalars` | `116..120` | 4 | replay `GameInfo::seed` |

That is 76 bytes per recording, 1,596 across the 21-file experiment. Those bytes remain
value-equal at the same section-local offsets in every current model image. This is weaker
than a transitioned owner map: procedural generation changed the walk shape or image, while
the ledger snapshot stayed at the prefix checksum. `InitialWorld::ownership_is_coherent()`
is false in all 21 cases. The analyzer deliberately calls the current 76 bytes
`value-bound`, not “last-writer owned.”

For the lexically first recording, the distinction is concrete:

```text
ledger snapshot  0x1389cbbb / 780168 bytes / 76 owned
current model    0x4e2a1e45 / 780276 bytes / owner ledger incoherent
retail turn 2    0xd63a3a53 / two same-group peers agree
```

No value was chosen from the retail checksum, and none of these three checksums reveals a
byte value or mismatch offset.

## Section/range census

Across the 21 current model images, the analyzer walks 13,119,100 bytes. Only 1,596 are
value-bound; 13,117,504 remain outside the exact ledger. `model delta` means a model byte is
different from the prefix snapshot at the same section-local offset (or was appended by a
walk-shape change). It is model-to-model evidence, not proof that retail differs there.

| rank | section | walked | value-bound | unknown | share of unknown | model delta | files with delta |
|---:|---|---:|---:|---:|---:|---:|---:|
| 1 | 6 `TDataAndFog` | 7,396,400 | 0 | 7,396,400 | 56.39% | 0 | 0/21 |
| 2 | 5 `WData` | 3,530,100 | 0 | 3,530,100 | 26.91% | 328,135 | 21/21 |
| 3 | 8 `Danger` | 1,344,800 | 0 | 1,344,800 | 10.25% | 0 | 0/21 |
| 4 | 9 `CollBlocks` | 672,400 | 0 | 672,400 | 5.13% | 0 | 0/21 |
| 5 | 7 `WCoordSeen` | 168,100 | 0 | 168,100 | 1.28% | 0 | 0/21 |
| 6 | 2 `StartArrays` | 4,108 | 0 | 4,108 | 0.03% | 3,886 | 19/21 |
| 7 | 4 `Scalars` | 2,520 | 1,428 | 1,092 | 0.01% | 0 | 0/21 |
| 8 | 3 `OilArrays` | 168 | 0 | 168 | <0.01% | 0 | 0/21 |
| 9–12 | Terrain sections 10–13 | 336 | 0 | 336 | <0.01% | 0 | 0/21 |

Section 2 is the earliest candidate because the walker visits it immediately after the
fully owned dimensions. Section 6 is the largest missing producer by byte volume. Section 5
is the only large plane the current procedural prefix actually changes, but all 328,135
changed bytes remain unowned because the completed generator stages do not yet advance the
ledger. None of those facts identifies the actual retail mismatch section.

## Source-stage correlation

The analyzer derives each endpoint by executing the existing replay reconstruction, not by
reading `schema/replay-validation.json`:

| current exact stop | recordings |
|---|---:|
| `place_all_mountains_add_mountain` | 12 |
| `place_all_world_set_oil_at` | 6 |
| `map_team_continent_partition` | 2 |
| `map_east_indies_nonplayer_islands` | 1 |

The two team-partition recordings have no generated start-array delta; the other 19,
including the East Indies stop, do. All 21 have a `WData` delta. This is correlation with
the reached source boundary, not per-byte causal attribution. Exact causal ownership
requires a before/after receipt for each completed continent, post-continent and fertility
stage.

## Ranked blockers

1. **Transition the owner ledger through already-executed stages.** The current checksum and
   walk image have moved, but the ledger has not. Add exact allowed-section receipts for the
   continent virtual, post-continent chain and fertility pass before claiming any newly
   sourced bytes.
2. **Complete the four measured generator stops.** `Mountains::add_mountain` covers 12 files,
   `World::set_oil_at` 6, team partition 2 and East Indies islands 1. These are the concrete
   completion frontier, not a checksum-derived guess.
3. **Populate and receipt section 6.** It is 56.39% of the current unknown walk and remains
   byte-identical to the zero prefix in every model image. Binary/source-stage evidence must
   decide its correct setup; the checksum cannot.
4. **Receipt section-5 mutations.** The current ports change 328,135 `WData` bytes across all
   21 recordings, but successful execution alone does not prove unchanged neighbours or
   final correctness.
5. **Acquire a checksum-bound retail byte image only when live work resumes.** Offline work
   can shrink the candidate set and complete reconstruction, but it cannot name the actual
   first retail/model byte. The existing capture contract remains the required evidence.

## Command and claims not made

```sh
cargo run -p don-replay --bin don-world-localize -- --corpus
cargo run -p don-replay --bin don-world-localize -- --ranges path/to/recording.rcx
```

The optional range view prints all exact owned and unknown section-local ranges and a bounded
preview of potentially numerous model-delta ranges. The analyzer opens the original replay
bytes, joins peers on `CommandPackage::group`, reconstructs the model through
`WorldSim::from_replay`, and captures the canonical thirteen-section walk directly.

This work does not claim a channel-12 match, an actual first-difference offset, correctness
of a zero-filled unknown section, ownership from a successful function call, or any byte
inferred from Adler-32. It changes no reconstruction, runtime, schedule, live tooling or
scoreboard artifact.
