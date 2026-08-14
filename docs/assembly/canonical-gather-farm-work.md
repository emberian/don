# Canonical saved Farm/Gather work transaction

Status: **production `Sim::unit_work`; four stable-animation grows, exact owner-2/o-5
animation 8→35 + Farm grow, and exact owner-2/o-9 MOVE arrival → animation 8→36 + Farm
snip; complete UnitGuys/Path/Farm owners; DoNSave v20 save/load/resume; zero RNG**.

Closure status remains **RED**. The owner-2/o-9 saved continuation now executes its exact
one-Guy land movement, Guy clock, MOVE completion, queued-Gather exposure, animation mutation,
and selected Farm snip. Collision-hit detour/repath arms, Farm relocation, periodic search,
animation-36 wrap/events, target retirement/replacement, and the remaining charged Gather
children still keep the row red.

The fresh save contains 17 Farm `GATHER` nodes. A structural walk and exact retail
branch audit found four bounded, checksum-changing continuations whose full mutable surface is
now owned. The original symmetric witness is owner 1 Unit `o=8,uid=16` gathering at Build
`o=2006,uid=12`, Farm index 12.
Retail requests the Guy's existing animation `0x23`, then `Farms::grow(12,2,2)` changes one
single-precision FarmStruct percentage:

```text
percent[2][2]: 0x3ea8f5bd -> 0x3eab8519
status[2][2]:  1          -> 1
RNG/order/Unit/Build/Guy/effects: unchanged
```

This is production credit for substantive saved ticks, not a scalar resource award and not a
zero-net surrogate. Every other Farm tail stays fail-closed unless its complete
FarmStruct, Guy, relocation and RNG surface is available.

## Shipped authority

| artifact/body | identity |
|---|---|
| `riseofnations.exe` | 9,925,120 bytes; SHA-256 `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079` |
| `rise.pdb` | 57,290,752 bytes; SHA-256 `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`; GUID `{51d4f219-61c6-4f84-9d5b-c3361b0d291f}` |
| `Unit::do_gather` | VA `0x005ef2a0`, size 3,780; SHA-256 `124ef94e77ca22ffd998ed3240eb94899962b749a659030963c89aa847329644` |
| `Farms::get_farm_type` | VA `0x008d9160`, size 47; SHA-256 `637119a1a48d315f327c6c5666fd70a360bbc865e6935bacbde3533b6469f01e` |
| `Farms::grow` | VA `0x008d91c0`, size 120; SHA-256 `c4fef6643766432d324e31fe0704a08e6c5564a8fafbac69a9b34ea174a688f6` |
| `Farms::snip` | VA `0x008d9240`, size 59; SHA-256 `41d07ee74b50174e2eedee6c13019ec2b1856d5fbc5d8d19a187beba172d7f6b` |
| `Unit::set_anim` | VA `0x00616f40`, size 201; SHA-256 `798f485753eb1853dc19ce55e43115674f6e3988370210dd9d5d4272d386f6ee` |
| `Guy::set_anim` | VA `0x005da300`, size 4,723; SHA-256 `be76d8eb8e4301d6c10888efa8b2ca1dde0ca02045f46b9c0c98b576d68f68b3` |
| `Wall::tile_corner` | VA `0x00643440`, size 136; SHA-256 `7eeec3717c4efa6ec1d9a60d05b8b1100bd50e8daf2d7d43e6b051b185ec96ba` |
| `Wall::covers_tile` | VA `0x006439b0`, size 90; SHA-256 `3a62e3ea46c48891d34b82030ded7efbda16bcca7985e7e1859734e18ed04879` |

