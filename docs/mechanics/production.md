# Production: construction, training queues, cost ramping, repair, destruction

**Lane:** `mech:production` · **Checksum channel served:** `check_builds` (channel 2 of 15,
`CheckSums::check_builds` `0x00937290`, checksums.cpp:646) · **Module:**
`crates/don-sim/src/systems/production.rs` (3,082 lines, 62 tests, all green).

---

## 1. What now works, and how it was measured

Everything below is implemented, compiles, and is covered by a test. Every entry is
**[measured]** — structure from `re/decomp-all/<VA>.c`, then every load-bearing constant,
comparison and division re-read at the instruction level with capstone against
`ron-bin/riseofnations.exe` (sha256 `30478a44…625079`). Field offsets are from the PDB TPI
stream (`schema/pdb-types.json`), not inferred from address arithmetic.

**Fidelity tier: C.** Nothing here has run against retail. It is not verified and not
differentially tested; there is no oracle case for any of it yet. Section 6 says what an
oracle run would have to cover.

| mechanic | retail function | Rust |
|---|---|---|
| Construction progress per worker | `Wall::do_construct` `0x006434D0` | `do_construct`, `construct_frame` |
| Build-rate reduction under attack | `Unit::do_build` `0x005EEBF0` | `construct_rate` |
| Construction time, cached | `Wall::update_construct_time` `0x0063D560` | `update_construct_time` |
| Construction time, queried | `BuildData::construct_time` `0x0062D5C0` | `construct_time` |
| Under-construction hit points | `Wall::update_hits` `0x0063F0D0` | `construct_hits` |
| Under-construction collapse | `Object::take_damage` `0x00652020` | `under_construction_collapses` |
| Hit points while razing (float!) | `BuildData::hits` `0x0062E740` | `build_hits` |
| Training/production queue tick | `Build::do_queue` `0x0061E410` | `queue_step`, `QueueKind` |
| `JOB_EXTRA_TIME` ramp + 3× cap | `ObjectData::train_time` `0x006508C0` | `train_time_ramp` |
| Age penalty + difficulty scale | same, tail | `train_time_age_penalty`, `train_time_finalize` |
| SUPPORT × PROGRESSION cost ramp | `TypeData::get_cost` `0x00664090` | `ramp_cost`, `progression_ramp_count` |
| Ramp cap by unit class | same, `0x0066536B`–`0x006656B5` | `RampClass` |
| Scholar super-linear extra | same, `0x006656C0` | `scholar_extra` |
| Research cost ramp | same, `0x006666D0` | `research_ramp_cost` |
| Repair | `Build::repair_damage` `0x00628130` | `repair_damage` |
| Unqueue refund, plain | `Build::unpay_cost` `0x006206E0` | `unpay_amounts` |
| Unqueue refund, age-adjusted | `Build::refund_cost` `0x00620490` | `refund_cost`, `refund_amount` |
| Footprint area / corner ↔ tile | `BuildTypeData::{area,corner_tile,tile_corner}` | `Footprint` |
| Placement verdict partition | `BuildTypeData::blocked_site` `0x00636A50` as consumed by `do_construct` | `SiteVerdict` |
| Build record layout (220 B) | PDB `.?AVBuildData@@` | `BuildData`, `off::*` |
| Queue record layout (20 B) | four independent indexers | `BuildQueueEntry` |
| Checksum walk | `BuildData::walk_data` `0x0062F270` + 4 sub-walkers | `BuildData::walk` |
| Channel iteration | `CheckSums::check_builds` `0x00937290` | `BuildPool::check_builds` |
| adler32 / `CheckSum::walk_function` | `0x00A46830` / `0x00936FF0` | `adler32`, `CheckSum` |

**Verification method.** 62 unit tests, run standalone (`rustc --edition 2021 --test`,
because `lib.rs` does not yet declare `pub mod systems;` — see §7). They are behavioural
assertions on the *derived* arithmetic, not captured retail vectors: they prove the port
matches what I read out of the instruction stream, and they pin the six places where a
plausible-looking reimplementation diverges (§3). They are **not** evidence of fidelity to
retail. `adler32` is additionally checked against the three standard published vectors.

