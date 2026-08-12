# Arena gather-enhancer founder lifecycle

Status: source-owned single-founder construction product inside the explicitly MODEL Arena.
This closes the measured 5x5 founder deadlock; it does **not** claim the complete retail
multi-unit `Group::action_swarm_around` body or whole construction parity.

## Shipped control-flow boundary

`Unit::do_build` `0x005EEBF0` does not let a non-Farm builder work from inside the target
footprint. Its failed-contact arm bare-kills the current `BUILD_AT`, creates a temporary
one-member Group, and calls `Group::action_swarm_around` `0x0070FBE0` with queue position
FIRST and order kind 6. The group body resolves a free coordinate through the 1,433-byte
location search at `0x0061DE70`. In the `BUILD_AT` arm it applies the visible `+/-0x30`
fine-coordinate adjustment, installs a coordinate move through `0x005E55C0`, then installs
the generational `(who,o,uid)` `BUILD_AT` through `Unit::add_build_order` `0x005E5210`.

The ordering is the important executable product:

```text
MOVE_TO(free approach coordinate)
BUILD_AT(target who, target owner-local o, target uid)
```

Arena previously installed only the second row. `Job::Work` then replaced any movement
prefix with a route to the building centre on every frame. Its final gate also approximated
contact as centre-to-centre tile distance <= 1. For a 5x5 building, every such tile is
inside the footprint, so no coordinate can be both accepted by that approximation and pass
retail's non-Farm covered-tile rejection. Natural Granary/Lumber Mill sites therefore
retained `build_left=1000` even after their commands had been accepted and paid.

## Arena transaction

The land-only, single-founder host now executes one coherent lifecycle:

1. Command admission and payment still allocate the ordinary persistent site and its
   `(who,o,uid)` identity.
2. The host walks the one-tile ring outside the exact half-open building footprint. It
   rejects off-map, impassable, or other-building-covered tiles, ranks the survivors by
   squared distance from the founder, then stable tile coordinates, and stores the chosen
   tile centre in a `MOVE_TO` prefix.
3. The existing movement driver executes that prefix with `UCELL` tolerance. There is no
   teleport and no construction credit while movement is active.
4. `Job::Work` preserves the stored destination instead of targeting the occupied centre.
5. The recovered `Unit::do_build` gate receives footprint-relative `covered` and
   one-ring `adjacent` facts. Only an uncovered contact tile can reach the existing
   construction transaction.
6. A later `Reswarm` plan requeues the same movement product ahead of the retained
   `BUILD_AT`; it contributes zero on that frame. If no free approach exists, the paid
   site and target order remain live with `MissingReswarmTransaction` rather than being
   completed, refunded, disbanded, or silently detached.

The planner consumes no RNG and changes no diplomacy, victory, death/reap, visibility,
economy, or research rule. Construction time still advances only through persistent
`BuildData` and the existing per-frame builder traversal.

## Evidence and remaining MODEL boundary

`crates/don-ai/tests/arena_enhancer_founder_lifecycle.rs` pins:

- exact 5x5 profiles for Granary 423, Lumber Mill 424 and Smelter 425;
- the `MOVE_TO -> BUILD_AT` queue shape and retained target identity;
- actual founder displacement to an uncovered footprint-relative contact tile;
- nonzero travel time and ordinary 1,000-frame `BuildData` completion, never an instant
  completion write;
- zero construction progress on the reswarm frame; and
- a fixed multi-seed/two-seat 30-minute policy cohort using only ordinary starting stock,
  gathering, completed-Market tax, University/Scholar knowledge, normal payment, and normal
  queue timers.

All three base enhancers complete exactly once in **4/4** measured runs, and Chemistry
completes in **4/4**. This convergence does not come from extra resources or a completion
write. The first trace isolated a policy payment race: after Chemistry, Scholar progression
bought each 32/34/36/38-wealth unit before the 50-wealth Smelter could become payable. The
policy now reserves that ordinary wealth window until `num_type_with_queued` observes the
paid Smelter. One dense seat then exposed a second cause: the generic capital-centred site
search had exhausted its bounded radius. Enhancers now use the already-reserved founder and
the same public placement predicate to choose local land, after which the ordinary stable
`MOVE_TO -> BUILD_AT` transaction performs every frame of the 1,000-frame build.

One run also pays and completes both Carpentry and Agriculture through their completed
producer buildings. The other runs preserve different food/metal spending choices at the
30-minute cutoff; they are not rewritten into research successes.

The exact `0x0061DE70` fine-coordinate search, multi-founder formation assignment,
temporary Group allocation/identity, reswarm animation receipt, dynamic crowd avoidance,
and full special-family `Build::activate` graph remain MODEL/RED. The Arena ring chooser is
the deterministic source-owned product for its one-founder land subdomain, not a claim that
those larger retail bodies have been reconstructed.
