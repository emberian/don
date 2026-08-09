# Bidirectional live retail control

Status: **live-validated for pause, unit movement, exact own-state observation, and a
bounded supervised scout policy against a retail solo skirmish.**
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
| `checksum` | `issue_check_sums` `0x00940770` | exact 65-byte opcode `0x39` packet when retail's multiplayer gates allow it |
| `halt WHO IDS...` | `issue_halt` `0x009418D0` | retail-generated group+halt bytes; selected unit order list becomes empty |
| `move WHO X Y QUEUED ORDER FORM WIDTH DISEMBARK IDS...` | `issue_move_to` `0x00941720` | retail-generated group+move bytes; current order pointer/vtable transition |
| `attack WHO TARGET_WHO TARGET_ID FLAGS QUEUED IDS...` | `issue_attack` `0x009415E0` | retail-generated group+attack bytes; current order pointer/vtable transition |

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
python3 tools/retail-control/retailctl.py deploy --pid 5236
python3 tools/retail-control/retailctl.py send observe
python3 tools/retail-control/retailctl.py send pause 1
python3 tools/retail-control/retailctl.py send pause 0
python3 tools/retail-control/retailctl.py send move 0 16000 12000 2 1 -1 -1 0 12 13
python3 tools/retail-control/retailctl.py stop
python3 tools/retail-control/retailctl.py rearm
```

Every response is NDJSON with `queued`, then (where state evidence exists) `applied` or
`timeout`. `command_hex` is copied from the exact range appended to retail's live
`CommandPackage`; it includes multiplayer padding when retail inserts it. Unit order
evidence reads the PDB-defined `UnitData::orderlist` at `+0xc8`, specifically its current
order pointer (`Unit+0xcc`) and length (`Unit+0xd8`). Vtable values can be resolved through
`schema/vtables.json` (for example `MoveOrder` `0x00B4A12C`, rebased at runtime).

## Reversibility and current exercise

`STOP` first prevents callbacks, then suspends other threads long enough to restore the
five original call bytes and flushes the emulator instruction cache through both
`FlushInstructionCache` and `WriteProcessMemory`. The DLL deliberately remains loaded and
parked; this avoids unloading code while another thread could still have a return address
inside it. Deleting `STOP` rechecks the prologue and reinstalls the detour.

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
- `upgrade --from-generation OLD --generation NEW` parks `OLD` first, leaving retail's exact
  five bytes restored, then loads `NEW`. A failed new load leaves the old generation parked and
  retail unpatched.

This path was exercised in place on PID `12324`, without restarting the match. Parked v1 remained
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
`GroupData::validate_build(x1,y1,x2,y2,type,queue)`, but the four Coord arguments remain opaque
retail placement-gesture endpoints: the autonomous policy does not collapse them to a guessed
single point.

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
