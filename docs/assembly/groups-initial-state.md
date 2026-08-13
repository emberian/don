# The initial `Groups` state — checksum channel 5

Lane: `replay-groups`, wave 3. Every address here was read from
`re/decomp-all/<va>.c` and cross-checked against `schema/types.json` and
`re/symtab.json`; every number is **[measured]**. Nothing here is verified in the
proof-assistant sense — see [`docs/CHARTER.md`](../CHARTER.md).

Companion to [`scenario-initial-state.md`](scenario-initial-state.md): an initial state
derived entirely from a retail initializer, with a single 32-bit statement against the corpus.
The initializer remains the fresh-state proof; the replay harness now projects the same exact
walker from live canonical `Sim.groups` on every compare.

---

## 0. Why this channel, and why not `leaders`

The wave brief suggested `leaders`. It is the wrong pick, and the corpus says so before
any disassembly does.

Take the value each of the 21 checksum-bearing recordings carries on its **first**
checksummed turn, per channel:

| channel | distinct first-turn values over 21 recordings |
|---|---:|
| `units`, `builds`, `guys`, `leaders`, `cities`, `goods`, `items`, `world` | **21** |
| `groups` | **15** |
| `scenario_data` | 1 |

A channel with 21 distinct first-turn values has a setup-dependent initial state: there is
no constant to derive and nothing to pre-register a test against. `leaders` is also the
largest surface in the whole checksum: `LeaderData::walk_data` `0x006d6750` walks
**27,182 bytes per leader**, eight leaders per turn, and its op 2 is one contiguous
`[8, 26922)` range covering **276 named fields**, with nine run-time-length `Array<T>`
bodies and six `sub_object` calls the extractor cannot resolve. Closing it means
reproducing every player's economy, technology, diplomacy and score record at
`Game::init`, which is a lane's worth of work per *section*, not per channel.

`groups` is the only dead object channel that is not 21-of-21, and its repeated value
`0x1c78f3f5` occurs in **exactly the seven recordings with no computer players** — the
same seven that survive on `scenario_data` and `script_run_time`:

| recordings | AI players | `groups` first-turn value |
|---|---:|---|
| 7 | 0 | `0x1c78f3f5` (all seven) |
| 14 | 2–5 | 14 distinct values |

Setup-independence is the signature that made `ScenarioFuncSet::init` derivable. That is
the whole selection criterion.

---

## 1. The traversal — `CheckSums::check_groups` `0x00937530`

109 bytes, and it is two loops [`re/decomp-all/00937530.c`]:

```c
if (0 < groups.list.count) {              /* [0x00e85f14] */
    do { Group::walk_data(cs); } while (...);   /* 0x00708400, once per group */
}
adler = cs->checksum;                     /* CheckSum +0x10 */
for (i = 0; i < 0x20; i += 4)
    adler = adler32(adler, (char *)groups.const_last_group + i, 4);
cs->checksum = adler;                     /* +0x10 only */
```

Four things in there are load-bearing.

**`0x00e85f10` is `Groups groups`** [`schema/rise-symbols.tsv`:
`?groups@@3VGroups@@A`]. `GroupsData::list` is an `Array<Group>` at offset 0, whose
`ArrayBaseMaster` begins at `+0x04`, so `[0x00e85f14]` is the element **count** and
`[0x00e85f20]` is the element pointer. `GroupsData::last_group` is `int[8]` at `+0x1c`
(`0x00e85f2c`) and `GroupsData::const_last_group` is the `const int*` at `+0x3c`
(`0x00e85f4c`), which `Groups::Groups` `0x00713ff0` sets to `&last_group`. So the
32-byte tail is `last_group`, reached through the pointer.

**The tail bypasses the visitor.** It calls `adler32` directly — `0x005089d0`, the
second of the binary's two byte-identical copies of the routine (the other, `0x00a46830`,
is the one the oracle pins at Tier B) — and writes back only `CheckSum+0x10`. It never
touches `+0x14`, so **retail's own byte counter under-reports this channel by 32.** The
hash is unaffected. `groups_channel::RETAIL_BYTE_COUNTER_SHORTFALL` records this so the
two counts cannot be silently conflated.

**`proc_group` is not in the checksum.** `Groups::walk_data` `0x00713e30` — the
**SaveGame** walker, a different function — emits a tag, walks `last_group` by immediate
address *and* walks `proc_group` (`0x00e85f50`). `check_groups` walks neither the tag nor
`proc_group`. This is the `docs/tracks/replay-validation.md` hazard "sim-critical is a
strict subset of save-game state", instantiated.

**The standing `Array<T>` hazard does not apply here.** `Groups::walk_data` opens with
`Array<Group>::walk_data` `0x0047ea30`, which walks the count, the allocated size, the
two-byte growth increment at `+0x0c` and a flags byte — exactly the capacity-and-growth
metadata that desyncs a `Vec`. `CheckSums::check_groups` **does not call it**: it reads
the count as a loop bound and walks elements only. Whoever ports `Groups` for the save
format still owns that hazard; channel 5 does not.

