# Bidirectional live retail control

Status: **live-validated for pause, unit movement, exact own-state observation, a bounded
supervised scout policy, and repeated active-main-thread STOP/rearm against retail solo
skirmishes. Multiplayer turn agreement and real-host peering remain separate open gates.**
Target: the one supported `riseofnations.exe`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.

## Why this path

Calling `CommandManager` from an injector-created worker would race the main thread. The
bridge instead redirects the sole direct `TurnControl::do_frame` call in `Game::loop`, at
`0x00591686`, to a wrapper that performs pre/post observation around the original method.
The five replaced bytes are exactly:

```text
e8 45 67 3c 00    call TurnControl::do_frame (0x00957DD0)
```

The DLL refuses any different bytes and also checks the PE machine (`0x14c`), entry RVA
(`0x15d699`), and image size (`0xbb4000`). `retailctl.py deploy` additionally hashes the
target file before injecting. Runtime addresses are always `module base + measured RVA`;
there are no guessed offsets.

The worker parses `request.txt`; a tiny main-thread callback consumes an atomic request.
That callback calls the shipped entry points and snapshots the live state. The worker only
serializes those fixed records to `events.ndjson`. Thus file I/O and parsing never occur
on the game thread.

## Supported retail calls

| request | shipped entry | proof recorded |
|---|---:|---|
| `pause 0|1` | `CommandManager::issue_pause` `0x00940BA0` | exact opcode `0x4c` bytes + pause bit transition |
| `speed N` (`0..4`) | `issue_speed_set` `0x00940B60` | exact opcode `0x34` bytes + `TurnControl+0x30` state |
| `speed-up` / `speed-down` | `0x00940B30` / `0x00940B00` | exact `0x35` / `0x36` bytes |
| `observe-network` | read-only post-`do_frame` snapshot | exact network/playback/immediate/player gates, seed/settings, package room, and eight peer totals |
| `halt WHO IDS...` | `issue_halt` `0x009418D0` | retail-generated group+halt bytes; selected unit order list becomes empty |
| `move WHO X Y QUEUED ORDER FORM WIDTH DISEMBARK IDS...` | `issue_move_to` `0x00941720` | retail-generated group+move bytes; current order pointer/vtable transition |
| `attack WHO TARGET_WHO TARGET_ID FLAGS QUEUED IDS...` | `issue_attack` `0x009415E0` | retail-generated group+attack bytes; current order pointer/vtable transition |
| `attack-visible WHO TARGET_WHO TARGET_ID TARGET_UID FLAGS QUEUED IDS...` | `issue_attack` `0x009415E0` after current-visibility replay | exact visible target identity; retail packet; applied `AttackOrder` owner/index/uid |

`WHO` and IDs are retail object owner/index coordinates, not DoN entity IDs. `X` and `Y`
are retail `Coord` integers, exactly as exposed by `donscan`; the control layer performs no
invented scaling. `QUEUED` is the shipped `QueuePos` enum (`0 first`, `1 last`, `2 new`).
`ORDER` is the shipped `OrderIndex`; ordinary move-to is `1`.

The synthetic `GroupOut` passed to retail contains only fields that
`CommandPackage::add_group` (`0x0094BB60`) demonstrably reads: `num +0x0c`, `who +0x4a`,
and `short list[128] +0x8cc`. Retail itself resolves each object, checks liveness/type,
converts indices to UIDs, enforces package capacity, and rejects empty/invalid groups.
The probe does not bypass those gates.

## Use

```sh
python3 tools/retail-control/wer_localdumps.py check
python3 tools/retail-control/retailctl.py prepare-injector
python3 tools/retail-control/retailctl.py preflight
python3 tools/retail-control/retailctl.py deploy --pid 5236
python3 tools/retail-control/retailctl.py send observe
python3 tools/retail-control/retailctl.py send observe-network
python3 tools/retail-control/retailctl.py send pause 1
python3 tools/retail-control/retailctl.py send pause 0
python3 tools/retail-control/retailctl.py send move 0 16000 12000 2 1 -1 -1 0 12 13
python3 tools/retail-control/retailctl.py stop
python3 tools/retail-control/retailctl.py rearm
```

`preflight` is read-only in the guest. It requires the current strict PE32 injector build and
guest copy to have the same deterministic SHA-256 and passing self-test, reconciles a complete
x86 module inventory to immutable controller roots, enforces the mapped-generation budget
(default one), classifies the external call-site bytes, and checks the two scoped WER LocalDumps
registry views. `wer_localdumps.py check` additionally proves the dump directory's exact protected
ACL; `setup` is the only command that creates that reversible per-executable policy and refuses
while retail is running.

