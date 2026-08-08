# The lockstep checksum, and therefore the definition of sim-critical state

Lane: `checksum`. Every claim below is marked **[measured]** (I verified it against
`ron-bin/riseofnations.exe` sha256 `30478a44…625079`, or against retail machine code
executing on hbox) or **[reported]** (read somewhere, not checked). Nothing here is
"verified" in the proof-assistant sense — see `docs/CHARTER.md`.

---

## Headline

**The lockstep checksum and the save-game serializer are the same visitor.** `CheckSum`,
`SaveGame` and `LoadGame` are sibling subclasses of one two-method pure-virtual interface
`DataWalk`, confirmed from the RTTI class-hierarchy descriptors [measured]. So
*sim-critical state ≡ save-game state*, and every `walk_data` implementation in the binary
is a machine-readable declaration of which bytes of a class are simulation state.

The hash is **adler-32** — confirmed statically *and* by executing the retail routine
[measured, Tier B].

The channel list reported to us is **confirmed exactly**, including `scenario_data` and
`script_run_time` [measured].

The inference that "the same descriptor tables that bind XML rule names also drive the
checksum" is **refuted** [measured]. They are two unrelated visitor interfaces. The
replacement finding is better: the checksum visitor yields *byte ranges*, which delimit
sim-critical state directly.

---

## 1. Where it lives

| thing | address / file | how found |
|---|---|---|
| `checksums.cpp` assert-file literal | `0x00af6adc` … `0x00af7318`, 56 copies | ASCII string scan |
| `check_all` — the whole-state checksum | **`FUN_00936560`** (`0x00936560`, 1614 bytes) | 51 of the 56 `checksums.cpp` literals are referenced from it; it pushes the UTF-16 literal `check_all` at `0x00af6ac8` |
| `process_check_sums` — the compare side | **`FUN_009459d0`** (`0x009459d0`, 1098 bytes), `commandpackage.cpp` | references UTF-16 `process_check_sums` at `0x00af7f28` |
| adler-32 primitive | **`0x00a46830`** | tail call from `CheckSum`'s virtual |
| `CheckSum::` walk-bytes virtual | **`0x00936ff0`** | `CheckSum` vftable slot 0 |
| `Skybox_SyncLogger.cpp` | `SyncLogger` = `FUN_00a2e880`, config parse `FUN_00a2f580` / `FUN_00a30080` | 42 copies of the file literal, 33 + 9 of them in those two functions |

RTTI type descriptors present [measured]: `.?AUDataWalk@@` @ `0x00c952e0`,
`.?AUCheckSum@@` @ `0x00c96814`, `.?AVSyncDisplay@@` @ `0x00c95884`,
`.?AV?$ObjectArray@URandomLogEntry@@@@` @ `0x00c9425c`.

Reproduce (from `/Users/ember/dev/don/ron-bin`):

```sh
uv run --quiet --with pefile --with capstone python - <<'PY'
import pefile
from capstone import *
pe = pefile.PE("riseofnations.exe"); base = pe.OPTIONAL_HEADER.ImageBase
img = pe.get_memory_mapped_image(); md = Cs(CS_ARCH_X86, CS_MODE_32)
ea = 0x00936560
for i in md.disasm(img[ea-base:ea-base+1614], ea):
    print(f"{i.address:08x}  {i.mnemonic:<8} {i.op_str}")
PY
```

---

## 2. The hash algorithm: adler-32 — confirmed, not adler-*ish*

`0x00a46830` is **zlib's `adler32`, `__fastcall`** [measured].

Convention recovered from the instruction stream: `ECX` = running checksum, `EDX` = buffer,
one stack dword = length; callee does not clean (`ret`, and the call site does `add esp,4`).

Structural fingerprints, all read directly off the disassembly:

| zlib feature | instruction evidence at |
|---|---|
| `if (buf == NULL) return 1` | `0xa4683d test edi,edi` → `0xa46842 lea eax,[edx+1]` (edx is 0 there) |
| `s1 = adler & 0xffff`, `s2 = adler >> 16` | `0xa46837 movzx esi,cx` / `0xa4683a shr ecx,0x10` |
| `NMAX = 5552` | `0xa46854 mov edx, 0x15b0` |
| `DO16` unrolled block | `0xa46880`–`0xa46905`, sixteen `movzx`/`add` pairs, `shr ebx,4` trip count |
| `% 65521` | `0xa4691f mov eax,0x80078071; mul; shr edx,0xf; imul eax,edx,0xffff000f; add` — `0xffff000f` is `-65521` signed, i.e. `x -= (x/65521)*65521` |
| return `(s2<<16) \| s1` | `0xa4694c shl ecx,0x10; or ecx,esi` |

**Behavioural confirmation [measured, Tier B].** I mapped the retail image on hbox and
called `0x00a46830` directly, comparing against a Rust adler-32 written from the algorithm
above.

- Harness: a **private copy** of the oracle at `hbox:~/don-oracle-checksum` (the shared
  `~/don-oracle` was not modified — other lanes are live in it). New subcommand `adler`.
  Local copy of the patch: `/private/tmp/.../scratchpad/chk/adler_patch.py`.
- `nice -n 15 taskset -c 0-3 ./target/i686-unknown-linux-musl/debug/oracle adler 500000`
- Input distribution: 12 hand-chosen lengths straddling both structural boundaries
  (`0, 1, 2, 15, 16, 17, 31, 5551, 5552, 5553, 11104, 11105`), then uniform random lengths
  in `[0, 24576]` with uniform random bytes and a uniform random 32-bit initial value,
  from a fixed xorshift seed `0x9E3779B97F4A7C15`. Plus the `buf == NULL` case.
- **Result: 500,000 cases, 0 mismatches, 0 faults.** NULL-buffer call returns `1`, as the
  disassembly predicts. (An earlier 40,000-case run also gave 0/0.)

This is Tier B. It is testing, not verification.

**`adler32` is called from only 8 sites in the whole image** [measured] — 6 of them are the
inlined `CheckSum` fast paths at `0x00936ff0`, `0x00937040`, `0x009370a0`, `0x009370f0`;
the other 2 are in `FUN_005a76b0`, which is the save/load "Incorrect Load Checksum" path.
Same primitive for saves and for desync detection.

---

## 3. `DataWalk` is a 2-method visitor, and `CheckSum` / `SaveGame` / `LoadGame` are siblings

This is the load-bearing result.

`DataWalk`'s vftable is at **`0x00b2bcd8`**; both of its slots point to `0x0055e0a6`, which
is the IAT thunk for `VCRUNTIME140!_purecall` [measured]. So **`DataWalk` is abstract with
exactly two virtual methods**:

| slot | offset | signature (from `CheckSum`'s implementation) |
|---|---|---|
| 0 | `+0x00` | `walk(void* begin, void* end)` — `__thiscall`, `ret 8`. Hashes `[begin,end)`. |
| 1 | `+0x04` | `walk_tag(void* p)` — `__thiscall`, `ret 4`. **`CheckSum` implements it as a bare `ret 4` (no-op)** at `0x0041bfe0`; save/load use it. |

`CheckSum`'s vftable is at **`0x00b3f920`** = `{0x00936ff0, 0x0041bfe0}` [measured].
`0x00936ff0` is nine instructions:

```
00936ff0  mov edx,[ebp+8]        ; begin
00936ff7  mov esi,[ebp+0xc]      ; end
00936fff  sub esi,edx            ; len
00937004  mov ecx,[edi+0x10]     ; this->checksum
00937007  add [edi+0x14],eax     ; this->bytecount += len
0093700a  call 0xa46830          ; fastcall(ecx=checksum, edx=begin, len)
00937012  mov [edi+0x10],eax
```

### The hierarchy, from the RTTI class-hierarchy descriptors [measured]

```
DataWalk                       (abstract, 2 pure virtuals)
├── CheckSum                   vft 0x00b3f920
└── WalkDataGame
    ├── SaveGame               vft 0x00b35ac4, slot0 = 0x0043d730
    │   └── ConquestSaveGame   vft 0x00b2d4f8, slot0 = 0x0043d730 (shared)
    └── LoadGame               vft 0x00b30c88, slot0 = 0x0043d950
        └── ConquestLoadGame
```

`SaveGame` / `LoadGame` also inherit `GameAccess` and `MiscAccess`. **Exactly six classes in
the whole binary have `DataWalk` in their base list**, and those are they.

Method: parse every `RTTICompleteObjectLocator` in `.rdata`, follow
`pClassHierarchyDescriptor → pBaseClassArray → pTypeDescriptor`, and select the classes
whose base list contains `.?AUDataWalk@@` (`0x00c952e0`). 1,510 class hierarchies parsed.

**Consequence: sim-critical state ≡ save-game state.** Anything the engine bothers to save
is checksummed by the identical traversal; anything it does not save is, by construction,
not part of the lockstep-critical state. That is the definition we wanted, and it is now
one traversal rather than a guess.

### `DataWalk` object layout [measured]

Reconstructed from the two constructors inlined at the top of `check_all` and from the
field uses in the walkers:

| offset | meaning | evidence |
|---|---|---|
| `+0x00` | vftable | `mov [ebp-0x28], 0xb2bcd8` then `0xb3f920` at `0x0093657e` / `0x009365a1` |
| `+0x04` | **direction: non-zero ⇒ reading/deserialising** | `cmp dword ptr [esi+4],0` at `0x00647885` selects the allocate-and-construct branch; `CheckSum` sets it to 0 |
| `+0x08` | set to 0 by `DataWalk`'s ctor, **1** by `CheckSum`'s | `0x00936585` / `0x009365a8` |
| `+0x0c` | **section mask** — gates optional sub-walks | `check_units` sets it to `-1` (`0x009371fd`); `Unit::walk_data` does `test byte ptr [edi+0xc], 2/4/8` |
| `+0x10` | running adler-32 (**initialised to 1**) | set to 1 before each channel, read after |
| `+0x14` | bytes-walked counter (initialised to 0) | `add [edi+0x14], len` |

The `+0x0c` mask is important and easy to miss: **the checksum has selectable sub-sections
per object**, so "the checksum covers X" is only true for a given mask value.

---

## 4. The channels — the [reported] list is confirmed exactly

`check_all` (`0x00936560`) walks fifteen channels in this fixed order, logging each, and
finally a `total` [measured]:

| # | channel | walker | notes |
|---|---|---|---|
| 1 | `units` | `0x009371d0` | per-leader unit lists |
| 2 | `builds` | `0x00937290` | dispatch via object vtable `+0xac` |
| 3 | `walls` | `0x00937360` | dispatch via object vtable `+0xb0` |
| 4 | `ammo` | `0x009374e0` | |
| 5 | `deaths` | `0x00936bb0` | |
| 6 | `groups` | `0x00937530` | |
| 7 | `guys` | `0x00937430` | |
| 8 | `leaders` | inline loop, 8 × `0x006d6750` | base `0x00e3a390`, stride `0x6eec` |
| 9 | `cities` | `0x00937600` | |
| 10 | `items` | `0x00937790` | |
| 11 | `goods` | `0x00937710` | |
| 12 | `world` | `0x006b5cf0(w, -1)` | **conditional**: only if `[[0x00c06188]+0x134] != 0` |
| 13 | `rules` | `0x00589550` | |
| 14 | `scenario_data` | `0x00997ad0` | |
| 15 | `script_run_time` | `0x009c41a0` | |
| — | `total` | `edi` | plain 32-bit **sum** of the fifteen values, not a hash of hashes |

The `all` of the [reported] list is `check_all` itself. So the reported set
`units, builds, walls, ammo, deaths, groups, guys, leaders, cities, items, goods, world,
rules, scenario_data, script_run_time, all` is **confirmed, complete, and in the right
order** [measured].

Two things worth noting because they are easy to get wrong:

- `total` is an ordinary wrapping **addition** of the fifteen channel checksums
  (`add edi, [ebp-0x18]` after each), not adler-32 over their concatenation.
- **The `leaders` channel covers 8 leaders; the object channels cover 9.** The `leaders`
  loop at `0x0093686b` runs `ebx = 0x00e3a390` while `ebx < 0x00e71af0`, stride `0x6eec` —
  exactly 8 iterations. `check_units` (`0x00937268`) uses the bound `0x00e789dc`, which is
  `0x00e71af0 + 0x6eec` — exactly 9. So the leader array has 9 slots (8 players plus one
  more, presumably nature/Gaia), and the 9th slot's *own* record is not checksummed even
  though the units/builds/walls/guys it owns are. Measured, not explained.
- `check_all` **returns the last channel's checksum, not the total**: `total` is stashed in
  `[ebp-0x10]` and logged, and the epilogue at `0x00936b8c` does `mov eax,[ebp-0x18]`,
  which still holds `script_run_time`. I am reporting this as observed; I have not
  established whether any caller uses the return value, so I am not calling it a bug.

### The wire format of `CheckSumsCommand` [measured]

Recovered from `process_check_sums` (`0x009459d0`), which reads the packet at `[ebp+8]`:

| field | packet offset | | field | packet offset |
|---|---|---|---|---|
| *(opcode / header byte)* | `+0x00` | | `cities` | `+0x21` |
| `units` | `+0x01` | | `items` | `+0x25` |
| `builds` | `+0x05` | | `goods` | `+0x29` |
| `walls` | `+0x09` | | `world` | `+0x2d` |
| `ammo` | `+0x0d` | | `rules` | `+0x31` |
| `deaths` | `+0x11` | | `scenario_data` | `+0x35` |
| `groups` | `+0x15` | | `script_run_time` | `+0x39` |
| `guys` | `+0x19` | | | |
| `leaders` | `+0x1d` | | **total size** | **`0x3d` = 61 bytes** |

Fifteen unaligned little-endian `u32` after a one-byte header. Note `rules` is present here
but the *log line order* differs from `check_all`'s — in `check_all`, `world` is channel 12
and `rules` 13; in the packet they are at `+0x2d` and `+0x31`, same order. Consistent.

### `scenario_data` is not what a name-match would suggest

There are `scenario_data` / `scenario_checksum` / `mod_checksum` strings all over `.rdata`
(e.g. `scenario_data` UTF-16 at `0x00b18ccc`; `scenario_checksum` **ASCII** at `0x00b18cf0`;
`mod_checksum` **ASCII** at `0x00b18c34` — the block mixes encodings, so search both). Those
are **Steam-lobby metadata keys**, sitting in a contiguous block with `starting_resources`,
`pop_limit`, `elorank`, `mod_workshop_id`, `qm_ingame` [measured]. They are unrelated to the
checksum channel of the same name, whose UTF-16 label lives at `0x00af7120`. Do not conflate
them.

---

## 5. What is actually walked: byte ranges, not named fields

### The refutation

`docs/binary-ground-truth.md` carries this inference: *"because the binding is a visitor
vtable call rather than direct XML parsing, the same descriptor tables are plausibly reused
for save-game serialization and possibly the lockstep checksum."*

**Half right, and the wrong half was the load-bearing one** [measured]:

- **Save/load ≡ checksum: TRUE**, and much more strongly than "plausibly" — same base class,
  same two virtuals, same `walk_data` implementations (§3).
- **Rules-descriptor tables ≡ checksum: FALSE.** They are different interfaces on different
  hierarchies:

| | rules loader | checksum / save |
|---|---|---|
| visitor class | `TypeOut` (vftable `0x00b43da4`, **40 slots**) | `DataWalk` (vftable `0x00b2bcd8`, **2 slots**) |
| method called | slot 7, **`vtable+0x1c`** = `0x00470780`, binds `(name, tag, field)` | slot 0, **`vtable+0x00`**, takes `(begin, end)` |
| implemented on | `UnitTypeData`, `BuildTypeData`, `ObjectType`, `GoodTypeData`, `ItemType`, … at **vtable `+0xc8`** | `Unit`, `Build`, `Wall`, `Good`, `Item`, `Animal`, `ObjectData`, … at **vtable `+0x7c`** |
| `TypeOut` in `DataWalk`'s base list? | **no** | — |

Concretely: `FUN_0065fc00` (the 35-name combat-stats loader) is vtable slot `0xc8` of ten
`*Type*` classes; `FUN_0061c490` is slot `0xc8` of `UnitType`. Those are *type* classes —
the static rule data. The checksum walks *instance* classes. Different hierarchy, different
interface, different slot.

So "enumerate the callers of the descriptor pattern and you get the sim-state schema" does
**not** hold. The correct sweep is: **enumerate vtable slot `+0x7c` and follow the
`walk(begin,end)` calls.**

*(Side note for the loader lane: the settings binder `FUN_005d6040` binds `checksum_deep` →
`this+0x08`, `checksum_window_size` → `this+0x0c`, `checksum_failure_threshold` →
`this+0x10`, via the same `call [eax+0x1c]` shape — and the third argument really is
`push dword ptr [ebx+8]`, a **load**, not a `lea`. That is a second independent instance of
the open question flagged in `binary-ground-truth.md`; it is not an artefact of one
decompilation.)*

### The sim-state schema, first order

Extractor written for this lane, committed at
**`/Users/ember/dev/don/docs/derivation/extract_datawalk.py`**. It linear-scans every
function, tracks which registers hold `this` (entry `ECX`) or `lea this+disp`, and records
the two pushes before each `call dword ptr [reg]`. Note the argument order: the callee is
`__thiscall`/`stdcall` with args pushed right-to-left, so **the last push is `begin`**.

```sh
cd /Users/ember/dev/don/ron-bin && uv run --quiet --with pefile --with capstone \
  python /Users/ember/dev/don/docs/derivation/extract_datawalk.py
```

Yield: **106 functions with at least one clean `this`-relative range, 183 ranges total.**
Of those, 8 are bound to vtable slot `+0x7c`, i.e. are `walk_data` overrides:

| function | classes at vtable `+0x7c` | byte ranges walked |
|---|---|---|
| `0x0060cf40` | `Unit` | `[0x48, 0xb7)` — 111 B |
| `0x00647830` | `UnitData`, `UnitOut`, `Object` | `[0x20, 0x42)` — 34 B |
| `0x006621d0` | `ObjectData`, `ObjectOut`, `SubObject`, `GoodData`, `GoodOut`, `ItemData`, `ItemOut` | `[0x08, 0x09)`, `[0x09, 0x18)` |
| `0x0062f270` | `Build`, `BuildOut`, `BuildData` | `[0x6c,0x70)`, `[0x70,0x86)`, `[0x7f,0x80)`, `[0x83,0x84)`, `[0xb4,0xb6)` |
| `0x00642510` | `Wall`, `WallOut`, `WallData` | `[0x48, 0x66)` — 30 B |
| `0x005d7ea0` | `Animal`, `AnimalData`, `AnimalOut` | `[0x150, 0x155)` — 5 B |
| `0x0066e5d0` | `Good` | `[0x20, 0x21)` |
| `0x00677150` | `Item` | `[0x20, 0x21)` |

Chaining is by explicit base call, e.g. `Unit::walk_data` → `0x00647830` →
`0x006621d0`, so a `Unit`'s sim-critical bytes are the union of the three rows above plus
the sub-object walkers it calls (`0x0046d8b0` on `this+0xb8`, `0x00730270` on `this+0xc8`,
`0x0046df30` on `this+0xe4`, each gated by the `DataWalk+0x0c` mask bits 2 / 4 / 8).

`Leader::walk_data` = **`0x006d6750`** is the biggest single object [measured]:
`walk(leader+0x0, +0x8)`, then if `leader[0] & 1`, `walk(leader+0x8, +0x692a)` — **26,914
contiguous bytes** — then 8 × `walk(p, p+0x5c)` from `leader+0x692c`. Leader stride is
`0x6eec` = 28,396 bytes, so essentially the whole player record is sim-critical.

The world/terrain channels use devirtualised fast paths that reveal SoA layout directly
[measured] — each does `cmp eax, 0xb3f920` (is the visitor a `CheckSum`?) and inlines the
adler call:

| function | array | element |
|---|---|---|
| `0x00937040` | `[[0xc06188]+0x134] + 28*i` | 21 of 28 bytes hashed |
| `0x009370a0` | `[[0xc06188]+0x138] + 2*i` | 2 bytes |
| `0x009370f0` | `[[0xc06188]+0x15c] + i`, `+0x160`, `+0x164`, … | 1 byte each, parallel byte planes |

**Honest limits of the extraction.** 183 ranges is a *first-order* result, not the complete
schema. The extractor is a linear scan with no dataflow join, so: it misses ranges computed
across branches; it misses array/loop walkers whose bounds are runtime values; and ~60 of
the 106 functions report *negative* `this`-relative offsets, which means my `this` taint was
wrong for them (they are almost certainly `__fastcall` helpers where `ECX` is not the
object). I have reported only the vtable-anchored rows as measured. Completing the schema
means walking the call graph from `check_all` with a proper abstract interpreter, and
resolving the `+0x7c` / `+0xac` / `+0xb0` virtual dispatches per receiver class. That is a
follow-on lane, and it is now a mechanical job rather than a research one.

---

## 6. The free per-frame oracle: yes, and here is the switch

**Answer to the lane question: yes, a config toggle exists, and it is file-based.**

`FUN_00a2f580` and `FUN_00a30080` (both `Skybox_SyncLogger.cpp`) read **`.\synclogger.ini`**
(UTF-16 literal at `0x00b18090` and `0x00b182dc`), section **`[Settings]`**
(`0x00b18294`) [measured]. The config-key string pointers sit in a contiguous `.data` array
at `0x00c06664`…`0x00c06674`, referenced from `FUN_00a2e880` and `FUN_00a30080` [measured]:

| key | string VA |
|---|---|
| `DesyncUploadsWanted` | `0x00b17a58` |
| `DesyncTrackingFrameHistorySize` | `0x00b17a9c` |
| **`DesyncTrackingEnabled`** | `0x00b17adc` |
| `SkipCountdown` | `0x00b17a3c` |
| `CheckDesyncsEveryXFrames` | `0x00b179e8` |

Also present as UTF-16 in the same block, though I did not trace their readers:
`DesyncCategoryMask` `0x00b179c0`, `RandomSeedHost` `0x00b17a1c`, `RandomSeedGame`
`0x00b17b60`, `RandomSeedMap` `0x00b17b44`, `SimulationFps` `0x00b17a80`,
`StartLoggingOnWorldTimeMS` `0x00b17b90` (this one **is** traced: `FUN_00a30080` stores it
to `this+0xa8` at `0x00a30327`).

This resolves the `[reported]`-and-unconfirmed names in `docs/oracle-architecture.md`:
`mCheckDesyncsEveryXFrames`, `mDesyncTrackingEnabled`, `mRandomSeedHostOverride` were PDB
member names we could not find. The **INI key** spellings are in our binary and are
`CheckDesyncsEveryXFrames`, `DesyncTrackingEnabled`, `RandomSeedHost` [measured] — close but
not identical, so cite the key names, not the member names.

Output side [measured]: log files are named `SyncLog %s %s.txt` (`0x00b18270`) with the tag
one of `SendLog` / `ReceiveLog` / `TurnLog` / `LogToWrite(Unknown%d)`. Per-turn banner
`****        BEGIN TURN %06u         ****`. Terminal messages `Game completed without
desync\n` (`0x00b18204`, **ASCII**) and `DESYNC ON WORLD TURN ` (`0x00b18224`, **ASCII** —
this block mixes encodings). There is a
`Sync Categories to Track:` dump (`0x00b1871c`), a `%d/%d prior games have desynched`
counter (`0x00b18680`), a ` (using default because tracking off)` annotation
(`0x00b18538`, ASCII), and settings provenance strings `SettingsCameFromConfig` /
`SettingsCameFromMultiplayer` / `SettingsCameFromQuickmatch` / `SettingsCameFromUNKNOWN`.

### The 38 sync categories [measured]

A 20-byte-stride table at **`0x00c06370`**, layout `{char abbrev[8]; char* name; u32 flags;
u32 id;}`. `DesyncCategoryMask` is presumably a bitmask over these ids (I did not trace the
mask consumer, so treat the *use* as [reported]).

```
 0 ----   1 Note  NoteOnlySync      2 Final Final       3 Misc  Misc
 4 Perf  Performance    5 ComMg CommandManager   6 Tunin TurnTuning
 7 TrnCt TurnControl    8 Time  Time             9 NetDm NetDaemon
10 DrpCt DropControl   11 ConnD ConnectionData  12 World World
13 Citie Cities        14 Build Builds          15 Units Units
16 Animl Animals       17 Walls Walls           18 Ammo  Ammo
19 Death Deaths        20 Group Groups          21 Leadr Leaders
22 Guys  Guys          23 GpcCd GraphicChads    24 GpcEt GraphicEvent
25 Goods Goods         26 Items Items           27 AnimC AnimCheck
28 PrgCt ProgressChart 29 MapMk MapMake         30 Terrn Terrain
31 Pthfd Pathfinder    32 Chksm Checksum        33 Sound Sound
34 Rules Rules         35 Scrpt Script          36 GpVfy GpieceVerify
37 Gmspy Gamespy
```

(The parallel uppercase table at `0x00af6894`…`0x00af6a10` — `NETSYS`, `MISC`,
`TURN_TUNING`, … `GPIECE_VERIFY`, `GAMESPY` — is `gamelog.cpp`'s category names for the same
enum.)

**Note the categories are a superset of the checksum channels.** `Terrain`, `Pathfinder`,
`Animals`, `AnimCheck`, `Sound`, `GraphicChads`, `GraphicEvent` have SyncLogger categories
but no `check_all` channel. Presentation-vs-sim is therefore *not* simply "has a sync
category"; the sharp line is the fifteen `check_all` channels and the `DataWalk` traversal.

### Game-settings knobs, separate from the INI [measured]

`FUN_005d6040` binds three settings through the rules-descriptor visitor:

| name | field | string VA |
|---|---|---|
| `checksum_deep` | `settings+0x08` | `0x00ad8d4c` |
| `checksum_window_size` | `settings+0x0c` | `0x00ad8da0` |
| `checksum_failure_threshold` | `settings+0x10` | `0x00ad8d68` |

They are also printed by the game-info dump — `  checksum_window_size: %d` `0x00ad5d98`,
`  checksum_deep: %d` `0x00ad5e14`, `  checksum_failure_threshold: %d` `0x00ad5e98` — and in
title case for the lobby UI (`  Checksum Deep: %d`, `0x00ad6c34`). All UTF-16.

**Unresolved [measured that it exists, not what it is]:** four of the object channels
(`units` `0x009371da`, `builds` `0x0093729a`, `walls` `0x0093736a`, `guys` `0x0093743d`)
open with `cmp dword ptr [<[0x00c0618c]>+0x1f4], 0` / `je <skip whole channel>`. So there is
a runtime flag that switches those four channels off entirely. `checksum_deep` is the
obvious candidate but I did **not** establish the link — I could not tie `[0x00c0618c]+0x1f4`
to `settings+0x08`, and `+0x1f4` is a popular offset (20+ unrelated writers). Settle it with
the oracle or a live probe before relying on it.

---

## 7. What this buys us

1. **A definition of sim-critical state that is not a guess.** Implement `DataWalk` in Rust
   as a trait with `walk(&[u8])`, mirror the traversal order, and our checksum is
   comparable to retail's byte-for-byte. Divergence localises to a channel, then to an
   object, then to a byte range.
2. **The save-game format is the same traversal**, so recovering one recovers both.
3. **A per-frame, per-subsystem differential oracle**, gated by an INI file we control,
   with a per-turn text log — no instrumentation of the running game required beyond
   dropping `synclogger.ini` next to the executable. This is the cheapest high-volume
   ground truth available to this project and it should be stood up early.
4. **A refutation banked.** The descriptor-table shortcut to the sim schema does not exist;
   anyone who assumes it will build against a hierarchy that never touches instance state.

---

## 8. What I could not establish

- **The complete field set per channel.** I have the channel list, the traversal mechanism,
  and 183 byte ranges. I do not have the closed traversal — see the honest limits in §5.
- **Whether `[0x00c0618c]+0x1f4` is `checksum_deep`.** Named as a hypothesis only.
- **How often `check_all` runs.** Both call sites are inside `FUN_0093ef10`
  (`commandmanager.cpp`, the `PROCESS_TURN` path) at `0x0093f103` and `0x0093fb63`; one is
  gated on `[[0x00c061ec]+0x820] & 0x10`. I did not decode the frame cadence, and
  `CheckDesyncsEveryXFrames` suggests it is configurable anyway.
- **The `DataWalk+0x0c` mask values in production.** `check_units` passes `-1` (everything);
  I did not enumerate what other callers pass, so "the checksum covers range X" is
  mask-dependent and currently unqualified.
- **`RandomLogEntry`'s layout.** RTTI for `ObjectArray<RandomLogEntry>` is present at
  `0x00c9425c`; the `{frame, file, line, seed}` shape is still **[reported]**, untested.
  `process_check_random seed: %d game.frame: %d` at `0x00afa720` is a real string in our
  binary, so the per-call RNG provenance idea has a foothold — but that is the RNG lane's
  to prove.
- **Anything about `SyncDisplay`.** Three vftables (`0x00b38b94`, `0x00b32c20`,
  `0x00b2bd00`); I did not open it. It is *not* in `DataWalk`'s hierarchy, so the
  `binary-ground-truth.md` note "`SyncDisplay` ⇒ a lockstep checksum routine exists" is
  right in its conclusion but wrong in its mechanism — the checksum is `CheckSum` /
  `checksums.cpp`, not `SyncDisplay`.

---

## Ledger entries to add to `docs/provenance-ledger.md`

| mechanic | source | tier | evidence |
|---|---|---|---|
| `adler32(adler, buf, len)` — lockstep checksum primitive | VA `0x00a46830`, `__fastcall` (ECX/EDX/stack) | **B** | 500,000 calls into retail machine code, 0 mismatches; lengths include `0,1,2,15,16,17,31,5551,5552,5553,11104,11105` then uniform in `[0,24576]`, uniform random bytes and initial value, fixed seed `0x9E3779B97F4A7C15`; harness `hbox:~/don-oracle-checksum`, `oracle adler` |
| Checksum channel set and order (15 channels + `total`) | `FUN_00936560` @ `0x00936560` | structural [measured] | UTF-16 log labels at `0x00af6af0`…`0x00af7210`, one per channel, in call order |
| `CheckSumsCommand` wire layout (1-byte header + 15 × u32, 61 B) | `FUN_009459d0` @ `0x009459d0` | structural [measured] | `lea eax,[edi+N]` per labelled field |
| `CheckSum : DataWalk`, `SaveGame : WalkDataGame : DataWalk` | RTTI CHDs; type descriptors `0x00c952e0`, `0x00c96814` | structural [measured] | 1,510 hierarchies parsed; exactly 6 classes derive from `DataWalk` |
| `DataWalk` is 2 pure virtuals; slot 0 = `walk(begin,end)` | vftable `0x00b2bcd8` → `_purecall` thunk `0x0055e0a6` | structural [measured] | `CheckSum` vftable `0x00b3f920` = `{0x00936ff0, 0x0041bfe0}` |
| Desync tracking toggle | `.\synclogger.ini` `[Settings]`, keys at `0x00c06664`… | structural [measured] | read by `FUN_00a2f580` / `FUN_00a30080` |

And **remove** from the "not yet derived" list: *"The lockstep checksum's field set
(`CheckSum` / `DataWalk`)"* — replace with *"the closed `walk_data` traversal (first-order
extraction done, call-graph closure outstanding)"*.
