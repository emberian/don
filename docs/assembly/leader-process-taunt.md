# `Leader::process_taunt` `0x006B8CC0` — step 8's last unported child

Lane: `taunt-body`. Closes `Gap::LeaderProcessTaunt`'s body on tick step 8
(`Leaders::process_all` `0x006ED2A0`). Read [`economy-step8.md`](economy-step8.md) first, and
[`wall-update-construct-time.md`](wall-update-construct-time.md) for the sibling child that
landed hours earlier and deferred this one.

**Tier C.** Structure, constants, branch order, **receivers** and rounding read from
`ron-bin/riseofnations.exe` (sha256 `30478a44…625079`) with radare2, cross-read against
`re/decomp-all/006b8cc0.c`, `ron-bin/sbl/rise.pdb` (`re/symtab.json`,
`schema/pdb-types.json`, `schema/symbols.json`) and `schema/rise-symbols.tsv`. **Nothing here
has been executed against retail and no oracle case was added.** The tests in
`crates/don-sim/tests/tick_step8_process_taunt.rs` drive the real `Sim::do_frame`; that makes
them integration tests of the port, not evidence about the game.

Step 8's `StepStatus` is **unchanged** (`Stub`). It has other charged children — automatic
Wall/base-Object query population — and this lane does not license flipping it.

---

## 1. The framing was wrong twice, and the second correction matters more

`crates/don-sim/src/tick.rs` calls this "the `Leader::process_taunt` AI-chat body". A sibling
lane corrected that to "a **resource transfer**, i.e. simulation state, above a `0x95`
threshold, calling `0x006D15E0`/`0x006D1780`/`0x006D03C0` with `amount/3`". That correction
is right about the important thing — this is checksum-visible economy, not presentation — and
it is wrong about two details and understates the function a third time.

| claim | verdict | why |
|---|---|---|
| "AI chat" | **false** | `0x006B94A4`'s five clamps and every `0x006B8F2E`..`0x006B9104` store run whether or not anyone is watching. |
| "cases 1–5 read the encrypted resource block (`^0x8221`)" | **confirmed** | `0x006B8E44`: `mov eax, [eax + 0xE41248]` (`leaders[who].data_encrypted`), `mov eax, [eax + ecx]`, `xor eax, 0x8221`. `0x00E41248 − 0x00E3A390 = 0x6EB8` = `LeaderData::data_encrypted`. |
| "above a `0x95` threshold" | **confirmed, with the sign** | `0x006B8E52`: `cmp eax, 0x96 ; jge`. **Signed** — a negative stockpile takes the refusal branch rather than wrapping into the transfer. |
| "with `amount/3`" | **confirmed, and it is a second read** | `0x006B8EF3` `mov eax, 0x55555556` / `imul` / `shr 31` / `add` is signed truncating `/3`. But the value divided comes from a **second** load at `0x006B8EEA`, after `action_clear_all` has run — and `action_clear_all` reaches `LeaderData::bucket_add` `0x0043ED10`, which writes the stockpile. |
| "a resource transfer" | **not at the call site** | `Leader::action_offer` `0x006D1780` moves **no** resources. It writes `dip[who].offers[res] += amount` and the exact negation into `leaders[who].dip[me].offers[res]` — a two-sided ledger. The stockpile moves inside `Leader::action_respond` `0x006D03C0` (3,988 B), which this port does **not** write. |
| "writing `Leader+0x354` / `Leader+0x374`" | **confirmed, and they are named** | `LeaderData::last_taunt[8]` (an `enum TauntRequest`) and `LeaderData::taunt_frame[8]`, per `schema/pdb-types.json`. Written at `0x006B8D46`/`0x006B8D54`, **before** the jump table, so even an unknown code stamps them. |
| "cases 1–5" | **understates it** | Codes 7–16 rewrite six AI build-priority scalars (`+0x794`..`+0x7A8`) and `Personality::raid` (`+0x6DEC`), then clamp five of them. That is *more* per-frame simulation state than the tribute arms touch, and it happens on every dispatch. |