---

## 2. The layout, confirmed exactly

The brief said "MiningList embedded at `Build+0x98` and GatherPointList at `Build+0xb8` —
those embeddings are [measured] from address geometry". The PDB TPI record agrees to the
byte, so the geometry inference was right:

```
BuildData  size 220 (0xDC)
  +0x48  u32  WallData::job_counter          construction progress
  +0x4C  u32  WallData::job_counter_2        cumulative work ledger
  +0x50  u32  WallData::constr_time          cached total time
  +0x54  i32  WallData::construct_hits       EFFECTIVE max HP right now
  +0x58  i32  WallData::gpiece
  +0x5C  i32  WallData::frame_started
  +0x60  i16  WallData::build_masks          0x20 under attack, 0x400/0x800 helper latches
  +0x62  u8   WallData::ever_seen
  +0x63  u8   WallData::ever_seen_completed
  +0x64  u8   WallData::helpers              RESET EVERY FRAME
  +0x65  u8   WallData::demolition
  +0x6C  i32  BuildData::orig_type
  +0x70..0x86  gather_down, city, city_down, wonder, dock|farm|fort|oil_well (union),
               recharging, attack_ox, stance, founder, gather_max, attack_whom,
               queued, max_age, infiltrate, infiltrate2
  +0x82  u8   BuildData::queued              logical queue length
  +0x88  ---  BuildData::build_queue         BuildQueue, 16 B: {i32 num; T* data; ...}
  +0x98  ---  BuildData::gather_from         MiningList, 32 B   <-- confirmed
  +0xB4  i8   MiningList::mtn
  +0xB5  i8   MiningList::cliff
  +0xB8  ---  BuildData::gather              GatherPointList, 28 B  <-- confirmed
```

### The queue record was not in any prior doc

Twenty bytes, stride `0x14`, recovered by triangulating four functions that index it:
`Build::do_queue` (`+0x00`, `+0x04`), `BuildData::count_queue` (`+0x04`),
`Build::unpay_cost` and `Build::refund_cost` (`+0x06/08/0A` and `+0x0C/0E/10`).

```
+0x00  i32  elapsed      progress, compared against ObjectData::train_time
+0x04  i16  type         TypeIndex being produced
+0x06  i16  res[0]       resource charged, or -1
+0x08  i16  res[1]
+0x0A  i16  res[2]
+0x0C  i16  amt[0]       amount taken from res[0]
+0x0E  i16  amt[1]
+0x10  i16  amt[2]
+0x12  i16  (never checksummed)
```

Two consequences. **Only three resources can be charged per queued item**, even though
`TypeData::costs` is `int[6]` — a hard cap in the record, not a rule. And the last two
bytes never reach the lockstep hash, because `BuildQueue::walk_data` (`0x006305F0`) emits
`w.walk(entry, entry + 0x12)`.

**Two queue lengths, and they are not the same.** `BuildData::queued` (`+0x82`, `u8`) is
what the game logic iterates; `BuildQueue::num` (`+0x88`) is the allocated entry count.
Every accessor guards on both and returns `-1` for a slot `>= num`. `count_queue` iterates
`0..queued` while reading `-1` for anything past `num`. A port that keeps one length
silently diverges the moment they differ.

### The band, pinned

`check_builds` starts its inner loop at `GameAccess::obj_base[1]`. The literal `2000`
appears directly in `BuildData::construct_time` where the same band is scanned
(`iVar5 = 2000; while (iVar5 < counts[p])`). Two independent sites ⇒ `obj_base[1] == 2000`,
and with the brief's 601 slots the band is `2000..=2600`, ending clear of the next band at
3000.

---

## 3. Six places a plausible reimplementation diverges

Each of these has a test that fails if you write the "obvious" version.