Every response is NDJSON with `queued`, then (where state evidence exists) `applied` or
`timeout`. `command_hex` is copied from the exact range appended to retail's live
`CommandPackage`; it includes the optional one-byte multiplayer padding when retail inserts it.
Passive `observe-network` never calls the sender. The controller contains a structurally strict
decoder for 65/66-byte opcode-`0x39` captures, but the request protocol deliberately does not expose
a manual checksum sender: retail already emits one automatically in `process_turn`, so a second
packet could consume package capacity and perturb lockstep. Multiplayer proof therefore comes from
the passive post-turn totals plus the recorded replay packet. Unit order
evidence reads the PDB-defined `UnitData::orderlist` at `+0xc8`, specifically its current
order pointer (`Unit+0xcc`) and length (`Unit+0xd8`). Vtable values can be resolved through
`schema/vtables.json` (for example `MoveOrder` `0x00B4A12C`, rebased at runtime).

## Reversibility and current exercise

`STOP` first prevents new work and cancels any pending request, verification, or trace. During
an active game loop, the main-thread wrapper restores the five original call bytes only after
the post-`TurnControl::do_frame` boundary and after every wrapper invocation from that generation
has left. It verifies that the live bytes are that generation's exact detour before restoring
them and requires `FlushInstructionCache` to succeed. If the loop is dormant, the worker uses a
fail-closed fallback: it completely enumerates and suspends the other target threads, refuses if
any instruction pointer is inside the call site, trampoline, or controller image, rechecks hook
ownership while quiescent, restores the bytes, and resumes every suspended thread. Normal STOP
does not use `WriteProcessMemory` as an emulator-cache workaround.

Only then does the controller publish an identity-bound `parked` acknowledgement. Deleting `STOP`
starts a fresh request epoch, rechecks the original prologue, and reinstalls the detour.

An explicit detach is a separate terminal transition. The host first persists one create-only
preintent bound to the parked record's PID, process creation time, controller base/hash,
attempt, epoch, revision, and worker identity. The x86 injector resolves
`RetailControlPrepareDetach` from that same hash-held PE file, calls it once, and refuses unload
unless the controller joins its worker, frees its trampoline, and atomically publishes
`detach-ready`. It then uses an RX-only `FreeLibraryAndExitThread` stub and requires both the
module to be absent and the retail call bytes to remain original. Any uncertainty after the
remote preparation call requires a fresh retail process; it is never retried or rearmed.

This path is source- and offline-regression-complete but has not been exercised in retail. It is
not involved in the NetSys Gen-7 load-only proof, which injects no controller.

### Live upgrades do not overwrite mapped DLLs

Windows keeps the DLL image section mapped after `STOP`; loading the same path again returns the
existing module and does not run a fresh `DllMain`, while replacing its backing file is not a
portable upgrade mechanism. The controller therefore treats a generation as immutable:

- each generation has a unique DLL basename and directory, such as
  `don-retail-control-v2/retail_control-v2.dll`;
- the DLL derives `request.txt`, `events.ndjson`, `ready.txt`, and `STOP` from its own module
  directory, so parked generations cannot consume a newer generation's control files;
- the x86 Toolhelp probe detects an already-mapped basename before download and refuses a
  same-generation deployment;
- `upgrade --from-generation OLD --generation NEW` parks `OLD`, performs its one token-bound
  detach, confirms the old module is absent, then loads `NEW`. A detach ambiguity stops before
  deployment and requires a fresh retail process; a failed new load leaves retail unpatched.

The earlier park-only upgrade path was exercised in place on PID `12324`, without restarting the
match; this is not live evidence for the new detach transition. Parked v1 remained
mapped at `0x6AFC0000`; generation v2 loaded from its unique path at `0x6AF60000`, armed the same
validated runtime call site `0x00EF1686`, returned a main-thread observation at frame `157`, and
parked again. A later same-generation deployment of trajectory-v3 was refused from the x86 module
map at `0x6AF00000` before injection. Both the old and new STOP markers remained independent.

### Bounded retail trajectory recorder

`trajectory WHO ID X Y` is intentionally a compound supervised operation. It requires retail to
start paused, resolves the selected unit through retail's owner/object tables, asks the shipped
`issue_move_to` for the exact plain move, and only appends `pause 0` if retail actually serialized
the move. It then records one sample whenever `Game::frame +0x550` changes. On order retirement or
the caller's frame cap it appends retail's `pause 1`, observes the pause bit, writes JSON, restores
the pause once more from the host fail-safe, and writes `STOP` to restore the hook bytes.

The detour wraps `TurnControl::do_frame`, which is earlier than `Game::do_frame` in `Game::loop`.
Consequently, a changed frame observed on the next loop is a coherent between-simulation-ticks
state; the recorder does not mislabel the post-TurnControl callback itself as post-simulation.
Repeated callbacks at an unchanged frame are discarded. Retail game time advances at exactly 15
simulation frames per `Game::seconds`; `TurnControl` speed changes wall-clock pacing only.