### `Group::walk_data` `0x00708400`

```c
walk(this + 4, this + 0x4c);                       /* 72 bytes, unconditional */
if (this->num != 0) {                              /* +0x0c */
    walk(this + 0x8cc, this + (num + 0x466) * 2);  /* list   short[num] */
    walk(this + 0x4c,  this + (num + 0x13)  * 4);  /* off_x  int[num]   */
    walk(this + 0x24c, this + (num + 0x93)  * 4);  /* off_y            */
    walk(this + 0x44c, this + (num + 0x113) * 4);  /* curr_x           */
    walk(this + 0x64c, this + (num + 0x193) * 4);  /* curr_y           */
    walk(this + 0x84c, this + num + 0x84c);        /* angles char[num] */
}
```

The six ends are computed from `num` at run time, which is why
`crates/don-replay/src/walk_gen.rs`'s generated `Group` spec resolves op 0 and marks the
other six `Unresolved`. `groups_channel::group_ops_agree_with_the_generated_table` pins
this module's op 0 against the generated one, so the hand-derived half cannot drift from
the extraction.

The unconditional window `[4, 0x4c)` is exactly 21 declared `GroupData` fields with no
padding and no hole — `id`, `army`, `num`, `form`, `stamp`, `ox`, `oy`, `o_dist`,
`o_angle`, `disband`, `order_num`, `priority`, `role`, `think_frame`, `new_speed`,
`speed`, `form_num`, then the four bytes `facing`, `buildings`, `who`, `march`. The
`GroupData` vftable pointer at `+0` is **not** walked.

---

## 2. The initializer — `Groups::clear` `0x00713f20`

Called from `Game::init` `0x0058c480`, and also from `Game::run` `0x00584590`,
`Game::init_rules_and_teams` `0x00589bb0`, `Setup::build_game` `0x005ac190` and
`ScenarioEditor::generate_map` `0x009a0950` [full-tree call scan of `0x00713f20`].

```c
if (groups.list.count < 0x200) {
    groups.list.count = 0;
    groups.list.cur_index = 0xffff;
    groups.list.flags = 0;
    if (groups.list.size < 0x200) Array<Group>::init();   /* 0x0047e8a0 */
    groups.list.count = 0x200;                            /* 512 slots */
}
i = 0;
do {
    Group::clear(i);                     /* 0x00713e80, this = &list[i] */
    i += 1;
    list[i].stamp    = 0;                /* +0x14, re-zeroed */
    list[i].priority = 0;                /* +0x30, re-zeroed */
} while (offset + 0x9d4 < 0x13a800);     /* 0x13a800 = 512 * sizeof(Group) */
last_group[0..8] = { 0, 0x40, 0x80, 0xc0, 0x100, 0x140, 0x180, 0x1c0 };
proc_group = 0;
```

Two numbers in that function agree with each other and neither was assumed: the forced
count `0x200` and the loop bound `0x13a800 = 0x200 * 0x9d4`, where `0x9d4 = 2516` is the
PDB `sizeof(Group)`. And `last_group[who] = who * 0x40` partitions the 512 slots into 64
per player across eight players — the same 512.

### `Group::clear` `0x00713e80`

```c
if (0 <= id_arg) this->id = id_arg;      /* +0x04 */
this->who = 0;                           /* +0x4a */
this->army = -1;                         /* +0x08 */
this->num = 0;                           /* +0x0c */
this->form = -1;                         /* +0x10 */
this->stamp = *(Game + 0x550);           /* +0x14 — the only non-constant store */
this->ox = this->oy = this->o_dist = this->o_angle = 0;
this->facing = this->buildings = 0;      /* +0x48 as one word */
this->disband = this->order_num = this->priority = this->role = 0;
this->new_speed = this->speed = this->form_num = this->think_frame = 0;
this->march = 0;                         /* +0x4b */
```

Every one of the 72 walked bytes is written. The single non-constant store, `stamp` from
`Game+0x550`, is the store `Groups::clear` immediately erases — which is why the initial
state has no dependence on the `Game` singleton at all, and why 512 slots hash to the
same value in every game ever recorded.

`Group::clear` reached from anywhere *else* keeps that store, so
`GroupWindow::cleared_by_groups_clear` is named for the path it models and is deliberately
not a general `Group::clear`.

---

## 3. The measurement

A cleared slot has `num == 0`, so it walks its 72-byte window and nothing else.

```text
512 slots x 72 bytes  = 36,864
last_group tail       =     32
                        ------
                        36,896 bytes  ->  adler32 = 0x1c78f3f5
```

`crates/don-replay/src/groups_channel.rs`'s
`the_derived_game_init_groups_state_is_the_value_retail_carries`, and
`crates/don-replay/tests/groups_channel_initial.rs`. **There were no free parameters**:
every byte is a store in `Groups::clear` or `Group::clear`, the slot count and the eight
`last_group` literals are immediates in `Groups::clear`, and the recorded wire checksum
is never an input to the producer. One 32-bit comparison, made once.