### 3.1 Construction is harmonic, not linear

`Wall::do_construct` (`0x006434D0`):

```
rate /= helpers + 1;      // BEFORE the increment
helpers += 1;
if (rate < 1) rate = 1;   // floor AFTER the division
job_counter_2 += rate;
job_counter   += rate;
```

`Wall::process` (`0x00640450`) zeroes `helpers` at the top of every tick. So the *n*-th
citizen to touch a site **this frame** contributes `rate/n`: four builders at rate 100 add
`100+50+33+25 = 208`, not 400. Total is `rate·H_n`. Worker iteration order therefore
changes the result, which ties this mechanic directly to `Objects::process_all`'s rotated
owner-slot order — a lane I do not own, but which this one depends on.

The floor of 1 lands **after** the division, so a 200th builder still adds 1. Construction
never stalls from crowding; it only stops accelerating.

`job_counter_2` receives the same increment but is never read by the completion test. Only
`job_counter` gates completion and only `job_counter` feeds the HP scaling.

### 3.2 The under-attack penalty is `/4` on the *rate*, and Korea is exempt

`Unit::do_build` `0x005EEEE3`–`0x005EEF75`, read instruction by instruction:

```
005eeeea  mov eax,[Constants+0x22c]     ; accel_construct  (100)
005eef03  call LeaderData::has_tribe_bonus(0x10)
005eef11  cmp dword [Constants+0x7dc],0 ; korean_build_under_fire  (1)
005eef18  jne skip                      ; bonus AND rule non-zero -> no penalty at all
005eef42  movsx eax, word [wall+0x60]   ; build_masks
005eef46  and  eax, 0x20                ; WallData::is_under_attack
005eef4b  je   skip
005eef4d  cdq / and edx,3 / add eax,edx / sar eax,2   ; rate /= 4
005eef75  call Wall::do_construct(rate)
```

`WallData::is_under_attack` (`0x00472420`) is literally
`return build_masks & 0x20;` — 8 bytes.

Rule `Constants+0x7DC` is `korean_build_under_fire`, shipped value `1`. The naming settles
what tribe bonus `0x10` is, and the bonus is tested *first*, short-circuiting the mask read.

### 3.3 Under-construction HP is quantised in 32-unit steps

`Wall::update_hits` (`0x0063F0D0`) tail:

```
this->myhits = h;                             // +0x20, the FULL value
if (!is_active()) {
    jc = job_counter;  ct = construct_time(0);
    if (jc < ct) {
        a = jc >> 5;  if (a == 0) a = 1;      // UNSIGNED shift
        b = ct >> 5;  if (b == 0) b = 1;
        if (!is_wonder) { h = a*h / b;         if (h < 1) h = 1; }
        else            { t = (h/2)*a / b; if (t < 1) t = 1; h = (h+1)/2 + t; }
    }
}
this->construct_hits = h;                     // +0x54
```

The `>>5` on **both** sides is a precision reduction to keep `a*h` inside 32 bits, and it
quantises progress. `h=1000, jc=500, ct=1000` gives `(500>>5)=15`, `(1000>>5)=31`,
`15*1000/31 = 483` — not 500. Full-precision `h*jc/ct` disagrees for nearly every input.
Each side is floored at 1 *after* the shift, so a site with `job_counter=1` still has
`1000/31 = 32` HP rather than zero.

**Wonders keep half their hit points from the foundation**: `(h+1)/2` unconditionally, plus
a scaled share of the other half.

### 3.4 The under-construction secondary damage rule

This is the one the brief named. It lives at the tail of `Object::take_damage`
(`0x00652020`), in the branch reached *after* the primary `hits <= damage` death test has
**not** fired:

```
if (is_valid_build()
    && !is_active()                  // still under construction
    && !(leader_flags & 4)
    && hits(0) <= damage * 2         // damage has reached HALF of effective HP
    && (wall->build_masks & 0x2000))
{
    Object::disband(0);              // the site collapses outright
    return 1;
}
```

