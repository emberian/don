# Player lifecycle command tails (rows 70, 71, 80)

Recovery lane: `op-life`, 2026-08-11.  **Tier C**, instruction-derived from the supported
PE32 executable `riseofnations.exe` (SHA-256 `30478a44…625079`) and `rise.pdb`.  Nothing in
this lane was executed against retail; the tests are transcription pins, not a differential
run.

Motivation: retail's drop-vote screen was observed live on 2026-08-10 when a DoN peer dropped
out of a hosted match, so opcode 80 is reachable in practice.  The three rows that reach the
`Player::*` lifecycle previously stopped at an unrecovered callee and could authorize no
state at all.

| retail | VA | size | recovered |
|---|---|---:|---|
| `Player::leave_game(int)` | `0x006EE010` | 256 | complete |
| `Player::resign(int)` | `0x006EDCB0` | 459 | complete |
| `Player::quit(int)` | `0x006EDC00` | 174 | complete |
| `Player::drop(void)` | `0x006EDE80` | 397 | complete |
| `DropControl::process_drop(int, int)` | `0x00959500` | 933 | complete |

Implementation: `crates/don-sim/src/systems/player_lifecycle_tails.rs`, mounted as
`command::tail_command_transactions::lifecycle`.  Pins:
`crates/don-sim/tests/player_lifecycle_tails.rs` (23 tests) and the new
`TailCommandFacts::PlayerLifecycle` arms in
`crates/don-sim/tests/tail_command_transactions.rs` (14 tests).

## Layout corrections this lane depended on

`Game::semaphore` is a `BitMask<256>` at `Game+0x814`.  `BitMask<N>` is
`bits:int +0`, `size:int +4`, `flags:int +8`, `ptr:unsigned char[…] +0xC`.  Therefore:

* the "flags dword" written as 0 or 2 by `Player::quit` and by
  `CommandPackage::process_quit` is `Game::semaphore.flags` at `Game+0x81C`;
* the semaphore bit bytes begin at `Game+0x820`, so `Game+0x820 & 0x04` is bit 2,
  `& 0x10` bit 4, `& 0x40` bit 6, `Game+0x821 & 0x80` bit 15, `Game+0x822 & 0x04` bit 18.

`GameInfo` is at `Game+0x0C`, so `GameInfo::team_style` is `Game+0x24`,
`GameInfo::elimination` is `Game+0x37` (the byte `victory_score` already reads) and
`GameInfo::player[8]` is `Game+0x44` with stride `0x8C`.  A `.text` expression of the form
`[[0xC061EC] + 0x74 + i*0x8C]` is `players[i].flags`, **not** a separate `Game` array — the
decompiler renders it as a `Game`-relative byte index and it is easy to overread.

`Player` fields used: `flags:u16 +0x30`, `who +0x33`, `team:i8 +0x34`, `play +0x36`.
`LeaderData` fields used: `leader_flags +0x00`, `who +0x08`, `multi_diff +0x50`,
`diplos[8] +0x74`, `lost_capital_timer +0x418`.  `Console::who +0x298`, `Console::play
+0x2A0`.

## `Player::leave_game(int reason)`

1. `players[this].flags |= 0x10`.
2. **Only** under semaphore bit 2 (`0x006EE022`, re-tested at `0x006EE047` after the
   telemetry `String` copy), scan the eight player rows for a *co-tenant*: another slot with
   `flags & 0x04`, `flags & 0x01`, `!(flags & 0x100)`, `!(flags & 0xD0)`, a different slot
   index, and the same `Player::who`.  A hit returns with **no defeat** — the leader is still
   held.
3. If `(leaders[who].leader_flags & 3) == 3` **and** `LeaderData::lost_capital_timer != 0`,
   the leave reason is discarded and retail defeats as `DEFEAT_CAPITAL` (1):
   * `GameInfo::elimination == 1` → `arg` is the second out-parameter of
     `LeaderData::find_capital(&a, &b, -1, -1)` `0x006EB930`;
   * otherwise `arg` is `LeaderData::who`.
