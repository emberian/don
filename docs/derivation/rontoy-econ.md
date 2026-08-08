# RoNtoy economy telemetry: exact read plan and coaching contract

**Status:** read-plan ready for the leader/economy R1; prescriptive allocation, per-building
queue ETA, and entity-exact idle counts still need the controlled experiments in §9. This
report describes read-only telemetry. It does not authorize calls into the game, process
writes, suspension, or memory scans.

This is deliberately a telemetry derivation rather than an opening-build model. Where the
two disagree, `docs/tracks/analytics-v2.md` supersedes
`docs/tracks/build-order-analytics.md`. In particular, the engine's current cap status is a
better coaching input than a static rule such as "the seventh worker is useless."

Evidence labels follow the charter:

- **[measured — PDB]** means a layout, symbol, or type was read from the matching shipped
  `rise.pdb`; a PDB name establishes structure, not behavior.
- **[measured — code]** means the instruction stream of the shipped
  `riseofnations.exe` was checked at the named address.
- **[measured — shipped data]** means a join comes from the extracted game XML on this
  machine.
- **[reported — live]** means the coordinating lane captured it from Ember's current
  process with targeted `ReadProcessMemory`; this lane received the capture rather than
  independently repeating it.
- **[inferred]** and **[open]** are intentionally not implementation facts.

## 1. Rebase, identity, and a bounded coherent read

All addresses below are preferred-image VAs for image base `0x00400000`. At runtime:

```text
delta       = module_base - 0x00400000
runtime(VA) = VA + delta
```

**[measured — PDB/code]** The stable roots are:

| root | preferred VA | interpretation |
|---|---:|---|
| `GameAccess::game` | `0x00C061EC` | address holding `Game*` |
| `GameAccess::objects` | `0x00C0618C` | address holding `Objects*` |
| `leaders` | `0x00E3A390` | inline `Leader[8]`, not a pointer |
| `sizeof(Leader)` | `0x6EEC` | leader slot stride |

