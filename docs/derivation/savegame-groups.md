# The group pool as save state

Lane: `save-groups` (`closure/stage: save_load`, wave 4). Target:
`ron-bin/riseofnations.exe` (PE32 i386, image base `0x00400000`), `ron-bin/sbl/rise.pdb`
(GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1). Every claim below is **[measured]**
(read here, at instruction level, with `capstone` over the shipped image) or **[PDB]** (a
name or a size taken from the private symbol/type stream, which names things and does not
establish behaviour). Nothing here is Tier A or Tier B; see `docs/CHARTER.md`.

`docs/derivation/savegame.md` established the general grammar and listed `Array<Group>` in
`WalkDataGame::walk_data`'s 91-op table of contents. This document is the one section, at
full resolution, and the reason `don-sim` could not save a game that had ever selected a
unit.

---

## 0. Why this section and not another

`crates/don-sim/src/systems/save_load.rs` is a working, deterministic DoNSave writer and
reader with a save → load → resave → resume determinism test. Its `reject_unsupported`
gate refused any `Sim` whose group pool differed from `Groups::default()`. That is not a
corner: `Sim::groups` is written by `tick.rs`'s `groups_process` (retail
`Groups::process` `0x006FA210`) on **every** frame, and again by step 11's
`Armies::leader_defeated` group halt. Any match that has ever formed, moved, or halted a
group was unsaveable, and the refusal was the single largest hole in the stage.

---

## 1. The three functions

All three disassembled here [measured].

### 1.1 `Group::walk_data` `0x00708400`

```
00708400  push ebp / mov ebp,esp / push ebx,esi,edi
00708406  mov edi, [ebp+8]                  ; the DataWalk visitor
00708409  mov esi, ecx                      ; this
0070840f  lea ebx, [esi+0x4c]
00708417  call [edx]                        ; walk(this+0x04, this+0x4C)     72 bytes
00708419  mov eax, [esi+0x0c]               ; num
0070841e  je  0x7084ae                      ; num == 0 -> done
00708438  call [edx]                        ; walk(this+0x8CC, this+(num+0x466)*2)
00708449  call [edx]                        ; walk(this+0x04C, this+(num+0x013)*4)
00708462  call [edx]                        ; walk(this+0x24C, this+(num+0x093)*4)
0070847b  call [edx]                        ; walk(this+0x44C, this+(num+0x113)*4)
00708494  call [edx]                        ; walk(this+0x64C, this+(num+0x193)*4)
007084ac  call [edx]                        ; walk(this+0x84C, this+0x84C+num)
007084b2  ret 4
```

The five `(num + K) * S` end addresses are the compiler folding `base + num*S`:
`0x466*2 == 0x8CC`, `0x013*4 == 0x4C`, `0x093*4 == 0x24C`, `0x113*4 == 0x44C`,
`0x193*4 == 0x64C`. So each optional walk is exactly `num` elements of the array that
starts at its `begin`. Element widths follow from the scale: `list` is `i16`, the four
offset arrays are `i32`, `angles` is `i8`.

The layout closes arithmetically, which is the check that could have failed: the four
`i32` arrays are `0x200 == 128 * 4` bytes apart, `angles` is `0x80 == 128 * 1` bytes long,
and `0x8CC + 128*2 == 0x9CC`, inside `sizeof(Group) == 0x9D4` [PDB]. So
`GROUP_MAX_MEMBERS == 128`.

Field names for the 72-byte header come from the PDB and are already transcribed in
`crates/don-sim/src/systems/groups_guys.rs` (`GroupData`, `+0x04 id` … `+0x4B march`).

**Two properties, both load-bearing.** The walk is *length-prefixed by `num`* — so the
declared order of the section is `list` before the offsets, which is **not** the struct's
declaration order — and everything at or past `num` is invisible to it. §3 shows why the
second property is sound and not a loss.

### 1.2 `Array<Group>::walk_data` `0x0047EA30`