The source save is `new save game 2026.08.11 15'42'57 (Tue).SVX`, compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7`,
decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
Frame is 1,199 and Objects begins at `0x4f21f`.

The Farm array is structurally decoded at `0xe35ca` with header
`(length,capacity,increment,flags)=(17,32,-1,0)`. Records begin at `0xe35d5`; their
17 × 190 walked bytes have SHA-256
`398c504bf711c6455091774082fc21926359704122757f3c64515155e8424b76`.
PDB `FarmStruct` size is 192, while the walker owns exactly:

```text
+0    i32 who
+4    i32 o
+8    float percent[16]
+72   float terrain_height[25]
+172  u8 status[16]
+188  u8 valid
+189  u8 farm_type
```

The exact Gather payload SHA-256 is
`b8bd2155f77f3539089b8ac63f2d705b271aaf0f7e4140fc16ee34cc6de0ab58`:
target `(who,o,uid)=(1,2006,12)`, `tx=ty=-1`, `build_type=0x1a1`, `wait=0`,
`(goto,non_flat,dist,been)=(1,0,0,1)`. Actor coordinate `(24216,1944)` maps to Farm
corner `(124,8)`, cell `(x,y)=(2,2)`. The Farm record has raw type zero, valid one and
retail `status[x][y]==1`. The lead Guy has animation `0x23`, hold-attack zero and
clock `33<47`.

The non-symmetric mutation-kill witness is owner 0 Unit `o=7,uid=14` at
`(48312,22584)`, targeting Build `o=2004,uid=4`, Farm index 2. Its Gather payload/node
SHA-256 values are respectively
`8a9c12ee3cb8894ffc4b1b15c946bd904b3742abb0ba914bb3d0df17fd3c7f77` and
`e6c544b192e13ee4af549aed6c969ea479c01a9a55cc32cc56ce1410d7a3bea8`;
the Unit, lead Guy and Farm images are
`c79cfafd1c32ab78fe19fb5f5ec9df14a873e20b0c47af5b5851d626ee9d7fa6`,
`ed910897ba05ca35929f19715b210e848216bc159c9c5b9ee9d2858fe34a67e6`, and
`3c74984a75b49f675b158665251a9a728003406ada808e3a1753b2b07354f24c`.
Its local cell is `(x,y)=(2,1)`: byte `status[2*4+1]` is one while the transposed
`status[1*4+2]` is two. The exact write is
`percent[2*4+1] 0x3e6147a9 -> 0x3e666661`; animation `0x23` and clock `22<47` are stable.

The formerly refused immediate animation-changing witness is owner 2 Unit `o=5,uid=11`,
at `(1464,33336)`, targeting Build `(who,o,uid)=(2,2004,4)` and Farm index 8. Its Gather
payload, node, Unit, Guys array, Guy, and Farm-record SHA-256 values are respectively
`10bfded57e642b88a27958d7e44fb131283d021d73aaf92761f9c64fbafea4f8`,
`0684e3b8534a7d7a32b31543e9b4805b3c4d6e5bdf4de3a72e2b0d9c2713ee2b`,
`b781d896a9f0a67ee2d1696f4867c975271cd161455fe6a1251a68c77fa506a5`,
`5a7e616a9bd534d24163a8d08e9ecf759cbbd41d64455926b6d7e09d9a09ec72`,
`bc8ee7257e525dae083b65db177e65d016061c8b0f910d0032d8d1778192b1f7`, and
`f488d2b03738a627dc17309fec79771a6092566c6a24a6d39b873c473a64128f`.
Farm record 8 spans `0xe3bc5..0xe3c83` and selects `status[2][1]==1`:

```text
Guy.cur_time:        1 -> 0
Guy.end_time:       15 -> 47
Guy.last_time:       0 -> -1
Guy.cur_anim:        8 -> 35
Farm percent[2][1]:  0x3da3d70a -> 0x3dae147b
order/Build/RNG/other 148 Guy bytes: unchanged
```

The literal call sequence at `Unit::do_gather` `0x005eff7b..0x005eff96` pushes
`(1,0,0x23)`, calls `Unit::set_anim`, then calls `Farms::grow`. `Unit::set_anim`
`0x00616f40` visits the initialized Guy prefix and forwards the same arguments. The shipped
type-50/gpiece-6336 animation packet makes the class-8 to class-35 arm reset exactly those
four Guy fields; no `Random::get` call is reached.

## Exact queued owner-2/o-9 continuation

The fresh save's owner-2 Unit `(o,uid,type)=(9,18,50)` is the first remaining Farm witness
whose Gather node is not current. The complete Unit record is `0x5982f..0x59a1b`, SHA-256
`bd554cbdddc2a13726a3ca10594da4deadeba0b879a92502c091744166567fdd`.
Its current coordinate is `(2013,32688)` and its fixed Unit destination is `(2232,32760)`.

The exact `Stack<PathData>` at `0x598db..0x598f4` has
`(capacity,length,increment)=(10,1,10)` followed by the one record
`(to_x,to_y,tolerance,flags)=(2232,32760,0,1)`. Its complete 25-byte walk has SHA-256
`34ebba9b0985dc83e49adfe8a7f563e8ee1fa8446de15e0a912364e930471c3a`.
DoNSave v20 now preserves both allocation fields rather than reconstructing them from the
logical records. `PathStack::walk_bytes` and transaction digests include the same nine-byte
header, so a capacity-only or increment-only divergence is observable.

The exact two-node OrderList is `0x598f4..0x5996e`, SHA-256
`01676b4d4e9dc4ded959b63302cb0660bad8c2f15e6aea7591721b83086af8c6`:

| execution position | node | exact evidence |
|---:|---|---|
| 0 | `MOVE_TO`, flags 1, `(x,y)=(2232,32760)`, angle `0x4ad30000`, dest 1, facing -1, offsets `(696,504)` | 77-byte payload SHA-256 `973ca9ab67548b69d3011e43756819caa3a5879a4762d9e2322ef6fa6e70c227`; 82-byte node SHA-256 `1cc91021f89a3d1f9529d437eee14300bc0e70cae7d81811a1b757023ff43350` |
| 1 | `GATHER` target `(2,2002,2)`, Farm property 417, `tx=ty=-1`, `(goto,nonflat,dist,been)=(1,0,0,1)` | 31-byte payload SHA-256 `59a882603b91746be7630220e7377f82628c2eb622b597dfac040e94bec4efd7`; 36-byte node SHA-256 `4d3a3cfda5dd0ac43de8f47725bc096ab44fc53c462c85187692d5b3985ca623` |

The Guys array at `0x5996e..0x59a1b` has exact shape `(1,1,1,0)` and SHA-256
`95e6f87337705c74c82eedcbdceb9fc4acc6f979105f0e050283c0180acea7ac`.
Its sole 155-byte Guy image has SHA-256
`b74f23ab73abd60305923d6659f7cdf8229c3262006a059a733da83585d86e15` and retains
animation 8, clock `3<15`, last speed 24, average speed 13, angle `0x4d1c0000`, and the
exact current/last/desire coordinates. Target Farm index 6 is `(who,o)=(2,2002)`; its
190-byte image at `0xe3a49..0xe3b07` has SHA-256
`fe4df1d7d1bb80e87f5ffb2239192db4f6c54737fcfd6f463a1703cdc7c89f67`.

The destination selects Farm-local `(x,y)=(3,2)`, whose x-major status byte is two. The Farm's
saved 5×5 x-major height image gives exact Guy ground Z 623 at the start and 626 at the
destination. Shipped type 50 contributes land domain, `myspeed=25`, Unit flags `0x1881`, one
squad Guy, radius one, and turn rule `0x20000000`; the save's Unit mask is `0x40008`.

`canonical_gather_queued_move_frontier.rs` reconstructs those literal Path, both nodes,
Guys, and Farm images and installs the exact type, constant, collision, and content projection.
The production continuation is:

```text
frame 1: Unit (2013,32688) -> (2036,32696)
         live delta heading find_angle(219,72) = 0x4d0b0000
         Guy last/current=(2013,32688)/(2036,32696), speed 24, avg 13->15
