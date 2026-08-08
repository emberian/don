# The simulation state schema, from the shipped PDB

**Status: [measured].** Every layout in this document is the *compiler's own record* of
what it emitted for the binary we are reverse-engineering. It is not a hypothesis, not a
Ghidra reconstruction, and not a wiki claim. Where this document disagrees with earlier
derivation notes, this document wins and the earlier note is corrected here explicitly.

Nothing here is a fidelity-tier claim. Tiers A/B/C describe *behavioural* agreement of our
Rust with retail; this document describes *structure*, which the PDB states directly.

---

## 0. Provenance, verified here

```
ron-bin/sbl/rise.pdb        MSF 7.00, 57,290,752 bytes
  sha256  334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5
  GUID    51D4F219-61C6-4F84-9D5B-C3361B0D291F   age 1

riseofnations.exe  IMAGE_DEBUG_TYPE_CODEVIEW record
  RSDS    51D4F219-61C6-4F84-9D5B-C3361B0D291F   age 1
          E:\agent\_work\2\s\main\game\rise.pdb
  sha256  30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079
```

GUID and age were parsed out of both files in this lane and compared byte-for-byte. They
match. **The PDB is for exactly this build.** Module paths in the symbol records
(`E:\agent\_work\2\s\main\Build\Production\game\*.obj`) confirm the CI-built 2024 recompile.

### What is in it

| stream | content |
|---|---|
| TPI | **305,734** type records; 89 unparsed (all `LF_VTSHAPE`, no data loss) |
| — | **19,916** named class/struct/union definitions, **5,948** of which have data members |
| — | **2,857** enumerations |
| DBI + globals | **22,199** procedure symbols at **19,976** distinct VAs, 18,986 carrying a `this` class |
| — | **6,779** data symbols at 5,036 VAs, each with a real type |
| — | 2,876 distinct classes have code; 235 `walk_data` and 218 `log_data` functions |
| modules | game 16,815 · basic 3,540 · bighuge 1,432 · pnglib/zlib/CRT the rest |

### Artifacts produced by this lane

| path | what |
|---|---|
| `schema/pdb-types.json` | 1,114 classes (priority set + DataWalk set + transitive closure over bases and member types), 36 enums, per-class function index. Each field carries `source: "pdb"`. |
| `schema/state-schema.json` | merged: every class the DataWalk pass named now carries a `pdb` block with real field names/types/offsets, plus `sizeof_vs_pdb`. `pdb_only_classes` lists the 898 covered classes DataWalk never reached. |

Extractor: `scratchpad/pdbtest` (Rust, `pdb` crate 0.8.0) → `pdb-classes.json`,
`pdb-enums.json`, `pdb-funcs.json`, `pdb-data.json`. Runs in 4 s.

---

## 1. The headline: cross-validation of `schema/bindings.json`

1,224 rule-name → struct-offset bindings, extracted from call sites, checked against the
PDB. `crates/don-rules/src/offsets.rs` is a faithful projection of that file — 1,223 of
1,224, the single drop being a duplicate `gather_slots_high[scan]` binding that collides on
the generated const name. The generator introduces **zero** additional error, so everything
below applies to `offsets.rs` unchanged.

### The eight named anchors

| rule | ours | PDB | type | class | source function |
|---|---|---|---|---|---|
| `attack` | +488 | **+488** | `int` | `ObjectTypeData` | `ObjectType::log_data` |
| `to_hit` | +492 | **+492** | `int` | `ObjectTypeData` | `ObjectType::log_data` |
| `attenuate` | +496 | **+496** | `int` | `ObjectTypeData` | `ObjectType::log_data` |
| `recharge` | +500 | **+500** | `int` | `ObjectTypeData` | `ObjectType::log_data` |
| `hits` | +528 | **+528** | `int` | `ObjectTypeData` | `ObjectType::log_data` |
| `armor` | +532 | **+532** | `int` | `ObjectTypeData` | `ObjectType::log_data` |
| `crew_size` | +780 | **+780** | `int` | `UnitTypeData` | `UnitType::log_data` |
| `base_form` | +784 | **+784** | `int` | `UnitTypeData` | `UnitType::log_data` |

**8/8 exact.** The combat stat block is real and lands exactly where we said.

### Full agreement, all 1,224

