# Canonical saved Farm/Gather grow transaction

Status: **production `Sim::unit_work`; exact fresh-SVX FarmStruct grow; DoNSave v16
save/load/resume; zero RNG**.

The fresh save has 17 active Farm `GATHER` orders.  A structural walk and exact retail
branch audit found one bounded, checksum-changing continuation whose full mutable surface is
now owned: owner 1 Unit `o=8,uid=16` gathers at Build `o=2006,uid=12`, Farm index 12.
Retail requests the Guy's existing animation `0x23`, then `Farms::grow(12,2,2)` changes one
single-precision FarmStruct percentage:

```text
percent[2][2]: 0x3ea8f5bd -> 0x3eab8519
status[2][2]:  1          -> 1
RNG/order/Unit/Build/Guy/effects: unchanged
```

This is production credit for that substantive saved tick, not a scalar resource award and
not a zero-net surrogate.  Every other Farm tail stays fail-closed unless its complete
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
row-major `status[y][x]==1`. The lead Guy has animation `0x23`, hold-attack zero and
clock `33<47`.

## Exact transaction and ownership

The host atomically revalidates actor Handle/identity/type, complete current Gather order,
Guys-array shape, Build target/UID/valid/active/Farm property/city/Farm index, actor tile,
4x4 footprint and covers result, Unit periodic-search phase, Farm array header and record
identity, row-major cell bytes, Guy animation/clock/hold byte, frame, RNG, and installed
authority revision/digest.

The canonical `Farms` owner stores the exact array header and 190-byte records. Float values
remain raw IEEE-754 bits. `Farms::grow` is reproduced as the shipped single-precision add of
`0.005f` (`0x3ba3d70a`) and clamp at `1.0`; a crossing also changes status to two. This saved
input does not cross, so the only changed field is percentage bits above. Commit repeats
preparation before the first write and publishes the complete FarmStruct after-image. Its
receipt includes Farm index and before/after percentage bits.

DoNSave v16 adds the Farms section, including allocation metadata and every walked record.
The Build's existing `dock/farm/fort/oil_well` i16 union is accepted only for Farm type and
must bind exactly one valid Farm record `(who,o,index)`. Older saves load with an empty
default Farms owner. Load resets Guy/content authority; callers must reinstall it before
work. The production witness proves direct and save/reload/reinstall/resumed `do_frame`
receipts and resaves are byte-identical.

## Exact census boundary

Using retail's row-major `status[y][x]`, the 17 images split into two type-one no-ops, six
two-RNG relocations, five grow paths, three snip paths and one animation-only no-op. Of the
five grow paths, this specimen alone already has the requested animation and unexpired
clock; the other four require canonical mutable Guy ownership. Relocation, snip, status
transition, animation change, expired animation, periodic special effect, invalid Farm
binding, Mine, capacity and retirement all refuse before any write.

Focused gates:

```sh
cargo test -p don-sim --test canonical_gather_farm_runtime
cargo test -p don-sim --test canonical_gather_runtime --test canonical_gather_saved_work
cargo test -p don-sim save_load --lib
cargo check -p don-sim
```
