# Live collision-block maintenance

## Outcome

`crates/don-sim/src/systems/collision.rs` already ports
`GameDaemon::process_coll_blocks` `0x00731F90` exactly. The ordinary tick nevertheless records
`Gap::GameDaemonProcessCollBlocks` because it never supplies the persistent scan cursor at
`GameDaemon + 0x20` and never calls the recovered body.

`collision_blocks_live::CollisionBlockRuntime` is the missing exclusive store. It owns that
cursor across frames and passes the authoritative `map_terrain::World` directly to the recovered
body. `CollisionBlockPass` exposes the before/after cursor and freed count so adapter tests catch
three damaging mutations: resetting the cursor each frame, reaping a cloned world, or discarding
the body's return value.

The path-import tests in `crates/don-sim/tests/collision_blocks_live_path.rs` deliberately use an
instrumented host rather than duplicating the 204-byte algorithm. The algorithm's flag, budget,
free, and wrap cases remain covered beside its implementation in `collision.rs`.

## Red-tick audit

The closure ledger currently marks nine of the 29 top-level tick rows red. This pass selected the
smallest high-impact live-store seam that does not overlap the active victory, army/group-action,
or script lanes.

| step | current executable frontier | reason not selected here |
|---:|---|---|
| 4 | script runtime executes twice; unrecovered builtins fail closed | active script/BHS work owns it |
| 8 | economy/leader dispatcher exists; several child hosts remain partial | broad leader/type host |
| 11 | strategy dispatcher exists; exploration, planning, diplomacy remain partial | overlaps victory/AI work |
| 12 | reaper body exact; persistent cursor and live call absent | **selected** |
| 13 | army dispatcher/prefix exists; full Group/Unit/City/type host is missing | army/group files are actively owned |
| 15 | ammo runs; Unit/Guy/build/corpse `inc_time` family remains partial | several RNG and animation hosts |
| 17 | recovered leader post-pass exists but remains a partial top-level row | lower simulation impact |
| 19 | recovered event-frame dispatcher exists but child feedback/event hosts remain partial | lower simulation impact |
| 22 | scanner exists; renderer-owned `RoadElementCandidate` facts fail closed | external candidate source |

## Frozen integration hunk

The proof pack intentionally does not edit `systems/mod.rs` or `tick.rs`. Integration is exactly
the following four changes; broadening this hunk should trigger a new ownership review.

```diff
diff --git a/crates/don-sim/src/systems/mod.rs b/crates/don-sim/src/systems/mod.rs
@@
 pub mod collision;
+pub mod collision_blocks_live;

diff --git a/crates/don-sim/src/tick.rs b/crates/don-sim/src/tick.rs
@@ pub struct Sim {
     pub map: MapState,
+    pub collision_blocks: crate::systems::collision_blocks_live::CollisionBlockRuntime,
     pub road_scan: crate::systems::roads::RoadScanState,
@@ pub fn new(seed: u64, wcells: u16) -> Sim {
             map,
+            collision_blocks: crate::systems::collision_blocks_live::CollisionBlockRuntime::new(),
             road_scan: crate::systems::roads::RoadScanState::default(),
@@ fn game_daemon_process_all(&mut self) -> (StepRun, u32) {
-        // process_coll_blocks: no port.
-        self.cover.gaps[Gap::GameDaemonProcessCollBlocks.index()] += 1;
+        let _coll_blocks = self
+            .collision_blocks
+            .process_step12(&mut self.map.world);
+        work = work.saturating_add(1);
```

The existing gap enum entry may remain temporarily as a zero counter for trace-schema stability;
the frozen hunk removes the only increment. A later schema migration can delete the dormant slot.

## Evidence boundary

- PDB inventory: `schema/rise-procs.tsv` records `0x00731f90`, 204 bytes.
- Recovered body and direct algorithm tests: `crates/don-sim/src/systems/collision.rs`.
- Authoritative collision bitmap store: `map_terrain::World::wdata[*].block`.
- This tranche adds no new retail derivation and claims the same Tier-C fidelity as the recovered
  body. It only makes the missing state ownership and call boundary executable.
