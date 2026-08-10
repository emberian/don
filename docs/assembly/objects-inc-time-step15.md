# `Objects::inc_time` — the step-15 shell, wired

Status: **shell executed in the real tick; five of its ten call sites charged as named gaps**.
Step 15 remains `stub` in `crates/don-sim/src/schedule.rs`; see "What this does not claim".

The executable surface is `Sim::objects_inc_time` / `Sim::inc_time_object_bands` /
`Sim::inc_time_deaths` / `Sim::inc_time_ammo` in `crates/don-sim/src/tick.rs`, driven from
`crates/don-sim/src/systems/unit_inctime.rs`
(`inc_time_band_traversal`, `unit_inc_time_animates`, `UnitEventView::path`).

## Ground truth

Preferred VAs in `ron-bin/riseofnations.exe`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`, matching PDB
`ron-bin/sbl/rise.pdb`. `[measured]` here means capstone disassembly of the mapped image on
this Mac; `re/decomp-all/0065db70.c` was read for orientation only.

| symbol | VA | size | role in step 15 |
|---|---:|---:|---|
| `Objects::inc_time` | `0x0065DB70` | 360 | the step |
| `Nuke::do_damage` | `0x0092BC80` | 2728 | head call at `0x0065DB82` |
| `NukeOut::graph_do_damage` | `0x00925970` | 682 | the empty-array tail of the head call |
| `Unit::inc_time` | `0x00610B40` | 122 | unit band, vtable `+0xA0` |
| `Unit::execute_events` | `0x0060EDC0` | 131 | unit band, vtable `+0x154` |
| `Wall::inc_time` | `0x0063FB60` | 2273 | building band, vtable `+0xA0` |
| `Wall::update_hits` | `0x0063F0D0` | 1509 | `Wall::inc_time`'s simulation payload |
| `Good::inc_time` | `0x0066D850` | 1 | goods band — a bare `ret` |
| `Ammo::inc_time` | `0x0067D380` | 1803 | ammo pool, direct call |
| `DeathObj::inc_time` | `0x008D5240` | 540 | death ring |
| `Farms::inc_time` | `0x008D8600` | 667 | tail |
| `Doober::inc_time` | `0x00846770` | 161 | tail, alpha fade |
| `Surf::inc_time` | `0x008A1A00` | 257 | tail, `internal_random` |

## The exact shape

One head call and six loops [measured, `0x0065DB70..0x0065DCD8`]:

```text
0065db82  call 0x92bc80                       ; Nuke::do_damage, unconditional
0065db87  edi = 0x00E3A390                    ; the ten LeaderData records
0065dba0    test byte [edi], 1                ; owner active
0065dba7    for o in 0 .. [Objects+0x15C]     ; the unit band; [ebx-0x28]
0065dbb5      test byte [obj+8], 1
0065dbbf      call [vt+0xA0]                  ; Unit::inc_time
0065dbc9      call [vt+0x154]                 ; Unit::execute_events
0065dbdb    for o in 2000 .. [Objects+0x184]  ; the building band; [ebx]
0065dbe9      test byte [obj+8], 1
0065dbf1      call [vt+0xA0]                  ; Wall::inc_time
0065dbff  edi += 0x6EEC; while edi < 0x00E7F8C8
0065dc30  for g in 0 .. [Objects+0x154]       ; the goods band
0065dc40    if vtable == Good::vftable 0x00B447E8: skip the call entirely
0065dccd    call [vt+0xA0]
0065dc60  for a in 0 .. [Objects+0x120]       ; the ammo pool, [Objects+0x12C]
0065dc69    test byte [ammo+4], 3
0065dc6f    call 0x67d380                     ; Ammo::inc_time, direct, not virtual
0065dc90  for d in 0 .. [Objects+0x140]       ; the death ring, [Objects+0x14C] stride 0xA4
0065dc98    if [corpse] == 0: skip            ; DeathObjData::valid
0065dc9d    call 0x8d5240
0065dcb2  push ecx; call 0x8d8600             ; Farms::inc_time(<stale ecx>)
0065dcbc  ecx = 0x00EAFDA0; call 0x846770     ; Doober::inc_time
0065dcc1  call 0x8a1a00                       ; Surf::inc_time
```

`(0x00E7F8C8 - 0x00E3A390) / 0x6EEC = 9.98`, so the owner walk is exactly ten iterations.

### Three ordering facts the driver now honours

1. **No owner rotation.** Step 14 (`Objects::process_all` `0x0065DCE0`) walks
   `(frame + i) % 10`; step 15 walks the leader records in address order and never reads
   `Game::frame`. Reusing step 14's traversal for step 15 is wrong.
2. **No wall band, and the building band is ten owners wide.** The only bounds are
   `Objects+0x15C` and `Objects+0x184`; there is no loop at base 3000. Because both inner
   loops live inside the *same* ten-iteration owner walk, step 15 visits the building band
   for owners 8 and 9, which step 14's separate eight-owner loop does not.
3. **Object bands precede the ammo pool; the death ring follows it.** An impact therefore
   lands after the same tick's animation clocks, and a corpse filed by that impact is visited
   by the same step 15.

## Findings this lane added or corrected

* **New: `Objects::inc_time` opens with `Nuke::do_damage`.** `0x0065DB82` calls
  `0x0092BC80` unconditionally, before every loop. It is simulation, not presentation — it
  reaches `Object::do_damage` `0x0064A480` and `Leader::action_declare` `0x006DAB50` — and
  `tools/pdb/callers.py 92bc80` reports exactly two callers, `Objects::inc_time` and
  `Ammo::do_damage`. It touches no `game_random` (`0x00C06184` appears nowhere in its
  2,728-byte extent), and an empty nuke array (`cmp dword [0x00C0A7FC], 0; jle` at
  `0x0092BC93`) falls through to `NukeOut::graph_do_damage` and returns. `Nuke::add_nuke`
  `0x0092BA30` has one caller, `Ammo::do_damage`.
* **Correction: `Wall::update_hits` runs on the *inactive* arm, and reaches buildings.**
  `crates/don-sim/src/systems/unit_inctime.rs` previously recorded `Wall::update_hits` as
  "gated on `vt[+0x4C] == WallData::is_active` and `flags & 4`, i.e. real walls only". That
  reads MSVC's devirtualisation as the gate. `0x0063FBD9..0x0063FBFF` is
  `mov eax,[vt+0x4C]; cmp eax,0x472350; jne` — a test for *whether slot `+0x4C` is
  `WallData::is_active` `0x00472350`*, taking the inlined `flags & 4` when it is and an
  indirect `call eax` when it is not — then `test eax,eax; jne 0x0063FC04`. So
  `Wall::update_hits(0)` runs when `is_active()` is **false**, on buildings as well as walls.
  Step 15's building band therefore does reach a 1,509-byte simulation body on every
  under-construction building, and the building band cannot be dismissed as presentation.
* **Correction: `Farms::inc_time` does not draw twice per tick.** The same header recorded
  "draws twice … and it runs unconditionally at the tail of every step 15". The *call* is
  unconditional; the draws are not. The whole body is inside `for f in 0 .. [0x00C0A908]`
  (`cmp dword [0x00C0A908], 0; jle 0x008D8877` at `0x008D8619`), so an empty farm array draws
  nothing. Within one farm, the first draw (`Random::get(0,0xFFFF) % 1000` at `0x008D87A9`)
  needs the count of cells that fell to zero this pass to be `>= 12`, or in `5..12` with
  `(n-4)*20/8 > 0`; the second (`% (n-1)` at `0x008D87D9`) additionally needs `n - 1 > 0`.
  **Zero, one or two draws per farm record.** Charging a fixed two per tick would have
  invented a divergence, so this driver charges the call and no RNG debt.
* **The goods loop is genuinely finished by being empty.** `Good::inc_time` `0x0066D850` is a
  one-byte `ret` (verified by direct `.text` read: `0x66D850` is `C3`, followed by `int3`
  padding), and `0x0065DC40` skips the indirect call outright when the vtable is
  `Good::vftable` `0x00B447E8`. The folded `+0xA0` slots of `Object`, `SubObject` and `Item`
  at `0x0041C150` are also one-byte `ret`s. Goods and items have no per-tick clock, so there
  is nothing to run and nothing to charge.

## What executes, and what is charged

`Sim::do_frame` step 15 now performs, in retail order:

| call site | outcome |
|---|---|
| `0x0065DB82` `Nuke::do_damage` | `Gap::NukeDoDamage`, once per tick |
| unit band `vt+0xA0` `Unit::inc_time` | **gate exact**; accepted units charge `Gap::UnitIncTime` |
| unit band `vt+0x154` `Unit::execute_events` | **branch exact**; execute arm charges `Gap::UnitExecuteEvents`, verify arm counted as graphics admission |
| building band `vt+0xA0` `Wall::inc_time` | `Gap::WallIncTime` per visited building |
| goods band | faithfully empty |
| ammo pool `Ammo::inc_time` | ported (`systems::ammo`), unchanged, now in its retail position |
| death ring `DeathObj::inc_time` | `Gap::DeathObjIncTime` per valid corpse |
| `Farms::inc_time` | `Gap::FarmsIncTime`, once per tick |
| `Doober::inc_time` / `Surf::inc_time` | presentation; `Surf` draws only `internal_random` |

Two pieces are runtime-admitted because they are complete transcriptions with no missing
input:

* `unit_inc_time_animates` — the whole of `Unit::inc_time`'s gate at `0x00610B43`:
  `inside_up < 0 || type == 0x34 || type == 0x35`. `inside_up` is a signed 16-bit read
  (`cmp word ptr [esi+0x82], 0; jl`), and the Scholar exception is two literal `TypeIndex`
  values in the machine code, not a rule field, so it is unreachable from `unitrules.xml`.
  Units the gate rejects are *reproduced* retail work — retail does nothing for them — and
  are counted in `Coverage::inc_time_units_gated` rather than charged.
* `inc_time_band_traversal` — the ten-owner, two-band, unrotated object walk, over the same
  `SparseObjectBands` registry step 14 uses.

`UnitEventView::path` (already recovered) decides `Unit::execute_events`' only branch from
live Unit state: `is_valid_unit` is the `flags & 1` the band loop already tested,
`UnitData::is_on_map` `0x0046CE30` is `(u16)inside_up >> 15`, and bit `0x10` of `unit_masks2`
selects verify-load.

## What this does not claim

**Tier C, and narrow.** Nothing here has been executed against retail and the oracle has no
case for step 15. The shell is an instruction transcription; the children it charges are
absent, not approximated.

Specifically **not** established:

* the animation clocks themselves. `Guy::inc_time` `0x005D9E10` calls `Guy::set_anim`
  `0x005DA300`, whose head resolves *which* animation plays and draws `game_random` 0–1 times
  per activation with unbounded recursion through the captain/uber path. Skipping it moves
  nothing; performing it wrongly would move the shared stream. Every accepted unit is charged.
* `Wall::inc_time`, including its `Wall::update_hits` arm.
* `DeathObj::inc_time`. The exact body exists in `crates/don-sim/src/systems/death_inctime.rs`
  and its admission requirements are unchanged (`docs/mechanics/death-inctime.md`): no
  authoritative gpiece/type pack, no `clear_blocking` terrain adapter, no
  `Scene::recalc_deaths` field. The frozen integration hunk in that document is now partly
  superseded — the death loop *position* is landed, the adapter is not.
* `Nuke::do_damage` and `Farms::inc_time`.
* any RNG debt figure. No step-15 call site has an established per-tick draw count, so this
  driver increments no `Coverage::rng_draws_missing`.

`crates/don-sim/src/systems/unit_inctime.rs::RUNTIME_FIDELITY_READY` stays `false`, and its
`RUNTIME_FIDELITY_BLOCKERS` list gained the `Nuke::do_damage` entry and the corrected
`Wall`/`Farms` wording.

### Recommended follow-ups this lane did not make

* `crates/don-sim/src/schedule.rs` step 15's `note` still reads
  `"animation/time advance for all objects"`. It should read something like
  `"exact Objects::inc_time shell executes: unrotated ten-owner unit/build walk, exact
  Unit::inc_time gate, ported ammo pool, death loop; Nuke/Guy/Wall/Death/Farms bodies remain
  explicit"`. The `StepStatus` should stay `Stub`: the shell running with five charged
  children is a weaker claim than steps 13 and 22, which are also `Stub`.
