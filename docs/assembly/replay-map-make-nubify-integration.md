# Replay `Map::make` forest-tail integration

## Result

The replay crate now exposes one real receipt chain from a successful
`TerrainGroups::place_all` transaction through the caller checksum deadline at
source token `0x1ebe`:

```text
place_all receipt
  -> Map::check_player_forest
  -> TerrainGroups::nubify_forest + typed World::is_edge_of_region receipts
  -> checkpoint 0x1eb9
  -> selected-tileset base gates + TerrainGroups::fix_transitions
  -> checkpoint 0x1ebe
```

`InitialItemReconstruction::advance_map_make_terrain_repairs` stages the entire
replay-side World/checksum transaction. It passes each receipt directly to the
next executor; no intermediate RNG state or checksum is reconstructed from a
seed or copied summary. Only after the final transition receipt succeeds does
the bridge replace the authoritative `InitialWorld`. A missing or stale edge
receipt fails inside the nubify transaction, and a missing or stale selected-
tileset fact rolls back the earlier forest work as well.

The edge host remains an explicit external boundary. Its observations or side
effects cannot be rolled back by the replay crate. Production callers that
need atomic external behavior must use immutable preproduced receipts or their
own prepare/commit protocol.

## Exact schedule ownership

`MAP_MAKE_SCHEDULE` no longer has the unresolved `terrain_repairs` bucket. The
post-placement rows and their immediately following caller checkpoints are:

| stage | call/evidence VA | checkpoint call | source token | RNG |
|---|---:|---:|---:|---|
| `terrain_groups_place_all` | `0x0068c010` | `0x0068c039` | `0x1eb3` | branch-dependent |
| `check_player_forest` | `0x0068c04d` | `0x0068c076` | `0x1eb5` | none |
| `nubify_forest` | `0x0068c08b` | `0x0068c0b4` | `0x1eb9` | exact receipted draws |
| `post_nubify_transitions` | `0x0068c101` | `0x0068c12a` | `0x1ebe` | signed base gates plus exact receipted draws |

The first three rows share the caller's `Game::semaphore & 0x02` skip gate;
the integration bridge admits only the ordinary clear-bit path. The post-
nubify checkpoint and transition tail still execute after the skipped branch.
The post-transition evidence VA is the unconditional `fix_transitions` call;
its receipt additionally retains all three preceding conditional base calls.

The `source_token` values belong to retail checksum-log instrumentation. They
pin chronology and deadlines; they are not serialized replay bytes and do not
increase `sourced_walked_bytes`.

## Live-fact boundary

Two fact classes remain deliberately outside the replay:

1. Each `World::is_edge_of_region` answer must carry an exact request echo,
   evolving staged World checksum, start-city mask, result, and admissible
   retail-capture or exact-port provenance.
2. The six selected-tileset tuning words must carry evidence bound to the
   shipped executable, `Rules` Constants checkpoint, selected object identity,
   post-nubify World checksum, incoming main-RNG state, checkpoint call
   `0x0068c0b4`, and source token `0x1eb9`.

The bridge accepts those typed producers; it does not derive either fact from
the replay's map-style ordinal, invent defaults, authenticate caller-carried
hashes, or promote the generated mutations to replay-owned bytes.

## Focused source proof

`crates/don-replay/tests/map_make_nubify_integration.rs` freezes:

- the four exact post-placement schedule rows and checkpoint tokens;
- direct checksum/RNG continuity across all three receipts;
- production-shaped edge and tileset evidence admission; and
- complete replay-side rollback when the selected-tileset receipt is stale,
  while explicitly acknowledging that edge-host observations survive.

Root convergence formatted the integration and validated all three focused tests in both modes:

- hbox debug: `replay-nubify-integration-20260809T225401Z-91613-1195-cb8b2b915d8e`;
- persvati release: `replay-nubify-integration-release-20260809T225359Z-91619-8452-cb8b2b915d8e`.

Neither job ran retail. The reproducible focused gate is:

```text
cargo test -p don-replay --test map_make_nubify_integration
```

## Files

```text
crates/don-replay/src/lib.rs
crates/don-replay/src/map_style.rs
crates/don-replay/src/initial.rs
crates/don-replay/tests/map_make_nubify_integration.rs
docs/assembly/replay-map-make-nubify-integration.md
```