| classification | n | % |
|---|---:|---:|
| `EXACT` — same class, same member, same byte offset | 761 | 62.2 % |
| `BASE_UNRESOLVED` — our extractor recorded `0`, i.e. gave up | 308 | 25.2 % |
| `HOISTED_BASE_REBASE` — off by one constant shared across a loop group | 52 | 4.2 % |
| `VIRTUAL_BASE_REBASE` — off by exactly `sizeof(class) - 8` | 41 | 3.3 % |
| `STATIC_MEMBER` — name is a `static` member; no `this`-offset exists | 26 | 2.1 % |
| `NAME_ABSENT_FROM_CLASS` — name belongs to a nested/pointed-to object | 17 | 1.4 % |
| `ARRAY_ELEMENT` — ours is `&arr[k]`, PDB gives `&arr[0]` | 10 | 0.8 % |
| pointer-indirection / junk artifacts | 9 | 0.7 % |

Derived rates:

- **Name accuracy: 1,181 / 1,224 = 96.5 %** — the rule name resolves to a real member of
  the real class the PDB assigns to that function.
- **Of the 873 bindings where our extractor actually resolved a base register:
  761 exact = 87.2 %, and 864 = 99.0 % once the two systematic rebases are applied.**
- **Genuinely wrong, unexplainable values: 1** (see below).

**There is no case where we named a field that does not exist, at an offset that cannot be
accounted for.** Every deviation has a mechanical cause, and every cause is a limitation of
the call-site extractor, not a misunderstanding of the game.

### The two systematic rebases, explained

**(a) Every `Order` class inherits `UnitOrder` *virtually*.** This is the single biggest
structural surprise in the PDB and it explains 41 of our "disagreements" at a stroke.

```
class MoveOrder : virtual public UnitOrder   // LF_VBCLASS, not LF_BCLASS
```

MSVC lays a virtual base out at the **tail** of the object, with a `vbptr` at offset 0.
`UnitOrder` is 8 bytes (vfptr@0, `flags:char`@4). So for a 92-byte `MoveOrder`, the
`UnitOrder` subobject sits at **+84**, and any code holding a `UnitOrder*` addresses
`MoveOrder`'s own members at **negative** offsets. Our extractor was reading exactly that
pointer. Prediction `offset = sizeof(class) - 8`, checked against every Order class for
which we have bindings:

| class | sizeof | predicted vbase | observed shift |
|---|---:|---:|---:|
| `MoveOrder` | 92 | 84 | 84 |
| `GroupAttackOrder` | 84 | 76 | 76 |
| `GuardOrder` | 56 | 48 | 48 |
| `TradeOrder` | 52 | 44 | 44 |
| `GatherOrder` | 52 | 44 | 44 |
| `AttackOrder` | 48 | 40 | 40 |
| `AirOrder` | 40 | 32 | 32 |
| `GroupOrder` | 36 | 28 | 28 |
| `AttackGroundOrder` | 32 | 24 | 24 |

9 for 9. `schema/pdb-types.json` records `derived_offset` for every virtual base on this
basis (the true offset lives in the runtime vbtable; this derivation is validated, not
assumed).

**(b) Loop-hoisted base registers.** MSVC picks one address as the induction base for a
group of array walks — often one-past-the-end of the first array, or the start of the
*next* array — and addresses everything else relative to it. Our extractor recorded that
register's offsets verbatim. Every such group has a single constant delta:

| function | class | shift | what the base register actually is |
|---|---|---:|---|
| `FUN_006e5110` | `LeaderData` | +116, +1152 | `&chat_status[8]` (84+32), and the resource block |
| `FUN_004813b0` | `FormData` | +132, +788 | ends of the `int[18]` and `Coord[128]` groups |
| `FUN_0045e1d0` | `GroupData` | +588 | end of the `int[128]` group |
| `FUN_00956cc0` | `TurnControl` | +212 | end of the `unsigned long[8]` group |
| `FUN_00475af0` | `ObjectsData` | +388 | end of `unit_mark`/`wall_mark` |
| `FUN_006b6080` | `WorldData` | +312 | `&tdata` (danger is at +316) |
| `FUN_0065fc00` | `ObjectType` | +624 | `&support_cost` (support is at +616) |
| `FUN_0066f2c0` | `GoodType` | +740 | `&bonus_num` (bonus_type is at +732) |
| `FUN_004b3bb0` | `LandData` | +20 | `&num_make` (make is at +4) |