So the honest one-line description is: **`Leader::process_taunt` is the AI's response to an
ally's taunt — a diplomatic tribute ledger for the five resource codes, an AI strategy-weight
rewrite for the nine build codes, and presentation for exactly one code.**

---

## 2. The body, exactly

`ECX` is the `Leader`, `[ebp+8]` is `kind` (`LeaderData::incoming_taunt[k]`, `+0x394`) and
`[ebp+0xC]` is `who` (`LeaderData::incoming_taunt_who[k]`, `+0x3B4`). The dispatcher pushes
`[edi]` then `[edi-0x20]` at `0x006ED3ED`/`0x006ED3F1`, so the *second* push is the first
parameter — the port's argument order is that, not the push order.

```text
me = this->who                                                     0x006B8CE0
if leaders[me].leader_flags & 4        return   ; HUMAN            0x006B8CEF
if !(leaders[me].leader_flags & 2)     return   ; ACTIVE           0x006B8CFB
if who != leaders[me].who:                                         0x006B8D0A
    if leaders[me].diplo[who] != 2               return            0x006B8D0E
    if leaders[who].diplo[leaders[me].who] != 2  return            0x006B8D29
if me == who                           return                      0x006B8D34
this->last_taunt[who]       = kind                                 0x006B8D46
this->taunt_frame[who]      = Game::frame                          0x006B8D54
switch (kind - 1) 0..0xF, else -> default                          0x006B8D5E
```

### Codes 1–5, the tribute (`0x006B8D89`..`0x006B8F2B`)

```text
res = [FOOD 0, TIMBER 1, WEALTH 2, METAL 4, OIL 5][kind-1]
if leaders[me ].type_avail(res, 1) != 4   return                   0x006B8D99
if leaders[who].type_avail(res, 1) != 4   return                   0x006B8DB6
locked = Game::teams_locked()                                      0x006B8DC4
if !locked || this->is_neutral():
    s = this->gift_stamp[who]
    if s != 0 && Game::frame - s < 0x1194:                         0x006B8DF3
        msg = loc[2553] ; chat_to_local(msg, who, 2, 1) ; return
v = leaders[me].data_encrypted->bucket[res] ^ 0x8221               0x006B8E44
if v < 0x96:                                                       0x006B8E52
    msg = loc[2554] ; msg = msg.parse(types[res]->name)
    chat_to_local(msg, who, 2, 1) ; return
if !locked || this->is_neutral():  this->gift_stamp[who] = frame   0x006B8ECD
this->action_clear_all(who)                                        0x006B8ED7
this->action_offer(who, res, (leaders[me].bucket[res] ^ 0x8221)/3) 0x006B8F0C
this->action_respond(who, 1)                                       0x006B8F16
return                                          ; no chat on this path
```

### Code 6, `TAUNT_NEED` (`0x006B9109`..`0x006B9261`) — presentation only

```text
best = 9999 ; pick = -1
for r in 0..6:
    if leaders[me].type_avail(r,1) == 0    continue                0x006B9130
    if leaders[who].type_avail(r,1) == 0   continue                0x006B9148
    if r == 3 (KNOWLEDGE)                  continue                0x006B9151
    v = leaders[me].bucket[r] ^ 0x8221
    if v <= best:  pick = r ; best = v      ; ties take the later r
if pick < 0                        return
id = [4, 5, 7, 0, 6, 8][pick]                                      0x006B95A0
if who != Console::who             return                          0x006B91C1
if id == 0                         return
msg = loc[2285] ; msg += taunts[Taunts::id_to_index(id)]->text
if !(player_profile[0x34] & 0x1000): chat_to_local(msg, who, 2, 1)
else:                                chat_to_local(msg, who, 2, 0)
                                     Taunts::play(id)
```

Note the predicate change: the tribute arms want `type_avail == 4`, this arm wants `!= 0`.

### Codes 7–16 and the default (`0x006B8F2E`..`0x006B954A`)