A construction site does not have to be reduced to zero: at 50 % damage it is removed.
`hits(0)` is `BuildData::hits(0)` (vtable `+0x11C`), whose `param == 0` arm returns
`this[0x15]` = `+0x54` = `construct_hits` — the *construction-scaled* value, **not**
`myhits`. Verified against `BuildData::hits` `0x0062E740`.

Compose that with §3.3 and the practical result is stark: a 1000-HP building one frame into
a 1000-frame construction has 32 effective HP, so **16 points of damage destroys it
outright**. That is the mechanic behind rushing a half-built barracks.

### 3.5 The train-time ramp caps at 3× — and `x` is now known

`ObjectData::train_time` (`0x006508C0`), block `0x00650AB5`–`0x00650B4D`:

```
base    = base * unit_rate_base / 100          ; 0x00650AF6      (120 -> ×1.2)
ceiling = base * 3                             ; 0x00650B2E  lea eax,[ecx+ecx*2]
v       = count * job_extra_time * unit_rate_progression + base   ; 0x00650B27/31/38
if (v < 0 || ceiling < 0) v = 0                ; 0x00650B4B
else if (v > ceiling)     v = ceiling          ; 0x00650B47
```

The cap is **3× the scaled base**, a compile-time literal, not a rule.

`crates/don-sim/src/mechanics.rs::ramped_rate` already had this arithmetic but its doc
comment says *"**What `x` is has not been established.** … So this is the ramp arithmetic,
not 'train time' — do not wire it to a build queue and call it derived."* **That gap is now
closed.** The enclosing function is `ObjectData::train_time(TypeIndex)` (object.cpp:1897);
`x` comes from the type's vtable `+0x6C` (normal) or `+0x70` (already-discovered variant),
called with the owning player; and the value *is* the build/train time in frames, consumed
by `Build::do_queue` as the queue total. `production::train_time_ramp` is the same integers
under the right name. `mechanics::ramped_rate` and its doc caveat should be retired once
the sim-core lane is finished with `mechanics.rs` — I have not touched that file.

`count` is a `u16` at `leaders + 0x56FE + (who*0x3776 + type)*2` (`0x00650B17`). Note the
per-player stride `0x3776`, which `docs/derivation/economy.md` §4.2 omits.

### 3.6 The refund can charge you

`Build::refund_cost` (`0x00620490`):

```
d = player_age - type_age
for i in 0..3, res[i] >= 0:
    adj  = amt[i] * 100 / (100 - tech_science_discount * d)
    adj += (d + 1) * tech_science_discount * adj / 100
    stockpile[res[i]] += amt[i] - adj          // NEGATIVE whenever adj > amt
    amt[i] = adj                               // the record is REWRITTEN
```

With `tech_science_discount = 10` and `d = 0`, `adj = 110` for `amt = 100`, so the credit is
`-10` — cancelling costs you even at the same age, and more as ages pass. The engine also
writes `adj` back into the record, so a second cancel of the same slot compounds. Both
behaviours are reproduced literally.

**The stockpile is XOR-obfuscated with `0x8221`.** `unpay_cost` reads `*p ^ 0x8221`, adds,
writes `result ^ 0x8221`. The checksum runs over the obfuscated bytes. This module returns
deltas and leaves the masking to the economy lane, which already handles the same
obfuscation set (`^0x8221` stockpile, `^0x63187` age, `^0x63637` coords).

---

## 4. Cost ramping: SUPPORT and PROGRESSION

`TypeData::get_cost` (`0x00664090`, 13,088 bytes, over the 8,192-byte decompiler cap, so
this was read entirely from disassembly).

**`PROGRESSION` is a mode selector at `UnitType + 0x2F4`** and the only located consumer is
one instruction:

```
00665355  test byte ptr [ecx+0x2f4], 2
0066535c  je   linear
0066535e  lea  eax,[edi+1]     ; n+1
00665361  imul eax,edi         ; n*(n+1)
00665364  cdq / sub eax,edx    ; round toward zero
00665369  sar  edi,1           ; /2
```