### The 43 name-level misses, itemised

- **26 `ScenarioData` entries** — `ScenarioData`'s members are all `static`
  (`camera_init_x : int[8]`, `units_killed : short[352][8]`, …). There is no instance
  offset to recover; they are absolute globals, and the PDB gives their addresses. Our `0`
  is arguably the correct answer to a malformed question.
- **`LeaderDataEncrypt::ages_get()` ×4** — these are accessor *calls* in the log, not
  fields. The underlying members are `ages`/`epochs`/`discovered`/`epoch[4]` at +220…+232.
- **`LeaderOptions::{peasants, peasants_wait, buildings}`** — members of the nested
  `LeaderOption` (+4/+8/+12 inside a 32-byte element of `LeaderOptions::list[10]`).
- **`ArmiesData::{increment,length,size}`** — members of the inner `PtrArray<Army>`, not of
  `ArmiesData` (whose only field is `lists : PtrArray<Army>[8]`).
- **`PtrLinkList*::metric`, `OrderList::{type,metric}`** — members of the *node*
  (`LLNode<T,M>`: next/prev/data/metric; `RecycledOrderNode` likewise), reached by pointer
  chase from the list head.
- **`Version::current_version`** — a global (`current_version : Version` @ `0x00E7FC94`),
  not a member.

Four further entries — `SimpleArray<int>`, `SimpleArray<Coord>`, `SimpleArray<WCoord>` and
`ObjectArray<SimpleArray<int>>`, all `list[scan]` at `+4` — are **pointer indirections**:
the walk is `*(this->list + k)`, and the number recorded is the loop-induction displacement,
not a `this`-offset. The real container layout (shared by every `Array`/`PtrArray`/
`SimpleArray` instantiation) is `vfptr@0, length@4, size@8, increment:short@12, list@16,
flags:u8@20, cur_index@24`. Our `length@4` and `size@8` bindings for the same functions are
exactly right, which is a good sign for the extractor: it failed only where a pointer had to
be followed.

The single genuinely bad value: `FUN_00471710` `queue[scan].job_counter` recorded `+1`.
`BuildQueueData` is `{queue_size:int @0, queue:QueueItem* @4}` and `QueueItem` is
`{job_counter:int @0, type:short @4, good:short[3] @6, cost:short[3] @12}`. `+1` is
meaningless — a decoding artifact.

### Independent check: `docs/derivation/rules-constants.json`

719 constants with offsets, checked against the PDB's `Constants` class:

- **719 / 719 name + offset exact.** No offset disagreements at all.
- One array-length error: `scholar_rate` recorded as 5 elements; the PDB says `int[6]`.
- **Four real rule constants we missed entirely**: `liberty_free_upgrades` (+1332),
  `eiffel_siege_range` (+1352), `aztec_move_speed` (+1396), `spanish_extra_scout` (+1664).
- One PDB member that is not a rule: `curr_element : XMLElement` (+3392) — the parser's
  own XML cursor, which is why `Constants` is 3,432 bytes and not 3,392.

### Independent check: sizes recovered by the DataWalk pass

The DataWalk lane recovered `sizeof` for 216 classes by abstract interpretation.
**216 / 216 agree with the PDB.** Two completely independent methods, zero divergence.

---

## 2. The class model

### `X` / `XOut` / `XData` is real, and `XData` is the sim state

```
Unit  →  UnitOut  →  UnitData  →  Object → ObjectOut → ObjectData
                                              → SubObject → SubObjectOut → SubObjectData
UnitType → UnitTypeOut → UnitTypeData → ObjectType → ObjectTypeOut → ObjectTypeData
                                              → Type → TypeOut → TypeData → GameAccessConst
Build → BuildOut → BuildData → Wall → WallOut → WallData → Object → …
```

`XData` carries the POD state, `XOut` adds presentation members (`on_screen`, texture
atlases, `render_gpiece`), `X` adds behaviour. **Model the `XData` layer.** Note `XOut` is
*interposed*, so `XOut` members sit between `XData`'s and the derived class's — you cannot
skip it when computing offsets.