The supported executable is SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`, PE32 with
preferred image base `0x00400000`, entry RVA `0x0015D699`, and
`SizeOfImage=0x00BB4000`. Readers must reject a mismatch before applying these offsets. A
`Leader*` for slot `s` is:

```text
leader(s) = runtime(0x00E3A390) + s * 0x6EEC,  0 <= s < 8
```

### Selecting the local player and team

**[measured — PDB]** Relevant `LeaderData` fields are `leader_flags +0x00`, `who +0x08`,
and `tribe +0x0C`. **[reported — live]** In the current game, the unique active record with
`leader_flags & 4` is slot 0; it has `who=0`, `tribe=22` (Dutch), while active AI records do
not have bit 2. The reader should select a local-human slot only when exactly one record
satisfies `(leader_flags & 0x7) == 0x7`: bits 0 and 1 are both required for an in-game,
playing leader and bit 2 is the observed local-console/human bit. Ambiguity is invalid
state, not permission to assume slot 0.

**[measured — PDB]** Team comes from `Game::info.player[s].team`, not
`LeaderData::team_color`. `Game::info` is at `Game+0x0C`; the `Player[8]` array begins at
`GameInfo+0x38`, with `sizeof(Player)=0x8C`:

```text
player(s) = game + 0x0C + 0x38 + s * 0x8C
flags     = u16(player + 0x30)
tribe     = u8 (player + 0x32)
who       = u8 (player + 0x33)
team      = i8 (player + 0x34)    // preserve signed value
handicap  = u8 (player + 0x35)
play      = u8 (player + 0x36)
difficulty= u8 (player + 0x38)
name      = String(player + 0x40)
```

The `String` begins with a pointer at `+0x00` and has `curr_len` at `+0x08`. Name decoding
must be independently range-checked and length-bounded; it is optional for coaching.
Cross-check `Player.who == LeaderData.who`, `Player.tribe == LeaderData.tribe`, and use the
signed `Player.team` as the feed's team value. Do not infer team from color.

Rate/ETA advice also needs a mode gate. **[measured — PDB]** Read at least
`GameInfo.flags +0x14`, `game_speed +0x1D`, `game_rules +0x1E`, `difficulty +0x1F`,
`scenario_type +0x33`, `script_type +0x34`, and `mods +0x35` relative to `Game+0x0C`.
Unknown or unsupported mode combinations may still emit raw telemetry, but must suppress
wall-clock and effective-accrual claims.

### Snapshot guard

**[measured — PDB]** `Game::frame` is `Game+0x550`. A low-frequency reader should:

1. read `frame_start`;
2. read one bounded leader record, its one encrypted block, and any explicitly enabled
   object-list slices;
3. read `frame_end`;
4. accept only `frame_start == frame_end`, otherwise retry a small bounded number of times;
5. return validity per component rather than zero-filling failed pointer reads.

This is a best-effort coherence guard, not a true process-wide seqlock. Pointer stability,
slot identity, sane array bounds, and the invariants in §8 remain required. A useful feed
never performs an address-space scan: the leader MVP is one `0x6EBC`-byte prefix through
`data_encrypted` plus one `0xF8`-byte pointed-to block, and detailed queues use only the
engine's bounded owner list.

## 2. The real economy block

`LeaderData+0x450` is a different PDB member named `econ[6]`; it is AI bookkeeping and is
**not the stockpile**. **[measured — PDB]** The real block is:

```text
enc = *(u32 *)(leader + 0x6EB8)  // LeaderData::data_encrypted
sizeof(LeaderDataEncrypt) = 0xF8
```

Every stored value is little-endian `i32` after XOR. **[measured — PDB/code]** The layout
comes from `LeaderDataEncrypt`; every mask is written explicitly by
`LeaderDataEncrypt::init` at `0x006D9C60`:

| offset | count | PDB member | XOR mask | coaching meaning and units |
|---:|---:|---|---:|---|
| `0x00` | 6 | `bucket` | `0x8221` | stockpile, whole resources |
| `0x18` | 6 | `leftover` | `0x3421` | fractional accrual carry; diagnostic, do not render yet |
| `0x30` | 7 | `resource_cap` | `0x1281` | pre-interest Commerce clamp threshold in 1/16-resource units; first six map to resources |
| `0x4C` | 6 | `over_cap` | `0x8932` | pre-interest clamp status, decoded precisely below |
| `0x64` | 6 | `resources` | `0x0872` | gross resource-production term, 1/16 units |
| `0x7C` | 6 | `support` | `0x26076` | support/expense term, 1/16 units |
| `0x94` | 6 | `income` | `0x90236` | displayed/net income, 1/16 resource per 30 game-seconds |
| `0xAC` | 6 | `rate` | `0x73862` | AI planning rate, whole resources per gather period; not live income |
| `0xC4` | 6 | `bonus` | `0x6722` | named bonus/source accumulator; exact presentation semantics open |
| `0xDC` | 1 | `ages` | `0x62766` | chronological age index |
| `0xE0` | 1 | `epochs` | `0x69587` | aggregate epoch state; diagnostic until validated |
| `0xE4` | 1 | `discovered` | `0x13985` | discovery state; diagnostic until validated |
| `0xE8` | 4 | `epoch` | `0x63187` | Military, Civic, Commerce, Science library-column levels |

**[measured — live/cross-checked against shipped types]** Array order is:

```text
0 food, 1 timber, 2 wealth, 3 knowledge, 4 metal, 5 oil
```

The seventh `resource_cap` entry is not a seventh stockpile; keep it in diagnostic raw
state until its behavior is derived.

**[measured — code]** At the pre-interest Commerce clamp, `Leader::do_gather` writes:

```text
over_cap[r] = 0                                      if pre-interest income <= cap
over_cap[r] = 1 + (resource_cap[r] > 0x3E6F)        otherwise
```

Thus 1 means clamped, while 2 means clamped and the configured cap is at least 16000 raw
units. It does **not** mean the later Dutch hard clamp fired. Dutch interest is added after
the Commerce clamp and can make displayed income exceed `resource_cap` while
`over_cap == 0`. Treat `over_cap` as the engine's authoritative current clamp state and the
cap as a pre-interest threshold, not as an absolute ceiling on the final HUD number.

### Which rate RoNtoy should use

**[measured — code]** `Leader::do_gather` (`0x006CE450`) constructs net income from the
gross `resources`, `support`, base rates, bonuses, and clamps, then caches the displayed
value at `+0x94`. `ScenarioFuncSet::gather_rate` (`0x009E90B0`, shift at `0x009E914E`)
returns it shifted right four, and `IFaceResources::draw_resource` (`0x00818840`, display
site `0x00818C05`) rounds the positive HUD value as `(income + 8) >> 4`. Therefore:

```text
hud_rate_per_30_game_seconds = decoded_income / 16
standard-speed_rate_per_minute = decoded_income / 8
```

Retain the exact raw `i32`; decimal display may use sixteenths. Wall-clock ETA must also
know game speed. The cached value is written before some difficulty/handicap/game-mode and
speed modifiers in `do_gather`, so it is the engine/HUD rate rather than a universal proof
of the next stockpile delta. For a normal human game it is the correct coaching basis; in
special modes, compare it with controlled no-spend stockpile captures before promising an
ETA.

**[measured — code]** `rate +0xAC` must not be substituted for this. During
`Leader::production_ai_setup` (`0x006C83E0`) it is written as approximately
`min(get_mod_resource_cap(r), income[r]) / 16` and used for AI scarcity decisions. On a
human leader it may be stale. Decode and expose it only as `ai_planning_rate` with a
validity bit.

The coach also must not infer a "spend rate" from stockpile deltas. Purchases, refunds,
tribute, plunder, ruins, market trades, and gather credit are confounded in the same bucket.
An affordability ETA is a conditional projection only:

```text
ETA = deficit / positive_income_rate
```

and must say "if income holds and you do not spend". Command/purchase events are required
before stock deltas can be decomposed.

## 3. Allocation, population, and fast idle telemetry

**[measured — PDB/code]** These direct `LeaderData` counters make a useful economy feed
without visiting any entity:

| offset | type | PDB name | feed meaning |
|---:|---|---|---|
| `0x3F8` | `i32` | `city_num` | current city count |
| `0x3FC` | `i32` | `village_num` | current village count |
| `0x7AC` | `i32` | `gather_stamp` | frame of last gather-cache refresh |
| `0x7E4` | `i32` | `pop_cap` | current population cap |
| `0x8A4` | `i32[6]` | `gather_slots` | available slots by resource |
| `0x8BC` | `i32[6]` | `filled_gather_slots` | occupied slots by resource |
| `0x8D4` | `i32[6]` | `gather_slots_high` | historical high-water; not current capacity |
| `0x940` | `i32` | `control` | exact gameplay population, despite the misleading PDB name |
| `0x970` | `i32` | `fishermen` | fishermen unit-class count |
| `0x974` | `i32` | `idle_fishermen` | idle fishermen unit-class count |
| `0x978` | `i32` | `peasants` | citizen-class count |
| `0x97C` | `i32` | `scholars` | scholar count |
| `0x980` | `i32` | `caras` | caravan count |
| `0x984` | `i32` | `merchants` | merchant count |
| `0x9BC` | `i32` | `free_peasants` | fast idle/free citizen counter |
| `0x9C4` | `i32` | `gatherers` | direct gatherer counter; live relation to scholars described below |

The population correction is load-bearing. **[measured — code]**
`ScenarioFuncSet::population` at `0x009E8E70` reads `leader+0x940`, while
`ScenarioFuncSet::population_cap` at `0x009E8EB0` reads `leader+0x7E4`.
`LeaderData::pop` at `+0x95C` is an AI combat-class counter and is not player population.
Never clamp population to cap: over-cap states are legal.

**[measured — code]** Gather-cache freshness is:

```text
cache_age_frames  = game.frame - gather_stamp
cache_age_seconds = cache_age_frames / 15
```

`Leader::calc_gather` (`0x006CEEE0`) refreshes a dirty economy on a modulo-8 fast path,
within about 0.53 game-seconds. The clean background path requires both
`frame >= gather_stamp + 300` and `(frame + 8*who) mod 256 == 0`; steady-state clean
refreshes are consequently 512 frames apart, about 34 seconds, rather than 256 frames.
Every income-based recommendation should carry `cache_age_frames` and suppress itself if
the cache is implausibly stale.

### Allocation observations and future advice

R1 may report `filled_gather_slots[r] / gather_slots[r]` as observed slot pressure. It must
not yet recommend a worker move or a capacity building from those ratios: one live
aggregate relation does not validate each resource's reassignment and source-change
semantics. Those become prescriptive only after the slot experiments in §9.

Use decoded `over_cap[r]`, not a hard-coded worker number, to say pre-interest production
is clamped. Civilization bonuses, wonders, Dutch interest, knowledge behavior, and current
Commerce level all defeat a universal threshold.

**[reported — live]** At frame 28439, slot 0 had:

```text
gather_slots        [6, 27, 7, 28, 21, 4]
filled_gather_slots [6, 22, 0, 27, 19, 3]
gatherers           50
free_peasants       2
gather_stamp         28384  (55 frames / 3.67 seconds old)
```

The capture invariant `sum(filled) - filled[knowledge] = 77 - 27 = 50` exactly matches
the direct `gatherers` counter in that capture. This is strong evidence that the filled
array includes scholars while `gatherers` excludes them. Retain this as a runtime sanity
check, but do not reject civilizations or scenarios until the next-match matrix tests
exceptions.

## 4. Exact idle citizens without calling the game

`free_peasants +0x9BC` is the cheap MVP signal. For entity-exact validation, visit only the
human owner's bounded unit list (§5), filter active objects to Citizen TypeIndex 50 and 51,
then emulate `UnitData::is_idle` read-only.

**[measured — shipped data/PDB]** Citizen variants are TypeIndex 50 and 51; scholars 52 and
53 are separate. `UnitData::orderlist` begins at `+0xC8`. **[measured — code]**
`UnitData::is_idle` at `0x0046FA40` behaves as follows:

```text
head = *(u32 *)(unit + 0xDC)
if head == 0:
    idle = true