* `docs/mechanics/COVERAGE.md` §2 row 15 still reads "module exists, runtime call
  unverified — `ammo.rs` lives here".
* `docs/mechanics/unit-inctime.md` is the module's admission ledger and carries the two
  corrected claims above.

## Tests

`crates/don-sim/tests/tick_step15_inc_time.rs`, eight tests, all through `Sim::do_frame`:

| test | what it kills |
|---|---|
| `objects_alone_execute_step_fifteen_without_any_projectile` | the old `ammo.live() == 0 → Vacuous` early return |
| `the_head_nuke_call_is_charged_once_per_tick_even_in_an_empty_world` | dropping the head call or gating it on population |
| `the_unit_gate_skips_garrisoned_units_but_never_scholars` | running guy clocks unconditionally; dropping either Scholar id |
| `the_building_band_reaches_the_nature_owners_that_step_fourteen_skips` | reusing step 14's eight-owner Build traversal |
| `walls_are_processed_but_never_inc_timed` | adding a third inner loop at base 3000 |
| `every_valid_corpse_is_reached_by_the_death_loop` | dropping the death loop |
| `one_tick_charges_the_whole_shell_when_every_family_is_populated` | dropping any one charged call site |
| `the_step_fifteen_walk_is_frame_independent` | reintroducing a `(frame + i) % 10` rotation |

Plus two module tests in `unit_inctime.rs`:
`sparse_step_15_traversal_is_unrotated_wall_free_and_ten_owners_wide` and
`unit_inc_time_gate_is_inside_up_plus_two_literal_scholar_ids`.

Mutation-checked, not just green: forcing `unit_inc_time_animates` to `true` and narrowing the
traversal to eight owners failed three of the eight; removing the death loop and the object
bands failed six of the eight. Both mutations were reverted and the suite re-run green.