`GameAccess` / `GameAccessConst` / `MiscAccess` are 1-byte empty accessor bases — ordinary
bases in some classes, **virtual** bases in others (`ObjectTypeData`, `UnitTypeData`,
`TechTypeData`, `Tribe`, `Terrain`, …). Empty, so they carry no members of their own, but a
virtual base still implies a `vbptr` word (which shares offset 0 with the `vfptr` where the
class has one), and they do shift member offsets — `CityData`'s first field is at +4 for
exactly this reason. `schema/pdb-types.json` records `derived_offset` for a virtual base
only when a class has exactly one; classes with several empty accessor vbases get no
derived offset, because `sizeof - sizeof(vbase)` is only valid for the single-vbase case.

### Sizes of the priority classes

| class | sizeof | `Data` layer | fields (own) |
|---|---:|---|---:|
| `SubObjectData` | 28 | — | 7 |
| `ObjectData` | 80 | — | 20 |
| `UnitData` | 344 | — | 78 |
| `WallData` | 112 | — | 11 |
| `BuildData` | 220 | — | 22 |
| `CityData` | 184 | — | 42 |
| `TypeData` | 236 | — | 23 |
| `ObjectTypeData` | 696 | — | 38 |
| `UnitTypeData` | 1,496 | — | 25 |
| `BuildTypeData` | 748 | — | 13 |
| `TechTypeData` | 648 | — | 4 |
| `GoodType` (flat) | 776 | `GoodTypeData` | 21 |
| `Constants` | 3,432 | — | **722** |
| `LeaderData` | 28,388 | — | 299 |
| `TerrainData` | 27,264 | — | 216 |
| `WorldData` | 364 | — | 46 |
| `WData` (per tile) | 28 | — | 17 |
| `PathFinderData` | 136 | — | 34 |
| `AmmoData` | 108 | — | 28 |
| `GuyData` | 188 | — | 46 |
| `Player` | 140 | — | 35 |
| `GameInfo` | 1,348 | — | 47 |
| `CommandPackage` | 536 | — | 7 |
| `Game` | 3,184 | — | 75 |
| `Tribe` | 1,520 | — | 12 |
| `Random` | 4 | — | 1 |
| `DataWalk` | 16 | — | 3 |

**There is no `RULES` class.** The globals at `[0x00C061E4]` and `[0x00C061F0]` are
`GameAccessConst::constantsc : const Constants&` and `GameAccess::constants : Constants&` —
two references to one `Constants` object. The prior live-read finding that they alias is
confirmed by the symbols themselves.

---

## 3. The layouts that matter

### `ObjectTypeData` — the combat stat block (`ObjectType` +0, all `int`)

```
+484 obj_masks          +540 los                 +592 fly_high
+488 attack             +544 science_los         +596 fly_low
+492 to_hit             +548 guy_spacing         +600 block_points
+496 attenuate          +552 x_spacing           +604 graft            : TypeIndex
+500 recharge           +556 y_spacing           +608 special_upgrade  : TypeIndex
+504 min_range          +560 abil                +612 special_upgrade_cost
+508 max_range          +564 x_size              +616 support          : TypeIndex[2]
+512 splash_area        +568 y_size              +624 support_cost     : int[2]
+516 splash_percent     +572 guy_radius          +632 age
+520 ammo_per_att       +576 block_radius        +636 is_list          : SimpleArray<u16>
+524 proj_speed         +580 big_radius          +664 is_strict_list   : SimpleArray<u16>
+528 hits               +584 new_block_radius
+532 armor              +588 new_big_radius
+536 domain
```

Inherited: `TypeData` at +0 (`type`, `job_time`, `res_time`, `tribe_mask`, `cat`,
`costs : int[6]` @+24, `preq : TypeIndex[3]` @+48, `from/where/upgrade/jump/obs`,
`show : TypeIndex[2]`, `modified`, `grid_x/grid_y`, then seven `String`s from +96 to +236);
`TypeOut` texture tables +240…+456; `SoundType` at +456.

### `UnitTypeData` — unit stats (`UnitType` +0)

