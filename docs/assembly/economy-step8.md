# economy-step8 — `Leaders::process_all` and its second level

Lane: `economy-step8`. Files written: `crates/don-sim/src/systems/leaders.rs` (new, 1,859
lines, 31 tests), `crates/don-sim/examples/step8.rs` (new, runnable driver), plus two
minimal edits to shared files recorded in §7.

---

## 1. What now runs that did not before

**Step 8 of `Game::do_frame` had no caller. It has one now, and it executes.**

`economy.rs` was 3,949 lines of correct, isolated per-player economy with 89 green tests and
no retail function calling it in the shape retail calls it. `crates/don-sim/src/tick.rs` (the
wire-tick lane, landed during this wave) drives `economy::leader_gather` from step 8, which
closes the biggest half. What it could not carry — because nobody had read `0x006ED2A0` at
the instruction level — is the *control flow around* the economy: which bit gates the loop,
what arms the two stat passes, what the timers do, what the taunt table is. That is this
lane.

Concretely, these execute today and did not this morning:

| retail function | VA | status before | now |
|---|---|---|---|
| `Leaders::process_all` | `0x006ED2A0` | uncited; step 8 approximated by a per-leader `for` | ported whole, 387 B disassembled |
| the diplomacy / hostile scan | `0x006ED2E0`..`0x006ED321` | absent | ported |
| `Leader::gather`'s `BitMask<44>` union | `0x006CE35F`..`0x006CE3D0` | absent | ported — **this is what arms the stat passes** |
| `Leader::calc_wall_stats` | `0x006CF7C0` | named `Gap::LeaderCalcWallStats` | traversal ported, vtable bodies are inputs |
| `Leader::calc_unit_stats` | `0x006CF970` | named `Gap::LeaderCalcUnitStats` | traversal ported |
| `Leader::calc_attrition` | `0x006CDEA0` | uncited by any Rust file | **ported whole** |
| `Leader::calc_anti_attrition` | `0x006CDCC0` | uncited by any Rust file | **ported whole** |
| the three grace timers | `0x006ED35F`..`0x006ED3CE` | absent | ported |
| the taunt scan | `0x006ED3CE`..`0x006ED405` | named `Gap::LeaderProcessTaunt` | dispatch ported, body not |

`cargo run -p don-sim --example step8 -- 27000` is a real execution path outside the test
harness. Its output is §4.

Tier is **C**: structure and constants read from `ron-bin/riseofnations.exe`
(sha256 `30478a44…625079`) with capstone, cross-read against `re/decomp-all/`. Nothing here
has been executed against retail. No oracle case was added, so nothing here is Tier B and
none of it is verified.

---

## 2. Six things step 8 does that the previous rendition did not

Each is `[measured]` from the disassembly of `0x006ED2A0`.

**1. The outer gate is `flags & 2`, not `flags & 1`.**
`0x006ED2B0` is `test byte ptr [esi], 2`. Bit 0 is a *different* gate: it guards the inner
diplomacy scan (`0x006ED2E2`) and step 19's `Leader::process_event_frame` loop
(`0x005924A5`). A port with one `active: bool` cannot be right in both places, and the
difference is observable — a leader with bit 0 and not bit 1 is scanned by everyone else's
diplomacy loop while its own economy never runs.

**2. `calc_wall_stats` and `calc_unit_stats` are edge-triggered on `0x8000000` / `0x4000000`,
and the only thing in the engine that sets them is `Leader::gather`.**
At `0x006CE35F` `Leader::gather` computes `BitMask<44> tmp = mask_A | mask_B`, compares it
against the effective mask at `Leader + 0x6D98`, and on a difference copies it in and does
`or dword ptr [esi], 0xC000000` — both dirty bits at once. So "the two stat functions have no
port" understated it: **the protocol that decides when they run was also missing**, and
without it they would never have run even fully ported. This is the single most important
thing this lane found.

**3. Three grace timers creep toward zero.**
`0x006ED381`..`0x006ED3CE`: when `RULES.TIMER_REFRESH_RATIO` (`RULES + 0xD00` = 3328,
shipped 5) is non-zero and `Game::frame % it == 0`, each of three `(value, frozen)` pairs at
`+0x414/+0x418`, `+0x440/+0x444`, `+0x448/+0x44C` is incremented if `value < 0 && frozen == 0`.
The first pair is the same two dwords `Leader::process_elimination` reads
(`0x006B8A36`/`0x006B8A4F`), which is what fixes the semantics: `frozen != 0` means the
capital-loss clock is running against you, and only while it is clear does the debt get
refunded a frame at a time. The XML text for the rule — *"5 seconds with timer off removes 1
second from clock"* — is describing exactly this refund.