`x /: n` below is `sar` after a sign-bias `add`, i.e. C truncating division.

| code | name | effect |
|---|---|---|
| 7 | `TAUNT_BUILD_WONDER` | `wonder_mod = 1` |
| 8 | `TAUNT_BUILD_GROUND` | `ground <<= 5`, `sea /: 16`, `air /: 16`, `infra /: 4` |
| 9 | `TAUNT_BUILD_SEA` | `ground /: 16`, `sea <<= 5`, `air /: 16`, `infra /: 4` |
| 10 | `TAUNT_BUILD_AIR` | `ground /: 16`, `sea /: 16`, `air <<= 5`, `infra /: 4` |
| 11 | `TAUNT_BUILD_INFRA` | `ground /: 4`, `sea /: 4`, `air /: 4`, `infra <<= 2` |
| 12 | `TAUNT_RUSH` | `ground = sea = air = 0x200`, `infra = 0x80`, `defense = 0x80`, `pers.raid = 1` |
| 13 | `TAUNT_BOOM` | `ground = sea = air = 4`, `infra = 0x200`, `defense = 0x80`, `pers.raid = -2` |
| 14 | `TAUNT_ATTACK` | `defense = 0x40` |
| 15 | `TAUNT_DEFEND` | `defense = 0x400` |
| 16 | `TAUNT_HELP` | nothing; marks only |
| — | default (`ja`) | nothing; does **not** mark |

Then, for every one of those arms including the default:

```text
msg = EMPTY_STRING                                                 0x006B9267
if marked && who == Console::who:
    if kind != 16:
        roll = internal_random.get(0, 0xB)                         0x006B92A3
        msg  = loc[2555 + roll] ; id = 0x50 + roll
        roll == 8 is special: msg = msg.parse(leaders[me].get_name()), id stays 0
    if who == Console::who && msg.curr_len != 0:                   0x006B9460
        if id == 0 || !(profile & 0x1000): chat_to_local(msg, who, 2, 1)
        else:                              chat_to_local(msg, who, 2, 0)
                                           Taunts::play(id)
ground_mod  = clamp(ground_mod,  1,    0x8000)                     0x006B94A4
sea_mod     = clamp(sea_mod,     1,    0x8000)
air_mod     = clamp(air_mod,     1,    0x8000)
infra_mod   = clamp(infra_mod,   0x80, 0x8000)
defense_mod = clamp(defense_mod, 0x10, 0x1000)
```

`TAUNT_HELP` therefore reaches the length gate with an empty `msg` and emits nothing at all;
its only effect is the five clamps. `wonder_mod` is never clamped.

---

## 3. The ported callees

### `Leader::action_offer` `0x006D1780` (205 B), whole

```text
IFaceDiploNeg::check_click_stamp(this->who, who)          ; UI
if amount > 0 && leaders[me].dip[who].offers[res] + amount
                 > (leaders[me].bucket[res] ^ 0x8221):    0x006D17D8
     if me == Console::who: SoundGlobal::play(0x40)
     return
this->dip[who].any_offer = 1                              0x006D17F8   ** dead store **
this->clear_agree(who)                                    0x006D1803
leaders[who].clear_agree(me)                              0x006D1817
leaders[me ].dip[who].offers[res] += amount               0x006D1830
leaders[who].dip[me ].offers[res] -= amount               0x006D183F
```

Three things worth stating. `amount <= 0` **skips the affordability test entirely**
(`0x006D17A9` `jle` jumps into the apply arm), so a non-positive offer always lands. The
`any_offer = 1` store is **dead**: `clear_agree` two instructions later ends with
`dip[who].any_offer = 0` unconditionally. And the second `clear_agree` runs on
`leaders[who]`, not on `this` — `0x006D1811` loads `ECX = who * 0x6EEC + 0x00E3A390`.

### `Leader::action_clear_all` `0x006D15E0` (167 B), whole

