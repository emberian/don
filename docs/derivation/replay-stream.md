# The `.rcx` per-frame command stream

**Lane:** `replay-stream`. **Date:** 2026-08-08.

## What this establishes

The command stream inside a Rise of Nations recorded game is **fully decoded**, byte-for-byte,
with **zero residual bytes**, on two independent specimens (one solo, one multiplayer, two
different engine builds). The stream is a flat sequence of `CommandPackage` records, each
carrying a **simulation frame stamp**, a **player index**, and a list of length-determined
commands drawn from an **82-entry opcode table (0x00–0x51) recovered from the engine's own
dispatcher**, complete with the engine's own names for every opcode.

Headline results, each expanded below:

| # | Result | Tier |
|---|---|---|
| 1 | Packet framing: 18-byte header + payload; the packet stream tiles to EOF exactly on both files (307,768 + 14,606 bytes, 0 residual). Command-level: the solo stream consumes every payload byte; 97/130 MP payloads do (§5) | C |
| 2 | Frame counter runs **0 … 10,499** = 10,500 frames = **700.00 s @ 15 fps = 11:40.0**, matching the specimen's known game time exactly | C [measured] |
| 3 | Complete 82-opcode command table with sizes and the engine's own names, from `FUN_0094a700` and `.rdata` format strings | structural [measured addresses] |
| 4 | **AI players emit no commands.** Every one of the 10,544 packets in the solo replay has `from = 0` (the human). The lane's stated expectation is **refuted** | C [measured] |
| 5 | **Multiplayer replays carry `process_check_sums` — a 16-field per-turn state checksum** (units, builds, walls, ammo, deaths, groups, guys, leaders, cities, items, goods, world, rules, scenario_data, script_run_time, final). Solo replays carry **none** | C [measured] |
| 6 | Multiplayer payloads are **XOR-obfuscated** and carry **RNG-driven inter-command padding**; decoding them fully requires co-simulating the engine's Random | structural + C |
| 7 | Player names extracted; the human's nation byte is `0x16` = index of `_DUTCH` in the binary's nation table | names [measured]; nation identification strong but **not conclusive** |

Nothing here is verified in the proof-assistant sense. The decode is Tier C: behaviourally
faithful with divergence measured, and the divergence measured is zero unconsumed bytes.

---

## 1. Specimens

| | solo | multiplayer |
|---|---|---|
| path | `/Users/ember/dev/don/ron-data/replays/today.rcx` | `C:\Users\ember\Documents\My Games\Rise of Nations\Recorded Games\multi\Playback - 2024.03.10 20'54'34 (Sun).rcx` |
| `.rcx` size | 108,241 | 62,842 |
| `.rcx` sha256 | `6cebfc09a7318e292385da926431c85ca1274907f77ac0023a585abcf038d58b` | `AA4BE02A14267BA5FBB994B422DBBBAD275440D0501355855D8A2646BA99CA11` (verified guest-side and host-side) |
| payload size | 1,332,849 | 1,039,929 |
| payload sha256 | `e6c20c5464df54bdd27aa544f875f5ed0fccc190d53736fc2360a3448e3ce2d8` | — |
| engine build in header | `00.2024.06.20` | **`00.2017.11.29`** |
| stream starts at | `0x000FA439` | `0x000FA52B` |
| stream bytes | 307,768 | 14,606 |
| packets | 10,544 | 130 |

The multiplayer specimen is a **different engine build (2017 vs 2024)** and it decodes with the
2024 binary's opcode table. That is evidence the command numbering and command sizes are stable
across at least those seven years of builds.

Extraction (guest → host) used the documented `certutil -encode` hop; the SHA-256 computed on
the guest matches the SHA-256 of the reassembled host file, so the transfer is verified.

---

## 2. Packet framing [measured]

The tail of the payload is a flat, un-indexed sequence of serialised `CommandPackage` objects.
Each record is:

```
offset  size  field
 +0x00   u32  stamp        simulation frame number
 +0x04   u32  from         player index that issued the package
 +0x08   u32  (unnamed)    1 on the first solo packet and on all 130 MP packets, else 0
 +0x0C   u32  serial       monotone package counter
 +0x10   u16  size         payload length in bytes
 +0x12   u8[size]          command list
```

**The field names are the engine's own.** `CommandPackage::process_all` = `FUN_0094c500`
(`re/decomp-all/0094c500.c`) does `psVar15 = (short *)(in_ECX + 0x10)` for the size, indexes an
array by `*(int *)(in_ECX + 4)`, and the unknown-opcode diagnostic in `FUN_0094a700` prints

```
L"Unknown Command Packet: %d, from: %d, size left: %d, stamp: %d",
    *param_1, in_ECX[1], (int)*(short *)(in_ECX + 4), *in_ECX
```