```
+692 unit_flags         +724 carry                 +756 progression
+696 unit_flags2        +728 carry_size            +760 push_size
+700 mode               +732 military_level        +764 push_circles
+704 moves              +736 research_premium_cost +768 target_size
+708 turn_speed         +740 research_premium_time +772 squad_size
+712 role               +744 job_extra_time : u32  +776 uber_size
+716 fire_proj          +748 mana                  +780 crew_size
+720 second_max_range   +752 control_cost          +784 base_form
+788 relative_value : short[352]        // 704 bytes, ends at +1492
```

### `UnitData` — the runtime unit (sizeof 344, `Unit` +0)

Selected; the full ordered list is in `schema/pdb-types.json`.

```
  (SubObjectData)  +8 flags:u8  +9 who:u8  +10 o:short  +12 z  +16 x  +20 y (Coord)
                   +24 ptype : ObjectType*
  (SubObjectOut)   +28 on_screen:u8
  (ObjectData)     +32 myhits:int   +36 damage:int   +40 inside_down  +42 up  +44 down
                   +46 down_who  +48 uid:u16  +50 hold_frames  +52 near_o  +54 near_who
                   +56 healing  +58 infiltrated  +59 damage_frac  +60 mylos  +61 targeted
                   +62 inside_down_who  +63 up_who  +64 visible  +65 launch_frames
                   +68 launching : SimpleArray<int>*
  (UnitData)       +72 collide_frame  +76 damage_frame  +80 angle
                   +84 rare | air_alt | former_type          <- anonymous union
                   +88 dest_angle  +92 trench_angle  +96 tolerance  +100 queue_time
                   +104 unit_masks:u32  +108 unit_masks2:u32
                   +112 orders_x  +116 orders_y  +120 los_x  +124 los_y   (Coord)
                   +128 group  +130 inside_up  +132 supply
                   +134 hero | cara | doober | special | herd  <- anonymous union
                   +136 collide  +138 collide_o  +140 collide_guy  +142 o_up  +144 o_down
                   +146 gather_down  +148 good_obj  +150 mana_burn  +152 spell_time
                   +154 myspeed:short  +156 myarmor:short  +158 attrition:short
                   +160 num_queued  +162 cavarch_o  +164 damage_o  +166 cavarch_uid:u16
                   +168..+182  cavarch_who, damage_who, form, form_mod, full, waiting,
                               recharging, path_recursion, idle, stance, safe,
                               collide_who, inside_up_who, guy_mark, play   (all 1 byte)
                   +184 path : Stack<PathData>          (16)
                   +200 orderlist : OrderList           (28)
                   +228 guys : PtrArray<Guy>            (28)
                   +256 const_guys : ConstPtrArray<Guy const>&
                   +260 openlist / +264 openlistrefs / +268 closedlist / +272 validlist
                   +276 blocklist                        <- A* working set, per unit
                   +280 temp_x +284 temp_y +288 avoid_x +292 avoid_y
                   +296 tol +300 offset +304 start_dist +308 valid_hit
                   +312 avoid_land +316 avoid_sea +320 endx +324 endy
                   +328 traversed +332 announce_frame
```

Two things worth flagging for the sim core: **each `Unit` owns its own pathfinder working
set** (+260…+276, five tree pointers), and **`myspeed`/`myarmor`/`attrition` are `short`**
while the type-level `attack`/`armor` are `int`.

### The Order hierarchy — the real action space

32 concrete order classes, all with `UnitOrder` as a **virtual** base.