**4. The taunt table is eight parallel triples**, at `+0x394` (arg 1), `+0x3B4` (arg 2) and
`+0x3D4` (frame stamp), walked by one cursor at `[edi-0x20] / [edi] / [edi+0x20]`
(`0x006ED3D7`). The scan is gated on `Game::frame != 0` (`0x006ED3CE`) — with a zeroed table
that gate is the only thing stopping all eight firing on frame 0.

**5. Two counters are zeroed for every processed leader, every frame** — `+0x7E8` and
`+0x9F4` (`0x006ED2CA`, `0x006ED2D4`). They are per-frame accumulators something else fills.

**6. `flags & 0x80000` is cleared at the tail** (`0x006ED407`) and nothing in step 8 sets it,
so it is an inbound request bit owned by another subsystem.

### The hostile scan, exactly

```
for b in leaders[0..8]:
    if !(b.flags & 1):                     continue
    if a.slot == b.slot:                   continue
    if b.diplo[a.slot] == 2 and leaders[a.slot].diplo[b.slot] == 2:  continue   # mutual allies
    if b.flags & 0x20000:                  a.flags |= 0x40000
```

Two details a paraphrase loses. The two diplomacy reads come off **different objects** —
`b.diplo[a]` off the leader being scanned, `a.diplo[b]` off `leaders[a.slot]`, addressed as
`base + a.slot*0x6EEC` at `0x006ED2F8` — and the second is short-circuited away when the
first is not 2. And `0x20000` is read on the *other* leader: without it, no amount of hostile
diplomacy raises the bit.

---

## 3. `Leader::calc_anti_attrition` is floating point in the simulation

`0x006CDCC0` is SSE binary32 end to end (`movss` / `mulss` / `divss` / `cvtdq2ps`). It stores
an **f32** at `Leader + 0x7F4` whose "no resistance" value is the literal `0x43800000` =
256.0f, moved as an immediate rather than loaded. Four reduction sources apply

```
x = x * 100.0f / (float)(100 - RULE)
```

in that order, and each short-circuits the whole function to `0.0f` when its rule is `>= 100`.
The port does not reassociate the multiply and the divide.

This matters beyond this function. `README-LLM.md`'s float note is scoped —
"`ObjectData::get_damage` and the road A\* contain no floating point at all", explicitly not a
whole-sim claim — and this is a concrete counterexample inside the leader tick, feeding
`Unit::update_speed` / `Unit::update_armor`. It is binary32 with no transcendental call, so
Rust `f32` reproduces it bit-exactly; the hazard is a port that promotes it to `f64` or
reorders it.

Sources and shipped rules, all from `docs/derivation/rules-constants.json` (captured from
`Constants::init`, never hand-computed):

| source | gate | rule | shipped |
|---|---|---|---|
| attrition upgrades | `has_preq(0x300 / 0x2FF / 0x2FE)`, best first | `ATTRITION_UPGRADE[2/1/0]` @456 | 75 / 50 / 25 |
| Statue of Liberty | `has_wonder(0x219)` | `LIBERTY_ATTRITION` @1316 | **100** → hard zero |
| Mongols | `has_tribe_bonus(0x11)` | `MONGOL_ATTRITION` @2032 | 50 |
| Titanium | rare **bit 30** in the effective mask **or** mask B | `TITANIUM_ATTRITION` @2396 | 50 |

The titanium test is `test byte ptr [.. + 3], 0x40` at `0x006CDE47` on `Leader + 0x6DA7` and
`0x006CDE50` on `Leader + 0x6DCF`. Those are `0x6D98 + 0xC + 3` and `0x6DC0 + 0xC + 3`, which
is the independent cross-check that a `BitMask<44>` is a 12-byte header plus a 6-byte payload:
`Leader::gather`'s copy at `0x006CE382` moves three header dwords, then `memcpy`s
`len & ~3` = 4 bytes and finishes a 2-byte tail one byte at a time — a split that only makes
sense for a length of 6.