**Bit 1 switches the ramp count from linear to triangular**, `n → n(n+1)/2`, making a
`progression = 2` unit's cost quadratic in how many you own. **Bit 0's consumer is not
located** — it is read nowhere in `get_cost` and nowhere in `ObjectData::train_time`. I have
left it as an open question rather than assuming it inert (§6).

**`SUPPORT` is a pair.** `ObjectTypeData::support : TypeIndex[2]` (`+0x268`) and
`support_cost : int[2]` (`+0x270`). The accumulation loop runs twice, adding
`support_cost[i] * n` for each `i` whose `support[i]` equals the resource being priced.

**The ramp cap is class-selected**, resolving `mechanics.rs`'s open note *"**Which class a
unit is in is not derived**"*:

| condition (first match wins) | cap rule | shipped |
|---|---|---|
| `type ∈ {0x34, 0x35}` | `unit_scholar_ramp_max` `+0x394` | 2000 % |
| `type ∈ {0x32..0x35}` ∨ `is_merchant()` ∨ `unit_flags2 & 8` | `unit_worker_ramp_max` `+0x398` | 500 % |
| `obj_masks & 4` | `unit_other_civilian_ramp_max` `+0x39C` | 200 % |
| otherwise | `unit_military_ramp_max` `+0x3A0` | 125 % |

`ceiling = ramp_max_pct * base_cost / 100`, and **a zero ceiling means *no* ceiling**
(`test esi,esi; je` at `0x00665452`) — the opposite of the natural reading.
`mechanics::clamp_cost_to_ramp_ceiling` already had that right.

**Scholars get an extra super-linear term** past the eighth, `0x006656C0`–`0x006656F0`:

```
ecx = 7; edx = 0;
if (n <= 7) extra = 0;
else do { edx += n - ecx; ecx += 7; } while (ecx < n);
```

i.e. an additional step every seventh scholar, on top of the 2000 % cap.

**Research has its own ramp** at `0x006666D0`: the engine counts techs in progress
(`LeaderData::researching` over `0..0x247`, accumulating 8 each), then
`pct = ramp_final * acc / 8 + 100` and `cost = pct * base / 100`. With `ramp_final = 50`,
each technology already in progress makes the next **50 % more expensive**.

---

## 5. The checksum channel

`BuildData::walk_data` (`0x0062F270`) recovered in full by decompiling all four walkers in
the chain and mapping every `w->vt[0](begin, end)` pair onto PDB offsets:

```
 1. walk [0x7F, 0x80)     founder                             1 B
 2. walk [0x83, 0x84)     max_age                             1 B
 3. WallData::walk_data                                0x00642510
      a. Object::walk_data                             0x00647830
           - SubObject::walk_data                      0x006621D0   (object lane)
           - walk [0x20, 0x42)  myhits..launch_frames           34 B
           - optional `launching` SimpleArray<int>
      b. walk [0x48, 0x66)  job_counter..demolition            30 B
 4. Object::must_walk gate                             0x00647930
 5. walk [0x70, 0x86)     gather_down..infiltrate2            22 B
 6. BuildQueue::walk_data                              0x006305F0
      - walk num                                                4 B
      - per entry: walk [e, e+0x12)                            18 B
 7. walk [0xB4, 0xB6)     MiningList::{mtn, cliff}              2 B
 8. Array<TCoordData>::walk_data                       0x00471C30
 9. PtrLinkListAbstract<GatherPoint>::walk_data        0x004708A0
      - walk count                                              4 B
      - per point: walk [node+0xC, node+0xD) 1 B, walk [gp+4, gp+0xD) 9 B
10. walk [0x6C, 0x70)     orig_type                             4 B
```

Steps 7 and 8 look inverted — the two `MiningList` tail bytes hash *before* the array they
follow in memory — but that is the emitted call order, and adler32 is order-sensitive, so it
is reproduced as written.

