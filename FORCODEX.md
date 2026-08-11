# FORCODEX — handoff

Written 2026-08-11 at the end of a very large swarm session. This is the orientation an
incoming agent actually needs: not a summary of the project (that is `README-LLM.md`), but
**what we learned the hard way, what is currently true, and where the next value is.**

Read in this order: `README-LLM.md` → `GOAL.md` → `docs/CHARTER.md` →
**`docs/tracks/swarm-strategy.md`** → **`docs/tracks/megaswarm-board.md`**.

---

## 1. State, measured

```
python3 tools/simulation-closure.py --check     120 red   (the backlog; RED is expected)
  tick 21/29   orders 23/28   group_actions 11/42   opcodes 47/82
  checksums 1/15 by the strict gate; 3/15 have real producers
  product_blockers 0/18   global_stages 0/9

cargo test --workspace --all-targets            252 suites green
tools/oracle-regress.sh                         25 cases, 19,022,634 trials, 0 mismatches
re/decomp-all/MANIFEST.jsonl                    46,727 / 46,727 ok
schema/replay-validation.json                   corpus non-trivial compares 1,114,690
```

The closure number went **124 → 120 while getting more honest**, not less: two rows were
*added* back after audits found them falsely complete. Expect that to keep happening. An
inventory that overcounts is worse than one showing more red.

---

## 2. The five things that cost us the most time — do not relearn these

### 2.1 ~9,900 lines of recovered mechanics were unreachable

Modules under `crates/don-sim/src/systems/` with **no `mod` declaration and no `#[path]`
mount**, compiled only from their own test files. Their tests passed — in isolation, never
through the sim — which is exactly why nobody noticed.

This paid out **five times in one day**: two order rows (`GUARD`, `GARRISON`) and three group
rows closed on *registration alone*; `Unit::come_out` turned out **56% already transcribed**
against a "7,201 bytes unrecovered" figure that was two tranches stale; `leader_set_diplo.rs`
was 938 complete lines of `Leader::set_diplo`; `leaders_diplomacy_opening_frontier.rs` the
same.

**Before deriving anything, check whether the body already exists.** The remaining work for
such a row is an adapter + host + registration in the shape of `guard_dispatch.rs`, not a
derivation. Scan with: for each `systems/*.rs`, grep the crate for `mod <name>` or
`path = "…<name>.rs"`.

### 2.2 "No file in `re/decomp-all/`" meant nobody looked

`BulkDecomp.java` took its cap as an argument defaulting to **8,192 bytes**, so 39 functions
were `skipped_large` — including `Constants::init` (the rules loader) and `Leader::diplomacy`
(20,348 B, the largest AI function). At least three lanes read that as "unrecovered".

**Fixed**: the corpus is now complete at 46,727/46,727 (`re/scripts/DecompileList.java`, and
the reproduction command is in the board). But the lesson generalises: **a generated artifact
that silently drops rows is how a repo comes to believe something is unknown.** Two more were
found the same day — `schema/vtables.json` was wrong in *both* directions (118 rows were not
vtables; 229 real vftables were missing, including the four classes `state-schema.json` is
defined by), and `schema/types.json` silently dropped 208 definitions.

### 2.3 The PDB names things; it does not establish behavior

Measured, repeatedly:

- `SyncPoint::process_sync_signal` is declared `static void __cdecl (NetMsg_SyncSignal*,
  NetPlayer const*)`. The emitted body takes **no stack parameters** and reads only `ECX`.
- `Crossplay::ICrossPlayService` (97 methods) is a vendor header; the **shipped** interface is
  `CrossplayProxy::ICrossPlayService` (58 slots) and every slot disagrees.
- `Unit` vftable `+0xD8` is `UnitData::is_moving`, not `is_hero` — the Arena was calling a
  different function entirely.
- `deviations.rs` cited `Object::find_auto_target 0x0064DDA0`. **No such PDB symbol exists**;
  that address is `ObjectData::count_inside`.

### 2.4 `re/decomp-all/` drops call arguments

It rendered `Player::walk_data`'s second `DataWalk` call as zero-arg where the instructions
push two operands, and `Group::action_unitmask` as one-arg while emitting `ret 8`. It cost one
lane a wrong conclusion **twice in one session**.

The C is a **control-flow map**. Capstone settles arguments, stack cleanup, and rounding.

### 2.5 Presentation-sounding names hide simulation state

Twice: `Leader::process_taunt` was catalogued as an "AI-chat body" and moves **resources**;
tick step 10 was named "AI diplomacy chat" and marked `out_of_scope` and is the **No Rush
timer expiry** — mass war declaration plus `attrition_stamp` zeroing. `simulation-closure.py`
counts `out_of_scope` as complete, so that one sat *inside* the completed set.

Also: opcode 68 `ChatCommand` is classed `Presentation` and reaches `ConsoleWin::parse_cmd` —
the same executor `process_console_cmd` uses.

---

## 3. How to run the swarm

`docs/tracks/swarm-strategy.md` is the full version. The parts that matter:

- **Lanes share a crate.** One module per recovered mechanic plus one export line; two lanes
  adding modules do not conflict. The genuinely hot files are few: `tick.rs`, `command.rs`,
  `command_tables.rs`, `order_dispatch.rs`, `systems/mod.rs`, `don-env/src/action.rs`.
- **`docs/tracks/megaswarm-board.md`** is an append-only noticeboard: file claims, **API
  CHANGE** notices, **HOOK NEEDED**, and **FINDINGS** so nobody re-derives. Read its standing
  findings before dispatching anything.
