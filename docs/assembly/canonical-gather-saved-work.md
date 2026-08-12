# Canonical saved Gather work transaction

Status: **production `Sim::unit_work` hook; revision-bound fresh-SVX Camp authority;
wait loop plus exact wait-zero/all-gathering/RNG tail**.

The full fresh Unit census found 29 `GATHER` nodes, making Gather the dominant concrete
saved executor.  `crates/don-sim/src/systems/canonical_gather_work.rs` binds all 29 exact
31-byte retail payloads and mounts one substantive branch shared by the 12 Camp payloads.
`crates/don-sim/tests/canonical_gather_saved_work.rs` carries every exact payload image and
SHA-256, mutation/fail-closed gates, PE/PDB anchors, atomic-host checks, and a DoNSave
save/reload/resume/resave test. `canonical_gather_runtime.rs` proves the production
`do_frame` hook and wait-zero tail.

This is not the earlier scalar `target + distance + GatherOutcome` approximation.  The
transaction snapshots the complete actor/order/build slice and either publishes every
recovered write together or publishes none.

## Retail evidence

The matched executable SHA-256 remains
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
The matched `rise.pdb` is 57,290,752 bytes, SHA-256
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`, GUID
`{51d4f219-61c6-4f84-9d5b-c3361b0d291f}`.

| body | VA | PDB size | SHA-256 |
|---|---:|---:|---|
| `Unit::do_gather(UnitOrder*)` | `0x005ef2a0` | 3,780 | `124ef94e77ca22ffd998ed3240eb94899962b749a659030963c89aa847329644` |
| `Unit::do_non_flat_gather(GatherOrder*)` | `0x005f0170` | 4,766 | `7f53403e10e71074f24a31ce13ca527dab71f876d15b11ef4fe75e5ea9231145` |
| `BuildData::is_gathered_by(int)` | `0x0062f520` | 74 | `976946e1dc69614a031f5fd9b6efdd058189bf38c8f1f55dbe5f3a939e8dd4d0` |
| `BuildData::num_gatherers(int,int)` | `0x00630450` | 305 | `2a19c878666ab286e63b7649cece89096e2d837b6c4c9bc1607cb0ceaf828e56` |
| `Build::all_gathering()` | `0x0062f570` | 198 | `50fcfef2cd8269a592953fa6ad829a3180ea50450e8cb06a853aa2b4adbea2d6` |
| `Unit::kill_current_order(int)` | `0x005e2cb0` | 1,312 | `9768692adfeef5d09c77bed21c9a2051979d4aa34d3dd8f7b3044ed536770606` |
| `Unit::set_anim(UnitAnim,int,int)` | `0x00616f40` | 201 | `798f485753eb1853dc19ce55e43115674f6e3988370210dd9d5d4272d386f6ee` |
| `Unit::do_job(OrderIndex,UnitOrder*)` | `0x00617a10` | 500 | `461d699806e595485fa6c0aef8c4054c195fcc23e589815b6a2ceb110d1ec71a` |

The PDB sizes come from `re/symtab.json`; hashes cover the whole mapped procedure bodies.
The focused Rust artifact gate also pins these exact instruction sites:

- `0x005f0136`: `do_gather` pushes the same order and calls `do_non_flat_gather`;
- `0x005f01e5..0x005f0231`: test building mask `0x800`, increment signed-short
  `recharging` at `+0x7a`, and set the mask at `+0x60`;
- `0x005f0db4..0x005f0dd9`: read lead-Guy animation `+0x9c`, return on `0x1d`, select
  animation `0x19`, decrement `GatherOrder::wait` at concrete `+0x20`, and return when the
  result remains nonzero; and
- `0x005f0ddf` onward: only a zero result reaches `Build::all_gathering`; false consumes
  exactly one `Random::get(0,0xffff)` draw and stores `draw % 100 + 300`, while true stores
  `wait=-1` without RNG.

The Gather `do_job` call is unique at `0x00617a72`; its ordinary caller is `Unit::work` at
`0x0060dad8` (the other direct caller is `Animal::work` at `0x005d734f`).

## The 29 exact saved images

The source SVX is compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and decompressed
SHA-256 `fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
The prior census already pins every individual payload hash and the complete order manifest
`3bc050ab7e0d49c9529ed879f0828217e664cbb3f778b1eb0c5ac5f4e1ad6dc9`.