This is not a generic container walk. MSVC specialised the whole body on the `groups`
global, so it addresses fields absolutely and takes no `this` [measured]:

```
0047ea4b  mov eax, [0xe85f14]      ; length     -> walk(&tmp, &tmp+4)              4 B
0047ea69  jne 0x47ea8b             ; length == 0 -> nothing further on the save side
0047ea8b  mov eax, [0xe85f18]      ; size       -> walk(&tmp, &tmp+4)              4 B
0047eaa6  push 0xe85f1e / 0xe85f1c ; increment  -> walk(0xE85F1C, 0xE85F1E)        2 B
0047eab2  mov al, [0xe85f24]
0047eaba  and al, 0xbf             ; flags, bit 6 forced clear, in memory *and* in
0047eabd  mov [0xe85f24], al       ;   the stream -> walk(&tmp, &tmp+1)            1 B
0047ebf0  mov ecx, [0xe85f20]      ; the element array
0047ebfa  call 0x708400            ; Group::walk_data, stride 0x9D4, length times
```

`?groups@@3VGroups@@A` is at `0x00E85F10` (`schema/rise-symbols.tsv:36709`) [PDB], so
`length` is `Groups+0x04`, `size` `+0x08`, `increment` `+0x0C`, the element pointer
`+0x10`, `flags` `+0x14`. That is the `u32 len; [u32 size; u16 inc; u8 flags; elems]`
header `savegame.md` §3 records for every `Array<T>`, seen in its specialised form.

Note `and al, 0xbf` writes back: the walk **clears** `flags` bit 6 on the live object as a
side effect, it does not merely mask the copy.

### 1.3 `Groups::walk_data` `0x00713E30`

```
00713e39  call 0x47ea30                     ; Array<Group>::walk_data(this + 0)
00713e50  call [edx+4]                      ; walk_test(int_str_array[0xE420/20 == 2920])
00713e5c  push 0xe85f2c / 0xe85f4c
00713e61  call [eax]                        ; walk(Groups+0x1C, +0x3C)   last_group[8]
00713e6c  push 0xe85f50 / 0xe85f54
00713e71  call [eax]                        ; walk(Groups+0x40, +0x44)   proc_group
00713e75  ret 4
```

`walk_test` is slot 1 of `DataWalk` — the one-byte section tag, `String::module_id` of an
`int_str_array` entry (`savegame.md` §3). Its *name* is one of the 86 tag sites that
document lists as still open; the tag byte itself is not needed by a DoNSave section, which
has its own chunk ids.

---

## 2. The two names for `Groups+0x1C`, and the second `Groups` in the tree

`0x00E85F2C` is `Groups + 0x1C`. `don-sim` models that array **twice**:

| Rust | doc comment | address it names |
|---|---|---|
| `systems::groups_guys::Groups::last_group` | "`GroupsData::last_group : int[8]` `+0x1C`" | `0x00E85F2C` |
| `command::Groups::cur` | "`groups.cur[who]` at `0x00E85F2C + who*4`" | `0x00E85F2C` |

They are the same eight dwords. `Sim` owns the `groups_guys` one; `command::Bridge` owns
the other, and `Bridge` is constructed **only from test code** — no production path builds
one. So today the duplication is latent rather than a live desync, but whoever wires the
command bridge into the tick inherits two owners of one retail field, and a save owner has
to pick one. This section carries the `groups_guys` one, because that is the pool the tick
and the checksum channel both read.

`check_groups` reaches the same array through a *pointer*: `mov eax, [0xe85f4c]` then
`[esi + eax]`, i.e. `Groups+0x3C` is `const_last_group` and points at `Groups+0x1C`
[measured].

---

## 3. What retail's walk does not cover, and why that is not a loss

`Group::walk_data` stops at `num`. Two shipped behaviours settle whether the region
`[num, 128)` is state:

* **`Group::add` `0x00714350` resets the slot before publishing it.** Its transcription in
  `groups_guys.rs` (from the shipped body) writes
  `off_x[num] = off_y[num] = curr_x[num] = curr_y[num] = angles[num] = 0` **before**
  `list[num] = o; num++`. A slot therefore cannot be read with stale content: growth
  re-initialises.