else:
    prev = *(u32 *)(head + 0x04)
    // prev must be non-null and readable; otherwise validity = false
    order = *(u32 *)(prev + 0x08)
    idle = (order == 0)
```

`RecycledOrderNode` is 16 bytes: `next +0`, `prev +4`, `data +8`, `metric +0xC`.
The retail method writes cached `current_node/current_data/current_metric` fields at
`unit+0xD4/+0xCC/+0xD0` while answering. RoNtoy must not call it; the pointer-chain
emulation above produces the same predicate without writes. A null or unreadable `prev` is
invalid telemetry, not evidence of idleness.

The PDB member `UnitData::idle` byte at `+0xB0` is not this boolean: the shipped
`is_idle` method never reads it. Do not use `byte != 0` as the idle predicate.

Validation counters should report all three:

```text
idle_citizens_fast  = LeaderData::free_peasants
idle_citizens_exact = count(type in {50,51} && is_idle_read_only)
idle_citizens_bad_reads
```

Only graduate the entity count into active advice after a controlled stop/reassign/build
test explains any delta from `free_peasants`.

## 5. Counts and queues

### Aggregate, no entity traversal

**[measured — PDB/code]** The leader has exact typed count tables:

| offset | type | indexing |
|---:|---|---|
| `0x555E` | `u16[129] num_buildings` | building TypeIndex `t=414..542` at `t-414` |
| `0x5762` | `u16[352] num_units` | unit TypeIndex `t=50..401` at `t-50` |
| `0x5A22` | `u16[806] num_queued` | direct TypeIndex `t=0..805` |

`Leader::compute_build_score` (`0x006BC3F0`) and `compute_unit_score` (`0x006BC500`)
independently use these index transforms and add `num_queued[t]`, establishing that
queued counts are separate from current owned counts. `num_queued` is the cheapest exact
answer to "what is being made or researched?"; join `t` to the shipped type tables for a
name and category.

There are also coarse `i32` counters at `+0x9F8..+0xA0C` for current
Barracks/Stable/Factory/Combat/Dock/Air attack-unit classes and `+0xA10..+0xA24` for
corresponding `*_queued` counts. These are **not six producer queue depths**.

**[measured — code]** `Leader::track_queued(type, delta)` at `0x006E0F30` first updates
`num_queued[type]`, then, only when the type is a unit with nonzero attack, classifies it:

```text
TypeData.where == 427 (Barracks) -> barracks_queued += delta; combat_queued += delta
TypeData.where == 428 (Stable)   -> stable_queued   += delta; combat_queued += delta
TypeData.where == 430 (Factory)  -> factory_queued  += delta
TypeData.where == 432 (Dock)     -> dock_queued     += delta
otherwise ObjectTypeData.domain == 2 -> air_queued += delta
```

They are aggregate queued attacking-unit counts by production class. Use
`num_queued[t]` for content-specific advice and per-`Build` queues for producer idleness.
When derived entirely through this writer, `combat_queued == barracks_queued +
stable_queued`; treat a violation as legacy/load drift or invalid telemetry rather than
repairing the values.

**[reported — live]** At frame 28439 the six `*_queued` counters were
`[9,3,1,1,0,2]`. This violates the writer invariant because `1 != 9+3`; the capture is
useful precisely because it proves that these counters must be diagnostic/validity-checked
and must not back a "keep your producers busy" or population-headroom nudge. A controlled
one-item queue/cancel in each producer plus a save/load boundary should locate the drift.

`num_buildings` excludes `num_queued`, but whether a just-placed unfinished object already
increments `num_buildings` remains **[open]**. Validate placement, completion, and cancel
instead of assuming "building count" means "completed count."

### Per-building queue and construction progress

Detailed "which city/building is training what?" telemetry uses the engine's owner list,
not a heap scan. **[measured — PDB/code]** The exact traversal is:

```text
objects = *runtime(0x00C0618C)
array(s) = objects + 0x04 + s * 0x1C
length   = i32(array + 0x04)
capacity = i32(array + 0x08)
list     = u32(array + 0x10)
end      = i32(objects + 0x184 + 4*s)   // build_mark[s], decimal offset 388

