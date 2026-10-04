# Replay Leaders live runtime frontier

Lane: `replay-leaders-runtime`. This is an unregistered, source-only merge frontier for
checksum channel 8. It neither installs a harness producer nor claims a retail checksum.

## Result

`crates/don-replay/src/leaders_runtime_frontier.rs` binds the replay setup Leader evidence
to the two current `don-sim` owners:

- `victory_score::Leaders`, including current flags, score/victory state, diplomacy,
  counters, tech masks, population/attrition fields, and territory;
- `leaders::Leaders`, including step-8 state, incoming taunts, production-AI state, live
  taunt/diplomacy records, rare masks, event rates, and the decoded economy subset.

Every duplicate field must agree byte-for-byte. A disagreement reports the exact Leader
slot and byte offset and produces no projection. Vector lengths must equal their retail
bounds (`129` building counts, `352` unit counts, `806` queued counts and tech bits).

For one checksum-active row the adapter currently owns:

```text
4,656 unique bytes in the Leader image
  196 bytes in the decoded LeaderDataEncrypt transcript (49 of 62 dwords)
4,852 checksum-visible bytes total
```

An inactive row owns and walks only its eight-byte header. The raw claim ledger can contain
body facts for such a row, but `runtime_claimed_walked_bytes()` deliberately excludes bytes
that retail's validity gate does not visit.

## Why setup and live state are separate

The landed `leader_initial_prefix` owns `8 + 704 * active` bytes at setup, but its spans are
not a live snapshot:

- the low flag byte changes during the match;
- diplomacy, treaty, taunt, and aggression arrays are explicitly mutable;
- the four-dword “identity” span combines `who` and `tribe` with mutable `defeated_by` and
  `gov`.

The new adapter therefore retains the setup byte count and expiration classification only
as a separate evidence ledger. It does not copy those bytes into current state. This is why
the current exact walk reaches the first active row's header and `who`, then stops at
`LeaderData+0x0C` (`tribe`). With `n` preceding inactive rows, the exact prefix is
`8*n + 12` bytes. Adler call boundaries do not change the value, so this is a lawful prefix
checkpoint, not a full channel comparison.

Later sparse fields remain valuable even though the first gap precedes them. A future owner
can merge them without retranscribing offsets. The projection includes:

- the score block, current diplomacy/init arrays, taunt inputs and match timers;
- AI production counters, pop/control/exploration/event fields;
- `num_buildings`, `num_units`, and `num_queued`;
- both 806-bit tech payloads and all three 44-bit rare payloads;
- all eight 92-byte `Diplomacy` children and the owned `Personality::raid` cell;
- the exact plaintext order of `LeaderDataEncrypt::walk_data`: eight known values per
  resource (the `rate` dword remains absent), plus `ages`. The seventh cap, four epoch
  counters, aggregate epochs, and discovered count remain absent.

## Exact retail walk boundary

The implementation follows `LeaderData::walk_data` `0x006D6750` rather than treating
`LeaderData` as a flat struct:

1. `[0, 8)` for every one of eight rows;
2. if `leader_flags & 1`, `[8, 0x692A)`;
3. eight `[0x692C + i*0x5C, +0x5C)` diplomacy records;
4. personality `[0x6DD4, 0x6E34)`;
5. six length-bearing bitmask/array payloads, four subobjects, three rare masks, and the
   decoded 62-dword `LeaderDataEncrypt` transcript.

`checksum()` returns a value only when this traversal reaches `Complete`. Any active row in
this revision returns `LeadersWalkFrontier` instead. No zero fill, setup reuse, partial
Adler value, or model-only economy checksum can be mistaken for retail agreement.

## Corpus implication

The local replay directory currently contains 63 files: 61 decode and two are explicitly
excluded by the replay parser (`no viable obfuscation key` and `no command-package chain`).
The 21 checksum-bearing recordings have 21 distinct, non-empty first Leaders values, all
in packet group/turn `0`. The harness's later turn-2 divergence is therefore not an
initializer checkpoint. The strict frame-zero setup producer admits 32 of the 61 decoded
snapshots, containing 175 active setup rows and 123,456 setup-owned bytes in aggregate.

The unique/non-empty result is the important boundary: the first recorded Leaders word is
already full live, replay-specific state. Neither a universal initializer checksum nor the
human-only setup expiration heuristic can stand in for a live owner.

## Hook still required

This lane intentionally edits no registry or harness file. Registration needs one future
`pub mod leaders_runtime_frontier;` line in `don-replay/src/lib.rs`.

Harness installation is later than registration. It requires:

1. one canonical synchronization point that supplies both `victory_score::Leaders` and
   `leaders::Leaders` for the same frame;
2. owners for every missing fixed-body byte and every length-bearing child header/body;
3. a complete 62-dword decoded economy transcript;
4. only then, a `SimState` direct-channel hook which accepts `checksum() == Ok(...)` and
   rejects every frontier result.

## Gates

- Local focused tests exercise source disagreement, sparse coverage, exact frontier
  refusal, and the real replay corpus.
- Persvati job `replay-leaders-runtime-20260811T161855Z-65292-5550-e49451e8ad11`
  compiled the isolated source/test overlay successfully. Its corpus tests correctly
  reported `SKIPPED — NOT A PASS` because replay assets are intentionally absent remotely;
  the local corpus gate supplies that evidence.
