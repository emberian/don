# Fresh SVX Unit/OrderList census

Status: **complete bounded Unit census; exclusive parser/evidence tranche; no runtime or
closure edit**.

The fresh retail v16 save has 800 present Unit bodies.  A structure-derived walk reaches
all 800, consumes the 800 interleaved Build bodies needed to reach later owners, and
enumerates 43 concrete order nodes and 55 `GuyData` images.  It does not search for tags or
fit byte patterns.  The caller supplies the exact `Objects::walk_data` boundary and the
parser follows the shipped container and virtual-body grammar in program order.

The parser is `re/scripts/savegame_unit_orderlist_census.py`; the six synthetic, mutation,
artifact, executable, and canonical-owner gates are in
`re/scripts/test_savegame_unit_orderlist_census.py`.

## Exact structural scope

The source is `new save game 2026.08.11 15'42'57 (Tue).SVX`, compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and decompressed
SHA-256 `fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
The independently derived `Objects::walk_data` boundary is `0x4f21f`.

`Objects::walk_data` iterates nine owner arrays.  Each nonempty container is read as its
length/history header, presence plane, concrete-type plane for present slots, duplicated
history, then present bodies.  The exact owner map is:

| owner | array/body range | header length | present concrete types | body end |
|---:|---|---:|---|---:|
| 0 | `0x4f2ae..0x504b7` | 3000 | 200 Unit (0), 200 Build (1) | `0x53dfc` |
| 1 | `0x53dfc..0x55005` | 3000 | 200 Unit (0), 200 Build (1) | `0x57786` |
| 2 | `0x57786..0x5898f` | 3000 | 200 Unit (0), 200 Build (1) | `0x5b38e` |
| 3 | `0x5b38e..0x5c597` | 3000 | 200 Unit (0), 200 Build (1) | `0x5f4a1` |
| 4 | `0x5f4a1..0x5f4a5` | 0 | none | `0x5f4a5` |
| 5 | `0x5f4a5..0x5f4a9` | 0 | none | `0x5f4a9` |
| 6 | `0x5f4a9..0x5f4ad` | 0 | none | `0x5f4ad` |
| 7 | `0x5f4ad..0x5f4b1` | 0 | none | `0x5f4b1` |
| 8 | `0x5f4b1..0x6039a` | 3000 | 200 Good (3) | not consumed |

The owner-8 type plane is a structural endpoint, not a guessed byte boundary: all 200
present concrete types are Good and none is Unit.  Stopping at its first body (`0x6039a`)
therefore completes the Unit census without claiming to parse Good bodies.

For every Unit the route is `SubObject -> Object -> Unit -> Stack<PathData> -> OrderList ->
PtrArray<Guy> -> GuyData`.  Active Build bodies are consumed through `Wall`, `Build`,
`BuildQueue`, `MiningList`, `Array<TCoordData>`, and `GatherPointList`; inactive inherited
gates take their exact short branches.  The census is:

- Units: 800 present; 47 active and 753 inactive.
- Builds: 800 present; 37 active and 763 inactive.
- Guys: 55 exact 155-byte images.  Active Guys arrays are 41 `(len=1,cap=1)`, four
  `(len=2,cap=2)`, and two `(len=3,cap=3)`; every increment is 1 and flags byte is zero.
- Path stacks `(capacity,length,increment)`: 32 `(10,0,10)`, four `(20,0,10)`, three
  `(10,1,10)`, two `(30,1,10)`, and one each of `(30,2,10)`, `(40,11,10)`, `(30,0,10)`,
  `(50,0,10)`, `(20,1,10)`, and `(50,23,10)`.

Exact compact manifests pin every traversed row, boundary, and image hash:

| manifest | SHA-256 |
|---|---|
| 43 order rows | `3bc050ab7e0d49c9529ed879f0828217e664cbb3f778b1eb0c5ac5f4e1ad6dc9` |
| 800 Unit rows | `b6f265645c006ff3432ca71f7ac852df020ec2804054666f24dc4f8df899d580` |
| 55 Guy rows | `1bf44c19f2efa2554809023176e50621ad5b1d3643cd18f8d5cbe52832c6a17f` |

The command's `--json` form emits every Unit, Build, Path, Guy, and order boundary, exact
image SHA-256, decoded payload field, and exact payload hex.

## Retail order grammar

`OrderList::walk_data` writes `count:i32`, then for each node:

```text
concrete OrderIndex : i32
RecycledOrderNode.metric : u8
UnitOrder.flags : u8
concrete virtual-walk fields
```

There is **no retail payload tag** after the metric.  That byte is `UnitOrder::flags`.
The observed concrete payload sizes below include that flags byte:

| types | family | bytes after metric | decoded retail fields |
|---|---|---:|---|
| 1--4 | MoveOrder | 77 | flags; 18 i32 movement words; two i16 offsets |
| 6 | TargetOrder | 11 | flags; target `o:i32`, `who:i32`, `uid:u16` |
| 7 | GatherOrder | 31 | TargetOrder; four i32 gather words; four u8 states |
| 14 | CastOrder | 27 | TargetOrder; x, y, paid, spell as four i32 |

All 43 metrics are zero.  Flags are zero on 30 rows, one on nine rows, and four on four
rows.  The concrete-type counts are Gather 29, ExploreTo 8, BuildAt 4, CastSpell 1, and
MoveTo 1.

## Every observed retail payload image

The range covers the complete node (`OrderIndex`, metric, and concrete payload).  The hash
is over the payload beginning with `UnitOrder::flags`; the parser also reconstructs every
payload byte-for-byte from decoded fields before accepting it.

| # | owner/slot | Unit `(who,o)` | type | metric | flags | node range | payload SHA-256 |
|---:|---:|---:|---|---:|---:|---:|---|
| 0 | 0/0 | `(0,0)` | 3 ExploreTo | 0 | 1 | `[0x50590,0x505e2)` | `58bfed302bc0482e983252de1360a5cdd8440e2339c95312c17093a646eb98e3` |
| 1 | 0/2 | `(0,2)` | 3 ExploreTo | 0 | 1 | `[0x50b32,0x50b84)` | `5efa1621c11e77f6694f637177902cdcdac5729c263b2a2dd5d48d1fdc05085d` |
| 2 | 0/3 | `(0,3)` | 7 Gather | 0 | 0 | `[0x50e22,0x50e46)` | `3d4d8ca9d0bd71b0f4af4416425953cd82c0fa5c452a5fba1a5e18f50615f0f2` |
| 3 | 0/4 | `(0,4)` | 7 Gather | 0 | 0 | `[0x50fac,0x50fd0)` | `ee2428e332615c830cf9e6c4a06630d13e49686b37b9a07c00166a2b6b11194e` |
| 4 | 0/6 | `(0,6)` | 7 Gather | 0 | 0 | `[0x5129c,0x512c0)` | `85cd4324ee00922c840d5ea974a3ace8efcfdad2961b3a8049fac9671778f23e` |
| 5 | 0/7 | `(0,7)` | 7 Gather | 0 | 0 | `[0x51426,0x5144a)` | `8a9c12ee3cb8894ffc4b1b15c946bd904b3742abb0ba914bb3d0df17fd3c7f77` |
| 6 | 0/8 | `(0,8)` | 7 Gather | 0 | 0 | `[0x515b0,0x515d4)` | `24e77ef2f7584057a6682a7c7255c9fe6aeca1b1579d58901eb37bc51f478043` |
| 7 | 0/9 | `(0,9)` | 7 Gather | 0 | 0 | `[0x5173a,0x5175e)` | `2f51227e20951ac115445c289fe2016aaaf3f754ba62ece746580d50259e4bd7` |
| 8 | 0/10 | `(0,10)` | 7 Gather | 0 | 0 | `[0x518c4,0x518e8)` | `32d9ba145f863b5b9946540fa95939ffdb4c48153aac7e8565f705ec19563278` |
| 9 | 0/14 | `(0,14)` | 3 ExploreTo | 0 | 1 | `[0x51e90,0x51ee2)` | `3c7df2e810a058de6c28b4d5b8665e84e9f12b6580ff237223b4acf51912c34e` |
| 10 | 0/16 | `(0,16)` | 14 CastSpell | 0 | 0 | `[0x521ae,0x521ce)` | `81cdba3f17e0185b23b1179e44b0b40fd841e2d0b819baa965a44d577bf62ab9` |
| 11 | 1/0 | `(1,0)` | 3 ExploreTo | 0 | 1 | `[0x550ce,0x55120)` | `8a5755e6b73832f0b853dcc04e342463615a2b5e85920b1275cf7135980ba3ac` |
| 12 | 1/1 | `(1,1)` | 3 ExploreTo | 0 | 1 | `[0x55332,0x55384)` | `4bf4d7f3287465661910acf1cc5ddf39f54ec63805f917e258f4578302db3d97` |
| 13 | 1/1 | `(1,1)` | 6 BuildAt | 0 | 4 | `[0x55384,0x55394)` | `6d600ef826c2e88730aa496adecf86e41eebbcd036cb2598bdfec704d001913c` |
| 14 | 1/2 | `(1,2)` | 7 Gather | 0 | 0 | `[0x554fa,0x5551e)` | `3a2b6ace2296cd90035afa63fd49f6f93b12163880793988257a8006c71b8fe6` |
| 15 | 1/3 | `(1,3)` | 7 Gather | 0 | 0 | `[0x55684,0x556a8)` | `a550e81535c305dc48e4a65cbd3f127b1f9e0701bf16f85644a1f60777708cb9` |
| 16 | 1/4 | `(1,4)` | 7 Gather | 0 | 0 | `[0x5580e,0x55832)` | `91649de6ff0e80aaa00ece4a31dc7c2b58b6a09a09d0c489043d520a1caed9e3` |
| 17 | 1/5 | `(1,5)` | 7 Gather | 0 | 0 | `[0x55998,0x559bc)` | `d329f9281993d88c6092b0c4b47e85ac4b8dffee9d2f6d8c29236bb592bcfaca` |
| 18 | 1/6 | `(1,6)` | 7 Gather | 0 | 0 | `[0x55b22,0x55b46)` | `76dbc21b39dd68697722e21bc428cf390d7eb2f3b38b45bcdcca55c087a89ff2` |
| 19 | 1/7 | `(1,7)` | 7 Gather | 0 | 0 | `[0x55cac,0x55cd0)` | `81caf3b9ef8499b9af9d92628e198abc2ab34fefe85af01a331627ce59183874` |
| 20 | 1/8 | `(1,8)` | 7 Gather | 0 | 0 | `[0x55e36,0x55e5a)` | `b8bd2155337b5943fe0377e0ef7f4c3fe16bfd8ad628465892e39d96c3e0ab58` |
| 21 | 2/0 | `(2,0)` | 3 ExploreTo | 0 | 1 | `[0x58a58,0x58aaa)` | `2b66a852315e841069d4e5d46c5d43a9f2421611bbcb39a1736eec23e60807f3` |
| 22 | 2/1 | `(2,1)` | 7 Gather | 0 | 0 | `[0x58cac,0x58cd0)` | `24b2f50867ffc0f84e41b3fa4694910d5fc32d0caddb29567c74ecf44f753c6d` |
| 23 | 2/2 | `(2,2)` | 7 Gather | 0 | 0 | `[0x58e36,0x58e5a)` | `84382088811bef103f5da1fbd40bdc1aaade0f50536e993d85c95c2b24e68ede` |
| 24 | 2/3 | `(2,3)` | 6 BuildAt | 0 | 4 | `[0x58fc0,0x58fd0)` | `026217de3bb8dbbaafc4dbde5f641bedd55c3b4144ff9f0147ad270c46dcc895` |
| 25 | 2/4 | `(2,4)` | 7 Gather | 0 | 0 | `[0x59136,0x5915a)` | `bfb9bc35f3bd316b11563ee216f937a8fa86fa1798cd6839615ffb1f726a93d3` |
| 26 | 2/5 | `(2,5)` | 7 Gather | 0 | 0 | `[0x592c0,0x592e4)` | `10bfded57e642b88a27958d7e44fb131283d021d73aaf92761f9c64fbafea4f8` |
| 27 | 2/6 | `(2,6)` | 7 Gather | 0 | 0 | `[0x5944a,0x5946e)` | `fbf5c8443178cf3f23697f21b3ca8fa89178220b9e65f5d8f572dcb6656a3020` |
| 28 | 2/7 | `(2,7)` | 7 Gather | 0 | 0 | `[0x595d4,0x595f8)` | `7472c4c64a228ad7105a41748ad19a14f0f0ebd3eafc31ca9f0c5815d88db504` |
| 29 | 2/8 | `(2,8)` | 7 Gather | 0 | 0 | `[0x5975e,0x59782)` | `4056cf959a2014e509fabcff9b66c2c329519b3b34f64c66d5b07470e68f9c3b` |
| 30 | 2/9 | `(2,9)` | 1 MoveTo | 0 | 1 | `[0x598f8,0x5994a)` | `973ca9ab67548b69d3011e43756819caa3a5879a4762d9e2322ef6fa6e70c227` |
| 31 | 2/9 | `(2,9)` | 7 Gather | 0 | 0 | `[0x5994a,0x5996e)` | `59a882603b91746be7630220e7377f82628c2eb622b597dfac040e94bec4efd7` |
| 32 | 3/0 | `(3,0)` | 3 ExploreTo | 0 | 1 | `[0x5c7c0,0x5c812)` | `e65316e6e650dd1d7352cb62f123dc2c971d13a3a9b93c1210be28bdcc851724` |
| 33 | 3/1 | `(3,1)` | 7 Gather | 0 | 0 | `[0x5ca14,0x5ca38)` | `b6320a93dddc9d14a72f25154424ce6b5ac6e66c346a64d31672995014e0e065` |
| 34 | 3/2 | `(3,2)` | 7 Gather | 0 | 0 | `[0x5cb9e,0x5cbc2)` | `55b5bb27f417f48ddfdfd5b7bbf2e34128f0670cd6c5d3f47bee92ad52f6c40b` |
| 35 | 3/3 | `(3,3)` | 7 Gather | 0 | 0 | `[0x5cd28,0x5cd4c)` | `f3351cdc6b35f2cdc5e59890bfb24d67a5c6cf1a2f26a060df528ba2b3b6cd18` |
| 36 | 3/4 | `(3,4)` | 7 Gather | 0 | 0 | `[0x5ceb2,0x5ced6)` | `046c8184c5b2fb722b7dca6a87a80272b52e668a8299110216ed829c2eba9be3` |
| 37 | 3/5 | `(3,5)` | 7 Gather | 0 | 0 | `[0x5d03c,0x5d060)` | `ed5a11ae0a2a3dcbc5a456010879dd3c8d65b6549554cdbb7f8d0568319df182` |
| 38 | 3/6 | `(3,6)` | 6 BuildAt | 0 | 4 | `[0x5d1c6,0x5d1d6)` | `35722375d0d35579d22740a5c1c1db9e0be39d554a73bc62bc95cf10396dbe6c` |
| 39 | 3/7 | `(3,7)` | 7 Gather | 0 | 0 | `[0x5d33c,0x5d360)` | `75847d72c000082d7cdf02c4aae8732a364339a4547311a380eea072a532e292` |
| 40 | 3/8 | `(3,8)` | 7 Gather | 0 | 0 | `[0x5d4c6,0x5d4ea)` | `2f8dceb36cb4fdf8b08042f6d1f12cd329eb7300ffb6d959e355c88d60f9f77e` |
| 41 | 3/9 | `(3,9)` | 3 ExploreTo | 0 | 1 | `[0x5d660,0x5d6b2)` | `580d12c3e6260c2b7ad682506c9e8b3a85726a9d5fb0692319ce8617873805b0` |
| 42 | 3/9 | `(3,9)` | 6 BuildAt | 0 | 4 | `[0x5d6b2,0x5d6c2)` | `10dbf4fc976e5ccc0f9a8d13c952eb6bd629f877bc6d89a99a08342c141db29b` |

## Canonical Order/v13 authority comparison

The numbers in the `v13 tag` column are internal DoNSave discriminators.  They are not
bytes in the retail images above.

| observed retail family | rows | canonical owner | internal v13 tag | fresh-image comparison |
|---|---:|---|---:|---|
| MoveOrder (MoveTo/ExploreTo) | 9 | `Order.move_state` | 1 (Move) | All 77 retail bytes decode and rebuild losslessly.  v13 additionally owns generic target/handle and group defaults. |
| TargetOrder (BuildAt) | 4 | generic `Order` target | 0 (None) | All targets are in the Build/Wall object band (`o >= 2000`), so the retail identity is save-owned without an additive Unit `Handle`. |
| GatherOrder | 29 | `EconomyOrderPayload::Gather` | 2 (Gather) | All targets are in the Build/Wall band; every target and 20-byte gather suffix maps losslessly. |
| CastOrder | 1 | `EconomyOrderPayload::CastSpell` | 3 (CastSpell) | The target is the exact `(-1,-1,65535)` no-target sentinel; x, y, paid, and spell map losslessly. |

Thus every observed fresh payload has a lossless canonical semantic owner, while retail
and DoNSave remain deliberately different wire formats.  In particular, DoNSave writes an
internal compact kind and later payload discriminator, whereas retail writes an i32
`OrderIndex`, node metric, inherited flags, and concrete virtual payload without a payload
tag.  A future SVX importer must translate between those formats; it must not reinterpret
the retail flags byte as the v13 discriminator.

## Executable and mutation gates

The matched executable SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
The test maps and pins full body spans for `PtrArray<Guy>`, `GuyData`, `BuildData`,
`WallData`, `BuildQueue`, `Array<TCoordData>`, `GatherPointList`, `GatherPoint`, shared
`TargetOrder`, `GatherOrder`, and `CastOrder`.  These extend the already pinned Objects,
Unit, Path, OrderList, and Move bodies from the first-witness tranche.

Synthetic coverage includes all four observed payload families with nonzero metrics and
flags.  Mutations kill Objects/SubObject/Object/Unit/Build tags, both container history
planes, inherited gates, a path length, order type, Guy presence, truncation, and a decoy
tag prefix.  The fresh artifact gate pins both file hashes, all counts and owner boundaries,
the three manifests, and byte-for-byte reconstruction of all 43 payloads.

Run the exact gates with:

```sh
python3 re/scripts/test_savegame_unit_orderlist_census.py -v
python3 re/scripts/savegame_unit_orderlist_census.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --objects-offset 0x4f21f
```

The next structure boundary, if a broader Objects census is desired, is the first owner-8
Good body at `0x6039a`.  It is not required to settle the Unit/OrderList census and this
tranche claims no opcode, replay-host, or save-closure credit.