`check_click_stamp`, then `this->clear_agree(who)` **and** `leaders[who]->clear_agree(me)`,
then the local-player `any_proposals` chat, then `Diplomacy::clear_all` on **both**
`leaders[me].dip[who]` and `leaders[who].dip[me]` (`0x006D166A`/`0x006D167B`).

### `Leader::clear_agree` `0x006D1AF0` (164 B), whole

```text
if this->dip[who].agree == 1:
    for r in 0..6:
        if dip[who].dows[r] != 0:                          ; note != 0, not > 0
            take = min(this->tributes[r], dows[r])
            this->tributes[r] -= take
            leaders[this->who].bucket_add(r, take)         0x006D1B3C
        if dip[who].offers[r] > 0:
            take = min(this->tributes[r], offers[r])
            this->tributes[r] -= take
            leaders[this->who].bucket_add(r, take)
    this->dip[who].clear_dows()
this->dip[who].agree     = 0                               0x006D1B84
this->dip[who].any_offer = 0                               0x006D1B87
```

`LeaderData::bucket_add` `0x0043ED10` is `bucket[r] = ((bucket[r] ^ 0x8221) + n) ^ 0x8221`
and a store of the plaintext to the scratch global `0x00CB195C`. This is the escrow refund,
and it is why the `/3` divides a value read *after* `action_clear_all`.

### `Game::teams_locked` `0x00594880` (29 B), whole

`GameInfo::team_style` (`Game + 0x24`) is none of `0`, `8`, `11`.

### `Diplomacy::clear_all` `0x0047E030` / `clear_dows` `0x0047E000`, whole

`clear_all` sets **`treaty = -1`**, not zero. `= Default::default()` is wrong here.

---

## 4. Four errors in `re/decomp-all/006b8cc0.c` and its callees

Ghidra's output for this family is unusually lossy — `006d03c0.c` has no parameters at all
(`unaff_EBP`, `extraout_ECX`) — and four of its errors are load-bearing.

1. **The two `LeaderData::type_avail` calls are printed as one repeated query.** Ghidra emits
   `iVar3 = FUN_006e33a0(iVar7,1);` twice. `0x006B8D89` sets
   `ECX = leaders[this->who]` and `0x006B8DA7` sets `ECX = leaders[who]`. The predicate is
   "both leaders have the resource enabled", which no reading of the C says. The same error
   appears in the `TAUNT_NEED` scan (`0x006B9120` vs `0x006B9139`).
2. **`action_clear_all`'s and `action_offer`'s second `clear_agree` is printed on `this`.**
   `FUN_006d1af0(*(undefined4 *)(in_ECX + 8))` reads as "clear my own record for myself"; the
   emitted `ECX` is `leaders[who]`. Refunding out of the wrong leader's `tributes` is a
   silent economy divergence.
3. **`clear_agree`'s `bucket_add` receiver is lost** to `in_ECX = extraout_EDX`. It is
   `leaders[this->who]`, while `tributes` is read off `this`. The two coincide only in a
   canonical array; `leaders.rs` already flagged the same `LeaderData::who`-vs-index trap for
   the step-8 diplomacy scan and for step 12.
4. **`FUN_00a39d70(0,0xb)` looks like a free function.** It is `Random::get` with
   `ECX = 0x00EB697C`.

---

## 5. `internal_random` is not `game_random`, and that is the determinism finding

`0x006B929E` is `mov ecx, 0xEB697C`. `schema/rise-symbols.tsv` line 36783 names
`0x00EB697C` `?internal_random@@3VRandom@@A`; the simulation stream is
`?game_random@GameAccess@@2AAVRandom@@A` at `0x00C06184` (`0x00E37A8C` as an object).

That draw sits **inside** a `who == Console::who` gate (`0x006B9284`). Had it been
`game_random`, every taunt aimed at the local player would have advanced the shared stream on
one peer only, and a lockstep match would desync on the next `Random::get`. It is not, so it
cannot — and the port must not "helpfully" route it through `World::random`.
`tick_step8_process_taunt::presentation_never_reaches_the_simulation_path` pins this by
running two `Sim`s that differ only in `Console::who` and the presentation die and asserting
`world.random.state()` is equal.