The 29 payloads reduce to two exact state shapes, without normalizing any individual target,
coordinate, timer, or UID:

| count | `build_type` | exact mutable suffix shape | transaction status |
|---:|---:|---|---|
| 17 | `0x1a1` Farm | `tx=ty=-1`, `wait=0`, `(goto,non_flat,dist,been)=(1,0,0,1)` | one exact grow image admitted by the [Farm continuation](canonical-gather-farm-work.md); residuals fail closed |
| 12 | `0x1a2` Camp | `tx,ty>=0`, `wait>0`, `(goto,non_flat,dist,been)=(0,1,4,1)` | admitted to the recovered timer branch when the live snapshot gates match |

Every node also has metric zero, flags zero, a same-owner Build-band target, and a positive
target UID.  The test decodes all 29 literal payloads, re-encodes all 29 byte-for-byte,
checks 29 distinct census SHA strings, and obtains exactly the 17/12 classification above.
Changing metric, flags, target owner, target band, or any shape discriminator rejects the
image.

Each of the 12 Camp witnesses also has a Guys array of exact shape `(length=1, capacity=1,
increment=1, flags=0)`, with slot zero present and one 155-byte `GuyData` image.  That saved
container fact supports the branch family, but a resumed runtime must still atomically prove
that the live slot-zero Guy exists and read its current animation.

## Exact admitted branch

The branch is deliberately narrow.  A coherent snapshot must prove:

1. one of the exact fresh Camp payload shapes;
2. actor identity `(who,o,uid)` and the complete order image;
3. a resolved same-owner Build/Wall target whose virtual `is_valid_wall` and `is_active`
   gates pass, whose `(who,o,uid)` exactly equals the order, and whose current property still
   equals `build_type=0x1a2`;
4. `(frame + actor.o * 4) & 0x7f != 0`, so the 128-frame capacity/count branch is not due;
5. a live slot-zero Guys body and lead `GuyData::animation == 0x19`; and
6. `wait > 0`. `wait>1` takes the no-RNG return; `wait==1` additionally requires the exact
   canonical gather-chain projection described below.

The whole after-image is then:

```text
actor.group          := -1
actor.unit_masks     := actor.unit_masks & ~0x78000000
if site.build_masks & 0x0800 == 0:
    site.recharging  := wrapping_i16(site.recharging + 1)
    site.build_masks := site.build_masks | 0x0800
order.wait           := wrapping_i32(order.wait - 1)
rng_state            := unchanged
external effects     := none
```

When `wait==1`, the installed runtime adapter constructs the exact owner-local
`BuildData::gather_down`/`UnitData::gather_down` chain from canonical World rows. It runs
`Build::check_gatherers`, recording every unlink in the same transaction, then checks every
survivor's concrete current Gather order. A chain is all-gathering exactly when every
survivor has `goto_build==0` and `wait>=0` (the empty chain is true). The zero tail is:

```text
order.wait := 0
if all_gathering:
    order.wait := -1
else:
    rng_state := rng_state * 0x0019660d + 0x3c6ef35f
    draw       := ((low16(rng_state) * 0xffff) >> 16)
    order.wait := draw % 100 + 300
```

Types `0x34/0x35` in the chain remain fail-closed because their
`is_gathering_at` path consults containment before the order. Ordinary Camp workers use the
canonical type, validity, identity, current order, and intrusive-link owners; there is no
boolean all-gathering oracle.