`CheckSum::walk_function` (`0x00936FF0`) is the whole visitor:
`bytes += end-begin; sum = adler32(end-begin, sum, begin)`. The **running** sum seeds each
call, so the channel value depends on window order and on every earlier window.

**Channel iteration** (`CheckSums::check_builds` `0x00937290`):

```
for p in 0..8:                                  ; leaders 0x00E3A390, stride 0x6EEC
    if (!(leaders[p].flags & 1)) continue;      ; inactive players contribute NOTHING
    for i in obj_base[1] .. objects.counts[p]:  ; obj_base[1] == 2000
        o = objects.list[p][i]->get_build();    ; vt +0xAC
        if (o->flags & 1) o->walk_data(w);      ; vt +0x7C
```

The outer loop is over **players**, not over a global object array — a flat pool hashes in a
different order and desyncs. The `flags & 1` test is on the object, so a
destroyed-but-not-compacted slot is skipped rather than hashed as zeros. Both are tested.

`BuildPool::check_builds` reproduces this; `BuildPool::channel_checksum` gives the isolated
channel value a replay diff wants (`check_all` itself carries one running `CheckSum` through
all 15 channels, so the isolated value is a debugging aid, not the wire value).

---

## 6. Honest gaps

**Not implemented, and named as such:**

- **`BuildTypeData::blocked_location` `0x006375B0` (5,120 B) and `blocked_tcoord`
  `0x00636DB0` (2,044 B)** — the full placement-legality predicate: terrain, water, cliffs,
  enemy territory, adjacency, dock tiles, town limits. I derived the *footprint geometry*
  (`area`, `corner_tile`, `tile_corner`) and the *verdict partition* `do_construct` applies
  to `blocked_site`'s return code, but not the predicates themselves. They depend on the
  world/terrain lane's tile queries. This is the largest single hole.
- **`BuildTypeData::snap_center` `0x00636190`** — placement snapping. Not read.
- **`Build::queue_up` `0x00620F40` (4,322 B) and `Build::train` `0x0062F9B0` (2,708 B)** —
  the *enqueue* and *spawn* halves. I implemented the queue **tick** and the **refund**;
  the payment-on-enqueue path and unit placement-on-completion are not ported. `can_queue`
  / `could_queue` / `can_make` were read (they gate on `queued < num` and a scholar cap of
  7 via `count_queue(1, 0x34) + num_gatherers > 6`) but are type-tree dependent.
- **`Build::activate` `0x00623E20` (13,088 B) and `Build::close` `0x00628980` (3,510 B)** —
  completion and destruction bookkeeping. I ported the *trigger* for both
  (`construct_time <= job_counter`; the collapse rule) and confirmed `Build::close` begins
  with `Build::clean_queue`, i.e. the queue is refunded on destruction. The plunder,
  city-membership and road-unmasking side effects are not ported.
- **Upgrades.** The brief asked for "upgrades and their cost formula". I found the surface —
  `ObjectType::load_upgrade` `0x006615B0`, `ObjectTypeData::upgrade_level` `0x00661090`,
  `TypeData::upgrade` (`Type+0x44`), `Leader::produce_upgrade` `0x006CB5D0`,
  `City::check_upgrade` `0x00738B20`, `CityData::{can_upgrade, upgrades_to, ready_to_upgrade}`,
  `ObjectTypeData::special_upgrade_cost` (`+0x264`) — and the *cost ramp* that upgrades
  share with everything else is implemented (§4). But the upgrade-specific cost path in
  `Leader::produce_upgrade` was **not** read. Do not treat §4 as "the upgrade cost formula";
  it is the ramp that upgrades participate in.
- **`Build::plunder` `0x00623660`**, `Build::check_capture` `0x006276A0`,
  `Build::swap_team` `0x00628210` — capture/plunder on destruction. Out of scope but
  adjacent; the rules (`plunder`, `city_plunder_per_level`, `capital_plunder`,
  `aztec_plunder`, `thedespot_plunder`) are all located if someone picks this up.