```
UnitOrder (8)  vfptr@0, flags:char@4, virtual base of everything below
├── TargetOrder (32)   ox@8, whom@12, uid:u16@16
│   ├── AttackOrder (48)          def_x@20 def_y@24 …
│   │   ├── GroupAttackOrder (84)  + GroupOrder@36
│   │   └── StrafeOrder (84)       + AirOrder@36
│   ├── AwaitBoardOrder(32) BoardOrder(32) BuildOrder(32) RepairOrder(32)
│   ├── CastOrder(48)  FollowOrder(44)  GarrisonOrder(36)
│   ├── GatherOrder(52)   build_type@28 wait@32 …
│   └── TradeOrder(52)    whose@24 started@28 loaded@32 …
├── MoveOrder (92)    x@4 y@8 angle@12 dest@16 tolerance@20 pause@24 retry@28
│   │                 attempts@32 timer@36 facing@40 dest_x@44 dest_y@48
│   │                 last_x@52 last_y@56 coll_x@60 coll_y@64 orig_x@68 orig_y@72
│   │                 off_x:short@76 off_y:short@78    [UnitOrder subobject @84]
│   ├── AttackToOrder(92)  ExploreToOrder(92)  FleeToOrder(92)
│   ├── FormOrder(100)
│   └── GroupMoveOrder(120) + GroupOrder@80  → GroupAttackToOrder(120)
├── PatrolOrder (76)  x_pos:SimpleArray<Coord>@4, y_pos@32, waypoint@60
│   ├── AirPatrolOrder(104) + AirOrder@64
│   └── GroupPatrolOrder(100) + GroupOrder@64
├── AirOrder (40)     oxx@4 whose@8 cruising_alt@12 sharp_turn@16 old@20 returning@24
├── GroupOrder (36)   oxx@4 whose@8 id@12 form_id@16 group_angle@20
├── AttackGroundOrder (32)  att_x@4 att_y@8 accuracy@12 attack_unit@16
│   └── AirAttackGroundOrder(72) + AirOrder@20
├── SpecialAnimOrder (44), ThinkOrder (8), EditorOrder (16)
```

`OrderList` is `LinkListBase<UnitOrder*, unsigned char, RecycledOrderNode>` at +4:
`current_data@4, current_metric:u8@8, current_node@12, length@16, head_node@20, ordered@24`,
sizeof 28. Nodes are `RecycledOrderNode{next, prev, data:UnitOrder*, metric:u8}`, 16 bytes.

### DataWalk and its three implementors

```
DataWalk (16)   vfptr@0   input:int@4   checksum:int@8   flags:int@12
├── CheckSum (24)   accum:u32@16   size:u32@20
├── SaveGame (52)   mp_save_name:String@20   file:CloudFile*@40   + vbase WalkDataGame
└── LoadGame (52)   file:CloudFile*@20       last_section:String@24  + vbase WalkDataGame
```

This settles the DataWalk lane's inferred field roles: `walker+0x08` is `checksum` and
`walker+0x0c` is `flags` — exactly the section mask it observed. `CheckSums` (the driver,
sizeof 52) holds `mark : CheckSumMark`, `last_mark : CheckSumMark[2]`, `size_estimate`.
864 classes declare a `walk_data` method; 235 have an emitted `walk_data` body.

### `CommandPackage` — the lockstep wire unit (sizeof 536)

```
+0   stamp   : unsigned long
+4   play    : int
+8   valid   : int
+12  group   : int
+16  size    : short
+18  data    : unsigned char[512]
+532 padding : Random          // a Random (u32 seed) used as trailing padding
```

`CommandPackage::process_check_sums` is `FUN_009459d0` (returns `int`).

### `GameInfo` (1,348) — the match settings, i.e. the RL env config

`version@0`, `seed@4`, `checksum_deep@8`, `checksum_window_size@12`,
`checksum_failure_threshold@16`, `flags@20`, then a `data : unsigned char[30]` overlay at
+24 that aliases the 30 individual settings bytes (`team_style`, `map_style`, `map_size`,
`players`, `game_speed`, `difficulty`, `pop_limit`, `tech_cost`, `victory`, `time_limit`,
…), `player : Player[8]` @+56, ELO block @+1176, then four `String`s.

### `WorldData` (364) and `WData` (28 per tile)

`WorldData` holds the dimension triples (`xs/ys/size`, `fog_*`, `tile_*`, `reg_*`), the
territory limits, the start-location arrays, and the actual grids as raw pointers:
`wdata : WData*` @+308, `tdata : TData*` @+312, `danger : int*[8]` @+316,
`seen/seen2/seen3 : unsigned char*` @+348/+352/+356, `wcoord_seen` @+360.

`WData` per tile: `flags:u16@0, land:char@2, land_sub:u8@3, region:short@4, region2@6,
down@8, down_who@10, val:u8@12, goods:u8@13, light:u8@14, who:char@15, who2@16,
blocked@17, bad@18, solid@19, was_seen@20, block:CollBlock*@24`.

### `PathFinderData` (136)

All `int` after the five tree pointers and `pathing_unit : Unit*` @+20 — including the road
cost weights (`road_base_val`@96 … `road_diag_penalty`@128) that the A* lane will need.
Coordinates here are `TCoord`, not `WCoord`.