The frame schema uses only PDB/decompilation-backed fields:

- object `x/y` are decoded from `UnitData +0x10/+0x14` with XOR `0x00063637`;
- current heading is the modulo-u32 binary angle at `UnitData +0x50`; `dest_angle +0x58` is the
  queued action's final heading, not current facing;
- the canonical current order is `orderlist.head->prev->data`, not the stale `Unit +0xCC` cache;
- a `MoveOrder` current pointer is its interior `UnitOrder` virtual base, so the recorder subtracts
  the measured `0x54` before reading the PDB fields at complete-object offsets `+0x04..+0x4E`;
- order vtables are normalized back to preferred VAs, making artifacts ASLR-independent. The
  `facing +0x28` member is preserved as a formation reversal/sentinel field, not called an angle.

The live artifact [`schema/live/retail-move-trajectory-v1.json`](../../schema/live/retail-move-trajectory-v1.json)
records owner-0 unit 0 moving exactly one tile, `(2904,31896)` to `(3096,31896)`, in six frames:
`2938, 2972, 3006, 3040, 3074, 3096` on X. Frames 158–162 carry the normalized `MoveOrder` vtable
`0x00B4A12C`; frame 163 has canonical order length zero and exact target position. The terminal
record proves pause `1` at the same frame, and trajectory-v3 then reported `state=parked`. Its exact
retail command bytes are preserved in the artifact. The checksum field is explicitly unavailable:
this was a solo run (`network=0`), and retail intentionally emits no checksum packet in that mode.

### Multi-body unit observation

`observe-guys WHO ID` is a read-only main-thread snapshot of a unit's physical bodies. It reads
the inline PDB `PtrArray<Guy>` at `UnitData +0xE4`: length `+0xE8`, capacity `+0xEC`, and list
`+0xF4`. It refuses malformed length/capacity invariants, follows at most 32 entries, and marks a
larger array truncated. Each tuple preserves `GuyData` type `+0x08`, raw coordinates
`+0x0C/+0x10/+0x14`, angle `+0x18`, desired position/angle `+0x5C/+0x60/+0x64`, last position
`+0x68/+0x6C`, signed offsets `+0x92/+0x94`, and `guy_num +0xA2`. Guy coordinates are raw;
unlike the owning `UnitData` anchor, they are not XOR-encoded.

Generation guys-v4 exercised this on the still-paused PID `12324`, frame `163`. Owner-0 unit 0
was type 69 at decoded anchor `(3096,31896)`, heading `0x40000000`, with a two-entry array:

| body | live pointer | current `(x,y,z)` | desired `(x,y)` | previous `(x,y)` | offset | number |
|---:|---:|---:|---:|---:|---:|---:|
| 0 | `0x0F2E1C84` | `(3096,31896,558)` | `(3096,31896)` | `(3074,31896)` | `(0,0)` | 0 |
| 1 | `0x0F2E0084` | `(3048,31800,556)` | `(3048,31800)` | `(3026,31800)` | `(0,0)` | 1 |

Both bodies had type 69 and current/desired angle `0x40000000`. This is direct retail evidence
that a settled multi-body unit materializes formation displacement in each Guy's coordinates
(body 1 is anchor `(-48,-96)`), rather than in the two `off_*` members for this state. No ctor,
`set_type`, spawn path, or simulation write was invoked. guys-v4 was then STOP/parked.

guys-v5 added the owning unit's `unit_masks +0x68`, `form +0xAA`, PDB `guy_mark +0xB5`,
and `UnitTypeData::guy_spacing +0x224`. Two further type-62 units both had masks `0x80000`,
form 0, guy spacing 144, and identical three-body relative positions `(0,0),(-192,-56),(0,0)`
despite different world anchors. The type-69 unit had masks 8, form 0, the same spacing 144, but
the distinct two-body lattice `(0,0),(-48,-96)`. Therefore the scalar type spacing is not a
complete formation rule. Also, `guy_mark` was 1 on both three-body arrays while a live body had
`guy_num=2`: `+0xB5` must not be renamed or used as a live-body count. The authoritative count is
the `PtrArray` length at `+0xE8`. v5 was parked without a simulation write.

### Supervised public-state player protocol

Generation `player-v7` implements `don.retail-player.v1`. `observe-player` runs entirely in the
retail main-thread callback and publishes only the human slot's own public state. It identifies
that slot by requiring exactly one `Leader` with `(flags & 7) == 7`, then cross-checks
`Leader.who/tribe` against the inline `GameInfo::Player`. There is deliberately no slot-zero
fallback. The observer brackets the sample with the same `Game` pointer/frame, `Objects` root and
array metadata, Leader flags, and encrypted-economy pointer; a match load or torn root fails the
whole observation closed.

