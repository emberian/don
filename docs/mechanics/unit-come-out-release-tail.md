# `Unit::come_out` release tail — and the closure of the 9,925-byte body

Status: fourth and final measured, source-only transaction planner for `Unit::come_out(int)`
`0x00617C10`. Registered in `crates/don-sim/src/systems/mod.rs`; **not** wired to any
executor. Fidelity tier **C** — derived from the instruction stream, the PDB and a full
decompilation, exercised only against this port, never differentially tested against retail.
Nothing here is verified in the proof-assistant sense.

## 1. Provenance

Shipped `ron-bin/riseofnations.exe` (SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`) and matching
`ron-bin/sbl/rise.pdb` (GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1). Instruction
stream read with local Capstone PE32 disassembly; control flow cross-read against a full
Ghidra decompilation of the body; every signature, field offset and global name taken from
`tools/pdb-extract` output (`symbols.json` / `types.json`). No retail process, VM or remote
host was touched.

**`re/decomp-all/00617C10.c` did not exist because nobody had asked for it.** `00617c10` is
one of the 39 `skipped_large` rows in `re/decomp-all/MANIFEST.jsonl` — `re/scripts/BulkDecomp.java`
caps bodies at 8,192 bytes and the body is 9,925. The recipe the `move-near` lane published
works unchanged:

```sh
cp -R re/ghidra "$SCRATCH/ghidra-comeout"
analyzeHeadless "$SCRATCH/ghidra-comeout" ron -process -noanalysis \
  -scriptPath re/scripts -postScript DecompileOne.java 00617c10 900 "$SCRATCH/00617c10.c"
```

~40 s, 1,228 lines. A missing `re/decomp-all/<EA>.c` means nobody looked, not that the body
resists decompilation.

## 2. The body is now closed. Here is the accounting.

| tranche | module | sequential | outlined | total |
|---|---|---:|---:|---:|
| prefix | `unit_come_out_full_frontier` | `0x00617C10..0x006186B4` = 2,724 | 72 | **2,796** |
| common release | `unit_come_out_common_release_frontier` | `0x006186B4..0x00618B22` = 1,134 | 35 | **1,169** |
| gather selection | `unit_come_out_gather_selection_frontier` | `0x00618B22..0x006191A5` = 1,667 | 16 | **1,683** |
| release tail | `unit_come_out_release_tail_frontier` | `0x006191A5..0x0061A206` = 4,193 | 84 | **4,277** |
| | | | | **9,925** |

`crates/don-sim/src/systems/unit_come_out_body_map.rs` asserts this **byte by byte**, not by
arithmetic: it walks all 9,925 addresses and requires each to resolve to exactly one owner.
Two extents could overlap by exactly as much as another pair gaps and still sum correctly, so
the totals alone are not evidence.

### The 72 bytes nobody had counted

MSVC placed 22 compiler-outlined virtual-call islands in `0x0061A206..0x0061A2D5`, past the
sequential body. Each has the shape

```text
  mov ecx, ebx      ; only when the receiver is not already in ECX
  call eax          ; the indirect call the fast path had devirtualised
  jmp <resume>      ; back into the sequential body
```

and is emitted once per `if (slot == <known fn>) fast(); else slot();` site. **An island
belongs to whichever tranche its `jmp` resumes into, not to whichever tranche's address range
it sits in.** Seven of them (`0x0061A206..0x0061A24E`, 72 bytes) resume inside the *first*
tranche, which published a flat `PREFIX_BYTES = 0x006186B4 - 0x00617C10` with no island
accounting at all. That is why the residual the gather-selection lane reported (4,349) is 72
larger than the release tail's own behaviour (4,277): the difference is the prefix's unbilled
islands, now mapped.

The island at `0x0061A216` is the only one that does not resume. It pops the frame and
returns **1** — so `Unit::come_out` returns non-zero through exactly one path, and that path
is the prefix's, not the tail's. Within the release tail the return value is always `0`.

## 3. What the tail is

The order-installation dispatcher. Once the unit has a location, retail decides what the
freshly released unit should *do*:

```text
  0x006191A5  face the container                Unit::set_angle(container->angle)
  0x006191FC  nothing selected           -> tail
  0x0061922D  building under the point?   ObjectsData::find_any_building_at
  0x0061923E  reposition beside it        find_angle / find_nearby_spot / set_new_location
  0x00619386  specialist dispatch         add_build_order / add_repair_order / add_gather_order
  0x0061960F  trade                       add_trade_order
  0x006196C7  garrison                    add_garrison_order      + the o_down chain
  0x00619854  enemy building              add_attack_order        + the o_down chain
  0x00619AA3  unit under the point?       ObjectsData::find_unit_with_radius
  0x00619D74  movement fallback           add_move_facing_order / Group::action_move_to
  0x00619FE2  SPECIAL_ANIM exit order     add_spec_anim_order / do_spec_anim
  0x0061A0B6  options.rebuild = 1
  0x0061A0C5  terminal army gate          Random::get -> Unit::add_to_army
```

129 of the body's calls are in this tranche — more than the other three combined.

### 3.1 The specialist arms are one code path with an inverted probe

Both the worker pair (`TypeIndex` `0x32`/`0x33`) and the Scholar pair (`0x34`/`0x35`) run the
same three-call preamble on the building — the `+0x0C` virtual, then
`obj->[+0xB0]()->[+0x18]->[+0x90]()`, then `obj->[+0xB0]()->is(0x1A4, false)` — and then
**disagree about the answer**:

* `0x0061947B  test eax, eax ; je skip`   — the Scholar pair gathers only *at* a University;
* `0x006195D5  test eax, eax ; jne skip`  — the worker pair gathers only at a *non*-University.

Before that probe the worker pair takes two earlier exits, in this order:

1. `building->[+0x4C]() == 0` and `objects.find_who == this->who` → `Unit::add_build_order`
   `0x006194F5`;
2. otherwise `ObjectData::damage` (`+0x24`) `!= 0` → `Unit::add_repair_order` `0x00619542`.

So a peasant leaving a building walks up to a *damaged* structure and repairs it, and to an
*undamaged* one and gathers from it. The pivot is one `cmp dword ptr [eax + 0x24], 0` at
`0x00619517`.

### 3.2 Every installed order is replicated across the `o_down` chain

`UnitData+0x90` is `o_down` [PDB `types.json`]. Garrison (`0x006197D9`), building attack
(`0x006198B9`, `0x00619A1D`) and unit attack (`0x00619C7A`, `0x00619D01`) all end in the
identical loop: read `o_down` as a **signed short**, stop when negative, resolve
`objects[this->who][o_down]`, take its `+0xA8` virtual to reach the `Unit`, issue the *same*
order with the *same* arguments, then read that link's own `o_down`. A host that orders only
the released unit under-installs.

### 3.3 Ghidra's decompilation is lossy in two places, and both change behaviour

* **`0x00619C54`.** Ghidra renders both arms of the unit-attack branch as
  `add_attack_order(u, who, 1)`. The PDB signature is
  `void Unit::add_attack_order(int, int, QueuePos, int, int)` and the instruction stream
  pushes `1,1,1` on the `action != 0` arm (`0x00619C6D`) and `0,0,1` on the `action == 0`
  arm (`0x00619CF4`). Two different orders.
* **`0x00619DA2` / `0x00619DBA` / `0x00619DD2`.** Ghidra drops the arguments of the three
  `TypeData::where` probes that promote the movement fallback from mode 1 to mode 2. They are
  `is(0x1AB, false)`, `is(0x1AC, false)` and `is(0x1B0, false)`, evaluated on
  `types[this->type->where]` and short-circuiting on the first hit.

Anyone porting this from the decompilation alone would ship both defects.

### 3.4 The terminal army gate is a frame-parity test, and it draws from `game_random`

```text
  0061a0c5  test dword ptr [ebx + 0x68], 0x40000   ; UnitData::unit_masks
            je   return                             ; not an army candidate
  0061a0e5  is_on_map()                             ; on the map -> return
  0061a0fb  type->type in {0x3D, 0x3E, 0x190}       ; -> return
  0061a12e  type->unit_flags2 & 0x10                ; ObjectTypeData +0x2B8
  0061a13b  is(0x143, false)
  0061a1aa  mov  ecx, [0x00c06184]                  ; GameAccess::game_random
  0061a1bb  call Random::get(0, 0xffff)             ; residue = draw % 3
  0061a1d5  call Random::get(0, 0xffff)             ; residue = draw & 1, sign-corrected
  0061a1cd  test [game + 0x550], residue            ; Game::frame
  0061a1f2  je   return
  0061a1f6  call Unit::add_to_army
```

Three consequences:

1. **`Unit::come_out` consumes a canonical `game_random` draw.** `[0x00C06184]` is
   `GameAccess::game_random` — the main simulation stream, the same one the board's standing
   findings say map generation draws from. The two call sites are mutually exclusive, so the
   whole-function budget on this path is exactly **one** draw, taken only when the unit is an
   off-map army candidate. A host that budgets zero desyncs.
2. **The gate is not a probability.** `Game+0x550` is `frame`, so the predicate is
   `(game.frame & residue) != 0`. On frame 0 no unit joins an army however the draw falls.
3. **One branch takes no draw at all**: `unit_flags2 & 0x10 == 0` together with
   `is(0x143) == 0` decides on `is(0x3A, false)` alone (`0x0061A161`) and never reaches
   `Random::get`.

The sign correction at `0x0061A1DF..0x0061A1E5` (`and eax, 0x80000001` / `jns` / `dec` /
`or 0xFFFFFFFE` / `inc`) is the MSVC idiom for a sign-preserving `% 2`; it is reproduced
exactly rather than replaced with `& 1`, because `Random::get` is typed `int`.

### 3.5 The search band

Both repositioning sites (`0x00619279` and `0x00619B0B`) compute the same band with the same
instruction sequence, including the `lea ecx, [eax + eax*2]; shl ecx, 4` encoding of `* 0x30`:

```text
lo = (type->x_size + type->y_size) * 0x30 + constants.unit_train_distance
hi = lo + (constants.unit_train_max_distance - constants.unit_train_distance)
```

`Constants::unit_train_distance` / `unit_train_max_distance` are `GameAccess::constants`
(`0x00C061F0`) `+0x84` / `+0x88` [PDB].

## 4. Globals, resolved

| address | PDB symbol | used for |
|---|---|---|
| `0x00C06184` | `GameAccess::game_random` | the one draw in §3.4 |
| `0x00C0618C` | `GameAccess::objects` | `+ who*0x1C + 0x14` is the owner's object list; `+0x200` is `ObjectsData::find_who` |
| `0x00C061EC` | `GameAccess::game` | `+0x550` is `Game::frame` |
| `0x00C061F0` | `GameAccess::constants` | `+0x84`/`+0x88`, the search band |
| `0x00C06204` | `MiscAccess::options` | `+0x90` is `Options::rebuild`, set to 1 unconditionally at `0x0061A0B6` |
| `0x00CAE5FC` | `div_3_table` | the tile↔world coordinate table |
| `0x00E85F20` | `groups.list` | stride `0x9D4`, the scratch group `Group::action_move_to` runs on |
| `0x00B42174` | `const Build::'vftable'` | the fast path of the `get_garrison_limit` devirtualisation |
| `0x00653790` | `ObjectData::is` | the 12-byte forwarder every `is(...)` site devirtualises against |

`ObjectsData::find_who` is **not** the gaia owner. It is the owner of the object the preceding
`find_any_building_at` / `find_unit_with_radius` located, and it is what every order in the
tail is addressed to.

## 5. What this lane could not derive, and did not write

* **`Unit::add_spec_anim_order`'s fourth (`QueuePos`) argument.** `0x0061A002 push ecx`
  sources a register that is not established on every path into `0x00619FE2`. The other three
  arguments are measured (`1` = `SpecialAnimKind::Exit`, the first tranche's
  `container_gpiece`, `0`). The planner emits
  `ReleaseTailBoundary::SpecAnimQueuePosUnestablished { push_va: 0x0061A002 }` rather than
  guessing.
* **`Object::eject_contents` `0x0064CD20` under `Group::action_eject_all`'s arguments.**
  Unchanged from `docs/mechanics/step8-eject-contents.md`: only the step-8 slice
  `(kill_failed=1, filter=-1, transfer=0, reset=1)` over a Build carrier is recovered.
  `action_eject_all` calls it with `kill_failed = 0`, `filter = 0x32` or `-1`, `reset` on both
  settings, over carriers that are usually units. Not derived, not written.
* **A host.** Every one of the four tranches is a planner. Nothing in `don-sim` can apply the
  resulting plan; see §6.

## 6. Registration, and what it did and did not buy

Before this lane, `unit_come_out_full_frontier`, `unit_come_out_common_release_frontier`,
`unit_come_out_gather_selection_frontier`, `step8_eject_contents` and
`unit_action_come_out_frontier` had **no `mod` declaration anywhere in
`crates/don-sim/src/`**. Each was compiled only from its own test file's `#[path]` include, so
the library crate did not contain them and no gameplay path could reach them. Their tests
passed, which is exactly why nobody noticed.

All five are now `pub mod` in `crates/don-sim/src/systems/mod.rs` (seven lines in total,
with the two new modules), and all five test files
were rewired from `#[path]` includes onto `don_sim::systems::…` — so removing a `pub mod`
line now breaks compilation instead of silently re-stranding the module.

That makes the family *present*. It does not make it *run*. `Unit::come_out` is stubbed at
five separate places in the registered library:

| stub | what it does today |
|---|---|
| `systems/production_runtime.rs::come_out` | writes `inside_up = -1`, `inside_up_who = -1`, returns 1 |
| `systems/production.rs` `UnitPlacementHost::come_out` | host-supplied `UnitComeOutReceipt` |
| `systems/unit_inctime.rs` `unit_come_out` | host-supplied `ComeOutReceipt { rng_draws }` |
| `systems/gathering.rs::come_out` | host-supplied |
| `systems/leader_set_diplo.rs` | `come_out_return: Option<i32>` fact |

`unit_come_out_body_map::ComeOutBoundary::NoHost { retail_va: 0x00617C10, mode }` is the
single shape those five should converge on. Writing that host is the next lane's work, and it
is not a transcription job — it needs an object host that can resolve
`objects[who][o]`, run two spatial finders, install seven order kinds, and account one
`game_random` draw. `crates/don-sim/src/tick.rs` owns the only candidate (`Sim`), and this
lane does not own `tick.rs`.

## 7. Files

| path | what |
|---|---|
| `crates/don-sim/src/systems/unit_come_out_release_tail_frontier.rs` | new — the fourth tranche |
| `crates/don-sim/src/systems/unit_come_out_body_map.rs` | new — the address-space accounting and the shared boundary |
| `crates/don-sim/tests/unit_come_out_body_map.rs` | new — library-path gate for the whole family |
| `crates/don-sim/src/systems/mod.rs` | six `pub mod` lines |
| `crates/don-sim/tests/unit_come_out_full_frontier.rs` | rewired onto the library path |
| `crates/don-sim/tests/unit_come_out_common_release_frontier.rs` | rewired |
| `crates/don-sim/tests/unit_come_out_gather_selection_frontier.rs` | rewired |
| `crates/don-sim/tests/step8_eject_contents.rs` | rewired |
| `crates/don-sim/tests/unit_action_come_out_frontier.rs` | rewired |

## 8. Gates

* `cargo test -p don-sim --lib -- systems::unit_come_out systems::step8_eject_contents systems::unit_action_come_out` → 27 passed.
* `--test unit_come_out_body_map` 7, `--test unit_come_out_full_frontier` 15,
  `--test unit_come_out_common_release_frontier` 11,
  `--test unit_come_out_gather_selection_frontier` 10, `--test step8_eject_contents` 8,
  `--test unit_action_come_out_frontier` 7 — all green.

Mutation-tested; each of these turns a test red:

| mutation | test that dies |
|---|---|
| drop the `o_down` chain replication | `a_garrison_order_is_replicated_across_the_whole_o_down_chain` |
| make the two unit-attack arms identical (Ghidra's reading) | `the_two_unit_attack_arms_carry_different_arguments` |
| drop the worker arm's `!is_university` | `the_university_probe_polarity_is_opposite_for_the_two_specialist_pairs` |
| replace the frame AND with `residue != 0` | `the_army_gate_is_a_frame_parity_test_not_a_probability` |
| make the no-draw army branch draw | `the_no_rng_army_branch_consumes_no_draw` |
| attribute an outlined island by address instead of resume | `outlined_islands_are_owned_by_their_resume_target_not_their_address`, `the_measured_tranche_sizes_are_the_ones_the_lanes_reported` |
| move a tranche seam by one byte | `every_byte_of_the_body_has_exactly_one_owner`, `the_four_tranches_tile_the_retail_body_exactly`, `the_measured_tranche_sizes_are_the_ones_the_lanes_reported` |