`README-LLM.md`'s standing warning is about ports that *skip* a `game_random` draw. This is
the mirror hazard: a port that *adds* one.

---

## 6. Localized strings, and which table they are in

`0x00C8CD00` is `loc_str_array_orig + 0x10` — the element pointer of a 24-byte `StringTable`
of 20-byte `String` records. Byte offset ÷ 20 is the ordinal. This is **not** the
`internal_strings.xml` array at `[[0x00C06378] + 0x10]` that the board's standing finding
describes, and decoding these offsets against `internal_strings.xml` gives wrong text.

| VA offset | ordinal | reached at | use |
|---|---|---|---|
| `+0xB284` | 2285 | `0x006B91DF` | `TAUNT_NEED` line |
| `+0xB504` | 2320 | `0x006D163E` | `action_clear_all` "proposals withdrawn" |
| `+0xC774` | 2553 | `0x006B8E04` | tribute refused, cooldown |
| `+0xC788` | 2554 | `0x006B8E63` | tribute refused, not enough `%1` |
| `+0xC79C`..`+0xC864` | 2555..2565 | `0x006B92C2`..`0x006B9443` | the eleven flavour lines |

The text itself is not recovered here. `String::parse` `0x00A1CD60` is the `%1` substitution
and takes `msg` in `ECX` and a source `String&`, returning a new `String`.

---

## 7. What is in the port and what is not

Landed as `crates/don-sim/src/systems/leader_process_taunt.rs` (new),
`crates/don-sim/tests/tick_step8_process_taunt.rs` (new), one `pub mod` line in
`systems/mod.rs`, the taunt-path hunk plus one struct field and two trace fields in
`systems/leaders.rs`, and two fail-closed clauses in `systems/save_load/step8_views.rs`.

**Executes:** the whole 2,340-byte body — all four entry guards with their exact receivers,
the two pre-switch stores, all sixteen arms and the default, the five clamps, the tribute
threshold and cooldown, the double stockpile read, `Leader::action_offer`,
`Leader::action_clear_all`, `Leader::clear_agree`, `LeaderData::bucket_add`,
`Game::teams_locked`, `Diplomacy::clear_all` and `clear_dows`.

**Refused, named, counted** (`TauntUnresolved`, and `TauntPassCounts::unresolved_calls` is
the number a scheduler should charge):

* **`Leader::action_respond` `0x006D03C0`** (3,988 B) — the actual stockpile write. Its head
  is a per-resource affordability pass over every ally that reaches `LeaderData::is_enemy`
  `0x006EBAA0`, `LeaderData::is_ally` `0x006EDB50`, a `Types` vtable `+0x78` price query and
  a second `type_avail` sweep, then writes `bucket[r] = (plain − taken) ^ 0x8221`,
  `LeaderData::escrow[r]` (`+0x468`, floored at zero) and `LeaderData::tributes[r]`
  (`+0x498`). It needs its own lane.
* **`LeaderData::type_avail` `0x006E33A0`** (1,091 B) and **`LeaderData::is_neutral`
  `0x006EBAE0`** — typed host answers on `TauntEnv`. `is_neutral` is
  `team_style == 7 && (players[i].flags & 1) && players[i].team == 8` over
  `GameInfo::player[8]` at `Game + 0x44` stride `0x8C`, a table with no owner in step 8.
* **`Random::get` `0x00A39D70` on `internal_random`** and **`Console::who`** — presentation
  only, counted in `presentation_unresolved` and never in `unresolved_calls`.
* **The initial values of the six `*_mod` scalars and `Personality::raid`.** They belong to
  `Leader::init`, not here. A default `TauntLeaderState` leaves them zero, so the first
  `TAUNT_BUILD_*` clamps them to their floors — exactly what retail does from a zeroed
  leader, and not a claim about a real match.