---

## 4. Two open questions, settled

### (a) Do parsed rule values land in `f32` or fixed-point integers?

**Fixed-point integers. There is not one float in the rule data.**

- `Constants` has **722 members, all of them `int` or `int[N]`** (690 scalars, 31 arrays,
  plus the one `XMLElement` parse cursor). Zero floats, zero doubles.
- `TypeData`, `ObjectTypeData`, `UnitTypeData`, `BuildTypeData`, `TechTypeData`,
  `GoodTypeData` — zero floats. Costs are `int[6]`. Every combat stat is `int`.
- `Coord`, `WCoord` and `TCoord` are each a struct wrapping a single `int value`. Positions
  are fixed-point integers, not floats.
- The tokenizer at `0x00A1D110` is `String::fraction`, and its PDB signature **returns
  `int`**. (Our notes called this `RString::AsScaled`; the real name is `String::fraction`.)

Sweeping all 154 `*Data`/`*Type` classes finds 77 float members, and they are confined to
presentation and worldgen: `AmmoData` ballistics (`v1z`, `dx`, `bank_dx`, `bank_dy`),
`GuyData` model orientation (`bank`, `pitch`, `turret_inc`), `TerrainData` coastline
generation, `RiverData`/`RiversData`/`SplineData`, `NukeParticleData`, `StormData`.

Two exceptions deserve a flag, because they sit inside walked state:
**`LeaderData::anti_att : float` @+2036 and `LeaderData::plunder_scale : float` @+2324.**
If those are checksummed, they are float in the lockstep path and must be reproduced
bit-exactly. Worth a targeted check.

### (b) Real element widths of the unit attribute arrays

- **`UnitTypeData::relative_value : short[352]` at +788** — 704 bytes, **int16 elements**,
  and `NUM_UNITTYPES = 352` exactly. This is the per-unit-type-versus-unit-type balance
  table, stored **inline in each `UnitTypeData`**, not in one global matrix.
- `LeaderData::num_units : unsigned short[352]` @+22370;
  `LeaderData::num_queued : unsigned short[806]` @+23074 (`NUM_TYPES = 806`);
  `LeaderData::last_unit_finished : int[352]` @+25204.
- `Tribe::graft : TypeIndex[352]` @+112.
- `ScenarioData::units_killed : short[352][8]`, `builds_destroyed : short[129][8]`
  (`NUM_BUILDTYPES = 129`) — statics.

`attack` itself is a **32-bit `int`** in `ObjectTypeData`; the ×10 storage lives in a full
`int`, not a narrow field. There is no narrower attribute array for it.

The `TypeIndex` enum (869 enumerators) gives the authoritative dimensions:

```
NUM_TYPES 806   NUM_UNITTYPES 352   NUM_BUILDTYPES 129   NUM_BONUSTYPES 122
NUM_TECHTYPES 85   NUM_SPELLTYPES 55   NUM_GOODTYPES 50   NUM_RARES 44
NUM_EPOCHTYPES 28   NUM_WONDERTYPES 17   NUM_GAIATYPES 12   NUM_AGETYPES 7
NUM_COMMON 6   NUM_GATHER 6   NUM_GOVTYPES 6   NUM_GOV_HEROTYPES 6
NUM_FINALTYPES 4   NUM_ITEMTYPES 1
```

---

## 5. Corrections to established ground truth

These entries in `README-LLM.md` / `docs/binary-ground-truth.md` are now **wrong** and
should be edited.

1. **`FUN_00570170` is NOT "the rules.xml constant loader".** It is
   `Constants::log_data(Log*)`, 63,382 bytes, in `game\constants.obj`. Likewise
   `FUN_0061c490` is `UnitType::log_data` and `FUN_0065fc00` is `ObjectType::log_data`.
   This is good news, not bad — a `log_data` pairs a literal name with a direct
   `this->field` read, which is why our binding extraction was so accurate. But the actual
   **loader** is a different function and we have not looked at it yet. Anything asserted
   about *parsing semantics* on the basis of `FUN_00570170` is asserted about the
   *logger*.

