# Replay frame-32 wildlife frontier

This slice owns the deterministic prefix of the supported 2024 golden replay's first wildlife
cadence. It is deliberately **not mounted into `Sim::do_frame`**: the replay does not serialize
the generated map image, post-setup RNG, owner-9 object identities, or the virtual
`SubObjectData::is_animal()` answers needed at frame 32.

## Native call chain and timing

The supported executable is the one identified by `SUPPORTED_RETAIL_EXE_SHA256`.

1. `Game::do_frame` `0x00591EF0` calls `Objects::process_all` at `0x0059246F`.
2. `Objects::process_all` `0x0065DCE0` first completes the rotated Unit walk and fixed Build/Wall
   walks.
3. `0x0065DEC9` reads `Game::frame` and enters the wildlife block when `frame & 0x1f == 0`.
4. `Game::frame++` is later, at `0x005924BF`; therefore this is the pre-increment frame. The
   supported replay's first unconditional entry is exactly frame 32.

The recovered prefix ends at the first viable-cell call instruction `0x0065E03B`, whose target is
`Objects::init_unit` `0x0065E0C0`. A successful return would next call
`Unit::add_air_patrol_order` `0x005E4350`; neither child is claimed here.

## Exact reads and RNG chronology

Retail computes:

```text
quota = min(10, wrapping_signed(map.xs * map.ys) / 100)
existing = count(owner 9 Unit slots [0, unit_mark)) where
           flags & 1 && virtual is_animal() && type_index == 402
attempts = max(quota - existing, 0)
```

For each attempt, in order:

1. if `map.xs <= 1`, use `x = 0`; otherwise call main-stream
   `Random::get(0, 0xffff)` (`0x00A39D70`) at `0x0065DFAD`, then signed `% map.xs`;
2. if `map.ys <= 1`, use `y = 0`; otherwise make the same draw at `0x0065DFD9`, then signed
   `% map.ys`;
3. read WData cell `y * xs + x` and test byte flag `0x20` at `0x0065E00B`;
4. reject a clear cell and continue, or request
   `Objects::init_unit(9, 402, x*0x300+0x180, y*0x300+0x180, -1, -1, -1)`.

There is no fixed draw count. Quota already satisfied means zero draws; each attempt consumes zero,
one, or two draws depending on the two dimensions; a normal two-dimensional map consumes two.
The exact `Random` LCG, low-16 scaling, and half-open range already live in `don_sim::rng`.

## Branch and mutation ledger

| branch | authoritative work | canonical mutation |
|---|---|---|
| quota satisfied | exact owner-9 census | none |
| every candidate rejects | clone RNG, receipt each conditional draw and WData flag read | publish only the final main RNG state at atomic commit |
| first viable candidate | same prefix through its exact child request | none; return typed `SpawnRequired`, including RNG-at-child, without publishing draws |

Retail has advanced its RNG before the spawn child. This detached transaction intentionally has
stronger atomicity: until `Objects::init_unit` and its complete mutation closure are owned, the
spawn branch publishes nothing. That child can allocate/mutate the Unit and Objects registries and
reach type, Guy, collision, visibility, Leader, and order state; a successful allocation then
reaches the patrol-order child. Those mutations and any nested RNG are the first open cone.

## Golden-state facts and capture debt

The independently recovered setup prefix identifies the ordinary pre-frame cohort as owner-0
Scout type 69 at `o=0`, Merchants type 62 at `o=1..2`, Citizens type 50 at `o=3..6`, and the
starting Village type 414 at Build `o=2000`. A Dutch Market type 436 at the next Build slot is
currently a setup finding, not frame-32 wildlife authority. None substitutes for the live owner-9
Unit band the wildlife census reads.

No checksum word in the replay localizes frame 32, and replay reconstruction does not generate the
missing WData/RNG/object state. `don_replay::wildlife_frame32` consequently requires one coherent
supported-process capture containing:

- the supported replay and executable hashes;
- exact frame and main RNG state;
- a SHA-256 of the canonical Sim snapshot;
- the full terrain `WorldChecksum` section ledger and SHA-256 of its complete checksum byte image;
- the object-World digest; and
- every live owner-9 Unit identity, retail `o`/`uid`/flags/type, and captured virtual
  `is_animal()` result, in sparse-slot order.

The contract's composition digest covers all of those claims. Binding also runs the detached
planner: both a proven zero-spawn state and a typed spawn-required state are admissible, while any
missing/stale owner fact fails closed. No production capture is checked into the repository, so the
general tick gap remains truthful.

## Implementation and gates

- `crates/don-sim/src/systems/wildlife_spawn_frontier.rs`: exact detached prepare/commit and typed
  spawn boundary.
- `crates/don-sim/tests/wildlife_spawn_frontier.rs`: zero branch, viable branch, stale atomicity,
  owner-9 virtual facts, conditional-axis draws, and exact-frame gates.
- `crates/don-replay/src/wildlife_frame32.rs`: supported-capture oracle contract and binder.
- `crates/don-replay/tests/wildlife_frame32.rs`: provenance/composition/map-image and branch
  admission gates.