Owned objects are followed only through retail's exact bands: units `[0, unit_mark)`, buildings
`[2000, build_mark)`, and walls `[3000, wall_mark)`. Each emitted object must be active and have
matching embedded `who` and `o` fields. The host artifact strips heap pointers and uses stable
identity `{slot, band, o, uid}`. Object coordinates decode all three stored axes with XOR
`0x00063637`; type and runtime class are independently validated. Unit orders use the canonical
`OrderList head->prev->data` link and a fixed shipped-vtable-to-`OrderIndex` table. An unknown live
type, runtime class, or current unit order rejects the observation instead of inventing a label.

Economy fields preserve their retail meanings:

- whole stockpiles are `LeaderDataEncrypt.bucket[6]`, XOR `0x8221`, in resource order food,
  timber, wealth, knowledge, metal, oil;
- `resource_cap[0..6)`, XOR `0x1281`, is exposed as `commerce_cap_x16_i32`, an income clamp in
  sixteenths—not maximum stored stock;
- `over_cap[6]`, XOR `0x8932`, is preserved as the retail cap state;
- current population is `LeaderData.control +0x940`; `LeaderData.pop +0x95c` is an AI counter and
  is intentionally excluded.

Enemy/neutral object lists, enemy economy/orders/targets, target-object dereferences, hidden map
tables, goods/rares/items, and visibility bits whose local-slot LOS semantics are not yet proven
are never read into this protocol. The complete contract is
[`schema/live/retail-player-protocol-v1.json`](../../schema/live/retail-player-protocol-v1.json).

The first policy is intentionally narrow but uses retail semantics, not a simplified simulation:
it deterministically selects the lowest object-index live idle owned `Scout`, proposes one
world-tile step, and leaves citizens, merchants, and buildings untouched. Dry-run is the default.
The v1 executor accepts at most four actions and currently executes only one-owned-unit `move`
actions within observed world bounds; every command uses the shipped main-thread ingress and a
bounded trajectory recorder.

```sh
python3 tools/retail-control/retailctl.py player-observe --generation player-v7
python3 tools/retail-control/retailctl.py rearm --generation player-v7
python3 tools/retail-control/retailctl.py policy --generation player-v7 \
  --output schema/live/retail-player-policy-dry-run-v1.json
python3 tools/retail-control/retailctl.py rearm --generation player-v7
python3 tools/retail-control/retailctl.py policy --apply --generation player-v7
```

The corrected live run began paused at frame 169. It observed 8 units and 7 buildings, population
`8/25`, stockpile `[214,210,103,0,0,0]`, and five exact `GatherOrder` citizen fronts. The policy
moved only owner-0 object 0 from `(3288,31896)` to `(3480,31896)`. Retail serialized the command,
the normalized `MoveOrder` advanced through X positions `3322,3356,3390,3424,3458,3480` at frames
170–175, the order retired, and the fail-safe restored pause `1`. A coherent after-observation
confirmed the scout at the requested destination while citizen orders remained `GatherOrder`.
The hook then reported `state=parked`; the live/dry observations and trace are preserved under
`schema/live/retail-player-*.json`.

### Fog-safe economy ingress (v2)

`don.retail-player.v2` adds only own-state fields needed for legal production decisions: the
encrypted age and four epoch counters, the 101-byte owned-TypeIndex bitset, exact nonzero
`Leader::num_queued` counts, and each owned building's logical `BuildQueue` with TypeIndex and
elapsed value. A `GatherOrder` target is emitted only when its `{owner,o,uid}` resolves back to a
matching active object in the same human owner's public table. Enemy and neutral lists are never
consulted.

All commands run in the existing pre-`TurnControl::do_frame` main-thread ingress and call shipped
outer APIs. `GroupOut::issue_gather` emits packed opcode `0x13`; `GroupOut::issue_queue_up` emits
`0x18`; `GroupOut::issue_build` emits `0x19`. Production is queried first with
`BuildData::can_queue(TypeIndex)`. Explicit construction can query
`GroupData::validate_build(x1,y1,x2,y2,type,queue)`. Through generation `economy-v10`, the four
Coord arguments remained opaque retail placement-gesture endpoints, so the autonomous policy did
not collapse them to a guessed single point. `economy-v11` resolves this below.

Generation `economy-v9` exercised three positive zero-frame transactions at frame 175, all while
pause remained `[1,1]`:

- a Citizen at City 2000 changed both `Leader::num_queued[50]` and the City queue from 1 to 2;
- City State (TypeIndex 565) changed the Library 2005 queue from 0 to 1 after retail
  `can_queue` returned 1;