* **`Group::update_positions` is the only shipped read past `num`, and it is write-only
  into that region.** Its loop bound is `form_num`, not `num`, and `form_num > num` is
  reachable — `Group::normalize` `0x00711540` compacts the arrays and decrements `num`
  without touching `form_num`, so losing one member to attrition leaves exactly that shape.
  The loop reads `off_x[i]`/`off_y[i]` and writes `curr_x[i]`/`curr_y[i]` for
  `i < form_num`; every *consumer* of the parallel arrays is bounded by `num`, so the
  excursion's outputs are never read.

`crates/don-sim/tests/save_load_groups.rs::member_slots_at_or_past_num_are_not_state`
drives that exact shape: it produces `form_num == 4, num == 3` through the real
`normalize`, fills `[num, 128)` with distinct garbage, and shows the `groups` channel and
the save bytes are unchanged, that `update_positions` diverges **only** at index `num` and
beyond, and that the next `add` zeroes the slot. The test asserts the divergence at index
`num` is real, so it cannot pass vacuously.

Conclusion: the DoNSave section writes the retail-covered prefix and reconstructs the tail
as zero. That is retail's semantics, not an approximation of it.

---

## 4. The section

`crates/don-sim/src/systems/save_load/groups.rs`, chunk id `0x0009`, DoNSave format version
10.

```
u32                       length, pinned to NUM_GROUPS == 8 * 64 == 512
512 x {
  72 B                    GroupData::header_bytes()  -- the retail Group::walk_data image
  if num != 0 {
    num x i16             list
    num x i32             off_x, then off_y, then curr_x, then curr_y
    num x i8              angles
  }
}
8 x i32                   last_group        (Groups+0x1C)
```

An empty pool is `4 + 512*72 + 32 == 36,900` bytes. The `512*72 + 32 == 36,896` inside it
is exactly the independently derived initial size of the `groups` checksum channel
(`docs/tracks/megaswarm-board.md`, `replay-groups` lane), from a different extractor — a
cross-check that could have failed and did not.

The 72-byte header is emitted as `GroupData::header_bytes()` verbatim, so the section and
the checksum channel consume the same image and cannot drift into two field orders.

**Deliberately not carried**, and each is a typed boundary rather than a claim of
inertness:

| retail | why not |
|---|---|
| `Array<Group>::size` `[0x00E85F18]`, `increment` `[0x00E85F1C]`, `flags` `[0x00E85F24]` | `groups_guys::Groups::list` is a fixed 512-slot `Vec`, not a growing `Array<Group>`; there is no value in `don-sim` to write. A `.svx` writer needs all three. |
| `proc_group` `[0x00E85F50]` | already owned by DoNSave's `CORE` section, with its own range check. Retail walks it here; DoNSave walks it there. |
| the `walk_test` tag byte | DoNSave has chunk ids; the retail tag's *name* is one of the 86 unresolved `int_str_array` sites in `savegame.md` §3. |

Validation is structural only — `0 <= num <= 128`, `0 <= form_num <= 128`, `who < 8`, pool
cardinality 512 — and deliberately does **not** check that a member names a live object.
Retail groups legitimately carry dead members: `Groups::process` revisits one slot index per
player per frame, so a group can name a dead unit for up to 64 frames, and `normalize`
treats a negative `list[i]` as a tombstone rather than as corruption. A liveness gate here
would refuse states the engine produces.

---

## 5. What moved, and what still blocks the stage

The `groups` refusal is gone. A `Sim` with a live pool now saves, loads, resaves to
identical bytes, and resumes:
`a_live_group_pool_round_trips_and_resumes_through_seventy_real_frames`
saves a two-player match with a four-member group each, reloads it, and runs
**70** frames on both the original and the reloaded sim, asserting the `groups` channel and
the whole-sim channel digest agree after every frame. 70 exceeds the 64-slot
`Groups::process` cursor period, so `Group::normalize` and `Group::compute_speed` both run
on the restored pool; the test also asserts the pool actually changed over those frames, so
a no-op resume cannot pass it.

