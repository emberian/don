# Canonical saved Farm/Gather grow transaction

Status: **production `Sim::unit_work`; four stable-animation grows plus exact owner-2/o-5
animation 8→35 + Farm grow; complete UnitGuys owner; DoNSave v19 save/load/resume; zero RNG**.

Closure status remains **RED**. The second animation-changing Farm node, owner 2 Unit `o=9`,
is queued behind a live `MOVE_TO`; reaching it charges the still-open movement-completion child.
Relocation, snip, periodic search, and retirement children also remain charged.

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

## Exact transaction and ownership

The host atomically revalidates actor Handle/identity/type, complete current Gather order,
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
The immediate owner-2/o-5 mutation is now admitted. Owner-2/o-9 remains charged behind its
current MOVE_TO. No fresh witness reaches relocation or snip. Expired animation, periodic
special effect, invalid Farm binding, Mine, capacity and retirement still refuse before a write.

Focused gates:

```sh
cargo test -p don-sim --test canonical_gather_farm_runtime
cargo test -p don-sim --test canonical_gather_farm_animation_runtime
cargo test -p don-sim --test canonical_gather_farm_xy_index
cargo test -p don-sim --test canonical_gather_runtime --test canonical_gather_saved_work
cargo test -p don-sim save_load --lib
cargo check -p don-sim
```