for i in 2000 .. end:
    build = *(u32 *)(list + 4*i)
```

Require `0 <= 2000 <= end <= length <= capacity`, a conservative capacity ceiling, a
non-null object, `(u8(build+0x08) & 1) != 0`, and `u8(build+0x09) == s`. The building's
TypeIndex is `*(u32 *)(build+0x18) + 0x04`. Useful `BuildData` fields are:

| offset | type | meaning |
|---:|---|---|
| `0x08` | `u8` | object flags; bit 0 is live; `WallData::is_active` tests bit 2 |
| `0x48` | `u32` | construction `job_counter` |
| `0x50` | `u32` | cached `constr_time` |
| `0x6C` | `i16` | `orig_type` |
| `0x72` | `i16` | associated city index |
| `0x74` | `i16` | `city_down` link |
| `0x80` | `u8` | gather-slot capacity for resource buildings |
| `0x82` | `u8` | logical `queued` count |
| `0x88` | `i32` | embedded `BuildQueue.queue_size` |
| `0x8C` | `QueueItem*` | embedded queue storage pointer |

`QueueItem` is `0x14` bytes:

```text
+0x00 i32 job_counter
+0x04 i16 type
+0x06 i16 good[3]
+0x0C i16 cost[3]
```

**[measured — code]** `BuildQueue::set_queue` (`0x006309F0`) fills up to three nonzero
resource/cost pairs and initializes unused pairs to `good=-1, cost=0`.
`BuildData::get_queue` (`0x0062D280`) bounds an entry against both `BuildData::queued` and
`BuildQueue.queue_size`. A remote reader should therefore read at most
`min(queued, queue_size)`, sanity-cap both, and record a mismatch as invalid/torn state.

**[measured — shipped data/PDB]** City centers are Build types 414 (Village/Small City),
415 (Town/Large City), and 416 (Metropolis/Major City); Library is 435. They live in the
same building band and use the same embedded `BuildQueue` layout as military producers.
Consequently a city or research queue needs no separate memory-scanning mechanism: identify
the Build by TypeIndex and retain its `city +0x72` association.

The construction-state interpretation of flag bit 2 must also be checked on a normal
building; the instruction-level fact established so far is the `WallData::is_active`
predicate, not every `Build` transition.

The queue item's `job_counter` is an exact progress numerator. Its denominator is not in
the item: retail computes it through `ObjectData::train_time(type)` at `0x006508C0`, with
owner/type-dependent rules. Until that join is implemented and validated, expose raw
progress but do not invent a percent or seconds-to-complete. The analogous construction
ratio `BuildData.job_counter / constr_time` is useful only after a one-building live test
establishes direction, reset behavior, and units.

## 6. Age, technology, and Commerce caps

The three independent pieces should remain distinct in the feed:

1. **Chronological age:** `decode(enc+0xDC, 0x62766)`.
2. **Library columns:** decode four `i32` values at `enc+0xE8` with `0x63187`, ordered
   Military, Civic, Commerce, Science.
3. **Owned techs:** `LeaderData::tech`, a `BitMask<806>` at `leader+0x6C0C` whose 101-byte
   payload begins at `leader+0x6C18`.

**[measured — code]** `BitMask<806>::get` at `0x0044F680` tests:

```text
owned(t) = (byte[leader + 0x6C18 + (t >> 3)] & (1 << (t & 7))) != 0
```

Join TypeIndex to the shipped `techrules.xml`/type table. The first chronological-age tech
TypeIndices are 544 through 550; all queued research, including age advancement, is also
visible in `num_queued[t]`. PDB bytes `ages_queued +0x67F4` and
`epochs_queued +0x67F5` may be retained diagnostically, but the type-specific queue is less
ambiguous.

**[measured — shipped data/code]** The `epoch` order is established by the shipped library
tech order and consumers: `epoch[0]` drives population cap, `[1]` the city limit, `[2]`
Commerce/resource caps, `[3]` Science. `Leader::calc_resource_caps` (`0x006CE900`) stores
the first six decoded caps in the same 1/16 units as income. Convert with `/16`; this is an
income-rate threshold for the pre-interest clamp, never a storage ceiling or an absolute
ceiling on final HUD income.

Knowledge is a special case (the live cap can be 999), and national/wonder modifiers can
make per-resource caps differ. The feed should expose all six current caps and statuses,
not reconstruct them from a static Commerce table.

## 7. Current live witnesses

These snapshots were read from PID 5236, runtime image base `0x00D60000` (delta
`+0x00960000`), with human slot 0. They are **[reported — live]** coordinating-lane
captures and are recorded to make the decoder falsifiable.

At frame 28439, `frame_start == frame_end`, `gather_stamp=28384`, and:

| resource | stockpile | HUD/net per 30 game-s | pre-interest cap per 30 game-s | over-cap |
|---|---:|---:|---:|---:|
| food | 1132 | 574 | 550 | 0 |
| timber | 1159 | 547 | 500 | 1 |
| wealth | 877 | 471.4375 | 550 | 0 |
| knowledge | 1003 | 610 | 999 | 0 |
| metal | 356 | 477 | 500 | 0 |
| oil | 146 | 245.5625 | 500 | 0 |

The exact decoded `income` integers before `/16` were
`[9184,8752,7543,9760,7632,3929]`. Timber is the useful clamp cross-check: decoded income
547 is above its pre-interest threshold 500 and `over_cap[timber] == 1`. Food is the
equally important Dutch cross-check: final displayed income 574 is above threshold 550
while `over_cap[food] == 0`, consistent with post-clamp interest. Do not reinterpret
`over_cap` as an amount or the cap as final-HUD maximum.

The same frame had gameplay population `194`, cap `200`, city/allocation values from §3,
and queue categories from §5. An earlier coherent capture at frame 26962 read raw
little-endian `b4 00 00 00` from slot 0 `leader+0x940`, hence population 180, while cap was
150. That witness both validates the counterintuitive `+0x940` population offset and proves
the feed must preserve legal over-cap state.

An earlier frame-21805 snapshot decoded `ages=3` and `epoch=[4,3,5,5]`. It is a separate
time sample and must not be merged into the frame-28439 object as if atomically captured.
At frame 31777, a separate coherent identity probe read signed `Player[0].team = 8` from
`Game+0x78`. Preserve that raw value; the FFA/team interpretation awaits the lobby/UI
validation in §9.

## 8. Runtime invariants and advice gates

Reject or validity-mask a component when any applicable invariant fails:

- exactly one local-human slot with `(leader_flags & 0x7) == 0x7`; `Leader.who`,
  `Player.who`, and array slot agree;
- executable fingerprint and mode gate are valid for any interpreted advice;
- frame start equals frame end; `0 <= gather_stamp <= frame` absent explicit wrap handling;
- `enc != 0` and the full `0xF8` block is readable;
- age and four epoch values lie in a conservative shipped range;
- stockpiles, rates, caps, counters, and slot arrays are non-negative unless a named field
  is known to permit negatives;
- `filled_gather_slots[r] <= gather_slots[r]` unless a controlled exception has been
  measured; do not silently clamp it;
- `sum(filled) - filled[knowledge] == gatherers` is a high-value warning invariant, not yet
  a universal hard reject;
- `over_cap[r]` is in `{0,1,2}`;
- object-list bounds satisfy the inequalities in §5; queue lengths and pointers agree;
- population is allowed to exceed population cap.

Advice should be gated by the source fields it needs:

| advice | required valid inputs | suppress when |
|---|---|---|
| "idle citizens" | `free_peasants` (MVP) or exact entity count | pointer disagreement / bad entity reads |
| "pre-interest production is clamped" | current `over_cap` + income + cap | `over_cap` outside 0..2 |
| slot-pressure observation | slots + filled | allocation cache invalid |
| "rebalance toward X" | slots, filled, cap state, stockpile, costs **plus completed §9 validation** | disabled in R1 |
| "queue a citizen" | pop/cap + Citizen `num_queued[t]` | population at cap or R2 type queue invalid |
| "keep producer X busy" | pop/cap + a valid per-building queue | disabled before R3 |
| affordability ETA | stockpile + HUD income + game speed + known cost | nonpositive rate, stale cache, spending/event ambiguity |
| build/train completion ETA | queue numerator + derived retail denominator | denominator not validated |

## 9. Next-match validation matrix

Perform these as low-frequency, targeted reads with HUD notes or screenshots. Capture a
coherent before/after record and the exact user action; never scan or write process memory.

| experiment | action | fields | success criterion |
|---|---|---|---|
| resource decoder | spend a known single-resource amount while paused between reads | `bucket`, queue cost | bucket delta matches cost once, no index swap |
| HUD rate/rounding | hold economy stable and note six HUD values | `income`, `gather_stamp` | HUD equals retail rounding; cache age explains delay |
| actual accrual | no commands/spend for at least one gather period | bucket, leftover, income, speed | stock delta establishes normal-mode conversion and modifiers |
| cap transition | move one worker onto/off a nearly capped resource | slots, filled, income, cap, `over_cap` | 0↔1 transition and clamp direction match HUD |
| slot semantics | add/remove one Farm/Camp/Mine/University/Oil source | slot arrays | exactly the expected resource slot changes; `high` does not fall |
| idle citizen | stop one Citizen, then give gather and build orders | `free_peasants`, exact order chain | fast/exact counters change at documented moments |
| population | queue/finish/lose one unit and trigger an over-cap state | `+0x940`, `+0x7E4` | HUD population and cap agree without clamping |
| building count | place, complete, cancel, and destroy one known building | `num_buildings`, `num_queued`, build flags | unfinished/count semantics become explicit |
| producer queues | add/cancel one item in city, Barracks, Stable, Factory, Dock, Airbase, Library | aggregate and per-build queues | type count, category count, queue location/order all agree |
| queue progress | train one known unit with no modifiers | item job counter, `train_time` model, frame | direction, units, denominator, completion reset agree |
| research | queue/cancel/finish one library tech and one age | age, epoch, tech bit, `num_queued` | ownership bit flips only on completion; column/age joins agree |
| player/team | record a team game and an FFA | Player/Leader identity fields | signed team value and slot mapping match lobby/game UI |

## 10. Staged feed schema

The first useful RoNtoy feed is the compact, leader-only R1:

```text
frame, coherence, cache_age_frames
mode { game_speed, game_rules, scenario_type, script_type, mods, supported }
player { slot, who, tribe, team, is_human }
economy[6] {
  stockpile, hud_income_raw, hud_income_per_30s,
  pre_interest_cap_raw, pre_interest_cap_per_30s, over_cap,
  slots, filled_slots
}
population { current, cap }
workforce { citizens, scholars, gatherers, idle_citizens_fast }
cities { total, villages }
tech { age, columns[4] }
validity { core, mode, identity, econ, allocation }
```

R2 adds the `num_units`, `num_buildings`, `num_queued`, and 101-byte owned-tech bitset from
the same leader record. R3 adds the bounded object-list traversal, per-building queues, and
entity-exact idle predicate. Do not call the full R3 object graph "compact."

R1 is enough for honest idle-worker alerts, observational slot-pressure/cap warnings,
population headroom, and mode-gated conditional affordability projections. Producer-idle
advice starts at R3. This staging avoids the two most dangerous attractive shortcuts:
reading `LeaderData+0x450` as the stockpile and reading `LeaderData+0x95C` as population.
