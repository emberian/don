# The wire → end-game hook (`global_stages: victory_endgame`)

Recovery lane: `victory-endgame`, 2026-08-11.  **Tier C** for everything it executes: the
retail bodies come from `docs/mechanics/player-lifecycle-command-tails.md` (lane `op-life`)
and `crates/don-sim/src/systems/victory_score.rs` (lane `mech:victory-score`), both
instruction-derived from the supported PE32 `riseofnations.exe`
(SHA-256 `30478a44…625079`).  Nothing here was executed against retail; the tests are
transcription pins driven through the real tick, not a differential run.

This lane derives almost no new retail behaviour.  What it adds is **execution**: the state
that could be planned but never committed now commits, and a match can end.

## 1. The gap that was closed

`docs/mechanics/player-lifecycle-command-tails.md` §"What is not claimed":

> **`Leader::defeat` `0x006ECB00`** — `victory_score::Leaders::defeat` already implements it
> completely […].  It is not executed here because neither `Fleet` implementor
> (`command::ObjectTable`, `don_env::EnvWorld`) owns a `Leaders`/`Match`.  This is the
> single missing hook between the wire and the `victory_endgame` global stage.

`tick::Sim` owns both — `Sim::vic_leaders` and `Sim::vic_match`.  The host is
`crates/don-sim/src/tick.rs -> #[path] pub mod lifecycle_host`
(`crates/don-sim/src/systems/lifecycle_host.rs`), a child of `crate::tick` so it can reach
`Sim`'s private terminal-cleanup drain the way step 11 does.  `systems/mod.rs` is untouched.

The pair asked for:

```rust
Sim::tail_command_facts(&self, request: &TailCommandRequest) -> TailCommandFacts
Sim::apply_tail_command_transaction(&mut self, request: &TailCommandRequest) -> SimTailReceipt
```

`apply_tail_command_transaction` is the **command-pump entry**, not a `Game::do_frame` step.
Retail runs `CommandPackage::process_*` from the turn pump; that is outside the 29 entries of
`DO_FRAME`, which is why `tick.rs` marks steps 0..3 `OutOfScope`.  Nothing was invented into
the frame schedule to make this run.

## 2. The state join

`LifecycleImage` is assembled from live `Sim` channels:

| `LifecycleImage` field | retail | `Sim` source |
|---|---|---|
| `players[8]` | `GameInfo::player[8]`, `Game+0x44`, stride `0x8C` | `Sim::players` (new) |
| `leaders[i].leader_flags` | `LeaderData+0x00` | `vic_leaders.slots[i].leader_flags` |
| `leaders[i].who` | `LeaderData+0x08` | `vic_leaders.slots[i].who` |
| `leaders[i].multi_diff` | `LeaderData+0x50` | `vic_leaders.slots[i].multi_diff` (new field) |
| `leaders[i].diplos[8]` | `LeaderData+0x74` | `vic_leaders.slots[i].diplos` |
| `leaders[i].lost_capital_timer` | `LeaderData+0x418` | `vic_leaders.slots[i].lost_capital_timer` |
| `team_style` | `GameInfo+0x18` = `Game+0x24` | `vic_match.options.team_style` |
| `elimination` | `GameInfo+0x2B` = `Game+0x37` | `vic_match.options.elimination` |
| `semaphore[32]` | `Game::semaphore.ptr`, `Game+0x820` | `vic_match.semaphore.to_le_bytes()` |
| `playing` | `Game::playing`, `Game+0x558` | `PlayerTable::playing` |
| `semaphore_flags` | `Game::semaphore.flags`, `Game+0x81C` | `PlayerTable::semaphore_flags` |
| `console_play` / `console_who` | `Console+0x2A0` / `+0x298` | `PlayerTable` |
| `drop_window_open` | `DropControl+0x94` | `PlayerTable` |

Two notes worth not re-deriving:

* **`Match::semaphore` and `BitMask<256>::ptr` index identically.**  `Match::semaphore` is a
  `u32` tested as `1 << bit`; `LifecycleImage::sem` reads byte `bit / 8`, mask
  `1 << (bit % 8)`.  Little-endian `to_le_bytes` makes those the same function for bits
  0..32, which covers every bit these bodies touch (2, 4, 6, 15, 18).  A plan that wrote a
  bit ≥ 32 would be state `Match` cannot hold and is refused
  (`LifecycleHostError::SemaphoreBitOutOfRange`), never truncated.
* **`Game::semaphore.flags` is outside checksum channel `Game`.**  `Game::walk_data`
  `0x00589600` walks `[0x550, 0x6E4)` then `[0x814, 0x81C)`.  `BitMask<N>` is
  `bits +0 / size +4 / flags +8 / ptr +0xC`, so the second range stops one dword *short* of
  `flags`.  That is why `semaphore_flags` lives on `PlayerTable` rather than on
  `victory_score::Match`.

`Sim::players` is `Option<PlayerTable>` and defaults to `None`.  With no table,
`tail_command_facts` returns `TailCommandFacts::NoExternalFacts` and rows 70/71/80 keep
op-life's whole-row boundary exactly as before.  The host is opt-in and fail-closed.

## 3. The row-71 prefix, and why it is pinned

`tail_command_transactions::plan_tail_command`'s Quit arm applies
`CommandPackage::process_quit` `0x00943A7B..0x00943AA6` — clear semaphore bit 15; if the
flags dword is 0 write 2; set bit 18; write flags = 0 — to a **private copy** of the image
before calling `plan_lifecycle`, and does not emit it as a `TailEffect`.  It is therefore
invisible in the effect list, and a host that wants to commit row 71 has to reproduce it.

This host does, in `quit_handler_prefix`, and then **pins** it: it replans the lifecycle body
from its own prefixed image and refuses the entire transaction unless the result is
bit-identical to the plan the row planner carried
(`SimTailError::QuitPrefixDisagreement`).  A silent divergence between the two
transcriptions is not reachable; the mutation sweep in §6 confirms it.

## 4. What commits, and what still refuses

`SimTailReceipt::outcome` is `Committed` or `Refused`.  Every refusal is computed **before**
the first mutation; `SimTailReceipt::validates()` replans the whole row from the recorded
facts and, for a commit, defers to op-life's own `LifecycleReceipt::validates`.

Committing today:

| row | arm | result |
|---|---|---|
| 70 Resign | ordinary | `Player::resign` → `leave_game(6)` → `Leader::defeat(6, -1, 0)` → `Game::check_victory` |
| 70 Resign | co-tenant under semaphore bit 2 | player bits set, **no** defeat, match continues |
| 71 Quit | local, `replay != 0` | handler prefix + `Player::quit(0)` + `Game::playing = 0` + defeat |
| 80 Drop | state 3 under semaphore bit 4 | `leader_flags &= ~HUMAN`, `multi_diff = 3`, no defeat |
| 80 Drop | any state outside semaphore bit 4 | the measured diagnostic-only no-op |

Refusing, with the exact retail function named:

| refusal | reached by | why |
|---|---|---|
| `LifecycleHostError::UnexecutableCall(LeaderActionDeclare)` | drop states 1 and 2 | `Leader::action_declare` `0x006DAB50` is command row 38's open tail |
| `LifecycleHostError::Boundary(FindCapitalForDefeat)` | `leave_game` under `GameInfo::elimination == 1` with a running `lost_capital_timer` | `LeaderData::find_capital` `0x006EB930` — see §5 |
| `SimTailError::UnsupportedRow` | rows 73, 78 | `Sim` owns no `LeaderOptionDataState` store and no console parser |
| `SimTailError::NoPlayerTable` | any row | no `GameInfo::player[8]` image installed |
| `LifecycleError::ConsoleWhoOutOfRange` | any **remote** departure with `Console::who == -1` | `departure_sound` indexes `leaders[Console::who]`; a host with no console cannot take the remote arm at all |

That last one is a real constraint on headless use and is not a defect: retail always has a
`Console`.  A host that wants remote resigns must seat one.

## 5. `LeaderData::find_capital` `0x006EB930` — decoded, deliberately not ported