`Leader::calc_attrition` `0x006CDEA0` is the integer half: count the *consecutive* run of
`has_preq(0x2DD..=0x2E0)`, stopping at the first miss, index `ATTRITION_IMPROVED[count-1]`
(@472, shipped `1, 2, 4, 8`), then multiply by Colosseum / Russians / CTW / Kremlin, each
`v = v*(RULE+100)/100` with a floor of 1 that fires on the **product** — so a multiplier
applied to zero raises it to 1. One dead branch is recorded rather than silently dropped:
`0x006CDEB8` compares the loop variable against `0x2AD`, which the `0x2DD..=0x2E0` range never
reaches, so the `has_tribe_bonus(4)` call behind it is unreachable in this build.

---

## 4. Measured

All numbers produced by running the code, on this Mac, arm64.

**`cargo test -p don-sim --lib systems::leaders`: 31 passed, 0 failed.**

**`cargo run -p don-sim --example step8 -- 27000`** — 27,000 frames (30 game minutes at 15
frames/second), 8 leaders, all active and mutually hostile except an alliance between slots
0 and 1:

```
  frames                          27000
  Game::seconds reached           1800
  leader-frames processed         216000
  Leader::gather calls            216000
  Leader::calc_gather recomputes  416
  rare-mask unions that changed   56
  Leader::calc_wall_stats passes  56
  Leader::calc_unit_stats passes  56
  process_elimination call sites  216000
  process_taunt dispatches        8
  grace-timer creeps              960
  leader-frames with a hostile    216000
```

Per leader at the end, with attrition and anti-attrition from the ported chain:

```
  slot   food  timber  wealth   know   metal    attrition  anti
     0    367     367     367   2943     367            1   256.0
     7    368     368     368   2949     368            8  2048.0
```

Food, timber, wealth and metal converge on ~368 for every leader despite `object_income`
differing by 25 % across slots: that is `commerce_cap` at age 1 (`COMMERCE_CAP[1]` = 100)
clamping every one of them, while knowledge runs to 2,943 because its cap is the hardcoded
999 rather than a commerce cap. The clamp is `economy.rs`'s, not new here; the point is that
it is now *observed running* rather than unit-tested in isolation.

### The one real correction to a number in the lane brief

**Gross income lags up to 512 frames, not 256.** `[measured]`, by running it for all eight
slots over 4,096 frames and recording every recompute
(`gross_income_actually_lags_512_frames_not_256`).

`economy::calc_gather_due`'s clean path needs *both* `frame >= last_calc + 300` and
`(frame + slot*8) % 256 == 0`. The beats are 256 apart and the floor is 300, so the beat
immediately after a recompute is always suppressed and the next one always fires. Steady-state
period is exactly **512** frames for every slot, phase `-slot*8`. At 15 frames per game second
that is one recomputation per leader per **34.1 s**, not 17.1 s. The 300-frame floor is not a
rarely-binding guard — it doubles the period, permanently.

The dirty path is unaffected and measures every **8** frames as documented
(`a_dirty_economy_recomputes_every_eight_frames`).

### Build state

`cargo test --workspace --all-targets`: **1,251 passed, 0 failed.** The hard gate holds.

Two transient reds were seen mid-wave and both were sibling lanes' in-flight edits, not this
one: three failures in `crates/don-sim/src/systems/order_dispatch.rs` (a file created during
this wave), and a compile error in `systems/combat.rs` from the checksum-consolidation lane
half-way through replacing its `adler32`. Both cleared on their own. Nothing in this lane's
four files could have caused either: `leaders.rs` and `examples/step8.rs` are new files, and
the two shared-file edits are a `pub` keyword and a `pub mod` line.

Per-module, for the record: `systems::leaders` 31 passed, `systems::economy` 89 passed
(unchanged), `systems::victory_score` 27 passed (unchanged).

---

## 5. Live read of the econ block — attempted, not landed

The brief named one live read of the leader econ block as the highest-value next
measurement, and the VM is up: `Windows 11` running, `riseofnations` **pid 5236**, image base
**`0x00D60000`**, so the ASLR delta for this boot is **`0x00960000`**.

A reader was written and run four times via `prlctl exec … -EncodedCommand`, and the gap is
**not closed**. Two guest-side obstacles were found and both are worth writing down, because
they cost this attempt its whole budget and `README-LLM.md` currently implies neither:

* **Writing to `\\Mac\deos` from `prlctl exec` hangs indefinitely.** `prlctl exec` runs as
  SYSTEM, and SYSTEM has no credential mapping for the Parallels shared folder, so
  `New-Item`/`WriteAllBytes` on the UNC path blocks forever rather than failing. Two runs sat
  for 15+ minutes with no output and no error. `README-LLM.md`'s "the UNC path works" is true
  for an interactive session and **not** for `prlctl exec`. Returning results as base64 on
  stdout worked on the first try.
* **`RD` is the built-in alias for `Remove-Item`.** A helper named `RD` silently binds to
  `Remove-Item` and every read fails with `PositionalParameterNotFound`. Name memory helpers
  `ReadMem` / `ReadDw`.

The working script is preserved at
`/Users/ember/dev/breadstuffs/don-econ-live/econ4.ps1` (session copy:
`…/scratchpad/step8/econ4.ps1`): it takes `Game` `[0x00C061EC]`'s frame
and seconds, the `Constants` `[0x00C061F0]` slots this lane depends on
(`TIMER_REFRESH_RATIO` `+0xD00`, `ATTRITION_IMPROVED` `+0x1D8`, `ATTRITION_UPGRADE` `+0x1C8`,
`TITANIUM_ATTRITION` `+0x95C`, `LIBERTY_ATTRITION` `+0x524`), one line per leader covering
every offset in `leaders.rs`'s `offsets` module including all three `BitMask<44>` headers and
payloads, and base64 of all eight 256-byte econ blocks at `*(Leader + 0x6EB8)`. It runs but
takes longer than a single `prlctl exec` window; a final attempt is in flight. **Everything
needed to finish this is that one file plus this boot's delta `0x00960000`** (pid 5236, image
base `0x00D60000`) — expect fifteen minutes, not two.

Until it lands: `economy::LeaderEcon::image` still emits zero for the dwords at `0x48`,
`0xAC..0xC4` and `0xE0..0xF0`, and the modelled channel remains comparable between two runs
of this code and **not** comparable to retail — `LeaderData::walk_data` `0x006D6750` walks
27,182 bytes where we emit 244. The script above also settles, for free, whether
`Step8Rules::shipped()` matches the live block, which is the one thing in this lane whose
constants come from a JSON file rather than from the process.

---

## 6. The tick integration, exactly

This integration is now live in `Sim::do_frame`. `Sim` owns

```rust
pub step8: leaders::Leaders,
pub step8_env: leaders::Step8Env,
pub step8_rules: leaders::Step8Rules,
```

`Sim::activate(who)` activates the exact leader too, and `leaders_process_all` invokes the
dispatcher at the real pre-increment step-8 boundary:

```rust
fn leaders_process_all(&mut self) -> (StepRun, u32) {
    let t = leaders::process_all(
        &mut self.step8, self.world.frame,
        &self.step8_rules, &self.econ_rules, &mut self.step8_env,
    );
    for who in t.elimination_calls.iter() {          // retail's own order
        self.vic_leaders.process_elimination(&mut self.vic_match, *who);
    }
    self.cover.leader_gathers += t.leaders_processed() as u64;
    self.cover.gaps[Gap::LeaderProcessTaunt.index()] += t.taunts.len() as u64;
    if t.leaders_processed() == 0 { (StepRun::Vacuous, 0) }
    else { (StepRun::Executed, t.leaders_processed() as u32) }
}
```

The landed adapter also synchronizes `LeaderSlot`'s economy inputs/outputs at this boundary
and rebuilds each `OwnerObjects` band from the live `ObjectRegistry`, preserving the
unresolved virtual answers and their counters.

Three integration facts remain load-bearing.

* `Gap::LeaderCalcWallStats` and `Gap::LeaderCalcUnitStats` are no longer unconditional.
  They now count active objects whose unresolved virtual bodies were reached by a stat pass,
  not frames on which retail correctly skipped the edge-triggered traversal.
* `Gap::LeaderProcessTaunt` becomes accurate rather than per-frame: it should count actual
  dispatches, which with a zeroed taunt table is zero.
* The `frame` passed in must be the **pre-increment** `Game::frame`. Step 20 (`0x005924BF`)
  is where it moves, and `Step8Driver` in `leaders.rs` shows the bracket.