Three bite tests keep that from being an accident: changing `id` on the 512 slots,
changing the slot count by one in either direction, and incrementing any one of the eight
`last_group` entries each move the channel while leaving the byte count identical.

**This agreement is substantive, not empty-state.** An absent channel walks zero bytes and
reads 1; this one hands the visitor 36,896 real bytes on every comparison, and the
scoreboard records `trivial = 0`, `unmodelled = 0` and `retail_empty_compares = 0` for it.
That last number matters: retail's own `groups` value is never 1, because retail always
has 512 slots.

---

## 4. The deadline, measured per recording

`SimBridge::populate_groups_live` now installs the independent walk of the canonical
tick-owned `Sim.groups` pool. `Sim::new` creates that pool through the instruction-derived
`retail_fresh_groups`, and the replay harness advances that owner through `Sim::do_frame`'s
29-step scheduler, including the single `Groups::process` `0x006fa210` call at step 12, before
re-projecting channel 5. The scheduler's current callback shell conservatively returns
`MemberState::Keep` and no leader speed; exact object-leaves-group and leader-speed authority
remain open. That is a simulation residual, not a checksum shortcut: a direct mutation of any
walked field or `last_group` word changes the scoreboard value, and an invalid slot count clears
the producer rather than leaving a stale exact checksum.

The corpus result remains the same, for a measured reason rather than because the checksum is
frozen. The canonical replay package hosts require post-worldgen Unit identity, content,
formation, order and path authority. The replay setup pipeline cannot yet produce those facts,
so the live pool remains at its initializer image until the first unowned retail mutation.
[`replay-groups-pre-pair-unit-authority.md`](replay-groups-pre-pair-unit-authority.md) pins that
boundary down to the exact missing receipts and preceding package chronology.

The corpus splits on exactly the same seven recordings as channels 14 and 15, and the
deadline is short — groups are the most-commanded object in the game
(`GroupCommand` `0x00` is 78,197 of the corpus's commands, against 28,993 for the next
player-order opcode, `QueueUpCommand`):

| recording | AI | `groups` survived | first divergence | `scenario_data` survived |
|---|---:|---:|---:|---:|
| 2024.02.23 20:49 | 0 | **64** | turn 66 `0x22a5074d` | 1,111 |
| 2019.03.24 | 0 | **18** | turn 20 `0x624df46d` | 4,922 |
| 2024.03.29 21:52 | 0 | **15** | turn 17 `0x1feaf88c` | 37 |
| 2018.11.17 | 0 | **14** | turn 16 `0x1118f44b` | 6,320 |
| 2020.02.21 | 0 | **14** | turn 16 `0x11d9f454` | 4,210 |
| 2018.12.01 | 0 | **8** | turn 10 `0x123cf3cf` | 6,059 |
| 2020.02.08 | 0 | **7** | turn 9 `0xdffdf4f4` | 1,572 |
| the other 14 | 2–5 | **0** | turn 2 | 1 |

Corpus totals: `groups` goes from 0 to **140 matches** over **222,938 non-trivial
compares**, best survival **64** turns. All 140 are real byte-against-byte comparisons;
none is trivial and none is unmodelled.

Two observations worth keeping:

- **The AI recordings diverge one turn earlier than they do on `scenario_data`.** Channel
  14 survives turn 2 on those files and dies on turn 3; channel 5 is already wrong on
  turn 2. So whatever the AI does before the first checksummed turn touches `Groups` but
  not `ScenarioData` — group creation precedes the first `ScenarioFuncSet` builtin write.
- **The human recordings die by turn 9–66**, i.e. within the first few seconds of play,
  and always at a turn where that game's command stream carries player orders. This is
  not a subtle expiry: it is the first `GroupCommand`.

---

## 5. What this does *not* establish, and what comes next

- It is **not yet retail-package evidence about group mechanics**. It proves that the
  canonical `Sim.groups` owner, its per-frame process pass, and the independent checksum
  projection are one live path. The real corpus still cannot reach `Group::add`,
  `Groups::push_group` or a `Group::action_*` without the missing setup Unit authority.
- Once live groups reach scheduled normalization, `SimGameDaemonHost::groups_process` still
  needs the source-backed object-removal predicate and leader-speed callback. The live owner
  closes the checksum bridge; it does not make those two absent inputs exact.
- The 140 matches are a small number and should be quoted as one. `rules` still walks
  997,846 bytes on 222,938 agreeing compares and remains the headline.
- **The next increment is the setup-to-package authority join.** The full move-near body,
  canonical Group transaction and channel walker now exist. The earliest strict witness still
  needs real `Setup::place_unit` / `Objects::init_unit` receipts, eight preceding Farm package
  transactions and their intervening frames; the clear-pool witness needs the four exact
  starting Citizen objects. Selecting either set from its eventual checksum would be fitting,
  not replay reconstruction.