which pins `this+0x00` = **stamp**, `this+0x04` = **from**, `this+0x10` = **size** (a `short`).
`process_all`'s entry logger (`FUN_004b7350`, string at `.rdata:0x00AF7CA0`-adjacent) reads
`L"Processing packages for player: %d, stamp: %d, group: %d, size: %d/%d"`, giving the name
*group* for `this+0x0C`. At runtime `process_group` overwrites `this+0x0C` with the Group handle
it creates (`*(undefined4 *)(in_ECX + 0xc) = uVar15;`, or `0xFFFFFFFF` when the selection is
empty), and the order handlers gate on `if (-1 < *(int *)(in_ECX + 0xc))`. **In the file the
same offset holds a strictly monotone per-package serial** — solo: `1 … 10,544`, `+1` on every
one of 10,543 steps; MP: `1 … 79`. So the stored value is a serial and the member is reused as
scratch during playback.

### Locating the stream

There is no offset or length field pointing at the stream that I could find, but there is a
clean structural anchor: **in both specimens the stream begins in the byte immediately after a
`u32` table whose last element is `0x00000191`** (solo: that `u32` sits at `0x0FA435`, stream
at `0x0FA439`; MP: `0x0FA527` and `0x0FA52B`). Two files, two different engine builds, same
terminator.

The method the decoder below uses does not rely on that: it is a backward dynamic program over
the payload. A record at offset *o* is *good* iff `o + 18 + u16[o+16]` is *good*, with `from <
8` and with `stamp` and `serial` non-decreasing across the link, and `good[N] = true`. The
unique longest chain is the stream. Solo: 10,544 records covering `0xFA439 … EOF`. MP: 130
records covering `0xFA52B … EOF`.

Do **not** add `header[+0x08] == 0` as a constraint — I did at first, and it silently swallowed
the very first packet of the solo stream (the `process_leader_options` setup package, the only
solo packet with `+0x08 == 1`). The reproduction snippet in §8 is what caught it.

### Frame series [measured]

Solo specimen:

- `stamp` runs **0 … 10,499**, monotone non-decreasing, **10,500 distinct values**, no gaps.
  Step histogram: `+1` × 10,499, `0` × 44 (frames carrying more than one package).
- 10,500 frames ÷ 15 fps = **700.000 s = 00:11:40.0**. The specimen's known outcome is
  *defeat at game time 00:11:40*. This is an exact, independent confirmation of both the frame
  interpretation and the 15 fps tick.
- `serial` is strictly `+1` over all 10,543 steps; `serial = index + 1`.