**What still blocks a mid-match save is no longer the group pool. It is the PlayerSetup
owner's frame-zero boundary.** `canonical_player_setup_snapshot` in `save_load.rs` returns
`Unsupported("player setup after the frame-zero boundary")` whenever
`sim.vic_leaders.setup_owner.applied().is_some()` and `world.frame != 0`, because the
`PLAYER_SETUP` section is *reconstructive*: it stores the `ManualPlayerSetup` request and
replays `Sim::start_manual_player_setup` on load. Replay is only correct while none of the
rows that transaction writes has evolved. And a `Sim` **without** that owner cannot have an
active leader at all, because `leader_is_supported` requires
`leaders[who].active == setup_owner.configured_mask() & (1 << who)`, which is `0` with no
owner.

Together those two facts mean DoNSave can currently save exactly two shapes: a player-less
sim at any frame, or a fully set-up sim at frame 0 — never a set-up sim mid-match.
`a_set_up_match_still_cannot_be_saved_past_frame_zero` pins that as a fact rather than
prose. Closing it means giving the derived rows real save owners instead of replaying the
transaction: `vic_leaders.slots[*].diplos` / `init_diplomacy` / `has_preq_2b0`,
`vic_match.options` / `on_team` / `num_sides` / `semaphore` / `frame`. Those are
`victory_score::LeaderState`, which `save_load/step8_views.rs` already names repeatedly as
having no DoNSave chunk, and which the `victory-endgame` lane holds.

The other named refusals in `reject_unsupported`, unchanged by this lane and each still
blocking a real match: step-12 visibility authority, cannon-time state, walls, herds,
projectile type rules, the live/consumed projectile pool, live death records, aircraft
crash hosts, the Wonder lifecycle, and installed rule overrides.

---

## 6. Ledger entries for `docs/provenance-ledger.md`

| mechanic | source | tier | evidence |
|---|---|---|---|
| `Group::walk_data` = 72-byte header, then six `num`-length member arrays in the order `list, off_x, off_y, curr_x, curr_y, angles` | `0x00708400` | **C** [measured] | disassembled; the five `(num+K)*S` folds resolve to `base + num*S`; layout closes inside `sizeof(Group) == 0x9D4` |
| `Array<Group>::walk_data` is whole-program specialised on the `groups` global and writes `length, size, increment, flags & 0xBF` before the elements | `0x0047EA30` | **C** [measured] | absolute operands `0xE85F14/18/1C/20/24`; element stride `0x9D4` |
| `Groups::walk_data` = `Array<Group>`, tag, `last_group[8]`, `proc_group` | `0x00713E30` | **C** [measured] | `0x00E85F2C..0x00E85F4C` and `0x00E85F50..0x00E85F54` against `?groups@@3VGroups@@A` at `0x00E85F10` |
| `Groups+0x3C` is `const_last_group`, a pointer to `Groups+0x1C` | `0x00937530` | **C** [measured] | `check_groups` loads `[0xE85F4C]` and indexes it; `Groups::walk_data` walks `0x00E85F2C` directly |
| `Array<Group>`'s length/size/increment/flags are **save**-critical but **not** checksum-critical | `0x00937530` vs `0x0047EA30` | **C** [measured] | `check_groups` addresses only `[0xE85F14]` and `[0xE85F20]`; it never reads `+0x08/+0x0C/+0x14` |
| member slots at or past `num` are not simulation state | `0x00714350`, `0x00711540`, `Group::update_positions` | **C** [measured] | `add` zeroes the slot before publishing it; the only read past `num` writes only past `num`; driven in `save_load_groups.rs` |
| DoNSave saves a set-up match only at frame 0 | `save_load.rs` `canonical_player_setup_snapshot` | implementation boundary | `a_set_up_match_still_cannot_be_saved_past_frame_zero` |