Recorded so the next lane does not re-derive it.  Ghidra `re/decomp-all/006eb930.c`, read
against the layout facts already in tree:

```
void LeaderData::find_capital(int* out_city, int* out_who, int skip_city, int skip_who)
```

* `PTR_DAT_00c061d4` is a `PtrArray<City>[8]`, stride `0x1C`, element pointer at `+0x10` —
  the same shape as the `Units units +0x10` / `PtrArray<BuildType> +0x10` finding the
  tick12 lane posted.  `LeaderData+0x408` is that leader's **city count**.
* `PTR_DAT_00c061e0` is `GameAccessConst::leadersc`, stride `0x6EEC` (`0x1BBB` ints), which
  op-life already named.
* Pass 1 walks this leader's own cities and returns the first with
  `City+0x04 & 1` (valid) **and** `City+0x04 & 0x10` set, skipping `(skip_city, this->who)`;
  it writes `*out_who = this->who`.
* Pass 2 walks all eight leaders, skipping `this->who`, and returns the first city of that
  leader with `City+0x04 & 1` and `City+0x68 & (1 << (this->who & 0x1F))`, skipping
  `(skip_city, skip_who)`.
* Fallthrough writes `*out_city = -1`, `*out_who = this->who`.

**Not ported, for two reasons.**  (a) `Sim` owns no City band: there is no store with
`City+0x04` flags or the `City+0x68` owner mask, so a planner here would be a module nothing
could call.  (b) The decompiler's pass-2 guard reads `leaders[this->who].leader_flags & 3`
from an address computed **once before the loop** — loop-invariant, which is suspicious for
a per-candidate test and has not been confirmed against capstone.  Porting it on that basis
would be inventing a branch.  `LifecycleBoundary::FindCapitalForDefeat` stays a boundary.

The arm is unreachable in a Conquest-elimination match, which is the default, so this does
not block the ordinary end-game path.

## 6. Gates

`git rev-parse HEAD` **does not build** while this was written — `crates/don-sim/src/command.rs`
declares `pub mod air_containment_host;` and `pub mod economy_group_actions;` and imports
`crate::systems::hotkey_group_action`, none of whose files are committed, so both
`swarm-cargo-remote` (which fetches the commit) and `git archive HEAD` produce a tree that
cannot compile.  The gate below is therefore against commit `6f6e8f2` — the last
self-consistent commit — plus the two untracked module files that commit needs, plus this
lane's four:

```sh
git archive 6f6e8f2 | tar -x -C <tree>
cp crates/don-sim/src/systems/{air_containment_host,economy_group_actions}.rs <tree>/…
cp crates/don-sim/src/{tick.rs,systems/lifecycle_host.rs,systems/victory_score.rs} <tree>/…
cp crates/don-sim/tests/victory_endgame_wire.rs <tree>/…
cargo test -p don-sim --lib --test victory_endgame_wire
```

```
test result: ok. 1671 passed; 0 failed; 2 ignored     # --lib
test result: ok.    8 passed; 0 failed                # --test victory_endgame_wire
```

Every one of the 8 integration tests runs `Sim::do_frame`; 4 further unit tests live in
`lifecycle_host`.

### Mutation sweep

8 seeded edits, 7 killed, 1 equivalent.