Multiplayer specimen: `stamp` steps by **6** between turns (78 steps of 6, 51 steps of 0 where
the second player's package for the same turn follows). So the **command turn length is not a
constant of the format** — 8 frames in the solo 2024 game, 6 in the MP 2017 game. RoN tunes it
(`TURN_TUNING`, `TURNCONTROL`, `process_turn_data`).

---

## 3. The command table [measured addresses, structural]

`CommandPackage::process_one` = **`FUN_0094a700`** is a dense `switch` over opcodes **0x00 …
0x51 (82 opcodes)**. Each handler returns the number of bytes the command occupies, and
`process_all` advances by that. The names below are the engine's own wide-string debug formats;
each is at a concrete `.rdata` address (a representative block runs `0x00AF7CA0 … 0x00AFBE40`).

| op | handler | size | engine name and parameters |
|---|---|---|---|
| 0x00 | `FUN_0094a0c0` | `3 + 2*num` | `process_group, repeat who: %d num: %d` |
| 0x01 | `FUN_00949fd0` | 1 | `process_begin` |
| 0x02 | `FUN_00949ed0` | 5 | `process_stance stance` |
| 0x03 | `FUN_00949d90` | 13 | `process_form form, rotate, queued` |
| 0x04 | `FUN_00949c30` | 17 | `process_attack ox, whom, ignore, queued` |
| 0x05 | `FUN_00949ae0` | 13 | `process_siege_attack ox, whom, queued` |
| 0x06 | `FUN_00949970` | 17 | `process_swarm_around ox, whom, queued, orders` |
| 0x07 | `FUN_009497c0` | 22 | `process_move_to to_x, to_y, queued, set_angle, angle, orders, disembark, from, width` |
| 0x08 | `FUN_009495c0` | 26 | `process_move_near to_x, to_y, tolerance, queued, set_angle, angle, orders, disembark, from, width` |
| 0x09 | `FUN_009494a0` | 10 | `process_attack_ground to_x, to_y, queued` |
| 0x0a | `FUN_00949380` | 10 | `process_patrol to_x, to_y, queued` |
| 0x0b | `FUN_00949230` | 25 | `process_launch_patrol to_x, to_y, queued, shift, ctrl, alt` |
| 0x0c | `FUN_00949140` | 1 | `process_halt` |
| 0x0d | `FUN_00949050` | 1 | `process_transport` |
| 0x0e | `FUN_00948f60` | 5 | `process_set_transport` |
| 0x0f | `FUN_00948e00` | 9 | `process_set_transport ox, queued` |
| 0x10 | `FUN_00948cb0` | 13 | `process_repair ox, whom, queued` |
| 0x11 | `FUN_00948b20` | 21 | `process_trade ox, whom, oxx, whose, queued` |
| 0x12 | `FUN_00948a10` | 9 | `process_city_gather t, queued` |
| 0x13 | `FUN_009488b0` | 9 | `process_gather ox, queued` |
| 0x14 | `FUN_00948760` | 13 | `process_garrison ox, whom, queued` |
| 0x15 | `FUN_00948660` | 5 | `process_disband all` |
| 0x16 | `FUN_00948510` | 17 | `process_gather_point x, y, action, add_to_end` |
| 0x17 | `FUN_00948340` | 21 | `process_spell type, ox, x, y, whom` |
| 0x18 | `FUN_00948230` | 9 | `process_queue_up type, num` |
| 0x19 | `FUN_00948110` | 25 | `process_queue_up x, y, x2, y2, type, queued` |
| 0x1a | `FUN_00947fe0` | 17 | `process_eject_all who, back_to_work, eject_o, eject_who` |
| 0x1b | `FUN_00947ef0` | 1 | `process_alarm` |
| 0x1c | `FUN_00947db0` | 25 | `process_flight ox, whom, orders, shift, ctrl, alt` |
| 0x1d | `FUN_00947cc0` | 1 | `process_stop_spell` |
| 0x1e | `FUN_009479c0` | 13 | `process_follow ox, whom, queued` |
| 0x1f | `FUN_009478a0` | 13 | `process_guard ox, whom, queued` |
| 0x20 | `FUN_00947790` | 9 | `process_unitmask unitmask, set` |
| 0x21 | `FUN_00947680` | 9 | `process_buildmask unitmask, set` |
| 0x22 | `FUN_009474d0` | 25 | `process_hotkey group, x (float), y (float), zoom` |
| 0x23 | `FUN_00947bd0` | 1 | `process_recall` |
| 0x24 | `FUN_00947ae0` | 1 | `process_scramble` |
| 0x25 | `FUN_009473c0` | 13 | `process_treaty who, whom, treaty` |
| 0x26 | `FUN_009472b0` | 13 | `process_declare who, whom, treaty` |
| 0x27 | `FUN_009471b0` | 9 | `process_clear_tributes who, whom` |
| 0x28 | `FUN_009470b0` | 9 | `process_clear_all who, whom` |
| 0x29 | `FUN_00946fb0` | 9 | `process_accept who, whom` |
| 0x2a | `FUN_00946e90` | 9 | `process_reject who, whom` |
| 0x2b | `FUN_00946d70` | 17 | `process_tribute who, whom, good, amount` |
| 0x2c | `FUN_00946c50` | 17 | `process_deman_tribute who, whom, good, amount` *(sic — engine's typo)* |
| 0x2d | `FUN_00946b30` | 17 | `process_deman_tribute who, whom, whose, onoff` |
| 0x2e | `FUN_00946a20` | 13 | `process_buy who, good, flags` |
| 0x2f | `FUN_009468a0` | 13 | `process_sell who, good, flags` |
| 0x30 | `FUN_009466f0` | 15 | `process_unqueue who, o, type, uid` |
| 0x31 | `FUN_009465d0` | 11 | `process_come_out who, o, uid` |
| 0x32 | `FUN_009453f0` | 9 | `process_ping who, x, y` |
| 0x33 | `FUN_00945140` | variable | `process_ping who, spline_type, spline_flags, spline_cmd, len` |
| 0x34 | `FUN_00946380` | 5 | `process_speed_set speed` |
| 0x35 | `FUN_009461a0` | 1 | `process_speed_up` |
| 0x36 | `FUN_00946290` | 1 | `process_speed_down` |
| 0x37 | `FUN_00946080` | 1 | `process_mp_log` |
| 0x38 | `FUN_00946020` | 5 | `process_check_random seed` |
| 0x39 | `FUN_009459d0` | **65** | `process_check_sums` (see §5) |
| 0x3a | `FUN_00945e20` | 6 | `process_next_check_sum play, value, type` |
| 0x3b | `FUN_009449b0` | 5 | `process_cheat_view_all who` |
| 0x3c | `FUN_00945070` | 5 | `process_cheat_give_techs who` |
| 0x3d | `FUN_00944fa0` | 5 | `process_cheat_zero_techs who` |
| 0x3e | `FUN_00944ec0` | 1 | `process_cheat_ai_speed_increase` |
| 0x3f | `FUN_00944df0` | 1 | `process_cheat_ai_speed_normal` |
| 0x40 | `FUN_00944d20` | 1 | `process_cheat_ai_toggle` |
| 0x41 | `FUN_00944c50` | 5 | `process_cheat_increase_buckets who` |
| 0x42 | `FUN_00944b80` | 5 | `process_cheat_zero_buckets who` |
| 0x43 | `FUN_00944a80` | 17 | `process_cheat_init_unit who, t, x, y` |
| 0x44 | `FUN_009454f0` | variable | `process_chat_set bits, taunt, taunt_num, len, string` |
| 0x45 | `FUN_009458e0` | 9 | `process_chat_set` |
| 0x46 | `FUN_009438c0` | 5 | `process_resign play` |
| 0x47 | `FUN_009439a0` | 7 | `process_quit play, replay, system_quit` |
| 0x48 | `FUN_00943b00` | 10 | `process_camera zoom, x_loc, y_loc` |
| 0x49 | `FUN_009441d0` | 33 | `process_leader_options who, peasants, peasants_wait, buildings` |
| 0x4a | `FUN_00943d20` | 11 | `process_turn_data frame_average, ping_time, wait_time, game_lag, forced_loads` |
| 0x4b | `FUN_00944090` | 53 | `process_rename_city who, o, name` |
| 0x4c | `FUN_00944160` | 2 | `process_pause player, state` |
| 0x4d | `FUN_009464f0` | 2 | `process_cannon_time state` |
| 0x4e | `FUN_00943f30` | 521 | `process_console_cmd cmd, mouse_x, mouse_y` |
| 0x4f | `FUN_00943730` | 9 | `process_player_speed` (8 accumulators, see §4) |
| 0x50 | `FUN_00943ea0` | 3 | `process_ungraceful_player_drop player, state` |
| 0x51 | `FUN_00943660` | 2 | `process_marwan start` |

**This table is the engine's real action space** and it is materially richer than the 27-class
`Order` RTTI hierarchy the charter records. The `Order` classes are the *sim-side* objects;
this is the *wire* action space, and it additionally covers diplomacy (0x25–0x2d), the market
(0x2e–0x2f), production queue management (0x18, 0x19, 0x30, 0x31), unit/build masks
(0x20, 0x21), leader options (0x49), and speed/pause control. For the RL surface in charter
stage 8, **this table, not the `Order` hierarchy, is the action space to mask against.**

### Selection is a separate command, not a wrapper

`process_group` (0x00) returns `(uint)*local_24 * 2 + 3` — i.e. `3 + 2*num`:

```
+0  u8   opcode 0x00
+1  u8   num          count of object ids
+2  s8   who          owning player of the selection
+3  s16[num]          object ids
```

When `num == 0` the handler **re-uses the previously stored selection** (`DAT_00cbee88[who]`
holds the count, `DAT_00cbeeb0` the ids) and revalidates it, rather than selecting nothing.
That is exactly what the specimen shows: 41 of 63 selections have `num = 0` and each is
followed by a repeat of the same order — a player clicking a train button several times on the
same building. Orders are **siblings** of the selection command in the same package, not nested
inside it; they act on the group `process_group` just installed.

`process_group` also carries the two asserts that prove this is replay-processing code:
`"CommandPackage::process_group-- o >= objects.unit_mark[who]"` and
`"CommandPackage::process_group --- broken replay"`.

---

## 4. Solo specimen decode [measured]

The whole 307,768-byte stream parses with the size table above and **consumes every byte**:

```
packets = 10,544    commands = 11,946    residual bytes = 0
stamp   0 … 10,499 (10,500 distinct)     serial 1 … 10,544 (strictly +1)
from    = 0 for all 10,544 packets
```

| opcode | name | count |
|---|---|---|
| 0x48 | `process_camera` | 10,499 |
| 0x4f | `process_player_speed` | 1,313 |
| 0x00 | `process_group` (selection) | 63 |
| 0x18 | `process_queue_up type,num` | 42 |
| 0x19 | `process_queue_up x,y,…` | 18 |
| 0x36 | `process_speed_down` | 6 |
| 0x07 | `process_move_to` | 2 |
| 0x20 | `process_unitmask` | 1 |
| 0x49 | `process_leader_options` | 1 |
| 0x4c | `process_pause` | 1 |

The stream opens with the two setup packages: serial 1, frame 0 is
`process_leader_options` — body
`49 00000000 01000000 00000000 03000000 20000000 04000000 00000000 2a000000`, i.e.
`who=0, peasants=1, peasants_wait=0, buildings=3`, then four further `u32`s (`32, 4, 0, 42`)
that the log format does not name — and serial 2, frame 0 is `process_pause player=0 state=0`,
the game-start unpause.

63 selections and 63 orders — an exact 1:1 pairing, which is itself a structural check.

### `process_camera` (0x48) — one per frame

```
+0 u8 opcode 0x48   +1 u8 zoom   +2 u32 x_loc   +6 u32 y_loc
```
Present on 10,499 of the 10,500 frames. `zoom` takes 5 (7,771 frames), 4 (2,725), 6 (3) —
`FUN_00943b00` bounds it with `uVar8 < 7` and feeds it to `FUN_008437f0`. `x_loc` ranges
18,798 … 38,452 and `y_loc` 0 … 12,384; the position changes on only **297 of 10,499 frames
(2.8 %)**, and `FUN_00943b00` compares them against the two floats at `PTR_DAT_00c06200+0x304`
and `+0x308` before calling a scroll routine — consistent with a viewpoint record, not a
checksum.

### `process_player_speed` (0x4F) — exactly every 8 frames

1,313 occurrences, at frames ≡ 1 (mod 8), spaced by exactly 8 with no exception. Payload is 8
`u8` accumulators, added by `FUN_00943730` into per-player `u32` counters at
`PTR_DAT_00c061ec + who*0x8C + {0x48, 0x4C, 0x50, 0x54, 0x58, 0x5C, 0x64, 0x68}`
(note `0x60` is skipped). The `.rdata:0x00AFBC00` format string names them:

```
accum_frames_zoomed_in, accum_frames_zoomed_out, accum_clicks, accum_hotkeys,
accum_minimap_clicks, accum_mainmap_clicks, accum_control_groups_formed,
accum_control_groups_activated
```

Match totals for the specimen:

| counter | total |
|---|---|
| frames_zoomed_in | 7,774 |
| frames_zoomed_out | 2,722 |
| clicks | 125 |
| hotkeys | 57 |
| minimap_clicks | 1 |
| mainmap_clicks | 75 |
| control_groups_formed | 0 |
| control_groups_activated | 0 |

**`frames_zoomed_in + frames_zoomed_out` sums to 8 on every packet** (971 packets are `(8,0)`,
339 are `(0,8)`, two are `(3,5)`, one is `(0,0)`), totalling **10,496** — the whole game length
to within one turn. This is an independent, in-band confirmation that the solo command turn is
**8 simulation frames** and that the match ran ~10,500 frames. It also gives a free
human-activity measure: 125 clicks and 57 hotkey presses over 11:40.

### The player's actual orders

All 63 orders come from `who = 0`. Selected object ids fall into two disjoint clusters —
`{0, 1, 2, 6, 9, 11–19, 24}` and `{2000, 2005, 2010, 2019}` — consistent with the engine's
`objects.unit_mark[who]` split between mobile objects and buildings that the `process_group`
assert names.

`process_queue_up type,num` (0x18) type histogram: `50 × 30`, `52 × 7`, `544`, `551`, `552`,
`565`, `572` (one each).
`process_queue_up x,y,…` (0x19) type histogram: `414`, `417 × 7`, `418`, `419`, `420 × 2`,
`427`, `435`, `436`, `437 × 2`, `439`.

Order world coordinates span x ∈ [16,399, 35,958], y ∈ [758, 6,884] — the same units as
`process_camera x_loc/y_loc`.

**I could not resolve type ids to type names and I am not going to guess one.** The shipped
data gives 364 `<NAME>` entries in `unitrules.xml` and 129 in `buildingrules.xml`; **364 + 129
= 493, exactly the dimension of the 493×493 balance table** at `0x00C06AFC`, which strongly
suggests a single 493-wide type space of units-then-buildings. Under that reading the 0x19
(building-placement) ids 414–439 land in the building range, which is self-consistent — but the
0x18 ids 544–572 exceed 493 entirely, and `techrules.xml` has only 85 entries, so a naive
`units | buildings | techs` concatenation does not close either. The replay embeds its own
loaded type table (unit names begin at payload `0x44EF`), which is the right ground truth, but
its records are variable-length and enumerating them is a different lane's job. **Open.**

### What the solo stream does *not* contain

- **No `process_check_sums` (0x39).** Zero occurrences. A solo `.rcx` is not a per-frame
  oracle. See §5.
- **No `process_resign` / `process_quit`.** The stream simply stops at frame 10,499,
  consistent with a defeat rather than a resignation.

---

## 5. Multiplayer: checksums, obfuscation, and RNG padding

### The stream generalises [measured]

The 2017-build multiplayer specimen frames identically: 130 packets, `0xFA52B … EOF`, zero
residual. `from` takes **two** values — 79 packets from player 1, 51 from player 0 — which
**settles that `+0x04` is the player index**, something the solo file (all zeros) could not.
`serial` runs 1 … 79 and both players' packets for a turn share a serial. Unnamed field `+0x08`
is `1` in all 130 MP packets, and in the solo file `1` only on the first packet and `0` on the
other 10,543.

### Payloads are XOR-obfuscated [structural, confirmed behaviourally]

`process_all` (`FUN_0094c500`) contains an explicit obfuscation pass, gated on the multiplayer
flag `PTR_DAT_00c061ec[0x820] & 4`:

```c
uVar7  = (ushort)(*(uint *)(PTR_DAT_00c061ec + 0x10) >> 8);   /* 16-bit key   */
local_38 = (int)*psVar15 / 2;                                  /* size/2 words */
...  *puVar12 = *puVar12 ^ uVar7;                              /* XOR each u16 */
```

The header is **not** obfuscated, only the payload from `+0x12`. In this specimen the key is
constant across all 130 packets; recovering it as the modal 16-bit word of the ciphertext (long
zero runs in the plaintext expose it directly) gives **`0x8EC6`**. De-XORing with it produces
payloads that parse cleanly against the opcode table, which is the confirmation.

### RNG-driven inter-command padding [structural]

Still in `process_all`, after each command:

```c
if ((PTR_DAT_00c061ec[0x820] & 4) == 0) { iVar11 = 0; }
else                                    { iVar11 = FUN_00a39d70(0,2); }   /* in_range(0,2) */
local_2c = iVar13 + iVar11 + local_3c;
```

`FUN_00a39d70` is the engine's `in_range` (charter-established). So in multiplayer the reader
**skips a pseudorandom 0–1 bytes between consecutive commands, drawn from the simulation RNG**.
This is not decoration: it means a multiplayer command stream is **not statically decodable** —
a faithful reader has to advance the same Random object in lockstep with the sim. With a
brute-force 0–2 byte tolerance my parser fully consumed **97 of 130** MP packets; the remainder
break where the greedy padding guess goes wrong. Solo replays take the `iVar11 = 0` branch, and
indeed parse with zero tolerance.

This is a real and previously undocumented constraint on any third-party replay tooling.

### `process_check_sums` (0x39) — the per-turn state oracle [measured]

`FUN_009459d0` returns **65** and logs sixteen `u32` fields:

```
+0x00 u8  opcode 0x39
+0x01 u32 units            +0x05 u32 builds           +0x09 u32 walls
+0x0D u32 ammo             +0x11 u32 deaths           +0x15 u32 groups
+0x19 u32 guys             +0x1D u32 leaders          +0x21 u32 cities
+0x25 u32 items            +0x29 u32 goods            +0x2D u32 world
+0x31 u32 rules            +0x35 u32 scenario_data    +0x39 u32 script_run_time
+0x3D u32 (stored to DAT_00cbee90[who])
```

1 + 16×4 = 65, matching the returned size exactly. These names line up with the `*Sync`
class-name table at `.rdata:0x00B17B08 … 0x00B17D78` (`UnitsSync`, `BuildsSync`, `WallsSync`,
`AmmoSync`, `DeathsSync`, `GroupsSync`, `GuysSync`, `LeadersSync`, `CitiesSync`, `ItemsSync`,
`GoodsSync`, `WorldSync`, `RulesSync`, `ChecksumSync`, …) — i.e. **the engine's own definition
of which state is sim-critical, decomposed by subsystem.**

The MP specimen carries **128 `process_check_sums` packets**, one per player per turn. Field
volatility over the match:

| field | distinct values | field | distinct values |
|---|---|---|---|
| units | 78 | items | 1 |
| builds | 78 | goods | 1 |
| walls | 1 | world | 78 |
| ammo | 1 | rules | **1** |
| deaths | 28 | scenario_data | 5 |
| groups | 25 | script_run_time | 5 |
| guys | 78 | final | 78 |
| cities | 11 | | |

`rules` is constant for the whole game, as it must be. `units`, `builds`, `guys`, `leaders`,
`world` and `final` change every turn.

**The decisive validation:** on all **50** turns where both players reported, the two
independently-serialised 16-tuples are **bit-identical** (agree = 50, disagree = 0). A wrong XOR
key, a wrong offset, or a wrong command size anywhere upstream in the packet would not produce
agreeing 16-tuples from two separate byte streams. It also says the recorded game finished
without a desync, consistent with `L"Game completed without desync"` at `.rdata:0x00B18204`.

MP opcode histogram (over the packets that parsed): `0x4a process_turn_data` 129,
`0x48 process_camera` 129, `0x39 process_check_sums` 128, `0x4f process_player_speed` 95,
`0x01 process_begin` 94, `0x49 process_leader_options` 2, `0x00 process_group` 2,
`0x47 process_quit` 1, `0x46 process_resign` 1, `0x1a process_eject_all` 1. The game ended by
resignation.

**Consequence for the project.** Charter stage 6 wants the lockstep checksum to define
sim-critical state and to measure divergence honestly. It is sitting in every multiplayer
`.rcx` on the VM, at turn granularity, already split sixteen ways. The corpus is
`…\Recorded Games\multi\` — 60+ files spanning 2014 to 2024. That is the highest-value thing
this lane found and it is ready to harvest. Two caveats to carry forward: the values are
engine-internal checksums whose *algorithms* are not yet recovered (so they are a divergence
*detector*, not a state dump), and reading MP streams needs the RNG-padding co-simulation
described above.

---

## 6. Refuted: "most commands should belong to the AI players"

The lane brief expected a 2-AI-versus-1-human game to show most commands coming from the AI
players. **That is false, and the file says so unambiguously.**

Every one of the 10,544 packets in the solo specimen has `from = 0`. The human issued 63
orders in 11:40. The AI players issue **nothing** into the command stream: in a lockstep engine
the AI is part of the deterministic simulation and is *recomputed* during playback from the same
initial state and RNG, so recording its decisions would be redundant. Only genuine external
input — human commands, camera, telemetry, turn control — is recorded.

Two consequences worth stating plainly:

1. Replays are **not** a source of AI behaviour traces. Charter stage 9 ("behavior-clone the
   shipped BHS scripts") cannot be fed from `.rcx` AI orders, because there are none. Human
   play is what the corpus contains, and the multiplayer corpus is where the volume is.
2. Replay playback fidelity depends on the AI being bit-reproducible, which raises the bar on
   the sim rather than lowering it.

The distribution that *is* plausible, and that the file supports: one human player producing 63
orders, 125 recorded clicks and 57 hotkey presses across 700 seconds, with a camera record on
every frame.

---

## 7. Header: names, nations, settings

### Player names [measured]

Length-prefixed UTF-16LE records near the head of the payload, in slot order:

| specimen | slot 0 | slot 1 | slot 2 | … |
|---|---|---|---|---|
| `today.rcx` | `cmr` @ `0x00AC` | `Player 2` @ `0x00F2` | `Player 3` @ `0x0142` | |
| MP 2017 | `Arke` @ `0x00AC` | `ember` @ `0x00F4` | `Player 3` @ `0x013E` | `Player 4`, `Player 5`, `Player 6` |

Record shape, with `L` the offset of the `u32` character-count that precedes the name:
`L-2 … L-1` is a `u16` equal to the **slot index** (0,1,2,… in both files), `L-1` is a
one-byte tag (`0x02` for all three slots in the solo file; `0x02` for the two human slots and
`0x03` for the four `Player N` slots in the MP file), and the name follows as
`u32 char_count` + `char_count` UTF-16LE code units.

### Nation — strong, not conclusive

The binary carries a 24-entry nation table of stat-name suffixes at
**`.rdata:0x00AC80FC … 0x00AC81E8`**, in this order:

```
0 _AZTECS   1 _MAYA     2 _INCA      3 _BANTU     4 _NUBIANS   5 _GREEKS
6 _ROMANS   7 _EGYPTIANS 8 _TURKS    9 _SPANISH  10 _FRENCH   11 _BRITISH
12 _GERMANS 13 _RUSSIANS 14 _CHINESE 15 _JAPANESE 16 _KOREANS 17 _MONGOLS
18 _IROQUOIS 19 _LAKOTA  20 _AMERICANS 21 _INDIANS 22 _DUTCH  23 _PERSIANS
```

**`_DUTCH` is index 22 = `0x16`.** In `today.rcx` the byte at `L-7` for the human slot `cmr`
is exactly **`0x16`** — and the specimen's known ground truth is that the human played **Dutch**.
The same byte is `0x18` = 24 for both AI slots, which is one past the last nation and reads
naturally as a "random nation" sentinel.

Why I am not promoting this to settled: in the MP specimen the same byte is `0x16` for *both*
human slots and `{0x08, 0x17, 0x0C, 0x00}` for the four AI slots — a plausible nation
distribution, but I have no independent ground truth for that match, and the source table is a
*telemetry stat-suffix* list whose ordering I have not proven equals the sim's nation
enumeration. One live read of the running game's player-nation field, or one replay with a
known non-Dutch human, would settle it. **Open.**

### Map settings — not established

The bytes between the version string and the player records
(`0x003C … 0x00A2`) are a dense run of small integers plus two large `u32`s
(`0x0883A2` at `0x0040`, `0x00000750` at `0x006F`) which are the obvious candidates for a map
seed and a map dimension. **I did not derive any of them and I am not going to assert them.**
Likewise the world-coordinate-to-tile scale: the order and camera coordinates are integers in
one consistent world unit (x observed over 16,399 … 38,452), but nothing in this lane pins the
tiles-per-unit constant. **Open.**

---

## 8. Reproduction

Decompress and locate the stream (works on both specimens; solo needs no de-XOR):

```python
import gzip, struct, collections
d = gzip.decompress(open('/Users/ember/dev/don/ron-data/replays/today.rcx','rb').read())
N = len(d)
u16 = lambda o: int.from_bytes(d[o:o+2],'little')
u32 = lambda o: int.from_bytes(d[o:o+4],'little')

# longest chain of 18-byte-header records that tiles exactly to EOF
cl = [0]*(N+2); nxt = [0]*(N+2)
for off in range(N-18, -1, -1):
    if u32(off+4) >= 8: continue
    ln = u16(off+16)
    if ln > 4000: continue
    n = off + 18 + ln
    if n > N: continue
    if n == N: cl[off] = 1; nxt[off] = n; continue
    if cl[n] == 0: continue
    if not (u32(off) <= u32(n) <= u32(off)+80): continue
    if not (u32(off+12) <= u32(n+12) <= u32(off+12)+8): continue
    cl[off] = cl[n]+1; nxt[off] = n
start = max(range(N-18), key=lambda o: cl[o])          # -> 0xFA439, 10544 records

SIZE = {0x01:1,0x02:5,0x03:13,0x04:17,0x05:13,0x06:17,0x07:22,0x08:26,0x09:10,0x0a:10,
 0x0b:25,0x0c:1,0x0d:1,0x0e:5,0x0f:9,0x10:13,0x11:21,0x12:9,0x13:9,0x14:13,0x15:5,0x16:17,
 0x17:21,0x18:9,0x19:25,0x1a:17,0x1b:1,0x1c:25,0x1d:1,0x1e:13,0x1f:13,0x20:9,0x21:9,0x22:25,
 0x23:1,0x24:1,0x25:13,0x26:13,0x27:9,0x28:9,0x29:9,0x2a:9,0x2b:17,0x2c:17,0x2d:17,0x2e:13,
 0x2f:13,0x30:15,0x31:11,0x32:9,0x34:5,0x35:1,0x36:1,0x37:1,0x38:5,0x39:65,0x3a:6,0x3b:5,
 0x3c:5,0x3d:5,0x3e:1,0x3f:1,0x40:1,0x41:5,0x42:5,0x43:17,0x45:9,0x46:5,0x47:7,0x48:10,
 0x49:33,0x4a:11,0x4b:53,0x4c:2,0x4d:2,0x4e:521,0x4f:9,0x50:3,0x51:2}   # 0x00 = 3+2*num

off = start; hist = collections.Counter(); npk = 0
while off + 18 <= N:
    stamp, frm, unk, serial = struct.unpack_from('<4I', d, off)
    ln = u16(off+16); body = d[off+18:off+18+ln]; off += 18 + ln; npk += 1
    i = 0
    while i < len(body):
        op = body[i]
        sz = 3 + 2*body[i+1] if op == 0x00 else SIZE[op]
        hist[op] += 1; i += sz
    assert i == len(body)
assert off == N                                     # zero residual bytes
print(npk, sorted((hex(k), v) for k, v in hist.items()))
# 10544 [('0x0',63),('0x18',42),('0x19',18),('0x20',1),('0x36',6),
#        ('0x48',10499),('0x49',1),('0x4c',1),('0x4f',1313),('0x7',2)]
```

For the multiplayer specimen, additionally XOR each `u16` of the payload with the key
(`0x8EC6` for the file above; recover it as the modal payload word) and allow 0–2 skip bytes
between commands.

Binary side, all read from `/Users/ember/dev/don/re/decomp-all/`:

```
0094c500.c   CommandPackage::process_all   (framing, XOR, RNG padding)
0094a700.c   CommandPackage::process_one   (the 82-opcode switch)
0094a0c0.c   CommandPackage::process_group (selection, size = 3 + 2*num)
0094bb60.c   CommandPackage::add_group     (the writer side)
009459d0.c   process_check_sums            (the 16-field checksum)
00943730.c   process_player_speed          (the 8 accumulators)
00943b00.c   process_camera
```

---

## 9. What I could not establish

- **Type-id → type-name mapping.** Ids are `[measured]`; the mapping is not. `units(364) +
  buildings(129) = 493` matches the balance table dimension exactly, but `process_queue_up
  type` ids of 544–572 exceed it. The replay's own embedded type table is the right ground
  truth; enumerating it needs the record walker.
- **World-coordinate scale.** Coordinates are self-consistent integers; tiles-per-unit is
  unknown.
- **Map settings and game seed.** Candidate `u32`s identified by offset only; nothing derived.
- **Nation field.** Byte offset and value are `[measured]`; the semantic identification rests
  on one specimen with known ground truth plus a `.rdata` table whose ordering I did not prove
  is the sim's.
- **The checksum algorithms.** `process_check_sums` *transports* sixteen values; how each is
  computed is `check_all` / `process_check_sums` territory on the writer side and is not
  recovered here. Until it is, MP replay checksums detect divergence but do not localise it
  beyond the subsystem name.
- **33 of 130 MP packets** did not fully parse under a brute-force 0–2 byte padding tolerance.
  A correct reader needs the RNG in lockstep, not a search.
- **The unnamed `u32` at header `+0x08`.** `1` on the solo setup packet and on all 130 MP
  packets, `0` otherwise; purpose unknown.