`leaders::Leaders` deliberately owns its own `economy::LeaderEcon`. The current adapter
keeps `LeaderSlot` as the later tick steps' shared façade and synchronizes it before and
after step 8; removing that adapter requires migrating those later consumers, not another
copy of the economy algorithm.

---

## 7. Shared-file edits, and corrections to existing artifacts

Two shared-file edits, both minimal, both stated per the standing rule:

* **`crates/don-sim/src/systems/economy.rs`** — one keyword: `fn pct` → `pub fn pct`, with a
  two-line doc note. `Leader::calc_attrition` and `Game::retake_capital` emit the same
  `0x51EB851F` divide-by-100 idiom, and re-typing it in a second module is how `adler32`
  ended up implemented nine times.
* **`crates/don-sim/src/systems/mod.rs`** — one `pub mod leaders;` line with its doc comment,
  as that file's own header instructs.

Corrections, two sentences each:

* **The lane brief and `economy.rs` §`calc_gather_due` — "income LAGS up to 256 frames".**
  Measured over 4,096 frames on all eight slots, the steady-state period is 512 frames,
  because the 300-frame floor always suppresses the beat that follows a recompute. The
  formula in `economy.rs` is right; only the period stated around it is wrong.
* **`docs/mechanics/COVERAGE.md` §6 item 8 — "`economy.rs` exists and is isolated".** Stale in
  two directions: `tick.rs` now drives `economy::leader_gather` from step 8, and the item's
  own list of second-level functions omitted the two that were genuinely uncited by any Rust
  file, `Leader::calc_attrition` `0x006CDEA0` and `Leader::calc_anti_attrition` `0x006CDCC0`.
* **`crates/don-sim/src/systems/economy.rs` header — "the leader's rare-resource bitmask,
  `leader + 0x6DCC`".** Correct but partial: `0x6DCC` is the *payload* of the `BitMask<44>`
  object at `0x6DC0`, and there are three such objects — `0x6D98` (effective), `0x6DAC` and
  `0x6DC0` — related by the union at `0x006CE35F`. `GatherInputs::rares` mirrors `0x6DC0`.
* **`Leader::process_elimination` `0x006B8A20` — the `else` arm is dead code.** It branches on
  `[0x00C061E8]->byte[0x37] == 1` after the identical test on `[0x00C061EC]`, and
  `schema/rise-symbols.tsv` names those `GameAccessConst::gamec` and `GameAccess::game` — the
  const/non-const reference pair to **one** `Game`, exactly like the `constantsc`/`constants`
  pair `README-LLM.md` already records. So elimination always routes through
  `LeaderData::find_capital` `0x006EB930`, and `victory_score.rs`'s port, which takes only
  that path, is right for a reason it does not state.

---

## 8. Boundaries, stated so they are not mistaken for coverage

* **`Leader::process_taunt` `0x006B8CC0`** (2,340 B) is not ported. Dispatches are recorded
  with both arguments and the table entry; the body is AI chat.
* **`Leader::process_elimination`** is not re-ported here on purpose —
  `victory_score::Leaders::process_elimination` already has it, over `Game::retake_capital`
  `0x00594530`. Two copies of one retail function in one tick is the failure mode
  `COVERAGE.md` §1 names about `adler32`.
* **Four virtual slots inside the two stat passes are unresolved**: `+0x4C`, `+0xE8`, `+0x15C`
  and `+0x160` on the object's data. `StatObject` carries their observable answers and counts
  the calls; it does not invent bodies. Also unpreserved-because-unobservable: the first
  `calc_wall_stats` loop fetches its guard through vtable `+0xAC` and the second through
  `+0xB0`, which is flagged in the source so nobody "tidies" it.
* **`Game::retake_capital` `0x00594530` is read but not ported here.** Its rescale is
  `max(1, (world[0] * RETAKE_CAPITAL + S/2) / S)` with `S = [[0x00E7FCA8] + 0x144]`, then
  `(leader[0x41C] * that) >> 8` with a toward-zero bias, and a CTW branch behind
  `gamec[0x822] & 2` that consults `has_wonder(0x213)` and can double or halve the result.
  `[0x00E7FCA8]` has no public symbol. Whether `victory_score`'s `Match::retake_capital`
  agrees with this is **untested** and is the obvious next check.
* **Nothing here is comparable to a retail checksum.** The modelled leaders channel is 244
  bytes of a 27,182-byte walk.