| # | seeded edit | observed |
|---|---|---|
| 1 | `commit` ignores `SetSemaphoreBit::on` and always sets | `a_quit_commits_…` — `assertion failed: !sim.vic_match.sem(SEM_LOCAL_LEFT)` |
| 2 | `quit_handler_prefix` drops its final `semaphore_flags = 0` | **survived** — see below |
| 2b | `quit_handler_prefix` *sets* bit 15 instead of clearing it | `a_quit_commits_…` fails at the `committed()` assertion: the pin fired `QuitPrefixDisagreement` |
| 3 | `preflight` lets every `LifecycleCall` through | `the_drop_states_that_declare_war_refuse_…` — panics on `unreachable!("unexecutable call reached commit")` |
| 4 | `commit_quit_handler_prefix` drops `set_sem(SEM_QUIT_PREFIX)` | `a_quit_commits_…` — `assertion failed: sim.vic_match.sem(SEM_QUIT_PREFIX)` |
| 5 | `PlayerTable::image` leaves the semaphore bytes zero | 3 failures: the drop row loses its `flags & 0x10` gate, and the co-tenant scan stops running |
| 6 | drop `self.flush_terminal_queue_cleanup()` | `a_resign_drains_…` — `left: 1, right: 0` on the residual cleanup mask |
| 7 | `DefeatType::from_i32` maps 6 to `Conquest` | 2 failures — `left: 0, right: 6` on `LeaderData::defeat_type` |
| 8 | `carried_lifecycle` ignores the `Apply` arm's `TailEffect::PlayerLifecycle` | 2 failures — the whole state-3 drop and the co-tenant resign commit nothing |

**Mutation 2 is a genuine equivalent mutant, and worth recording.**  The row-71 prefix's
final `Game::semaphore.flags = 0` store (`0x00943AA1`) is unobservable on the reachable
path, because `Player::resign`'s local arm writes `semaphore.flags = 0` unconditionally a
few instructions later and `Player::quit` then rewrites it to 2 from the `== 0` test.  No
pin and no test can distinguish the two; mutation 2b shows the pin does fire for a prefix
divergence that the plan actually sees.

## 7. How far `victory_endgame` actually got

**A real game can now reach a real end state through the real path**: a decoded opcode-70
packet → `Player::resign` → `Player::leave_game` → `Leader::defeat` `0x006ECB00` →
`Game::check_victory` `0x005926B0` → `game_sem::GAME_OVER` + `game_sem::VICTORY_RESOLVED` →
`Sim::do_frame` step 27 `Game::process_end_game` `0x00591CE0` consumes the latch, once.
The same path already existed from inside the tick (Wonder, Tech Race, territory, armageddon,
musical chairs, via step 12 `GameDaemon::process_victory`); what was missing was every
*player-initiated* ending, which is how most real matches actually finish.

Still open, and why the stage is **not** complete:

1. **Score is not the retail score.**  `Leader::compute_pop_score` and
   `compute_explore_score` are retail stubs and `compute_combat_score` /
   `compute_wonder_score` are empty `ret`s, so those are faithful — but the totals feed off
   `LeaderData::num_units` / `num_buildings` / `num_queued` / `territory` / the encrypted
   economy buckets, and several of those are populated by steps this tree still counts as
   gaps.  The scoring *arithmetic* is ported; the *inputs* are not all live.
2. **Two of step 11's five children are still call-counted gaps**:
   `Gap::LeaderPlanStrategy` and `Gap::LeaderDiplomacy` (20,348 B).  `check_explore`,
   `compute_score` and `check_victory` execute.
3. **The end-game transition is the simulation half only.**  `Game::process_end_game`
   `0x00591CE0` also runs statistics, the leaderboard and the menu; `Match::process_end_game`
   consumes the latch and deliberately stops there.  That is a product boundary, not a
   simulation one, but the stage description says "end-game transition".
4. **Drop states 1 and 2 cannot commit** until command row 38 (`Leader::action_declare`
   `0x006DAB50`) lands, so a multiplayer drop-vote that dissolves teams is still refused.
5. **The capital-elimination ending is a boundary** (§5).
6. `Leader::victory`'s and `Leader::defeat`'s presentation envelopes (message window,
   notices, sound categories, `GameLog::create_report`) are emitted as ordered
   `LifecyclePresentation` records and rendered by nobody.

## 8. Finding for `op-life` (their file, not touched)

`tail_command_transactions::decide_lifecycle` takes a `suffix` argument and appends it only
on the `Apply` arm; the `Boundary` arm **drops it**.  The only current suffix is
`TailPresentationReceipt::SystemQuitCallback`, and row 71 always reaches the `Boundary` arm
whenever the quit actually defeats someone — i.e. the product exit callback is silently lost
in exactly the case that matters.  It is presentation-only, so nothing simulation-visible
changes, and this lane did not work around it by re-deriving the predicate.
