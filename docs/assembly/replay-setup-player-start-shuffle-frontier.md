# Replay `Setup::build_game` player/start shuffle frontier

This note freezes the exact main-RNG shuffle that `Setup::build_game`
(`0x005AC190`) runs after procedural map construction and before the first
`Leader::init`. It deliberately does **not** join the current post-`place_all`
frontier to the shuffle. That join is not source-complete.

Authority is the supported retail executable, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`, and its
GUID-matched PDB, SHA-256
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`.

## Exact upstream boundary

The current resource schedule owns all nonempty `BONUSES` rows, their category-tail
branch, BONUSES cleanup/FISH lookup, and the exact first row of a nonempty FISH section.
That branch's first independently unowned instruction is recurrence routing at
`0x00690215`. If FISH is empty, its cleanup and GOODIES lookup/enumeration are owned;
the residual is GOODIES row zero at `0x0068FBB3`, or GOODIES cleanup at `0x00690225`
when empty. FISH recurrence/later rows, GOODIES rows, document cleanup, the
`Map::place_resources` return, the `Map::make` caller continuation at `0x0068C72D`, and
source token `0x1EF7` remain open. Therefore
neither the RNG at `Map::make` return nor the later shuffle-entry RNG may be
derived from the current post-`place_all` receipt.

The direct procedural setup chronology after `Map::make` is:

```text
0x005AC657  virtual Map::make(...)
0x005AC65A  load the returned Map
0x005AC65F  load Map+0x2EC (randomize player starts)
0x005AC665  retain that flag for the later shuffle
0x005AC778  Terrain::init(...)
0x005ACC41  Terrain::mark_forest()
0x005ACDC2  Random::get(0, 0xFFFF) in the player/start retry loop
0x005ACEF7  Leader::init(...) -- first downstream simulation child
```

`Terrain::init` does contain a direct `Random::get` at `0x00850FFF`, but it is
not a draw from `GameAccess::game_random`. The call sets `ECX` to the distinct
global `internal_random` object at `0x00EB697C`. It calls
`Random::get(0, 100)` exactly `N*N` times, where
`N = 4 * map_width + 1`, and fills the allocated `N` by `N` byte plane in row
order. Those draws advance the separate internal stream, not the main
simulation stream used by `Map::make`, the setup shuffle, and Market placement.
`Terrain::mark_forest` has no RNG call.

This corrects any audit that classifies the `0x00850FFF` call merely by its
callee and therefore mistakes it for a main-stream draw. RNG ownership is
selected by the `this` object, not by `Random::get`'s address.

## Exact shuffle

`Map::Map` (`0x006A06E0`) initializes `Map+0x2EC` to one. The Golden 2024 Great
Lakes `Map::new_map` branch retains that constructor value, so the randomized
branch is taken. Setup first clears an eight-dword stack occupancy array, sets
the destination offset to `Game+0x65C`, and repeats:

```text
draw      = GameAccess::game_random.get(0, 0xFFFF)
candidate = draw % 8

if occupancy[candidate] == 0:
    occupancy[candidate] = 1
    Game.player_start_order[accepted] = candidate
    accepted += 1

stop only after accepted == 8
```

The signed-remainder instruction sequence in the binary reduces to ordinary
`draw % 8` because `Random::get(0, 0xFFFF)` is nonnegative. The range is
half-open, so every draw is in `0..65535`. Duplicate candidates consume a main
RNG draw and write nothing. The accepted values are written in acceptance order
as eight dwords at `Game+0x65C`, `+0x660`, ..., `+0x678`.

The draw count is variable: at least eight and unbounded in principle. A fixed
eight-draw shuffle or Fisher-Yates shuffle is not retail-compatible. The
post-shuffle RNG state is the exact LCG state after every accepted and rejected
attempt.

For the non-randomized branch (`Map+0x2EC == 0`), the loop takes no RNG draws and
writes `[0,1,2,3,4,5,6,7]`. That branch is not the Golden Great Lakes path.

## Why there is no positive receipt yet

A standalone function beginning from an asserted shuffle-entry seed could
reproduce the leaf loop, but it would not produce that seed from the current
replay frontier. More importantly, shuffle exit is not Market entry. Setup next
consumes the permutation in order in the `Leader::init` loop beginning at call
site `0x005ACEF7`, performs graphics and score setup, and enters
`Setup::build_empire` at `0x005AD40B` / `0x005AD4D2`. Those children must be
owned or independently captured before any post-shuffle state can be certified
as Market-before.

The fail-closed binding rule is therefore:

- do not equate post-`place_all`, `Map::make` return, shuffle entry, shuffle
  exit, Market entry, or Market exit;
- a future shuffle receipt must accept a separately authenticated main RNG
  state until `Map::place_resources` and the rest of `Map::make` are closed;
- Market-before still needs an exact continuation through `Leader::init` and
  the preceding `build_empire` work, or its own capture authority; and
- Market-after alone may bind the `Setup::build_units` entry boundary already
  documented by the Golden Market transaction.

The first upstream implementation target is now the selected FISH residual described
above. The first downstream simulation target after a completed shuffle is
`Leader::init` at `0x005ACEF7`.