4. Otherwise `Leader::defeat(reason, -1, 0)`.

`reason` is 6 from `Player::resign` and 7 from `Player::drop`, which are exactly
`victory_score::DefeatType::Resign` and `::Disconnect`.

## `Player::resign(int from_quit)` — opcode 70

`CommandPackage::process_resign` `0x009438C0` logs and calls `Player::resign(0)` on
`Game+0x44 + play*0x8C`, then returns wire length 5.

`Player::resign` itself:

1. `players[this].flags |= 0x40`.
2. **Local** (`Player::play == Console::play`): category is `0x80` when `from_quit == 0` and
   `0x153` otherwise, and the entire message/notice/diplomacy block is skipped.
3. **Remote**: `Player::name_with_platform_symbol` `0x006EE1D0`, `MessageWin::add_message`
   `0x007E9FB0` with the `[0x00C8CD00]` record `0xF104 / 20 = 3085`; then, unless semaphore
   bit 6 is set (and subject to two product globals), `IFaceMainBase::do_notice` `0x008107D0`
   with internal-string record `0x1A3EC / 20 = 5375`.  Category is `7` when `Console::who`
   equals the departing `LeaderData::who` **or** the two are mutually allied
   (`leaders[who].diplos[console_who] == 2` and `leaders[console_who].diplos[leader_who] == 2`,
   the reverse read going through `GameAccessConst::leadersc` `0x00C061E0`), else `0x22`.
4. `SoundGlobal::play(category)`, then `leave_game(6)`.
5. Only for the local player: semaphore bit 15 set and `semaphore.flags = 0`.

## `Player::quit(int force_stop)` — opcode 71

`CommandPackage::process_quit` `0x009439A0` decodes `play:i32`, `replay:u8`,
`system_quit:u8` (wire length 7) and runs three things that must commit together:

1. **prefix**, when `play == Console::play && replay != 0` (`0x00943A7B..0x00943AA6`): clear
   semaphore bit 15; if `semaphore.flags == 0` write 2; set semaphore bit 18; then
   unconditionally write `semaphore.flags = 0`.
2. `Player::quit(0)`.
3. **suffix**, when `system_quit != 0 && play == Console::play && !(semaphore bit 4)`:
   `[0x00E335C4] = 1` and the `Console` virtual slot at `+0xC4`.  A product exit callback.

`Player::quit` samples semaphore bit 15 **before** calling `Player::resign(1)` — which may
set it — and then:

* sampled set → set bit 15, `semaphore.flags = 0`;
* sampled clear → clear bit 15, and if `semaphore.flags == 0` write 2.

Then, if the player is **not** local and semaphore bit 2 is set, it returns.  Otherwise
`Game::playing = 0` unless semaphore bit 4 is set with `force_stop == 0`, and
`GameLog::create_report` runs with internal-string record `0x1A400 / 20 = 5376`.

## `DropControl::process_drop(int play, int state)` — opcode 80

`CommandPackage::process_ungraceful_player_drop` `0x00943EA0` decodes `play:u8`, `state:u8`
(wire length 3), logs, and enters `DropControl` **only** under semaphore bit 4
(`Game+0x820 & 0x10`, `0x00943EFA`).

Prologue, once per `DropControl` instance (`DropControl+0x94 == 0`): `DropControl::clear`
`0x0095A100`, a log line at internal-string record `0xCD28 / 20 = 2626`, and
`MPDropWin::init` `0x0095A640` — this is retail's drop-vote screen.

**State 3** — the vote resolved, this player is gone:

* return immediately if `players[play].flags & 0x80`;
* run a co-tenant scan that is **not** the `leave_game` scan: it excludes the subject slot
  first and does **not** test `flags & 0x04`.  A hit returns.
* post the name and `[0x00C8CD00]` record `0x56F4 / 20 = 1113`;
* `players[play].flags &= 0xFFEB` (clears `0x04` and `0x10`);
* `leaders[who].leader_flags &= ~0x04` — that bit is
  `victory_score::leader_flag::HUMAN` — and `LeaderData::multi_diff = 3`.