- **Build on `persvati`** (24 cores, ~78 GB free) via `tools/swarm-cargo-remote`. `hbox` is
  memory-tight under a co-tenant HOL build — reserve it for the i686 oracle. The Mac's
  "4 compiling lanes" is a *Mac* limit.
  ⚠ Remote jobs fetch the **pushed** `HEAD`; lanes do not commit, so **push promptly** or they
  die on `upload-pack: not our ref`.
- **Do not `git add` a shared file blindly.** Staging named files is necessary and *not
  sufficient*: `command.rs` carried three lanes' `#[path]` declarations and I committed it
  without their module files, breaking `HEAD` from a clean checkout for an hour. Verify with
  `git archive HEAD | tar -x -C /tmp/x && cargo check` in `/tmp/x`.
- **The umbrella is the real gate.** Twice, a lane was green on its own targets and the
  whole-workspace run caught a defect — including a test whose *conclusion* depended on the
  build profile (`groups_channel_initial`, now fixed).

### The norm worth protecting

**Five lanes in a row refused to promote a `StepStatus` for a shell with charged children**,
and four refused to tune a value into checksum agreement. Nobody enforced that; it propagated
through the prompts. Keep saying it: *a fitted agreement is worse than a divergence, and tier
inflation is worse than a red row.*

---

## 4. Where the next value is

**Weight by decisions, not rows.** Closure is 57.3% unweighted but **16.3% weighted by real
player commands** — five *scheduled* opcodes (Camera, TurnData, PlayerSpeed, both checksum
streams) are 96.5% of corpus traffic and were already complete. **83.7% of player decisions
land on a red opcode.** `analysis/scoreboard.py` computes this.

Concretely open, in rough value order:

1. **One `Sim`-side `Leaders`/`Match` host.** Blocks four separate rows: `Leader::defeat` for
   opcodes 70/71/80, arena diplomacy's `Leader::victory`, and `save_load`'s mid-match shape.
   The single highest-leverage hook on the board.
2. **`Group::action_move_near`'s garrison/disembark tail** — every movement opcode is
   downstream of it. Its decompilation now exists.
3. **`world` checksum channel** — 780,476 real bytes/compare, 0 matches. Blocked on
   `Mountains::add_mountain` / `World::set_oil_at` inside `TerrainGroups::place_all`.
4. **BHS builtins** — 88 of 873 tree-wide. Gates the AI opening book and channel 15. Note
   `don-sim/src/script_runtime.rs` and `don-bhs/src/scenario.rs` implement overlapping sets
   independently and **agree everywhere they overlap**; collapse direction is don-sim → don-bhs.
5. **The 18 product blockers** — zero complete, the only domain with none.

### Two things not to waste a week on

- **You cannot mine retail AI behaviour from `.rcx`.** Zero of 5,055,253 commands came from a
  non-HUMAN slot across 301 present slots, in games that *do* contain AI. Lockstep never
  transmits AI decisions. This is in-principle, not a gap.
- **A bot-vs-bot scoreboard measures the physics under test**, not the bot. Our opening
  optimiser already scores *worse* than the shipped AI's order (40% vs 78% inside the human
  envelope) and degrades as the beam widens — the diagnosis is an income model under-counting
  mid-game knowledge/wealth, and `econ.py`'s own header names the three unmodelled terms.

---

## 5. Multiplayer: parked, deliberately

A DoN peer joined a real retail-hosted lobby through our replacement `CrossplayNetLib.dll`,
readied, and **the match launched into the game world**. Two real defects were fixed to get
there (both preferred-base-vs-loaded-image comparisons that ASLR broke).

It is parked because **the replay corpus is already the retail-compatibility harness** — 61
recordings, 585,152 turns, deterministic, offline, ~30 s — and live attachment adds nothing to
checksum agreement until the simulation produces the fifteen channels. Resuming costs one run;
the `SyncPoint` barrier work is landed with a self-localising diagnostic.

`crates/don-crossplay` now emits a real PE32 `CrossplayProxy.dll` with exact export parity.
Its lane found an ABI bug **every static gate passed**: `#[repr(C, align(8))]` on
`MsvcFunction` made i686 pass the aggregate *as a pointer*, so a thunk did `ret 8` where retail
pops 44 — 275 layout assertions, export parity and the whole host suite were green. Only
executing PE32 code under Wine caught it.

Live retail runs need **explicit per-run authorization from ember every time**, plus a human
at the UI (`prlctl exec` is SYSTEM/session 0). The VM auto-pauses between commands.

---

## 6. Traps that will bite you personally

- **zsh eats backticks in `git commit -m`.** Write prose messages to a file, `git commit -F`.
  (I did this all session and still slipped once.)
- **Never `git stash`.** Parallel lanes share the working tree.
- `capstone`/`pefile` are absent from the system `python3` — use `uv run --with`. Do not name a
  helper `dis.py`; it shadows the stdlib module capstone imports.
- `schema/vtables.json` **used to** list classes twice 8 bytes apart with the higher address
  being the real vtable. Regenerated now, but check any doc that repeats it.
- `MANIFEST.jsonl`'s `size` is Ghidra's body address-count, **not** PDB code size
  (`ConsoleWin::run_cmd`: 664 vs 43,008).
- `Array<T>` metadata is **not** universally checksummed — false for `Array<Group>`. `check_*`
  and `walk_data` are different traversals and disagree. Read the `check_` function for your
  channel.

Good luck. The evidence discipline is the whole product — velocity without it would have
landed at least a dozen plausible, compiling, wrong things today.