2. **The balance table is not at `0x00C06AFC`, and it is not 493×493.** That address falls
   inside `s_SteamWorkshopTagLinks` (`SteamWorkshopTagLinks[64]`, 72 bytes each, spanning
   `0x00C068D0`–`0x00C07AD0`), whose members are `enum tag`, `enum category`,
   `char fileSearchPattern[60]`, `bool recursive`. Reading ASCII search patterns as `int16`
   is exactly why the captured window had "unexplained negatives". The real table is
   `UnitTypeData::relative_value : short[352]`, per unit type.

3. **`RString::AsScaled` @ `0x00A1D110` is `String::fraction`**, returning `int`.

4. **`FUN_00644130` is `ObjectData::get_damage`**, `__thiscall`, returns `int`, 3,954
   bytes, in `game\Object.obj`. `FUN_00936560` is `CheckSums::check_all` → `unsigned long`.
   `0x00A46830` is `adler32`. `0x00A39CF0` and `0x00A39D70` are two overloads of
   `Random::get` (returning `float` and `int` respectively).

5. **There is no `RULES` class**; `[0x00C061E4]` / `[0x00C061F0]` are
   `GameAccessConst::constantsc` and `GameAccess::constants`, both `Constants`. The aliasing
   we measured live is confirmed by the symbols. `[0x00C06184]` is
   `GameAccess::game_random : Random&`; `0x00EB697C` is `internal_random : Random`.

6. `??_7DataWalk@@6B@` `0x00B2BCD8`, `??_7LoadGame@@6B@` `0x00B30C88`,
   `??_7SaveGame@@6B@` `0x00B35AC4`, `??_7CheckSum@@6B@` `0x00B3F920` — all four confirmed
   against the PDB's public symbols. That derivation stands unchanged.

---

## 6. What this changes for the project

- **`schema/bindings.json` and `crates/don-rules/src/offsets.rs` are superseded** by
  `schema/pdb-types.json` for every class the PDB names — which is all of them. Keep
  `bindings.json` as the independent-derivation record (it is 96.5 % name-correct and that
  is a real result about the method), but implement against the PDB. Regenerating
  `offsets.rs` from `pdb-types.json` removes the 308 zero placeholders, the 93 rebases and
  the 43 misattributions in one pass.
- **The 620-class RTTI estimate was low by 30×.** 19,916 named types, 5,948 with data
  members, and `schema/vtables.json`'s 1,777 vtables can now be typed with real layouts.
- **A live heap crawler is now cheap.** Every object starts with a vtable pointer, every
  vtable maps to a class name, and every class has a real field table with names and types.
  That is a full typed memory inspector, not a hex dump.
- **The action space is settled**: 32 concrete Order classes with exact parameter fields.
  Parameter-level masking has a schema now.
- **Sim-critical state is settled**: `XData` layers, walked by DataWalk, with the
  `sizeof` cross-check at 216/216.
- **Do not hand-write these structs.** Generate the Rust from `schema/pdb-types.json`,
  including the virtual-base tail placement for Orders, or the first hand-typo becomes a
  silent divergence.

## 7. Reproducing

```sh
# extractor
cd <scratchpad>/pdbtest && cargo build --release
./target/release/pdbtest /Users/ember/dev/don/ron-bin/sbl/rise.pdb <outdir>
#   -> pdb-classes.json  pdb-enums.json  pdb-funcs.json  pdb-data.json  pdb-type-errors.txt

# cross-validation + artifacts
python3 <scratchpad>/xval3.py     # the agreement table in §1
python3 <scratchpad>/merge.py     # builds schema/pdb-types.json
python3 re/scripts/merge_pdb_schema.py   # folds it into schema/state-schema.json
```

⚠ **`schema/state-schema.json` has two writers.** `re/scripts/gen_state_schema.py`
regenerates it wholesale from the DataWalk trace and drops anything merged in afterwards.
`re/scripts/merge_pdb_schema.py` is idempotent and re-applies the PDB layer from the
committed `schema/pdb-types.json`; run it *after* `gen_state_schema.py`, every time.
`schema/pdb-types.json` is self-contained and has a single writer, so it is the safe thing
to depend on.

The other five shipped PDBs (`CrossplayNetLib`, `CrossplayProxy`, `dssl`, `PartyWin`,
`PlayFabMultiplayerWin`, 56 MB more) have not been mined yet. `dssl.pdb` in particular is
the scripting layer and is the obvious next target.