- Citizen 3 received opcode `0x13` and its canonical order still resolved to own Woodcutter Camp
  2001/uid 1. Classical Age (544) returned 0 from the same Library legality query and was not
  issued.

Generation `economy-v10` adds `run-frames`, a supervised 1–30 frame boundary. It issues retail's
explicit unpause only from a verified paused state and automatically issues pause at the exact
requested `Game.frame` delta. The live proof advanced 175→177 and later in 30-frame blocks, with
pause restored after every block. A Barracks attempt serialized the correct `0x19` payload, but
no building materialized by frame 357 and the worker returned to its prior GatherOrder. That is
recorded as a negative attempt, not a positive proof; guessed degenerate placement is disabled in
the opening policy. See `schema/live/retail-player-protocol-v2.json` and the
`schema/live/retail-economy-*-v1.json` fixtures.

Generation `economy-v11` resolves the four-coordinate ambiguity from retail itself. PDB-backed
`Options::picked_spot` is called by the UI as `(x,y,-1,-1)` for an ordinary click; its drag path
alone supplies the second coordinate pair. `find-build` runs on the retail main thread and examines
at most 289 candidates (radius eight) on the exact 48-Coord `UCoord` lattice, nearest ring first.
It returns a site only when shipped `GroupData::validate_build(x,y,-1,-1,type,QUEUE_NEW)` returns
nonzero. The host additionally requires an observed own Citizen, an enabled local TypeIndex,
public prerequisites/age/base-cost minimum, and observed world bounds. Base cost is intentionally
not presented as the final price: retail still owns count ramping and civilization modifiers.

The live validation-only query at paused frame 357 found Farm 417 at
`(2832,32448,-1,-1)` for Citizen 8 on its first candidate. The apply proof re-ran the shipped
validator, serialized opcode `0x19` as
`000100080019100b0000c07e0000ffffffffffffffffa101000002000000`, and crossed one supervised
30-frame boundary. At frame 387, own building mark advanced 2007→2008 and new own Farm
`{o:2007,uid:16}` existed at its retail-snapped object anchor `(2688,32448)`; the click coordinate is
therefore not relabelled as the final object anchor. Pause was `[1,1]`. The generation was then STOP-parked with the original
call bytes restored. The validation and materialization records are
`schema/live/retail-build-placement-proof-v1.json` and
`schema/live/retail-economy-build-proof-v1.json`.

### Arena Marshal retail adapter

`marshal-policy` follows `Marshal::act` in source order—sense, economy, scout, military,
army control, employ—and selects at most the first command the v2 observation can faithfully
support. Queue commands preserve Arena's producer/type/count semantics and its ten-head RL form;
`QUEUE_UP` is heads `[23,0,0,0,TypeIndex,0,0,0,0,1]`. The adapter reads `CITY_GATHER`,
`PEASANT_RATE`, and `TECH_COST_FACTOR` from shipped `rules.xml`, combines those with retail's live
own commerce-cap values, and uses the same CapFirst citizen-target integer equation. Final legality
still comes from retail `BuildData::can_queue`.

This is explicitly a supported subsequence, not a synthetic Arena world. Enemy sensing/attacks are
omitted because v2 exposes no fog-approved sightings; scout waypoints are omitted because it has no
explored-map plane; employment is omitted because exact gather capacity/occupancy is not yet in the
snapshot. Generic BUILD_AT ingress now has a positive retail oracle, but Marshal's current branch
requests a terrain/gather-capacity-derived Camp before a Farm. The adapter therefore does not
substitute the newly proven Farm action. Its trace records each omission.

The live dry run at frame 357 observed City State already queued, so Arena's `next_tech` selected it
and `queue_at` suppressed the duplicate without falling through—matching the Rust source. Marshal's
later CapFirst step counted six live Citizens plus one queued, derived a target of 21, and retail
accepted one Citizen at City 2000. The apply run issued exactly heads
`[23,0,0,0,50,0,0,0,0,1]`; packet opcode `0x18` changed both the aggregate Citizen count and City
queue from 1 to 2 at unchanged frame 357, with pause `[1,1]`. `economy-v10` was then STOP-parked.

### Exact Camp-first Marshal placement (v3)

`don.retail-player.v3` closes the Camp/Farm decision without publishing a terrain oracle. Each own
building now carries its signed retail `BuildData::gather_max` byte (`+0x80`) and completion bit.
Marshal seats are the sum of positive capacities for all live own gather buildings, including
unfinished ones. Useful seats use the observed commerce cap and only complete cities:

```text
max(0, trunc0((cap_x16 - complete_cities * CITY_GATHER * 16)
              / (PEASANT_RATE * 16)))
```