frames 2..10: exact one-Guy land movement + WALK clock, byte-identical across save/resume
frame 11: Unit/Guy arrive (2232,32760); MOVE retires; Path header remains (10,0,10);
          queued GATHER becomes current; Guy animation/clock remains 8 at 14/15
frame 12: Unit::set_anim(36,0,1) changes Guy (cur,end,last,anim)=(14,15,13,8)
          -> (0,85,-1,36); Farms::snip changes status[3*4+2] 2->3; zero RNG
```

The MOVE node's saved `0x4ad30000` angle is evidence, but it is not reused as the live heading:
`Unit::move_step` recomputes `find_angle` from the current delta. The first residual is below
retail's `0x02222220` ignore threshold, so the Guy snaps exactly to `0x4d0b0000` before
translation. This distinction is asserted directly.

Direct execution and save/load/rehydrate/resume are byte-identical after every frame. A stale
type-speed composition fails before changing Unit position/angle/masks, OrderList, PathStack,
UnitGuys, or Farms. The movement collision transaction also restores World, terrain collision,
collision runtime, and Path on driver/store/path rejection.

## Exact transaction and ownership

The host atomically revalidates actor Handle/identity/type, complete current Move/Gather queue,
Path header/top record, live type/constant/mask/speed projection, one-Guy collision source,
complete current Gather order,
and, for the animation-changing branch, the complete canonical UnitGuys array and all 155
walked lead-Guy bytes, Build
target/UID/valid/active/Farm property/city/Farm index, actor tile,
4x4 footprint and covers result, Unit periodic-search phase, Farm array header and record
identity, x-then-y cell bytes, Guy animation/clock/hold byte, frame, RNG, and installed
authority revision/digest.

The canonical `Farms` owner stores the exact array header and 190-byte records. Float values
remain raw IEEE-754 bits. `Farms::grow` is reproduced as the shipped single-precision add of
`0.005f` (`0x3ba3d70a`) and clamp at `1.0`; a crossing also changes status to two. This saved
input does not cross, so the only changed field is percentage bits above. Commit repeats
preparation before the first write and publishes the complete FarmStruct after-image. Its
receipt includes Farm index and before/after percentage bits.

DoNSave v19 adds the per-live-row optional UnitGuys section. A present row preserves capacity,
increment, flags, `guy_mark`, null topology, and every 155-byte Guy image without float/NaN
normalization. Formats v7–v18 restore rows as explicitly unmaterialized; they do not fabricate
empty Guy arrays. Whole-owner stale comparison is byte-based, so equal NaN payloads remain equal.
DoNSave v20 extends the existing Path section with the walked Stack capacity and signed-byte
increment. A v19 stream reconstructs the historical constructor/growth sequence; v20 retains
arbitrary imported allocation history exactly and rejects negative, undersized, or unbounded
capacity before allocation.
DoNSave v16 added the Farms section, including allocation metadata and every walked record.
The Build's existing `dock/farm/fort/oil_well` i16 union is accepted only for Farm type and
must bind exactly one valid Farm record `(who,o,index)`. Older saves load with an empty
default Farms owner. Load resets the installed content authority; callers must reinstall it before
work. The production witnesses prove direct and save/reload/reinstall/resumed `do_frame`
receipts and resaves are byte-identical. The partial simulation digest includes each owned
UnitGuys image and explicit row absence without claiming absent rows are retail-empty arrays.

## Exact census boundary and indexing correction

The executable does not use conventional row-major indexing here. At
`0x005eff2d..0x005eff4b`, `Unit::do_gather` forms local x and y separately, then addresses
`farm + 0xac + local_x*4 + local_y`: `status[x][y]`. Transposing that expression invents six
relocation and three snip witnesses which are not present in this save.

With the instruction-exact x-then-y lookup, the 17 images split into two type-one no-ops,
nine already-stable status-three animation no-ops, four already-stable status-one grows, and
two grow paths which require a Guy animation mutation. The four stable grows are owner/unit
`0/7`, `2/1`, `3/3`, and `1/8`; all are admitted by the same atomic production transaction.
The immediate owner-2/o-5 mutation is now admitted. Owner-2/o-9's complete saved frontier is
now admitted through MOVE completion and the exact status-two snip. Thus one fresh queued
Gather witness reaches snip; no fresh current Gather witness reaches relocation. Expired
animation, periodic
special effect, invalid Farm binding, Mine, capacity and retirement still refuse before a write.

Focused gates:

```sh
cargo test -p don-sim --test canonical_gather_farm_runtime
cargo test -p don-sim --test canonical_gather_farm_animation_runtime
cargo test -p don-sim --test canonical_gather_farm_xy_index
cargo test -p don-sim --test canonical_gather_queued_move_frontier
cargo test -p don-sim --test canonical_gather_runtime --test canonical_gather_saved_work
cargo test -p don-sim save_load --lib
cargo check -p don-sim
```