`Player::drop` is **not** called.  The leader continues under AI control at difficulty 3.

**States 1 and 2** — the match continues without teams:

* `GameInfo::team_style = 0` (state 1) or `1` (state 2);
* every `flags & 1` player row gets `Player::team = 8` (the `TEAM_AUTO` sentinel
  `setup_diplomacy` already names);
* for every ordered pair of `leader_flags & 1` leaders `k < m`,
  `leaders[k].action_declare(m, 0, 1, 1)`;
* then `Player::drop()`.

**Any other state** goes straight to `Player::drop()`.

## What is not claimed

`DropControl::process_drop` `0x00959500` is byte-for-byte covered, but two callees are
emitted as typed authoritative calls rather than flattened, and a receipt only validates as
applied when the host acknowledges each in order:

* **`Leader::defeat` `0x006ECB00`** — `victory_score::Leaders::defeat` already implements it
  completely, including terminal queue and Unit cleanup and `Game::check_victory`.  It is not
  executed here because neither `Fleet` implementor (`command::ObjectTable`,
  `don_env::EnvWorld`) owns a `Leaders`/`Match`.  This is the single missing hook between the
  wire and the `victory_endgame` global stage; this lane does not claim that stage.
* **`Leader::action_declare` `0x006DAB50`** — command row 38's open tail, reached only by
  drop states 1 and 2.

and one arm stops at an unrecovered callee entirely:

* **`LeaderData::find_capital` `0x006EB930`**, reached only by `Player::leave_game` under
  `GameInfo::elimination == 1` with a running capital timer.  `LifecycleBoundary::FindCapitalForDefeat` carries it, and a boundary authorizes no mutation.

Rows 70, 71 and 80 therefore stay **closure-red**.  What changed is the shape of the refusal:
they now have a real `Apply` arm (a co-tenant keeps the leader alive; a state-3 drop already
gated; a non-network row-80 packet), and every other request names the exact retail function
that must run next instead of "the `Player::*` body is unrecovered".

## Findings for other lanes

* `Leader::action_declare(whom, treaty, no_payment, over)` skips **both** `afford_dow`
  `0x006D5CE0` (`0x006DABC7`) and `pay_dow` `0x006D2B10` (`0x006DACEC`) on the same
  `no_payment != 0` predicate.  It also **downgrades a war declaration to peace** when
  `Game::war_allowed` `0x00594670` returns zero and `over != 0`
  (`0x006DABFD..0x006DAC19`), re-entering the action spine with `treaty = 1`.  Row 38 should
  not re-derive this.
* `[0x00C8CD00]` is a *second* array of 20-byte `String` records, distinct from the
  internal-string array at `[[0x00C06378] + 0x10]`.  Every byte offset seen into it in this
  family (`0xF104`, `0xF0F0`, `0x56F4`, `0xB518`, `0xB414`, `0xB52C`) is an exact multiple of
  20.  Do not decode those against `internal_strings.xml`.

## Gates

```sh
# standalone, while `command.rs` is red under a sibling lane's migration
rustc --edition 2021 --test crates/don-sim/tests/player_lifecycle_tails.rs -o /tmp/plt && /tmp/plt

# full crate targets, off the Mac
tools/swarm-cargo-remote submit persvati op-life \
  --path crates/don-sim/src/systems/player_lifecycle_tails.rs \
  --path crates/don-sim/src/systems/tail_command_transactions.rs \
  --path crates/don-sim/tests/player_lifecycle_tails.rs \
  --path crates/don-sim/tests/tail_command_transactions.rs \
  -- test -p don-sim --test player_lifecycle_tails --test tail_command_transactions \
     --test command_tail_dispatch
```

23 + 14 + 2 tests green on `persvati` at base `371a473d`.  Mutation sweep: 23 seeded edits
(constants, branch predicates, loop bounds, dropped gates), 23 killed.
