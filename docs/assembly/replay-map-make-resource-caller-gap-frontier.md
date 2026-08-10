# Replay `Map::make` resource caller-gap frontier

`crates/don-replay/src/map_make_resource_caller_gap_frontier.rs` closes the
caller-only interval between the landed terrain-transition checkpoint and the
direct resource-placement call:

```text
GameLog::say_checksum call 0x0068c12a, map.cpp token 0x1ebe
  -> caller resume 0x0068c12f
  -> progress/checksum/diagnostic-only interval
  -> lazy resource-placement gate 0x0068c6dd..0x0068c704
  -> conditional Map::place_resources call 0x0068c707
  -> Map::place_resources entry 0x0068f4f0
```

The reversal used shipped `ron-bin/riseofnations.exe` (SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`)
and `ron-bin/sbl/rise.pdb` (SHA-256
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`).
The PDB identifies the enclosing 3,021-byte function as
`void Map::make(int, int, int)` at `0x0068bc90` and the endpoint as
`int Map::place_resources(int)` at `0x0068f4f0`.

## The gap is not hidden world generation

Complete disassembly of `0x0068c12f..0x0068c707` finds no call to
`Random::get` (`0x00a39d70`) and no write through the `Map` or `World`
objects. Before the endpoint the calls are limited to `String` construction,
assignment, parsing and destruction; `GameLog::say_checksum`; optional
`SplashScreen::refresh`; `Log::say`; and read-only `vector_dist` formatting.
The bridge therefore preserves all of these deterministic inputs bit-for-bit:

- the main RNG state;
- the complete `WorldChecksum` and its sourced-byte count; and
- an opaque SHA-256 of the authoritative `Map` projection.

Progress and logging are retained as receipt observations, not promoted to
simulation mutations. A nonzero third `Map::make` argument causes exactly two
refreshes in this interval, at calls `0x0068c16a` and `0x0068c6c7`.
Diagnostics emit four unconditional log records, one additional record when
`World::map >= 0`, and one start-position record per player only outside CTW.
The signed first argument controls that player loop and is forwarded unchanged
to `Map::place_resources`.

## Exact checksum chronology

The two intervening `GameLog::say_checksum` calls are:

| call VA | pushed source token | decimal source line | deterministic writes before it |
|---:|---:|---:|---|
| `0x0068c1a2` | `0x1ec5` | 7877 | none (`Map`/`World`/RNG) |
| `0x0068c1da` | `0x1ec8` | 7880 | none (`Map`/`World`/RNG) |

The receipt preserves both the upstream `0x1ebe` identity and the latest
`0x1ec8` deadline. Source tokens are `map.cpp` line numbers supplied to the
checksum logger; they are not replay bytes and do not increase
`sourced_walked_bytes`.

## Lazy resource-placement gate

The endpoint gate is exact and short-circuiting:

1. `0x0068c6dd` tests `Game::semaphore` byte `Game +0x821`, mask `0x02`
   (bit 9). If set, it skips resource placement without reading either CTW
   state or the conquest style.
2. `0x0068c6e6` tests byte `Game +0x822`, mask `0x02` (bit 17). If clear,
   retail calls `Map::place_resources` directly.
3. In CTW only, `0x0068c6ef` loads `ConquestGame::game_style` from PDB offset
   `+0x1480`. A null style still admits the call.
4. For a non-null style, `0x0068c6fe` reads PDB field
   `ConquestStyle::no_rare` at `+0x68`. Zero admits the call; any nonzero value
   skips it.

`ResourceGateRead` freezes the lazy read sequence, so a superficially correct
boolean result cannot hide an eager null dereference or reordered mode test.
When the call is admitted, `PlaceResourcesCallerEntryReceipt` carries the
exact call/entry VAs, both checkpoint identities, object identities, unchanged
map/world projections, sourced-byte count, signed player argument, and RNG
state at callee entry. It deliberately does not reconstruct that state from a
seed.

## Evidence and residuals

Production facts are admitted only when a nonempty immutable capture is bound
to the shipped EXE/PDB hashes, exact function/call VAs, all four object
identities, map digest, World checksum, sourced-byte count, RNG state, both
semaphore bytes, and the nullable `no_rare` fact. Synthetic fixtures are
test-only.

This source owns no `Map::place_resources` mutation. Its successful residual is
the typed entry at `0x0068f4f0`; its skipped residual is the common caller
continuation at `0x0068c70c`. The next caller checksum is intentionally outside
the frontier: call `0x0068c72d`, source token `0x1ef7`. The resource-placement
body remains owned by its separate frontier.

## Source-only proof

`crates/don-replay/tests/map_make_resource_caller_gap_frontier.rs` freezes:

- both intervening checksum calls and line tokens;
- exact call, entry, continuation and next-checkpoint boundaries;
- zero RNG draws and unchanged RNG, `Map`, `World`, and sourced-byte state;
- optional progress and diagnostic cardinality without gameplay ownership;
- signed player-loop behavior and unchanged argument forwarding;
- bit-9, CTW, null-style, zero-`no_rare`, and nonzero-`no_rare` branches;
- lazy gate-read chronology and nullable pointer behavior;
- capture provenance and refusal of stale/empty evidence; and
- mutation sensitivity for every resource-gate input while continuity stays
  invariant.

Per lane constraints, this handoff is source-only: no formatter, build, test,
retail process, shared schedule, `lib.rs`, `initial.rs`, post-nubify file, or
resource-placement file was touched.