* **Out-of-range `LeaderData::who` or `incoming_taunt_who`.** Retail reads past
  `diplo[8]`/`last_taunt[8]`; this port refuses and charges `SlotOutOfRange`. That is the
  path `tests/tick_step8_dispatch.rs`'s `arg = 22` fixture takes, which is why that test's
  `gaps[LeaderProcessTaunt] == 1` still holds.

Presentation leaves through `TauntCall::product`, a typed ordered outbox of
`SetMessage`/`SubstituteResourceName`/`SubstituteLeaderName`/`AppendTauntText`/`ChatToLocal`/
`PlayTaunt`/`PlayUiSound`/`DiploClickStamp`/`FlavourRoll`, following
`leaders::EventProductOutbox` from `Leader::process_event_frame`.

---

## 8. The gap charge, and the one hook this lane did not make

`crates/don-sim/src/tick.rs` line ~2255 is

```rust
self.cover.gaps[Gap::LeaderProcessTaunt.index()] += trace.taunts.len() as u64;
```

`trace.taunts.len()` is a **dispatch count**. With the body ported it charges a fully
executed call as if it were absent. `tick.rs` is another lane's file, so this lane did not
change it; the one-line replacement is

```rust
self.cover.gaps[Gap::LeaderProcessTaunt.index()] += trace.taunt_pass.unresolved_calls as u64;
```

and the gap note should become

```text
"step 8  Leader::process_taunt 0x006b8cc0 - body executes; Leader::action_respond 0x006D03C0 and the LeaderData::type_avail/is_neutral host answers remain"
```

Both existing assertions in `crates/don-sim/tests/tick_step8_dispatch.rs`
(`leader_taunt_dispatches == 1`, `gaps[LeaderProcessTaunt] == 1`) hold either way: that
fixture dispatches `kind = 11, arg = 22`, and `arg = 22` is out of range, so the dispatch is
one unresolved call under both formulas.

---

## 9. Measured

`cargo test -p don-sim --lib systems::leader_process_taunt`: **18 passed, 0 failed.**
`cargo test -p don-sim --test tick_step8_process_taunt`: **13 passed, 0 failed**, every one
driving `Sim::do_frame`.
`cargo test -p don-sim --lib`: **1,671 passed, 0 failed, 2 ignored.**
`cargo test -p don-sim --test tick_step8_dispatch --test tick_step8_construct_time --test
save_step8_views`: **11 passed, 0 failed.**

Three mutations, each reverted, with the failure they produced:

| mutation | tests that failed |
|---|---|
| `infra_mod` clamp floor `0x80` → `1` (`0x006B9518`) | `a_build_taunt_rewrites_and_clamps_the_ai_scalars_inside_the_tick` (left `64`, right `128`), `an_unknown_taunt_code_still_normalises_the_scalars` (left `1`, right `128`) |
| both `type_avail` receivers `leaders[me]`, i.e. Ghidra's rendering | `a_tribute_needs_the_resource_available_on_both_leaders` (`call.stop` was `None`, expected `ResourceNotAvailable`), `a_withheld_host_answer_refuses_the_dispatch_and_stays_charged` |
| drop the second stockpile read after `action_clear_all` | `the_offered_amount_is_read_after_action_clear_all_refunds` (offered `100`, expected `120`) |

## 10. Boundaries, stated so they are not mistaken for coverage

* Nothing here is comparable to a retail checksum. `LeaderData::dip[8]` is 736 bytes and
  `LeaderData::walk_data` `0x006D6750` walks 27,182; the modelled leaders channel is still
  the 244 bytes `economy-step8.md` §8 describes plus this slice.
* The offer ledger is **two-sided**, which is why `save_load/step8_views.rs` refuses any
  non-default `Leader::taunt` rather than admitting one side of it.
* `Leader::action_respond`'s absence means a taunt tribute currently stages an offer that
  nothing ever settles. That is a faithful partial execution, not a working tribute.
* `IFaceDiploNeg::check_click_stamp` `0x008035E0` is recorded as a product receipt and its
  67-byte body is not read; it is reached from a UI singleton at `[0x00E37B9C] + 0x2364`.