Planning does not mutate.  `AtomicGatherWorkHost::compare_exchange` must compare the full
actor/order/site/RNG/revision before-image and publish all changed fields in one commit.
The receipt repeats the complete after-image, authority revision transition, exact changed
field count, zero RNG draws, and zero external effects.  A stale snapshot, unavailable host,
or mismatched receipt is not success.

## Explicit fail-closed boundary

| branch | missing exact owner | result |
|---|---|---|
| Farm tick | one exact FarmStruct grow now owned; residual animation/relocation/snip tails | one saved grow commits; residuals no commit |
| Mine tick | MiningList/object identity, terrain and collision | no commit |
| 128-frame Camp/Mine phase | exact gather chain and capacity/count result | no commit |
| `goto_build != 0` | destination selection, access, collision and movement insertion | no commit |
| animation other than `0x19` | animation/location-specific resource and movement arms | no commit |
| `wait == 1`, ordinary exact chain | canonical chain cleanup and all-gathering; one exact RNG draw only when false | admitted atomically |
| `wait == 1`, scholar/special chain member | containment-first `is_gathering_at` facts | no commit |
| stale/dead target | Gather retirement, replacement search and queue surgery | no commit |
| direct resource or special/cast gather | object/type-specific resource/cast authority | no commit |

In particular, the executor does not award a scalar yield, guess a distance, manufacture a
terrain tile, count workers from a number, or silently consume an RNG draw.

## Production authority and save/reload/resume witnesses

`GatherWorkAuthority` is an installed, non-serialized adapter keyed by stable Unit Handle and
Build `(who,o,uid)`. A nonzero composition digest and revision bind exact type, Guys-array
`(length,capacity,increment,flags)=(1,1,1,0)`, slot-zero presence/animation, and the target's
Build/Wall virtual projection. Mutable Unit, GatherOrder, Build latch/recharge/chain, frame,
and RNG bytes remain canonical `Sim` owners. Load restores those canonical owners but resets
the adapter to default, so Gather work fails closed until content reinstalls the matching
authority.

The integration test installs the exact first Camp image from the fresh SVX on owner 0 Unit
`o=3`, targeting Build `o=2001, uid=1`.  Its `wait=295`, `tx=270`, `ty=116`, and payload
SHA-256 is
`3d4d8ca9d0bd71b0f4af4416425953cd82c0fa5c452a5fba1a5e18f50615f0f2`.

The original isolated witness:

1. saves the production `Sim` through DoNSave and reloads it;
2. reconstructs the atomic snapshot from the reloaded canonical Unit, order, Build, frame,
   and RNG owners plus the explicit lead-animation/type gates;
3. runs the transaction on both the direct and reloaded controls;
4. writes the attested whole after-image into the canonical owners;
5. proves both saves are byte-identical; and
6. reloads the post-tick save and proves `wait=294`, group `-1`, cleared action masks,
   building mask `0x800`, `recharging=1`, and unchanged RNG survive.

The production witness now installs that authority on both the direct and reloaded controls,
executes the ordinary object pass through `Sim::do_frame`, and proves byte-identical saves
after the frame. A second save/reload/do-frame witness starts at `wait=1` with an empty
gather chain, proves the exact all-gathering `wait=-1` result, and proves zero RNG draws.
Focused direct tests cover a nonempty false chain (one exact LCG draw and animation-`0x19`
reschedule), stale compare/exchange, exact `check_gatherers` unlink publication, malformed
Guys authority, and zero writes on every refusal.

This is production credit for the exact Camp branches above, not general Gather closure:
Residual Farm, Mine, capacity phase, destination/collision, resource payout,
retirement/replacement, and containment-special arms remain explicit boundaries.

Run the focused gate with:

```sh
CARGO_TARGET_DIR=/tmp/don-gather-runtime-target \
  cargo test -p don-sim --test canonical_gather_saved_work \
  --test canonical_gather_runtime
```
