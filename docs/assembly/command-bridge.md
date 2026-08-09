# The command→order bridge

Lane `command-bridge`, wave "assembly, not new derivation". 2026-08-08.

## What now runs that did not before

**A decoded command packet now makes a unit hold an order.** `don-net` could frame a
`.rcx` stream and `don-env` could emit an action; neither could turn either into a
`UnitOrder`. `crates/don-sim/src/command.rs` closes that gap:

```rust
let cmds   = don_net::decode_commands(&payload, &mut Obfuscation::none())?;
let mut b  = don_sim::command::Bridge::new();
let mut p  = don_sim::command::Package::new(play, stamp);
for c in &cmds { b.process_one(&mut p, c.bytes, &mut fleet); }
// fleet's units now hold MoveTo / Attack / Guard / … orders in their OrderList
```

That exact call is a passing test:
`crates/don-replay/tests/command_bridge_agreement.rs::a_don_net_decoded_stream_makes_a_unit_hold_an_order`.

Concretely, what executes now:

* **`CommandPackage::process_all` `0x0094C500`** — walks a payload by the length each
  handler returns and dispatches all 82 opcodes.
* **`CommandPackage::process_group` `0x0094A0C0`** (opcode 0) — the *selection*. It
  interns a member list into the `groups` pool via `Groups::push_group` `0x0070F9E0`,
  writes the slot into `CommandPackage::group`, and back-links every member's
  `ObjectData::group`. Without this every other command is a no-op, which is exactly why
  nothing downstream could act before.
* **22 of the 35 wire-reachable `Group::action_*`** — 15 order installers, three
  complete transactional actions (`begin`, `halt`, and `disband`), one reproduced state
  action, and three capability-gated state paths.
* **Twenty-three inline state handlers** — control-group save/camera, MP-log toggle, speed
  set/up/down, all eight player-speed accumulators, two lockstep report stores, chat-route
  status, reveal-map/turn telemetry, three bounded cheat-state handlers, three AI controls,
  and three measured simulation no-ops are complete; pause's common state path is wired
  but remains partial.
* **`Unit::add_*_order`'s `QueuePos` handling**, including the `QUEUE_FIRST` stash /
  `action_halt` / re-issue-as-`QUEUE_NEW` / `finish_insert` replay dance.

15 distinct `OrderIndex` kinds can now be installed from the wire: `MOVE_TO`, `ATTACK_TO`,
`EXPLORE_TO`, `FLEE_TO`, `ATTACK`, `ATTACK_GROUND`, `GROUP_PATROL`, `AIR_PATROL`,
`FOLLOW`, `GUARD`, `GARRISON`, `REPAIR`, `GATHER`, `BOARD_SHIP`, `TRADE_ROUTE`.

**Tier C.** Nothing here has been executed against retail. Every number below is
`[measured]` on this Mac by capstone disassembly of `ron-bin/riseofnations.exe`
(sha256 `30478a44…625079`), named from `ron-bin/sbl/rise.pdb`; shape taken from
`re/decomp-all/*.c` is marked `[structure]`.

## Files

| path | what | lines |
|---|---|---:|
| `crates/don-sim/src/command.rs` | the bridge: opcode dispatch, inline state, `Groups` pool, `Group::action_*`, `Fleet`, 29 tests | 4,201 |
| `crates/don-sim/src/command_tables.rs` | generated: 42 `ActionDef` + 23 `InlineDef` + 82 `OpDef` | 173 |
| `crates/don-sim/tests/command_simple_state.rs` | byte/state mutation pins for opcodes 1/14/32/33, 1 test | 105 |
| `crates/don-sim/tests/command_speed_state.rs` | byte/state mutation pins for opcodes 34/52–66/69/72/74/76/79/81, 8 tests | 520 |
| `crates/don-sim/tests/command_cheat_init_unit.rs` | transactional world-receipt pins for opcode 67, 2 tests | 270 |
| `crates/don-sim/tests/command_group_lifecycle.rs` | atomic HALT/DISBAND receipt, mask, ordering, and rollback pins, 2 tests | 184 |
| `crates/don-replay/tests/command_bridge_agreement.rs` | don-net ↔ don-replay ↔ don-sim, 6 tests | 226 |
| `crates/don-env/tests/command_bridge_agreement.rs` | don-env ↔ don-sim, 9 tests | 548 |

**Shared-file edit, one line**: `crates/don-sim/src/lib.rs` gained `pub mod command;` with
a three-line doc comment, inserted after `pub mod checksum;`. Nothing else in that file was
touched.

57 bridge tests, all green. `cargo test -p don-sim --lib command::` 29/29,
`cargo test -p don-sim --test command_simple_state` 1/1,
`cargo test -p don-sim --test command_speed_state` 8/8,
`cargo test -p don-sim --test command_cheat_init_unit` 2/2,
`cargo test -p don-sim --test command_group_lifecycle` 2/2,
`cargo test -p don-replay --test command_bridge_agreement` 6/6,
`cargo test -p don-env --test command_bridge_agreement` 9/9.