At frame 687 this yielded useful food/timber `[9,9]`, existing seats `[4,4]`, and positive gaps
`[5,5]`. Marshal's placement wants were therefore Camp, City, Farm in that exact order. The Camp
was not replaced by a Farm when its bank was initially short; the supervised match advanced in
30-frame paused slices until retail accepted the count-adjusted price.

Prospective Camp capacity remains private until its entire read footprint is currently visible.
For every snapped candidate, the controller reproduces `calc_gather`'s W-cell enumeration using
retail's `circle_x`, `circle_y`, and `circle_radius` tables. A W cell is admitted only when its
centre passes shipped `vector_dist <= WOODCUTTER_RADIUS`. All four overlapping F cells must then
pass `WorldData::is_really_seen` for the local player. This gate runs before
`GroupData::validate_build`, because its `blocked_site` path can itself call `calc_gather`; it also
runs before `BuildTypeData::max_gatherers(-1,who,corner)`. Raw forest, terrain, ownership,
reservation, and blocker fields never leave retail.

The adapter enumerated capital-centred tile rings 2 through 23 at 192 Coord per tile. No
capacity-five site existed. It therefore preserved Arena's capacity-first score and selected the
first capacity-four candidate on ring 12, click/snap `(4800,32064)`, retaining four as the full
retail result rather than treating five as a retail maximum. Immediately before apply it replayed
that ring and required the same click, snap, legality, and capacity.

Retail serialized BUILD_AT as
`000100030019c0120000407d0000ffffffffffffffffa201000002000000`. One exact 30-frame boundary
materialized own Camp `{o:2008,uid:19}` with signed capacity four, and pause remained `[1,1]` from
frame 687 to 717. Citizen 3 was distant, so retail correctly left a front `MoveOrder` and queued the
`BuildOrder` behind it. Generation `economy-v15` walks the bounded circular order list and proved
that pending BuildOrder resolves to the new Camp's exact own `{o,uid}`. It issued no second action,
then STOP-parked with the original call bytes restored. The protocol, dry plan, command proof, and
post-state are `schema/live/retail-player-protocol-v3.json`,
`retail-arena-marshal-camp-dry-run-v1.json`,
`retail-arena-marshal-camp-action-proof-v1.json`, and
`retail-player-observation-v3-post-camp.json`.

Erratum (2026-08-09): the economy-v15 capture remains positive evidence for retail's BUILD_AT
packet, materialized Camp, and exact queued BuildOrder identity. It does **not** prove that its
pre-validation fog footprint used the exact coordinate conversion. That generation indexed from
the `div_3_table` pointer variable instead of dereferencing it. Current source corrects the lookup
and has a regression test; the v3 protocol record carries the same scoped erratum.

### Finite supervised Marshal loop

Generation `marshal-loop-v16` turns the single-decision adapter into a finite transaction loop,
not a background bot. A run is limited to eight decisions and 30 simulation frames per decision.
Each iteration takes a coherent `don.retail-player.v3` observation, preserves Marshal source order,
selects at most one action, re-runs the shipped legality query at the same paused frame, applies
only the already-proven queue or build ingress, advances exactly the requested horizon, and takes
a second coherent observation. Missing or unsupported actions are literal no-ops; the controller
does not fall through to a lower-priority substitute.

The loop pins the supported executable hash, local owner/who/tribe/team, and world dimensions for
its entire lifetime. Immediately before a command it also compares the exact frame, pause bit,
economy, population, technology, queue summary, object metadata, and every complete public own
object record against the plan observation. A changed identity, recycled object, order/position
change, failed retail predicate, unexpected packet opcode, frame delta, or pause boundary aborts
the artifact. Build materialization is capped at that decision's single frame horizon; the generic
one-action proof's longer settlement cap is not inherited by the loop. The `finally` path
explicitly requests pause and runs `STOP`, restoring the original five call-site bytes even after
failure. The complete contract is recorded in
`schema/live/retail-arena-marshal-supervised-loop-protocol-v1.json`.

The live apply run stayed in the existing solo match and covered eight 30-frame decisions,
frame 807→1047. At frame 807, retail's `can_queue` accepted one Citizen at City 2000. Retail
serialized `000100d007183200000001000000` (packed opcode `0x18`) without advancing a frame; the
City and aggregate queue both gained exactly one Citizen. The decision then advanced 807→837.

At frame 837, Marshal's food-locked placement branch selected a Farm. The adapter chose Citizen 9
by the fog-safe own observation, re-ran the current-fog-gated site search, and got the same legal
capacity-one site `(3456,29184)` on ring five. Retail serialized
`000100090019800d000000720000ffffffffffffffffa101000002000000` (packed opcode `0x19`). One
30-frame boundary materialized own Farm `{o:2009,uid:20}` at that exact snapped anchor with signed
capacity one; Citizen 9 had a front `MoveOrder` and a queued `BuildOrder` resolving to the new
Farm's exact own identity.

