# Death corpse clock — exact step-15 body and live seam

Status: **exact body, mutation-pinned, not wired**.  The exclusive implementation is
`crates/don-sim/src/systems/death_inctime.rs`.  This lane did not edit `systems/mod.rs`,
`tick.rs`, or any other shared file, and did not run a build or checker.

## Ground truth

All addresses below are preferred VAs in the supported PE32 image
`ron-bin/riseofnations.exe`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
The matching PDB gives `DeathObj::inc_time` a 540-byte extent:

| Symbol / datum | VA | Evidence and role |
|---|---:|---|
| `DeathObj::inc_time` | `0x008D5240..0x008D545B` | `schema/rise-procs.tsv`; disassembly and `re/decomp-all/008d5240.c` |
| `DeathObj::clear_blocking` | `0x008D4AC0` | simulation mutation called when a blocking corpse becomes invalid |
| `DeathObjOut::inc_bleed` | `0x008D3B90` | presentation tail, outside walked `DeathObjData` |
| `AnimationPacket::get_game_frames` | `0x00918CC0` | supplies the animation duration; its invalid-animation fallback is 3 |
| `UnitTypeData::blocks_while_dead` | `0x00470440` | vtable slot `+0x120`; tests `ObjectTypeData+0x2B4 & 0x800000` |
| `MiscAccess::scene` pointer | `0x00C0620C` | PDB symbol; body stores 1 to `Scene::recalc_deaths` at `+0x22F` |
| ordinary corpse padding | `0x00B2176C` | little-endian `.rdata` value `0x87` = 135 |
| skeleton corpse padding | `0x00B21774` | little-endian `.rdata` value `0x273` = 627 |

`schema/pdb-types.json` identifies type offset `+0x218` as
`ObjectTypeData::domain`.  Domain 1 omits the ordinary 135-frame tail.  A present
`skel_gpiece` overrides the domain arm and selects the 627-frame tail.

## Exact deterministic order

For every nonzero `DeathRecord.valid` slot, in slot order:

1. Resolve `gpiece`, the original object `(who,o)`, its type, and its animation packet.
2. Increment the original object's `ObjectData::hold` at `+0x32` as a wrapping `u16`.
3. If the gpiece lookup is null, set `valid=0`, store 1 to
   `Scene::recalc_deaths` at `+0x22F`, clear blocking when the type blocks while dead, and
   return.  This arm does **not** increment
   `cur_frame` or call `inc_bleed`.
4. If the gpiece exists but its packet is null, set `valid=0`, dirty deaths, and continue
   with duration 0.  Retail also visits its assertion/reporting machinery; the body exposes
   this case in its receipt rather than fabricating an animation.
5. Otherwise call `get_game_frames(cur_anim)`.  Increment `cur_frame` with signed 32-bit
   wrap.
6. Select the signed expiry threshold: `duration + 627` for a skeleton, `duration` for
   domain 1, and `duration + 135` otherwise.  All additions wrap as `i32`.
7. The instruction at `0x008D53CD` is signed `jl`; therefore equality expires.  On expiry,
   set `valid=0` and dirty deaths.  A missing-packet call can perform this dirty store a
   second time.
8. If the corpse blocks while dead, call `clear_blocking` when invalid.  When still valid,
   reload source hold and unconditionally store `max(hold,30)`.  This is a second hold store
   even when the reloaded value was already at least 30.
9. Call `DeathObjOut::inc_bleed(duration)` on every non-early path.

`DeathRecord.valid` and `cur_frame` are in the walked deaths channel; source hold is in the
walked units channel.  Bleed fields are in `DeathObjOut`, after the 76-byte `DeathObjData`,
and are not walked.  `clear_blocking` changes the collision/terrain world, so it is not
presentation and may not be replaced with a no-op.

## Adapter contract and atomicity

`DeathIncTimeWorld::prepare` is the only fallible operation.  Before `hold++`, it must
resolve:

- the original object identified by the corpse's `(who,o)` and its stable hold location;
- the original type's domain and `blocks_while_dead` result;
- the gpiece/packet and exact `get_game_frames(cur_anim)` result; and
- every terrain/collision resource needed if `clear_blocking` is reached.

The remaining adapter methods are infallible.  This makes a missing live fact leave both
checksum channels unchanged; after preparation, the adapter cannot return halfway through
the retail transaction.  `DeathIncTimeReceipt` retains repeated hold/dirty calls and the
exact bleed duration so an integration test can distinguish a final-state approximation
from the retail ordering.

The current lightweight `Sim` does not yet satisfy that contract: newly filed corpses do not
carry an authoritative gpiece/type resource pack, there is no exact `DeathObj::clear_blocking`
terrain adapter, and `Scene::recalc_deaths` has no live field.  Those are admission
requirements, not defaults.  In particular, supplying `MissingGraphicPiece` for every
synthetic corpse would immediately invalidate all of them and is not a shipped-data model.

## Mutation pins

The module-local tests are designed to kill the control-flow mutations most likely to create
a quiet desync:

| Mutation | Pin |
|---|---|
| move `hold++` after graphic validation | missing-gpiece event-order test |
| advance the clock on missing gpiece | early-return test |
| clear a nonblocking missing-gpiece corpse | nonblocking early-return test |
| turn missing packet into an early return | zero-duration continuation and bleed test |
| collapse repeated dirty stores | missing-packet/domain-1 double-dirty test |
| change expiry `>=` to `>` | equality-boundary test |
| apply 135 frames to domain 1 | domain-one test |
| test domain before `skel_gpiece` | skeleton-override test |
| collapse the two blocking hold stores | below-floor and above-floor event-order tests |
| use saturating/native-debug arithmetic | hold, clock, and threshold-add wrap tests |
| mutate before a failed resource lookup | rejected-preflight atomicity test |
| drift an address or constant | retail-address/constants test |

## Frozen integration hunk — do not apply before admission

Retail `Objects::inc_time` at `0x0065DB70` performs the ammo slot loop, then the death slot
loop, then Farm, Doober, and Surf.  Ammo damage may create a corpse, and that new corpse is
therefore visited by the death loop in the same step.  The exact shared-file placement is
frozen below.  `inc_time_deaths_after_ammo` denotes the not-yet-admitted live adapter described
above; it must iterate `self.deaths.slots` from index 0 upward and call only nonzero-valid
records.

```diff
diff --git a/crates/don-sim/src/systems/mod.rs b/crates/don-sim/src/systems/mod.rs
@@
 pub mod combat;
+pub mod death_inctime;

diff --git a/crates/don-sim/src/tick.rs b/crates/don-sim/src/tick.rs
@@ fn objects_inc_time(&mut self) -> (StepRun, u32) {
-        if self.ammo.live() == 0 {
-            return (StepRun::Vacuous, 0);
-        }
+        if self.ammo.live() == 0 {
+            // With no ammo, this is still the exact point where the death loop begins.
+            let work = self.inc_time_deaths_after_ammo();
+            return if work == 0 {
+                (StepRun::Vacuous, 0)
+            } else {
+                (StepRun::Executed, work)
+            };
+        }
@@ after the existing `for (slot, c) in calls` damage-application loop
+        // Retail loop setup 0x0065DC7D / call 0x0065DC9D: deaths run after all ammo
+        // and before Farms::inc_time at 0x0065DCB2.
+        // The adapter preflights each record atomically and charges missing facts as a gap.
+        work = work.wrapping_add(self.inc_time_deaths_after_ammo());
         (StepRun::Executed, work)
 }
```

The hunk intentionally does not clear `Gap::UnitIncTime`: step 15 still lacks admitted
Unit/Guy, building, Good, Farm, Doober, and Surf children.  It only closes the recovered
`DeathObj::inc_time` body once the live adapter meets the requirements above.