## How many `Group::action_*` exist, and how many are ported

**42**, not "~45". `COVERAGE.md` §6 item 3 says "`Group::action_move_near` and the ~44
other `Group::action_*`"; the PDB has 42 procedures matching `Group::action_`, so
`move_near` plus **41** others. (`SelectGroup::action_begin` `0x00716940` is a separate
class's override of the same virtual and is not counted.)

They carry **209 direct `call`/`jmp` sites** across all named procedures in `.text`.

| status | count | meaning |
|---|---:|---|
| `Port::Complete` | 3 | complete transactional mutation (`begin`, `halt`, `disband`) |
| `Port::Orders` | 15 | installs orders per member, with `QueuePos`, from a command |
| `Port::State` | 1 | reproduced, and retail installs no order either (`stance`) |
| `Port::StateWired` | 3 | exact wire/state path; host capability/state columns remain explicit |
| `Port::Todo` | 13 | dispatched and counted, body not ported |
| `Port::NotOnTheWire` | 7 | no `CommandPackage` handler reaches them |

So **22 of the 35 wire-reachable actions execute a recovered mutation** and 13 remain
bodyless. Only `Complete` is whole-simulation closure-green; `Orders`, `State`, and
`StateWired` remain honest intermediate tiers until every retail tail and required product
host is present. `BridgeStats` counts the runtime split (`acted` vs `unported`).

### The full table, in descending call-site order

`installs` is the `OrderIndex` set the action allocates *directly*, measured from every
`push imm; call OrdersMemManager::get_obj 0x00730AC0` site reachable through the
`Unit::add_*_order` it calls. `→` marks an action it delegates to.

| action | VA | bytes | sites | what it does to the target group | port |
|---|---|---:|---:|---|---|
| `move_to` | `0x0070FBA0` | 49 | 34 | forwards to `move_near` with `tolerance = 0` — that is its entire body | Orders |
| `move_near` | `0x00704990` | 9205 | 23 | each member gets `MOVE_TO`/`ATTACK_TO`/`EXPLORE_TO`/`FLEE_TO` per the `orders` byte, at the commanded coords + tolerance; `GROUP_MOVE`/`GROUP_ATTACK_TO` when marching, `GARRISON` on the transport branch → `halt` | Orders (spine) |
| `attack` | `0x00712490` | 3833 | 16 | `ATTACK` on each member; retail also splits out `CAST_SPELL` casters and sends out-of-reach members via `move_to` → `halt`, `move_to` | Orders (no split) |
| `halt` | `0x0070D0C0` | 685 | 14 | atomically retires orders, applies the exact masks and aircraft skip, and resets `GroupData::form` to −1 | Complete |
| `swarm_around` | `0x0070FBE0` | 3044 | 13 | `BUILD_AT`/`REPAIR`/`CAST_SPELL` + move, spread around a target → `halt`, `move_to` | Todo |
| `guard` | `0x006FCD30` | 2012 | 8 | `GUARD` on each member, plus pulls nearby idle units into the group → `halt` | Orders |
| `stance` | `0x0070D440` | 928 | 8 | writes `UnitData::stance` `+0xB1` / `Build::stance` `+0x7E`, sets flag bit `0x10`; installs nothing | State |
| `garrison` | `0x00700490` | 1791 | 7 | `GARRISON` on each member → `halt` | Orders |
| `flight` | `0x006FB260` | 3398 | 6 | `STRAFE` for aircraft → `attack`, `guard`, `launch_flight` | Todo |
| `queue_up` | `0x006FDBB0` | 1516 | 6 | `Build::queue_up` `0x00620F40` on producers; installs no unit order | Todo |
| `form` | `0x00707220` | 746 | 6 | sets `GroupData::form`, computes the captain destination, then installs `GROUP_MOVE`/`GROUP_ATTACK_TO` | Orders |
| `air_patrol` | `0x007029D0` | 1763 | 5 | `AIR_PATROL` → `move_to` | NotOnTheWire |
| `patrol` | `0x007030C0` | 1215 | 5 | **`GROUP_PATROL`** (not `PATROL`) → `air_patrol` for aircraft | Orders |
| `follow` | `0x006FD510` | 645 | 4 | `FOLLOW` on each member except the target itself → `halt` | Orders |
| `alarm` | `0x0070EC30` | 3169 | 4 | garrisons civilians → `alarm_peasant`, `garrison` | Todo |
| `board_ship` | `0x00700010` | 1149 | 3 | `BOARD_SHIP` on passengers, `AWAIT_BOARD` on the ship → `halt` | Orders |
| `gather` | `0x00700B90` | 3052 | 3 | `GATHER`, `CAST_SPELL` → `move_near` | Orders |
| `trade` | `0x00701CC0` | 1022 | 3 | `TRADE_ROUTE` → `halt` | Orders |
| `repair` | `0x007020C0` | 999 | 3 | `REPAIR`, `CAST_SPELL` → `halt` | Orders |
| `attack_ground` | `0x00704520` | 1133 | 3 | `ATTACK_GROUND` → `air_attack_ground` for aircraft | Orders |
| `disband` | `0x0070E260` | 693 | 3 | atomically executes the reverse direct/build-queue plan; `all == 0` stops after one and dead identities remain in the group | Complete |
| `eject_all` | `0x00710B40` | 766 | 3 | unloads garrisons; installs nothing | Todo |
| `hotkey` | `0x006FA7A0` | 64 | 2 | binds a control group; **only caller is `Console::on_key_down`** | NotOnTheWire |
| `recall` | `0x006FA7E0` | 1373 | 2 | `STRAFE` → `return` | Todo |
| `return` | `0x006FAD40` | 1307 | 2 | `STRAFE` back to base | NotOnTheWire |
| `buildmask` | `0x006FC9A0` | 487 | 2 | toggles `BuildData::build_masks` for `Build::mask_me`-eligible members | StateWired |
| `spell` | `0x006FE1A0` | 4100 | 2 | `CAST_SPELL` | Todo |
| `gather_point` | `0x006FF1B0` | 3668 | 2 | sets a building's rally point → `flight` | Todo |
| `transport` | `0x00702620` | 932 | 2 | toggles transport mode; clears orders | Todo |
| `air_attack_ground` | `0x00703D80` | 1939 | 2 | `AIR_ATTACK_GROUND` | NotOnTheWire |
| `siege_attack_to` | `0x0070D830` | 2037 | 2 | AI-only (`Army::do_forming`, `Army::march_to_target`) → `guard`, `move_to` | NotOnTheWire |
| `scramble` | `0x007111C0` | 894 | 2 | `AIR_PATROL` over the unit's own position | Orders |
| `launch_flight` | `0x006FBFB0` | 2544 | 1 | → `flight` | NotOnTheWire |
| `unitmask` | `0x006FCB90` | 404 | 1 | toggles `UnitData::unit_masks`; mask `0x100` clears canonical orders | StateWired |
| `stop_spell` | `0x006FD7A0` | 480 | 1 | cancels casting | Todo |
| `alarm_peasant` | `0x006FD980` | 550 | 1 | allocates `GARRISON` **directly**, not through an `add_*_order` | NotOnTheWire |
| `city_gather` | `0x00701780` | 1333 | 1 | city rally point; installs nothing | Todo |
| `set_transport` | `0x007024B0` | 357 | 1 | toggles unit mask `0x00800000` under owner transport capability | StateWired |
| `launch_patrol` | `0x00703580` | 2043 | 1 | **`AIR_PATROL`** | Orders |
| `siege_attack` | `0x00706FF0` | 549 | 1 | → `attack`, `guard` | Todo |
| `build` | `0x00707510` | 1256 | 1 | → `swarm_around` | Todo |
| `begin` | `0x00714100` | 8 | 0 | virtual receiver; writes `GroupData::disband = 0` | Complete |

### Simple state opcode tranche (2026-08-09)

| opcode | command | recovered target | status |
|---:|---|---|---|
| 1 | `BeginCommand` | `GroupData::disband = 0` | `complete` |
| 14 | `SetTransportCommand` | unit mask `0x00800000`, gated by owner transport state and `can_ever_transport` | `state_wired` |
| 32 | `UnitmaskCommand` | `UnitData::unit_masks` with the recovered first-eligible carry rule | `state_wired` |
| 33 | `BuildmaskCommand` | `BuildData::build_masks` after the `Build::mask_me` capability predicate | `state_wired` |

Opcode 1 corrects an extraction blind spot: `process_begin` `0x00949FD0` reaches the
group's virtual slot `+0x14`, whose concrete `Group::action_begin` implementation at
`0x00714100` is only `mov [ecx+0x28], 0; ret`. A direct-call-only scan therefore reported
zero sites and incorrectly classified it as off-wire.

The fixed mask commands decode `mask` at wire `+1` and a second signed dword at `+5`.
Both recovered optimized bodies ignore the second dword. For mask `0x40000`, UNITMASK
force-clears; otherwise the first eligible member chooses set versus clear and the
decision carries through the remaining selection. BUILDMASK uses the same carry rule.
The bridge requires explicit `Fleet` predicates for transport and build-mask capability,
so missing product data skips instead of guessing. UNITMASK `0x100` also clears modeled
order queues, but retail's adjacent partial-path and presentation state is not represented;
that is why these three rows remain `state_wired`, not `complete`.

Diplomacy opcodes 37–45 remain inert. Their action bodies cross proposal/offer ownership,
resource transfer, and target retasking; reducing those effects to a relation matrix would
silently manufacture lockstep behavior that has not been recovered.

### Inline speed/pause state tranche (2026-08-09)

| opcode | handler | recovered target | status |
|---:|---|---|---|
| 52 | `process_speed_set` `0x00946380` | signed dword `+1` → `TurnControl+0x30`, behind the exact network/speed-lock gate | `complete` |
| 53 | `process_speed_up` `0x009461A0` | increment speed through Fast (index 3), with the same gate | `complete` |
| 54 | `process_speed_down` `0x00946290` | decrement nonzero speed, with the same gate | `complete` |
| 76 | `process_pause` `0x00944160` | raw byte `+1`, pause bit/delay and network allowance counter | `state_wired` |
| 79 | `process_player_speed` `0x00943730` | eight `u8` deltas at `+1..+8` wrapping into player `u32` counters | `complete` |

The speed handlers' deterministic mutation is only the speed index and its two game-state
gates: network is `Game+0x820 & 4`, and the lock is `Game.info_flags+0x20 & 0x100`.
Audio, UI messages, and the `timeGetTime` wall-clock pacing fields do not enter headless
simulation state. Opcode 79 uses `CommandPackage::play`, not a byte in its own body, and
all eight additions wrap as x86 `u32` arithmetic.

Pause remains red on purpose. The solo toggle, duplicate-state no-op, immediate-process
unpause gate, ten-pause network allowance, and per-player pause-count increment execute;
the network restart tail (`Game+0x821` bits, callback frontier, and leader resumption) is
not represented in this bridge. The metadata therefore says `state_wired`, never
`complete`.

### Control-group and MP-log tranche (2026-08-09)

| opcode | handler | recovered target | status |
|---:|---|---|---|
| 34 | `process_hotkey` `0x009474D0` | one of 162 `HotKeyGroups` slots: selection subset or raw camera bookmark | `complete` |
| 55 | `process_mp_log` `0x00946080` | `Game+0x820` bit `0x20` and restart delay `Game+0x81C` | `complete` |

HOTKEY decodes `group/clear/valid` as signed dwords at `+1/+5/+9`, preserves the raw
IEEE-754 x/y bits at `+13/+17`, and reads zoom at `+21`. The selection arm reproduces
`HotKeyGroups::copy_group` `0x00715120`: it copies the measured scalar subset and only
the live prefixes of the six parallel arrays, preserves destination identity/unlisted
fields, stamps the current frame, and clears the camera bookmark. The clear arm writes
only `GroupData::num = 0`, then optionally installs x/y/zoom. The subsequent
`HotKeyGroups::update` call computes local UI labels/sounds and is presentation-only.

MP_LOG has no payload fields. On the first command it sets bit `0x20` and zeroes the
restart delay; on the second it clears the bit and changes a zero delay to two. There is
no `EndCommand` row in the 82-entry `CommandTypes` dispatch: `CommandPackage::end_process`
`0x0094C800` is a package finalizer, not a wire opcode, so the bridge does not invent one.

### AI controls and measured no-op tranche (2026-08-09)

| opcode | handler | recovered target | status |
|---:|---|---|---|
| 56 | `process_check_random` `0x00946020` | diagnostic log of `u32 seed` at `+1`; no comparison or store | `complete` |
| 62 | `process_cheat_ai_speed_increase` `0x00944EC0` | `GameAccess::ai_speed`, wrapping increment capped at 10 in solo | `complete` |
| 63 | `process_cheat_ai_speed_normal` `0x00944DF0` | `GameAccess::ai_speed = 1` in solo | `complete` |
| 64 | `process_cheat_ai_toggle` `0x00944D20` | `GameAccess::ai_off = (ai_off == 0)` in solo | `complete` |
| 81 | `process_marwan` `0x00943660` | diagnostic log of its start byte only | `complete` |

All three AI mutations are refused when `Game+0x820 & 4` marks network play. Their
`Log::say` calls are diagnostic-only; `ai_speed` and `ai_off` are the
simulation globals at `0x00C061C0/0x00C061C4`. Opcode 56 is intentionally green as an
exact simulation no-op: this build logs its seed and frame but does not compare or store
them; the lockstep seed array is updated elsewhere in `begin_process/process_turn`.
Opcode 81 likewise has no simulation tail after logging its byte. The bridge still
consumes both exact fixed wire bodies (5 and 2 bytes),
so a following command starts at the retail cursor.

### Lockstep report, chat-route, and camera tranche (2026-08-09)

| opcode | handler | recovered target | status |
|---:|---|---|---|
| 57 | `process_check_sums` `0x009459D0` | sixteen logged `u32` channels; total at `+61` → `CommandPackage::checksums[play]` | `complete` |
| 58 | `process_next_check_sum` `0x00945E20` | `u32` value at `+2` → the same peer slot when `checksum_recheck == 0` | `complete` |
| 69 | `process_chat_set` `0x009458E0` | eight status bytes → sender `Player::who` row in the chat matrix | `complete` |
| 72 | `process_camera` `0x00943B00` | viewpoint log and local Console/Scene zoom/scroll only | `complete` |

The bridge carries the exact eight-entry `checksums` table at `0x00CBEE90` and the
`checksum_recheck` gate at `0x00CC00B0`. Opcode 57 always replaces its sender slot with
the sixteenth wire word; opcode 58 ignores its `checksum_type` byte for state and refuses
the store during a recheck. Both retain raw `u32` bits. Opcode 69 zero-extends all eight
wire bytes into the row selected through `Player::who`; those statuses later gate chat
and ping delivery, so the routing matrix is retained rather than dismissed as UI.
Opcode 72 is closure-green as a measured simulation no-op: every non-log branch targets
local camera presentation, and the full ten-byte body is still consumed before the next
command.

### Reveal-map and turn telemetry tranche (2026-08-09)

| opcode | handler | recovered target | status |
|---:|---|---|---|
| 59 | `process_cheat_view_all` `0x009449B0` | toggle reveal bit, restart delay, and addressed player's accumulated-cheat byte | `complete` |
| 74 | `process_turn_data` `0x00943D20` | five `u16` telemetry arrays + sender flag; conditional remote reveal | `complete` |

`Game::action_cheat_view_all` `0x00592CD0` toggles semaphore bit `0x02`: clearing it
changes a zero restart delay to two, while setting it zeroes the delay. Its
`action_cheat_warning` tail increments `Player::accum_cheated` with byte wrapping; UI
invalidation and external telemetry remain outside the headless core.

TURN_DATA decodes unsigned `ping_time/frame_average/wait_time/game_lag/forced_loads` at
`+1/+3/+5/+7/+9`, writes the five exact `TurnControl` arrays for `package.play`, and ORs
that sender into the byte flag at `TurnControl+0x168`. `ping_time & 0x80` reveals only
for a non-local sender while reveal-map is clear, using the sender's `Player::who` map.
The newly implemented `ATTACK_TO`/`GROUP_ATTACK` executors were also re-audited here, but
their command receivers still cross formation, caster, and target-split tails; those rows
remain honestly `orders_partial`.

### Technology and resource cheat-state tranche (2026-08-09)

| opcode | handler | recovered target | status |
|---:|---|---|---|
| 60 | `process_cheat_give_techs` `0x00945070` | set technology bits 0..805, status = 0, accumulated-cheat byte | `complete` |
| 61 | `process_cheat_zero_techs` `0x00944FA0` | clear technology bits 0..805, status 0→2, accumulated-cheat byte | `complete` |
| 65 | `process_cheat_increase_buckets` `0x00944C50` | add 1000 to six XOR-encoded resource buckets, accumulated-cheat byte | `complete` |
| 66 | `process_cheat_zero_buckets` `0x00944B80` | zero six encoded buckets, consume sound RNG, typed external sound receipt | `complete` |
| 67 | `process_cheat_init_unit` `0x00944A80` | direct/all-player unit initialization, nearby search, warning tail | `complete` |

The tech actions reproduce the exact 806-bit loops from `Game::action_cheat_give_techs`
`0x00593180` and `Game::action_cheat_zero_techs` `0x00593120`; the two padding bits in
the final byte are preserved. Give writes status zero throughout. Zero changes only an
initial zero status to two and preserves every other nonzero value.

The six resource buckets retain their retail encoding: decode with XOR `0x8221`, add
1000 with wrapping `u32` arithmetic, then XOR again. ZERO_BUCKETS writes the literal
encoded zero `0x8221` to all six slots and retains the same wrapped diagnostic byte.

Its network-only response tail calls `SoundGlobal::random` at `0x00E85F0C` with
`(0, response_count - 1)`. This measured ECX target refutes the earlier hypothesis that
ZERO_BUCKETS consumes the main `game_random` stream (`0x00E37A8C`). Because
`Random::get` is half-open, a one-entry list consumes no draw and the last entry of every
longer list is unreachable; the bridge intentionally preserves both quirks. A valid
selected ID becomes a typed `CheatResponseReceipt` for the product layer's external
`SoundRef::play` call; invalid IDs still consume the sound draw but produce no receipt.
Solo and empty-list paths consume no draw.

INIT_UNIT crosses the much larger `Objects::init_unit` world boundary without replacing
it with a toy spawn. Non-negative `who` issues exactly one allocation at the two raw wire
coordinates with trailing arguments `[-1; 3]`. Any negative `who` scans player slots
0..7 in order, gates on `PlayerData::valid & 1`, and calls the addressed `UnitType`'s
`find_nearby_spot` with the exact trailing tuple
`[0, 0xC00, 0, 0x55555555, 3, -1, -1, 0, 0, -1, 0, -1]`; only a zero return proceeds to
allocation at the returned coordinates.

`Fleet::apply_cheat_init_unit_transaction` is an atomic typed host boundary: unavailable
hosts promise zero world mutation, while applied hosts return the entire ordered nearby /
allocation trace. The bridge validates the echoed command, every player identity, the
constant call tuple, branch order, coordinates, and `[-1; 3]` allocation tail, retaining
mismatches as auditable receipt records. It then reproduces `action_cheat_warning`: direct
mode warns `who`, while the all-player loop warns its completed counter value 8. The
wrapped accumulated-cheat byte now covers all ten owner slots, and network warnings emit
a typed external `SoundGlobal::play` category-99 receipt rather than pretending the audio
selection belongs to the simulation.

### HALT and DISBAND transactional lifecycle tranche (2026-08-09)

HALT and DISBAND now cross an atomic typed `Fleet` boundary instead of mutating the
bridge's lightweight object table piecemeal. For HALT, the host snapshots the scenario
`ignore_orders` prelude and every selected member, then returns the exact ordered
`plan_action_halt` result. Receipt validation recomputes that plan, including the
`0x04000000` and `0x100` unit-mask clears, form reset, airborne-plane skip, flags vetoes,
and order/path/action retirement. The queue-first and queued-FORM branches abort before
inserting or reissuing anything when the transaction is unavailable or malformed.

DISBAND follows the same fail-closed contract with the scenario validation gate, owner
locality, reverse member scan, active-building queue path, direct retirement path, and
local feedback steps all carried in one receipt. The receiver assigns the validated
post-action `GroupData` verbatim: retail retains dead member identities and does not
compact the selection. Product hosts promise that an unavailable or malformed receipt
means zero world mutation, which is pinned both in the command-only fixture and through
the real `EnvWorld` callbacks.

## Five things worth keeping

### 1. `process_<x>` returns `sizeof(<X>Command)` — 79/79, zero mismatches

Traced over all 82 handlers. Seventy-nine end in `mov eax, <imm>` whose value equals the
PDB `sizeof` **exactly**. The other three compute their return, and they are precisely the
three variable-length commands: `GroupCommand` (`3 + 2·num`), `SplineCommand`
(`6 + 8·len`), `ChatCommand` (`19 + 2·len`).

This is the first confirmation of `schema/command-wire.json` from *code* rather than from
the type stream — two independent derivations of the same 82 numbers, agreeing. It also
independently confirms `don-net::Command::wire_len`'s three special cases were the right
three.

### 2. The receiver is `groups.list[package.group]`, not `groups[play]`

Every order-issuing handler ends:

```
mov  eax, [ebx + 0x0c]      ; CommandPackage::group
test eax, eax
js   skip                   ; group < 0 -> command dropped
imul ecx, eax, 0x9d4        ; sizeof(Group) == 2516
add  ecx, [0x00e85f20]      ; groups.list
call Group::action_<x>
```

`[measured, CommandPackage::process_halt 0x00949140 at 0x00949201.]` The group index is
written by opcode 0 and persists for the rest of the package, so **a `CommandPackage` is
one selection followed by the actions that apply to it**. A packet with no `GroupCommand`
addresses whatever slot the previous one left behind — the bridge reproduces that, and it
is why `Package` is a struct with a `group` field rather than a parameter.

`Groups::get_open_slot` `0x006FA460` bases each owner's window at `who * 0x40` and bounds
the scan at `+0x2E`: **64 slots of stride per owner, 46 reachable by the allocator**, LRU
by `GroupData::stamp`, never recycling `groups.cur[who]`. It logs
`"UH OH, NEED MORE GROUPS! No non-singular groups found!"` when it fails.

### 3. `Unit::add_<k>_order` → `OrderIndex` is a total, measured function

Every `push imm; call OrdersMemManager::get_obj 0x00730AC0` site in `.text`, resolved:

| add function | VA | allocates |
|---|---|---|
| `add_move_facing_order` | `0x005E55C0` | `MOVE_TO` / `ATTACK_TO` / `EXPLORE_TO` / `FLEE_TO` |
| `add_move_order` | `0x00616ED0` | forwards to the above after computing a facing angle |
| `add_group_move_order` | `0x005E4710` | `GROUP_ATTACK_TO` when selector `== 2`, else `GROUP_MOVE` |
| `add_patrol_order` | `0x005E4560` | **`GROUP_PATROL`** |
| `add_air_patrol_order` | `0x005E4350` | `AIR_PATROL` |
| …and 17 more | | the `OrderIndex` their name says |

`add_move_facing_order`'s selection, at `0x005E56BC`:

```
if (orders == ATTACK_TO && unittype.flags[+0x2C8] & 0x10) {
    unit.flags |= 0x04000000;  orders = EXPLORE_TO;
}
switch (orders) { 2 -> ATTACK_TO; 3 -> EXPLORE_TO; 4 -> FLEE_TO; default -> MOVE_TO }
```

So the `orders` byte on `MoveToCommand`/`MoveNearCommand` **is an `OrderIndex`** and is
carried through unchanged. `move_order_kind()` implements the switch; the type-flag
promotion is left to a caller that holds unit-type data, and says so.

`Unit::add_move_order` is also where the `^ 0x00063637` object-coordinate XOR shows up in
this path — it un-XORs `unit.x`/`unit.y` to compute the facing angle before delegating.

### 4. Three `OrderIndex` values are constructed nowhere in the binary

`NONE` (0, the empty-list sentinel), **`PATROL` (5)**, **`CHANGE_FORM` (18)** and
**`GROUP_ATTACK` (20)** have no construction site anywhere in `.text`. The only
non-constant allocation is `copy_order` `0x0072F900`, which duplicates an existing order
and therefore cannot originate a kind.

`PATROL` being dead was already known from the `do_job` jump table
(`crates/don-sim/src/order.rs`); this is the same conclusion reached independently from the
allocation side. `CHANGE_FORM` and `GROUP_ATTACK` are new: their executors
`Unit::do_form_change` `0x005E8670` and `Unit::do_group_attack` `0x005E75A0` are
unreachable dead code.

### 5. `QUEUE_FIRST` is a group-level dance, not a per-unit insert

`QueuePos` is `{QUEUE_FIRST = 0, QUEUE_LAST = 1, QUEUE_NEW = 2}` `[measured, PDB
LF_FIELDLIST 0x216F]`. `Unit::add_<k>_order` handles only `QUEUE_NEW` (clear the list
first). Ten `Group::action_*` handle `QUEUE_FIRST` themselves `[structure]`:

```
if (queued == QUEUE_FIRST) {
    OrderList saved;
    set_up_insert(&saved);        // 0x0070E520
    action_halt(0);               // 0x0070D0C0
    action_<x>(args, QUEUE_NEW);  // re-enter
    finish_insert(&saved);        // 0x0070E620 — replays the stash as group actions
    return;
}
```

`Group::finish_insert` is why `action_guard`, `action_follow`, `action_attack`,
`action_move_near`, `action_board_ship`, `action_trade`, `action_gather`,
`action_garrison`, `action_spell`, `action_attack_ground` and `action_swarm_around` all
list it as a caller: it *re-issues* the saved orders through the same API. The bridge
reproduces that shape; the test
`queue_first_puts_the_new_order_in_front_and_keeps_the_tail` pins the observable.

## Cross-check: don-net, don-replay, don-sim, don-env

### The wire layer agrees completely

`crates/don-replay/tests/command_bridge_agreement.rs`, 6 tests, no disagreement found:
all 82 struct names, all 82 handler names, all 79 fixed sizes and all three variable-length
formulas match across `don-net::COMMAND_SIZES`, `don-replay::wire::COMMAND_SIZEOF` and the
measured `process_*` return values. A real payload walks to identical boundaries under both
length functions.

### don-env: three disagreements, each now pinned by a test

`crates/don-env/tests/command_bridge_agreement.rs`, 7 tests.

**(a) Three group-scoped opcodes sit on the env's player head.** 35 opcodes are handed to
`groups.list[package.group]` — they act on the current *selection*. `don-env` puts 31 of
them on its unit head and classifies **`ALARM` (27), `UNITMASK` (32) and `BUILDMASK` (33)**
as player verbs. Measured, all three take the group as `this` exactly the way `MOVE_TO`
does: `Group::action_alarm` `0x0070EC30`, `Group::action_unitmask` `0x006FCB90`,
`Group::action_buildmask` `0x006FC9A0`. A player head carries no selection, so those three
cannot be expressed faithfully where they are. Not fixed here — `don-env/src/generated.rs`
is generated by another lane's `gen_spec.py`.

Opcode 1 `BEGIN` is the additional group receiver. Its command handler uses the group
vtable rather than a direct `call Group::action_begin`, so it does not add another env
classification disagreement.

**(b) `PATROL` and `LAUNCH_PATROL` are not `MOVE_TO`.** The env now
follows the measured answer: opcode 10 → `Group::action_patrol` → `Unit::add_patrol_order` →
**`GROUP_PATROL` (22)**, except that true planes take its `action_air_patrol` delegate and
install **`AIR_PATROL` (17)**; opcode 11 → `Group::action_launch_patrol` →
`Unit::add_air_patrol_order` for true planes only. The type predicate is retail's
`UnitData::is_plane` (`domain == AIR && !(unit_flags & 0x20)`), so helicopters remain on
the group-patrol path. The agreement test drives the bridge, checks all four routing
branches, and exercises the env mask/application path with shipped type records. A second
agreement test covers the recovered patrol exceptions: ground `QUEUE_FIRST` replaces,
compatible `QUEUE_LAST` extends the active dynamic waypoint arrays, the true-plane
installer replaces on every incompatible case, and the recovered executors retain their
order while inserting `ATTACK_TO` / `STRAFE` at the front. Ordinary EnvWorld frames now
leave AIR_PATROL stationary; only the mandatory, no-default `AirPatrolHost` may cross the
post-physics transition. The three remaining readiness blockers are the adjacent
`Unit::do_air_physics`, mod-16 air/bomber search, and mod-32 building search hosts—not
command routing, queue identity, or a silent straight-line/empty-search substitute.

**(c) Opcode 34 `HOTKEY` is not a selection command.** `don-env` classifies it as SELECTION
alongside opcode 0. Measured, only opcode 0 writes `CommandPackage::group`;
`process_hotkey` `0x009474D0` calls `HotKeyGroups::copy_group` `0x00715120` and never
touches the package's group field. `Group::action_hotkey` `0x006FA7A0` exists but its only
caller is `Console::on_key_down` — the local UI, not the wire.

**No disagreement** on wire sizes for any of the 49 verbs, on the 82-opcode partition, or
on the numbering of the shared `OrderIndex` and `QueuePos` enums.

## Corrections to existing artifacts

Two sentences each, per the standing rule. None of these files was edited.

* **`COVERAGE.md` §3 — "23 absent" `do_job` arms.** Three of the 28 arms are not merely
  unimplemented but *unreachable*: `PATROL` (5), `CHANGE_FORM` (18) and `GROUP_ATTACK` (20)
  have no `OrdersMemManager::get_obj` construction site anywhere in `.text`. The honest
  denominator for `do_job` is 24 reachable arms plus the `NONE` idle arm, not 28.

* **`COVERAGE.md` §6 item 3 — "the ~44 other `Group::action_*`".** There are **42**
  `Group::action_*` procedures in total, so `move_near` plus 41 others, carrying 209 direct
  call sites. `move_near`'s 23 call sites are confirmed, but `Group::action_move_to`
  `0x0070FBA0` has **34** and is the real hot entry point — it is a 49-byte forwarder that
  inserts `tolerance = 0`.

* **`crates/don-net/src/lib.rs` — `CommandPackage::group` is "scratch at runtime".** At
  runtime it is the `groups` pool slot that `process_group` interned via
  `Groups::push_group`, and every subsequent handler in the package reads it and drops the
  command when it is negative. The comment is right that the *file* reuses the field as a
  monotone serial, but "scratch" understates what it does in the engine.

* **`docs/derivation/architecture.md` (implied) — `Group::action_*` is not a UI-only
  layer.** The same 42 entry points are called by `CommandPackage` (the wire), by the
  shipped AI (`Army::find_target`, `Army::march_to_target`, `Leader::produce_building`,
  `Leader::check_orphaned_buildings`) and by the BHS scenario API (`ScenarioFuncSet::*`,
  `ScenarioData::issue_order`). Porting this layer serves the RL action space, the AI
  replication track and the BHS track at once.

* **`process_turn_data` (opcode 74) reveals the map.** Measured: if
  `TurnDataCommand::ping_time & 0x80` is set, the sender is not the local player, and
  `game.flags & 2` is clear, it calls `Game::action_cheat_view_all` `0x00592CD0` with the
  sender's leader slot `[structure, 0x00943E2E]`. This is almost certainly the
  resigned-player-spectates path, but the gate at `[0x00C06210 + 0x2A0]` is unidentified
  and it is recorded here because a telemetry opcode with a reveal-map side effect is worth
  someone else looking at.

## What is deliberately not here, and what it blocks

* **`Group::compute_form` `0x00707C80` / `Form::categorize` `0x0072E250` /
  `Group::update_positions` `0x00713810`.** The bulk of `action_move_near`'s 9 KB is
  formation layout: each member's destination is the group anchor plus its rotated
  `curr_x`/`curr_y` offset. The `groups_guys` lane owns `update_positions`; until they meet,
  every member of a moving group gets the *same* destination. This is the single largest
  fidelity gap in the ported set.
* **`Unit::find_attack_pos` `0x00601280` (7,124 B, uncited).** `action_attack` splits the
  group by whether each member can reach the target — reachable members get `ATTACK`,
  the rest get `move_to`, casters get `CAST_SPELL`. Without the reach test the port
  installs `ATTACK` on everyone.
* **`Unit::work` `0x0060D180` (2,885 B, uncited).** Orders installed here sit in their
  lists. Nothing advances or retires them yet; that is `COVERAGE.md` §6 item 2 and it is
  the natural next lane after this one.
* **`GroupData::get_stance_type` `0x0070D370`.** `action_stance`'s negative arguments are
  cycles whose modulus is 6/4/2/2 by stance type `[measured, the switch at 0x0070D47B]`.
  The type needs unit-type data the bridge does not hold, so a negative argument resolves
  against the combat cycle of 6 and the assumption is stated in the doc comment.
* **The 13 `Port::Todo` actions.** They dispatch and increment `BridgeStats::unported`
  rather than pretending to act, so a replay run reports its own coverage.

## Regenerating `command_tables.rs`

```sh
cd /Users/ember/dev/don/ron-bin
uv run --quiet --with capstone --with pefile python - <<'EOF'
# For each CommandPackage::process_* in schema/rise-procs.tsv: disassemble its extent,
# record every direct call target, resolve it against the proc table, and take the last
# `mov eax, imm` before `ret` as the wire length.
# Also resolve virtual receiver slots: process_begin's vtable +0x14 target is the measured
# Group::action_begin 0x00714100 exception that a direct-call scan cannot see.
# For each Group::action_*: same, plus count direct call/jmp sites to it over every named
# procedure, and follow the Unit::add_*_order calls to their
# `push imm; call 0x00730AC0` (OrdersMemManager::get_obj) argument.
EOF
```

The working scripts are in the lane scratchpad (`/private/tmp/cmdbridge/`); the recipe
above is the whole method and re-deriving it takes about five minutes. Do **not**
hand-edit `command_tables.rs`.