The remaining six decisions issued no action. They still advanced through exact paused boundaries,
including completion of the queued Citizen (population 11→12), and did not substitute the
unsupported City, scout, enemy, or employment branches. Every decision recorded identity stable,
pause `[1,1]`, frame delta 30, and `unsupported_substitution=false`. The full loop trace and its two
command proofs are `schema/live/retail-arena-marshal-supervised-loop-v1.json` and sibling
`step-00`/`step-01-action-proof.json` files. The terminal ready record is `state=parked` for PID
12324 at frame 1047.

### Current-visible tactical Marshal loop (v4)

`don.retail-player.v4` extends v3 with a deliberately narrow enemy surface: exact objects that are
enemies of the unique local human and are visible *now*. The callback uses shipped
`LeaderData::is_enemy`, then dispatches only to the measured concrete `UnitData`, `AnimalData`,
`BuildData`, or `WallData` `is_seen` leaf. An unknown class fails the whole observation closed;
there is no layout-based cast. The public record contains stable `{slot,band,o,uid}` identity and
ordinary visible type/class/position/hits fields. It still excludes enemy economy, queues, orders,
remembered-but-hidden objects, and every raw fog or terrain plane. The complete additive contract
is [`schema/live/retail-player-protocol-v4.json`](../../schema/live/retail-player-protocol-v4.json).

The v4 scout adapter preserves Marshal's persistent citizen, ring leg, and waypoint. It tests at
most three one-tile frontier candidates toward that waypoint: diagonal, X-only, then Y-only. For
each candidate, `WorldData::is_really_seen` for the local F cell runs before the shipped
`WorldData::is_passable` W-cell query. Only the accepted destination leaves the process. The
coordinate conversion dereferences the shipped `int *div_3_table` at `0x00CAE5FC` before indexing;
the same correction was applied to the older gather-footprint gate and is protected by a source
regression test. Apply replays the entire query against an identical paused public-state token.

The tactical supervisor also carries Marshal's `Massing`/`Pushing` state between decisions. It
enters `Pushing` only after a currently observed enemy base and observed own live military value
at least 420, returns to `Massing` after a greater-than-60-percent observed loss, and attacks only a
target still present in the current-visible list. `validate-attack` replays enemy relation, active
object index, uid, concrete class, and current visibility immediately before `attack-visible`.
The executor then requires retail's exact applied `AttackOrder` target owner/index/uid. With no
visible enemy or military unit in the current match, that positive attack lifecycle remains
validation- and unit-tested rather than claimed as a live attack.

Generation `tactical-v19` was exercised in the existing paused solo match. The dry transaction at
frame 1047 selected Citizen `{slot:0,o:4,uid:11}`. Its diagonal and east candidates entered a
shipped-impassable W cell; the third, currently visible cardinal candidate `(1368,32520)` was
accepted. The bounded apply replayed the same result, and retail serialized MOVE_TO as
`00010004000758050000087f000000000000000000000102ffff00`. At the unchanged paused frame, the
front order was the exact rebased `MoveOrder` vtable `0x00B4A12C` with that destination. One exact
15-frame boundary advanced 1047→1062, preserved actor uid 11, and ended paused. The controller then
reported `state=parked`; an external read again found the original call bytes
`E8 45 67 3C 00`. The full transaction and its zero-frame command proof are
[`schema/live/retail-arena-marshal-tactical-v19-live-proof.json`](../../schema/live/retail-arena-marshal-tactical-v19-live-proof.json)
and its sibling `retail-arena-marshal-tactical-v19-live-proof-step-00-action-proof.json`.

A later validation-only corrected gather query completed coherently at paused frame 1062
(`tested=40`, `currently-visible=32`, no legal site, `note=0`). The immediately following STOP did
not publish `parked`; PID 12324 then exited with Windows Error Reporting event 1000, BEX execute
access violation `0xC0000005` at null. No dump survived, so this record does not pretend to prove
whether the query or the old worker-thread unhook was uniquely causal. The temporal association was
enough to retire that normal unhook path.

Current source asks the active retail main-thread callback to restore the future call site and
acknowledge it before the worker reports `parked`. Its dormant fallback refuses unless every owned
thread suspends and every context read succeeds, and the redundant post-protection
`WriteProcessMemory` is gone. Generation `tactical-v21` validated that fallback twice against a
fresh dormant pinned process, including rearm and the expected no-loop timeout; both external reads
found `E8 45 67 3C 00`, and no new Application Error was recorded. That deliberately does not stand
in for an active-match exercise of the new acknowledgement path. The incident and remediation are
captured in
[`schema/live/retail-control-stop-incident-v1.json`](../../schema/live/retail-control-stop-incident-v1.json).