**Open questions, stated rather than papered over:**

1. **`PROGRESSION` bit 0 has no located consumer.** Bit 1 is the triangular selector; bit 0
   is read nowhere in `get_cost` or `train_time`. Values 0–3 are therefore only half
   explained.
2. **The wonder-uniqueness collapse in `BuildData::construct_time`.** Gated on
   `has_tribe_bonus(0x14)` plus two type exclusions, the engine scans every *other* active
   player's build band from index 2000; if the scan finds no matching type it sets
   `construct_time = 1` — instant. Reproduced as an input flag
   (`ConstructQueryGates::wonder_unique_instant`) because the semantics ("only if nobody
   else has one") are surprising enough that I do not want to assert them without a live
   check.
3. **`Wall::process` uses floats in walked-state-adjacent code.** The wonder-race helper
   count compares `job_counter / construct_time` as `f32` across all eight players to decide
   whether to spawn assistance. It feeds `helpers`, which is walked. Not ported (it belongs
   to the AI/wonder path) but flagged: it is a float in the sim.
4. **`BuildData::hits` interpolates in `f32`.** While a building is being razed
   (queue type `0x29A` or `0x286`) its hit points fall linearly to 1 through an `f32`
   multiply/divide with a truncating cast. This is a genuine exception to README-LLM's "the
   sim is INTEGERS" and belongs on the same list as `LeaderData::anti_att`,
   `LeaderData::plunder_scale` and `Unit::move_step`. Reproduced literally in `build_hits`.
5. **Multi-slot production.** `Build::do_queue` recurses into `do_queue(slot+1)` for types
   satisfying `is(0x1B3)`, and `BuildData::get_queue` remaps slots past `queued` onto a
   *different building* (`FUN_006DB6C0`, apparently a per-player primary/capital). Neither
   is ported; both are type-tree/leader lookups. This is the mechanism behind buildings that
   train several things at once and needs its own pass.
6. **`Object::must_walk` `0x00647930`** is an input to the walk. Getting it wrong changes
   which windows are hashed, so it must be settled by whoever owns `Object`.

**What would make this Tier B.** Oracle cases on hbox for, in priority order:
`Wall::do_construct` (`0x006434D0`) swept over `rate × helpers × job_counter`;
`Wall::update_hits` (`0x0063F0D0`) swept over `hits × job_counter × construct_time` with the
wonder flag both ways; `ObjectData::train_time` (`0x006508C0`) swept over
`base × count × job_extra_time`; and `Build::refund_cost` (`0x00620490`) over
`amt × ages_elapsed`. The first three are leaf-ish arithmetic on a `this` pointer plus
globals, which is the shape the existing oracle already handles.

---

## 7. Wiring (one line each, not done by this lane)

The module is self-contained — it compiles standalone with `rustc` and has no crate-internal
dependencies — but it is **not reachable from the crate** yet:

- `crates/don-sim/src/systems/mod.rs` lists `ammo, borders_fog, economy, groups_guys,
  map_terrain, movement, victory_score` as of this writing. It needs one more line:
  `pub mod production;`.
- `crates/don-sim/src/lib.rs` does not declare `pub mod systems;` at all, so the whole
  directory is stranded until it does.

I did not edit either file: `lib.rs` is the sim-core lane's, and `systems/mod.rs` is shared.
Verification was therefore `rustc --edition 2021 --test` on the file directly —
**62 passed, 0 failed**, and a `--crate-type lib` build with zero warnings.

Two things to reconcile once the tree is wired:

- `production::adler32` duplicates `economy::adler32` byte for byte. If `don-sim` grows a
  shared checksum module, both should defer to it.
- `production::train_time_ramp` supersedes `mechanics::ramped_rate` (§3.5). Same integers,
  correct name, and the "what is `x`" caveat resolved.