Generation `relaunch-v22` supplied that missing active-match exercise on 2026-08-09 in a freshly
paused solo skirmish, PID `13876`, supported executable SHA-256 and runtime base `0x00D60000`. The
immutable controller DLL had SHA-256
`e2829ae2ae93e24e95b87d2d9e469fc79e915b67e30f53d9638b84fbe6c92527`, loaded once at
`0x6AEA0000` with image size `0x165000`. Five consecutive rearm cycles and a final park each
acknowledged on the main-thread boundary with `dropped_events=0`; after every STOP, the host's
independent external read reproduced `E8 45 67 3C 00`. No new file appeared in the scoped WER dump
directory.

The run also exercised the refusal boundary rather than bypassing it. The first preflight rejected
the injector's enriched `peek` header because the host still expected the older three-field form.
After that parser was made exact, deployment armed the controller but host identity comparison
rejected the ready root: POSIX tokenization had removed one leading slash from the canonical
`\\?\C:\Users\...` Toolhelp path. The host immediately requested STOP, and the controller parked with
the original bytes restored. The host now parses the injector's quoted machine records without
POSIX backslash semantics and compares normalized Windows paths; only then was the same immutable
generation rearmed. Both failure modes have negative regressions.

Passive post-frame evidence in that paused match reported `network=0`, `network_is_solo=1`,
`playback=0`, `immediate_process=0`, a valid local play slot, package size zero, checksum room, and
zero peer totals, with `mutates_outgoing_package=0` and `checksum_gate=network_clear`. The bounded
[`don.retail-player.v4` observation](../../schema/live/retail-player-pid13876.json) contained 15
owned objects with no unknown class, unknown type, or truncation: eight units and seven buildings
at frame zero, including live `GatherOrder` identities for the starting Citizens. The observation
command then parked the controller itself.

Finally, while the process remained parked, an exact external read of all 24 live Tribe records
completed the local Rules input channel without retaining raw memory on the host. The normalized
Tribe image SHA-256 was
`4a271dcca8a7c1223e61b9f58b4e5809f45f0fcfd43ce1b5f79a8c55dfde14bb`; the independent walk
reproduced Types `0x72e0c3b6`, Constants `0x50625668`, Balance `0x56daabc1`, final Rules
`0x12ba3104`, and exactly 997,846 walked bytes. This closes the active solo lifecycle and local
Rules-capture gates, not the multiplayer turn/replay or real-host client gates.

On 2026-08-08, PID `5236` was inspected read-only before this probe was built:

- module base `0x00D60000`, ASLR delta `0x00960000`;
- `GameAccess::game` resolved to `0x01797EC0`;
- five observations of `Game::frame` at 500 ms intervals all read `0`;
- the process had no main window and no older `donhook`/`donhook2` module loaded.

The built DLL was then exercised against that exact process. Host preflight reproduced the
expected executable SHA-256; `donject` verified identical local/remote `kernel32` bases,
`LoadLibraryA` returned module base `0x6AFB0000`, and the DLL reported `state=armed` with
runtime call site `0x00EF1686`. An `observe` request correctly produced no response because
the dormant process never reached `Game::loop`. `STOP` then reported `state=parked`, and a
fresh external read proved the retail call bytes were restored exactly:

```text
00EF1686: E8 45 67 3C 00 85 C0 74 6D 8B ...
```

This proved image gating, deployment, attachment, hook installation, the no-loop failure
mode, and reversible removal before any gameplay write was attempted.

The requested fresh solo skirmish was then exercised as PID `12324`, at the same runtime
base. This time `observe` ran on the retail main thread and reported frame `0`, paused `1`,
speed `2`, and network `0`. The reversible command sequence produced:

| operation | exact retail packet evidence | applied-state evidence |
|---|---|---|
| unpause | `4c00` appended at package `0..2` | pause bit `1 -> 0` |
| move owner-0 unit index 0 from `(2712,31896)` to `(2904,31896)` | `000100000007580b0000987c000000000000000000000102ffff00` (group prefix + opcode `07`) | order length `0 -> 1`, runtime vtable `0x014AA12C` = rebased `MoveOrder`; an independent post-run read found exact position `(2904,31896)` and retired order length `0` |
| re-pause | `4c01` appended at package `10..12` | pause bit `0 -> 1`; final frame `157`, seconds `10` |

The checksum request in that same state recorded `network=0`, no bytes appended, and
`phase=rejected`: this is retail's expected solo gate, not a missing packet silently called
success. No 16-channel checksum vector has been claimed from this solo exercise. Finally,
`STOP` parked the DLL and a fresh external read again reproduced the original call bytes
`E8 45 67 3C 00`. The retail-control bridge is therefore gameplay-validated for the pause
and move lifecycle; multiplayer checksum capture remains a separate, intentionally
unexercised gate.
