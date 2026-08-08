# The `.rcx` replay reader and writer

Lane: `replay-io`. Target: `ron-bin/riseofnations.exe` (PE32 i386, image base `0x00400000`,
2024-06-20 MSVC rebuild). Every claim is marked **[measured]** (I verified it against the
binary or against real `.rcx` files) or **[reported]** (read, unchecked). Nothing here is
"verified" in the proof-assistant sense — see `docs/CHARTER.md`. No public documentation
or parser for this format exists (`docs/prior-art-survey.md` §3), so nothing below is
cross-checked against folklore; there is none to cross-check against.

---

## Headline

**A `.rcx` is a save-game prefix plus a lockstep command stream, and the command stream is
the engine's own network protocol.** [measured]

The file is one gzip stream. Decompressed it is:

1. a `SaveGame`-serialised header written by the same `DataWalk` visitor as save games
   and as the lockstep checksum (`Game::walk_data` = `FUN_00589600`),
2. a ~1 MB fixed-size dump of the rules/world state (same visitor, `FUN_00589550` etc.),
3. then, to the last byte of the file, a sequence of **command-package records** — an
   18-byte framing header plus a payload of command packets, appended once per turn by
   `FUN_00952fb0`.

The command packets are decoded by `CommandPackage::process` = **`FUN_0094a700`**, a switch
over **82 opcodes, `0x00`–`0x51`**, where **each handler returns the packet's byte length**.
That return value *is* the wire format, and the full table is recovered below and
implemented in `re/scripts/rcx_parse.py`.

Two results matter most for the project:

- **Opcode `0x39` `check_sums` is a 65-byte packet: sixteen `u32` in exactly the
  `check_all` channel order** (`units, builds, walls, ammo, deaths, groups, guys, leaders,
  cities, items, goods, world, rules, scenario_data, script_run_time, total` — see
  `docs/derivation/checksum.md`). Multiplayer replays carry one roughly every other turn.
  **That is a per-turn, whole-state oracle for our simulation, recorded by the retail
  engine, in a file we already have 60+ of.** [measured]
- **Single-player replays record only the human player's commands.** In every solo
  specimen the `play` field is `0` for every record; the AI opponents emit nothing. So a
  solo replay is *not* sufficient to re-derive the game — replaying it requires running
  the shipped AI deterministically. Multiplayer replays record every human player.
  [measured]

The parser is `/Users/ember/dev/don/re/scripts/rcx_parse.py`. On the reference specimen it
consumes the command stream **exactly to EOF with zero leftover bytes and zero
undecodable packets**.

---

## 0. Anchor conversion

`.rdata` RVA is `0x006c5000` (VA `0x00ac5000`, raw `0x006c3800`); `.data` RVA `0x00806000`
(VA `0x00c06000`, raw `0x00803a00`). The lane's file offsets convert to: [measured]

| file offset | VA | section | content |
|---|---|---|---|
| `0x6e585c` | `0x00ae705c` | .rdata | ASCII `.rcx` |
| `0x804d88` | `0x00c07388` | .data | ASCII `.rcx` inside a stride-`0x48` file-type table at `0x00c07380` (siblings `.xml` `0x00c07340`, `.4`, `.7`, `.9`) |
| `0x6e5391`…`0x6e53c1` | `0x00ae6b91`… | .rdata | `\playbacks\Solo`, `\playbacks\NewWorld`, `\playbacks`, `\playbacks\CTW`; the full set runs `0x00ae6b90`–`0x00ae6c10` and is mirrored by `\feedback\*` at `0x00ae6b04` |
| `0x6faed8` | `0x00afc6d8` | .rdata | UTF-16 `playback_speed` |
| `0x6fac38` | `0x00afc438` | .rdata | UTF-16 **`recordgame.tmp`** — a filename, not a subsystem tag |
| `0x766724` | `0x00b67f24` | .rdata | `zlib` version string, referenced from `FUN_0054dcf0`, `FUN_00554ae0`, `FUN_00556fc0`, `FUN_00558170` — zlib is statically linked |

The `\playbacks\*` paths are referenced only from `FUN_007b2fc0`, which `_wmkdir()`s each
one at startup; the ASCII `.rcx` at `0x00ae705c` is referenced only from `FUN_007b4f50`
(16,984 bytes, in the bulk-decomp skip list), which is the file-browser UI and also writes
the recording filename global at `0x00e8f7c0`. Neither is the format. [measured]

Source-file literals recovered by scanning `.rdata` for UTF-16 `*.cpp`: `turncontrol.cpp`
(`0x00afc61c`, `0x00afc63c`), `commandmanager.cpp` (`0x00af7008`), `ecommandmanager.cpp`
(`0x00af73c6`), `commandpackage.cpp` (`0x00af7974`), `netsys.cpp`, `connectiondata.cpp`,
`gameinfo.cpp`, `e:\agent\_work\2\s\main\game\save.h`. [measured]

---

## 1. The objects

### The recording object at `0x00e8f7a8` [measured]

One global drives both recording and playback. Field offsets recovered from the functions
that use it:

| offset | VA | meaning | evidence |
|---|---|---|---|
| `+0x04` | `0x00e8f7ac` | a `String` (directory) | `FUN_009537a0`, `FUN_00952920` |
| `+0x18` | `0x00e8f7c0` | `String` — the recording's file path | `FUN_00952a50` passes it to `File::open`; `FUN_00952b40` `_wremove`/`_wrename`s it |
| `+0x40` | `0x00e8f7e8` | embedded `File` (gz handle) | `SaveGame` gets `this+0x28 = 0x00e8f7e8` |
| `+0x44` | `0x00e8f7ec` | the `FILE*` of that `File` | `fwrite`/`fread` targets in `FUN_00952fb0` |
| `+0x64` | `0x00e8f80c` | "a package is pending" flag | set 1 by `FUN_009537a0`, cleared by `FUN_00952fb0` |
| `+0x68`/`+0x70`/`+0x74` | | bit-array length / flag / bit array | `FUN_00953990` `memset`s `(len+7)/8` bytes |

The `String` layout, from `String::walk_data` (`FUN_00a1b2d0`) and `SaveGame::walk_tag`
(`FUN_0043d840`): `{ void* buf @0x00; u16 @0x04; u16 offset @0x06; u16 length @0x08;
u8 flags @0x0a (bit0 = inline); u32 name_hash @0x0c; u8 section_tag @0x10 }`. [measured]

### The `File` class (`file.cpp`) [measured]

`File::open` = **`FUN_00a2dcd0`**, `(path, mode)`. Mode bits, read off the branch tree:
`(mode & 6) == 0` → `"r"`, else `(mode & 4) == 0` → `"w"`, else `"a"`; `(mode & 8)` → `"t"`
else `"b"`. So mode `1` = `"rb"`, mode `2` and `3` = `"wb"`.
`File::write` = **`FUN_00a2cc10`** → `fwrite` if `this[1]` is set, else zlib `gzwrite`
(`FUN_00509350`). `File::close` = **`FUN_00a2dc70`** → `fclose`, or the gz close path
(`FUN_005093f0`/`FUN_00509670`/`FUN_00509090`). `File::read` path uses `FUN_00509140`
(`gzread`).

### `DataWalk` [measured]

Established in `docs/derivation/checksum.md`: `DataWalk` (vft `0x00b2bcd8`) is abstract with
two virtuals; `SaveGame` (vft `0x00b35ac4` = `{0x0043d730, 0x0043d840}`) and `LoadGame`
(vft `0x00b30c88` = `{0x0043d950, 0x0043da60}`) are the concrete pair used here.

- `SaveGame::walk(begin, end)` = `FUN_0043d730` → `fwrite(begin, 1, end-begin, file)` or
  `gzwrite`. Raw bytes, no encoding, no alignment.
- `LoadGame::walk` = `FUN_0043d950` → the mirror `fread`.
- `SaveGame::walk_tag(String*)` = `FUN_0043d840` → **writes exactly one byte**: the byte at
  `String+0x10`, filled in by `FUN_00a1b6b0` when it hashes the tag name.
- `LoadGame::walk_tag` = `FUN_0043da60` → reads one byte and, on mismatch, raises
  `"Error loading section, probably in <name>"`.

This is what makes the file self-delimiting at section granularity, and it is exactly why
the payload begins `16 42`.

---

## 2. Writer and reader — the named paths

| role | function | what it does |
|---|---|---|
| **start recording** | `FUN_009537a0` | resets state (`FUN_00953990`), builds the path into `0x00e8f7c0`, `_wmkdir`s it, calls `FUN_00952a50`, and if `game[0x822] & 2` (Conquest) `FUN_00952990`; sets `0x00e8f80c = 1` |
| **header writer** | **`FUN_00952a50`** | `File::open(0x00e8f7c0, 2 /*"wb"*/)`; `SaveGame` on the stack with `file = 0x00e8f7e8`; `Game::walk_data(FUN_00589600)`; then `String::walk_data` of `game+0x53c` (the map/scenario name) |
| **rules writer** | `FUN_00953630` | `SaveGame` → `FUN_00589550` (`Rules::walk_data`) |
| **conquest writer** | `FUN_00952990` | `SaveGame` → `FUN_00798410` |
| **per-player extra writer** | `FUN_009534e0` | 8 slots × (1 byte at `game+0x78+i*0x8c`, 4 bytes at `0x00e3a39c + idx*0x1bbb`), then 4 bytes at `[0x00c06188]+0x30` |
| **per-turn record writer** | **`FUN_00952fb0`** | one 18-byte header + payload (§4) |
| **flush** | `FUN_00953120` | `fflush` / gz flush |
| **finish: gzip + rename** | **`FUN_00952b40`** | see below |
| **stop / reset** | `FUN_00953990` | `FUN_00952b40`, then `memset(this+0x74, 0, (this[0x68]+7)/8)`, `this[0x70]=1`, `this[0x64]=1` |
| **start playback** | **`FUN_00586ea0`** | `FUN_00953710`; on failure shows `L"Unable to load recorded game, Cloud Storage is full"` (`0x00ad1190`); on success sets `game[0x820] |= 0x10` — **the playback flag** |
| **header reader** | **`FUN_00953710`** | `File::open(this+0x18, 1 /*"rb"*/)`; `LoadGame` global at `0x00c12b70` with `file = this+0x40`; `Game::walk_data(FUN_00589600)`; `game[0x558] = 0`; `String::walk_data` |
| **metadata-only reader** | `FUN_00953160` | opens the file `"rb"`, `LoadGame::walk_tag`, `GameInfo::walk_data(FUN_005d6570)`, closes. This is what the replay browser uses to show version/players without loading the game |
| **rules / conquest / per-player readers** | `FUN_00953610` / `FUN_009536f0` / `FUN_009533e0` | `LoadGame` mirrors of the writers above |
| **per-turn record reader** | **`FUN_00952d90`** | mirror of `FUN_00952fb0`; on EOF clears `game[0x820] & 0x10` and `& 4` and ends playback |
| **section order** | `FUN_00584590` at `0x00585287`–`0x00585300` | `if (players > 3) { playback ? FUN_009533e0 : FUN_009534e0 }` then `playback ? FUN_00953610 : FUN_00953630` |

### The gzip step, exactly [measured]

`FUN_00952b40` (disassembly at `0x00952c7b`–`0x00952d30`):

```
mov ecx, ebx ; call 0xa1dcd0                 ; dir = this->path's directory
push 0xafc438 ; call 0xa1dad0                ; append L"recordgame.tmp"
push 3 ; lea eax,[ebp-0x24] ; push eax
lea ecx,[ebp-0x5c] ; call 0xa2dcd0           ; File::open(<dir>\recordgame.tmp, "wb")  [gz]
lea ecx,[ebp-0x80] ; call 0xa48770           ; FileMap::open(this->path)  CreateFileW/MapViewOfFile
mov esi,[0xac5120]                           ; GetFileSize
push 0 ; push [ebp-0x68] ; call esi          ; n = GetFileSize(map.hFile)
push eax ; push [ebp-0x10]
lea ecx,[ebp-0x5c] ; call 0xa2cc10           ; File::write(mapped_view, n)   -> gzwrite
lea ecx,[ebp-0x5c] ; call 0xa2dc70           ; close (finishes the gzip member)
mov ecx,ebx ; call 0xa1f0a0 ; call [0xac54d4]; _wremove(this->path)
                             _wrename(recordgame.tmp, this->path)
```

So: the game writes the recording **uncompressed** while playing, then on game end
memory-maps it, streams it through the gz writer into `recordgame.tmp`, deletes the raw
file and renames the tmp over it. That is why the whole `.rcx` is a single gzip member
starting at offset 0 with no outer header, refuting the [reported] "10-byte header then
gzip" claim carried in `docs/prior-art-survey.md`. [measured]

The `else` branch (`this[0x64] != 0`) just `_wremove`s the file — that is the "discard
recording" path.

---

## 3. The header layout, field by field

`Game::walk_data` = `FUN_00589600`:

```
walk_tag("…")                               -> 1 byte   = 0x16
GameInfo::walk_data(FUN_005d6570)                       (see below)
walk(game+0x550, game+0x6e4)                -> 404 bytes   (game.frame is game+0x550)
if (!checksum) {
    walk(game+0x814, game+0x81c)            -> 8 bytes
    walk(game+0x820, game+0x814 + *(int*)(game+0x818) + 0xc)   -> variable
    walk(game+0x844, game+0x848)            -> 4 bytes
}
```

`GameInfo::walk_data` = `FUN_005d6570`, disassembled at `0x005d6596`–`0x005d66c1`:

```
0x5d65ab  call [edx+4]                      walk_tag           -> 1 byte  = 0x42
0x5d6603  "(Version: " + FUN_0092ef00() + ")"
0x5d6656  call 0xa1b2d0                     String::walk_data  -> u32 len + len*2 bytes UTF-16LE
0x5d6664  call [edx]        walk(gi+0x00, gi+0x04)   -> 4 bytes
0x5d6672  call [edx]        walk(gi+0x04, gi+0x14)   -> 16 bytes
0x5d669b  call [edx]        walk(gi+0x14, gi+0x18)   -> 4 bytes   (save/load only)
0x5d66ae  call [edx] x0x1d  walk(p, p+1)             -> 29 single-byte walks, gi+0x18..gi+0x35
0x5d66c1  call [edx]        walk(gi+0x35, gi+0x36)   -> 1 byte
          for i in 0..7, p = gi + 0x68 + i*0x8c:
              walk_tag(...)                          -> 1 byte  = 0x50
              walk(p+0, p+2)                         -> 2 bytes
              if (p[0] & 1):
                  walk(p-0x30, p+9)                  -> 0x39 = 57 bytes
                  String::walk_data(name)            -> u32 len + len*2 bytes
```

`String::walk_data` (`FUN_00a1b2d0`) writes `u32 character_count` then that many UTF-16LE
code units, **not NUL-terminated**. [measured]

### Therefore the first bytes are

| offset | width | value on `today.rcx` | meaning |
|---|---|---|---|
| `0x00` | u8 | `0x16` | `walk_tag` byte, `Game::walk_data` section |
| `0x01` | u8 | `0x42` | `walk_tag` byte, `GameInfo::walk_data` section |
| `0x02` | u32 | `0x0000001a` = 26 | character count of the version string |
| `0x06` | 52 B | `"(Version: 00.2024.06.2000)"` | UTF-16LE, no terminator |
| `0x3a` | u32 | `0x06ac012c` | GameInfo `+0x00` — a **format/build stamp** (see below) |
| `0x3e` | 16 B | `a2 83 08 00 …` | GameInfo `+0x04`..`+0x14`; the first u32 differs per game |
| `0x4e` | u32 | `0x00000121` | GameInfo `+0x14` |
| `0x52` | 29 B | | GameInfo `+0x18`..`+0x35`, written one byte at a time |
| `0x6f` | u8 | `0x00` | GameInfo `+0x35` |
| `0x70` | | 8 × player slot | tag `0x50`, `u16` flags, and if `flags & 1` a 57-byte blob + name |

**This settles the two open questions in `docs/replay-format.md`.** `0x1a` is the version
string's *character count* (26); **`0x28` is not a length at all — it is the UTF-16 code
unit `'('`**, the first character of `"(Version: …)"`. The previous note that "`0x28`
immediately preceding the string is a plausible length field" is **refuted** [measured].

The structure is confirmed by exact arithmetic on the real file: for `today.rcx` the
first player's 57-byte blob starts at `0x73` and `0x73 + 0x39 = 0xac`, which is precisely
where the `03 00 00 00 "cmr"` name string sits; the second slot's blob starts at `0xb9`
and `0xb9 + 0x39 = 0xf2`, exactly where `08 00 00 00 "Player 2"` sits. All eight slots
land on their tag byte `0x50`. [measured]

### Cross-version behaviour [measured]

Six specimens, five engine builds' worth of files, all parsed by the same code:

| file | version string | GameInfo `+0x00` | GameInfo `+0x04` (u32) | header end |
|---|---|---|---|---|
| `today.rcx` (2026 solo) | `(Version: 00.2024.06.2000)` | `0x06ac012c` | 557986 | `0x165` |
| `solo2024` (2024-03) | `(Version: 00.2017.11.2900)` | `0x06d2013a` | 15217019 | `0x1b2` |
| `solo2020` (2020-02) | `(Version: 00.2017.11.2900)` | `0x06d2013a` | 437421384 | `0x1b2` |
| `mp2024` (2024-02) | `(Version: 00.2017.11.2900)` | `0x06d2013a` | 12294099 | `0x114` |
| `mp2020` (2020-07) | `(Version: 00.2017.11.2900)` | `0x06d2013a` | 188705235 | `0x1b0` |
| `mp2014` (2014-04) | `(Version: 03.02.03.2905)` | `0x051d0362` | 1198543798 | `0x2ac` |

- The section tags `0x16 / 0x42 / 0x50` and the whole field layout are **identical from
  2014 to 2026**, across the original Thrones & Patriots `03.02.03` build and two Extended
  Edition builds. [measured]
- **GameInfo `+0x00` is constant per engine build** (`0x051d0362` / `0x06d2013a` /
  `0x06ac012c`) and varies across builds — it is a format/compat stamp, not per-game data.
  A replay therefore carries two independent version signals: this word and the string.
- **GameInfo `+0x04` (u32) is different in every single file** and looks like a
  32-bit random value. It is the strongest candidate for the **game seed**; I did *not*
  confirm this against the RNG (see §7, open).
- The version string is the *engine/data* version, not the PE build date: files recorded
  in 2024 on the pre-update client say `00.2017.11.2900`. Only `today.rcx` says
  `00.2024.06.2000`, matching this exe. [measured]

### The gap between header and command stream [measured]

After the header there is a **fixed-size** block before the record stream:

| build | header end | stream start | gap |
|---|---|---|---|
| `00.2024.06.2000` (`today`) | `0x165` | `0xfa439` | 1,024,724 |
| `00.2017.11.2900` (`solo2024`/`solo2020`) | `0x1b2` | `0xfa48b` | 1,024,729 |
| `00.2017.11.2900` (`mp2020`) | `0x1b0` | `0xfa489` | 1,024,729 |
| `00.2017.11.2900` (`mp2024`) | `0x114` | `0xfa3e3` | 1,024,719 |
| `03.02.03.2905` (`mp2014`) | `0x2ac` | `0xfa6b9` | 1,025,037 |

The gap is constant to within the header's own length, because `SaveGame::walk` only ever
copies fixed struct ranges. Its content is the rest of `Game::walk_data` plus
`Rules::walk_data` (`FUN_00589550`) — scanning it recovers 1,170 length-prefixed UTF-16
strings including the resource table (`Food`, `Timber`, `Wealth`, `Knowledge`, `Metal`,
`Oil`, … `Olive Oil`, at a ≈`0x14a`-byte stride) and diplomacy strings. I did **not**
field-decode this block; `rcx_parse.py` skips it. [measured that it is there and roughly
what it holds; not decoded]

---

## 4. The command-package record

Writer `FUN_00952fb0`, six writes in this exact order [measured]:

```c
fwrite(game + 0x550, 1, 4, f);   // u32 game.frame at the moment of the write
fwrite(pkg  + 0x04,  1, 4, f);   // u32 'play'   (GameLog name at 0x00af7718)
fwrite(pkg  + 0x08,  1, 4, f);   // u32 'valid'  (0x00af7738)
fwrite(pkg  + 0x00,  1, 4, f);   // u32 'stamp'  (0x00af76b8)
fwrite(pkg  + 0x10,  1, 2, f);   // u16 size
fwrite(pkg  + 0x12,  1, size, f);// payload
```

Reader `FUN_00952d90` reads the same six in the same order, and:

- if the record's frame is **ahead** of the caller's frame it seeks back 4 bytes and
  returns 0 — i.e. records are consumed lazily, one turn at a time;
- if `param_3 >= 0` and the `play` field does not match, it seeks back 8 and returns 0 —
  the caller can demand a specific player's package;
- at EOF (`param_4 == 0`) it closes the file, clears `game[0x820] & 0x10` (playback) and
  `& 4`, sets `game[0x820] |= 0x40` and `game[0x823] |= 1` — the "playback finished" state.

Field names come from the `GameLog` debug-variable registration at `FUN_0094b7c0`, which
binds `L"stamp"` (`0x00af76b8`) to `[ebx+0]`, `L"play"` (`0x00af7718`) to `[ebx+4]`, and
`L"valid"` (`0x00af7738`) to `[ebx+8]`; `L"group"`, `L"size"`, `L"data[scan]"` follow.
[measured]

So the on-disk record is **18 bytes + payload**:

```
u32 frame
u32 play      // player slot that issued this package
u32 valid
u32 stamp     // turn stamp; increments once per turn per player
u16 size
u8  data[size]
```

`group` is a `CommandPackage` field but is **not** written to disk. [measured]

Call sites of the writer: `FUN_0093ef10` (`CommandManager::process_turn`) at `0x0093f170`,
`0x0093fa3f`, `0x0093fbb3`, each guarded by `game[0x820] & 8` (= "recording on"), plus
`FUN_009598b0`. Call sites of the reader: `FUN_0093ef10` at `0x0093f0f6`, `0x0093f122` and
`FUN_00940120` at `0x00940259`. Recording and playback are therefore the *same* point in
the turn loop. [measured]

---

## 5. The opcode table — 82 commands

`CommandPackage::process` = **`FUN_0094a700`** switches on `data[0]`. Handlers are
`__thiscall` with the packet pointer as the single stack arg and **return the packet
size**. Names are the UTF-16 log-format literals each handler pushes.

| op | size | name | handler | op | size | name | handler |
|---|---|---|---|---|---|---|---|
| `0x00` | `n*2+3` | `group` | `0094a0c0` | `0x29` | 9 | `accept` | `00946fb0` |
| `0x01` | 1 | `begin` | `00949fd0` | `0x2a` | 9 | `reject` | `00946e90` |
| `0x02` | 5 | `stance` | `00949ed0` | `0x2b` | `0x11` | `tribute` | `00946d70` |
| `0x03` | `0x0d` | `form` | `00949d90` | `0x2c` | `0x11` | `demand_tribute` | `00946c50` |
| `0x04` | `0x11` | `attack` | `00949c30` | `0x2d` | `0x11` | `demand_tribute_onoff` | `00946b30` |
| `0x05` | `0x0d` | `siege_attack` | `00949ae0` | `0x2e` | `0x0d` | `buy` | `00946a20` |
| `0x06` | `0x11` | `swarm_around` | `00949970` | `0x2f` | `0x0d` | `sell` | `009468a0` |
| `0x07` | `0x16` | `move_to` | `009497c0` | `0x30` | `0x0f` | `unqueue` | `009466f0` |
| `0x08` | `0x1a` | `move_near` | `009495c0` | `0x31` | `0x0b` | `come_out` | `009465d0` |
| `0x09` | `0x0a` | `attack_ground` | `009494a0` | `0x32` | 9 | `ping` | `009453f0` |
| `0x0a` | `0x0a` | `patrol` | `00949380` | `0x33` | `n*8+6` | `ping_line` (spline) | `00945140` |
| `0x0b` | `0x19` | `launch_patrol` | `00949230` | `0x34` | 5 | `speed_set` | `00946380` |
| `0x0c` | 1 | `halt` | `00949140` | `0x35` | 1 | `speed_up` | `009461a0` |
| `0x0d` | 1 | `transport` | `00949050` | `0x36` | 1 | `speed_down` | `00946290` |
| `0x0e` | 5 | `set_transport` | `00948f60` | `0x37` | 1 | `mp_log` | `00946080` |
| `0x0f` | 9 | `set_transport_o` | `00948e00` | `0x38` | 5 | **`check_random`** | `00946020` |
| `0x10` | `0x0d` | `repair` | `00948cb0` | `0x39` | `0x41` | **`check_sums`** | `009459d0` |
| `0x11` | `0x15` | `trade` | `00948b20` | `0x3a` | 6 | `next_check_sum` | `00945e20` |
| `0x12` | 9 | `city_gather` | `00948a10` | `0x3b` | 5 | `cheat_view_all` | `009449b0` |
| `0x13` | 9 | `gather` | `009488b0` | `0x3c` | 5 | `cheat_give_techs` | `00945070` |
| `0x14` | `0x0d` | `garrison` | `00948760` | `0x3d` | 5 | `cheat_zero_techs` | `00944fa0` |
| `0x15` | 5 | `disband` | `00948660` | `0x3e` | 1 | `cheat_ai_speed_increase` | `00944ec0` |
| `0x16` | `0x11` | `gather_point` | `00948510` | `0x3f` | 1 | `cheat_ai_speed_normal` | `00944df0` |
| `0x17` | `0x15` | `spell` | `00948340` | `0x40` | 1 | `cheat_ai_toggle` | `00944d20` |
| `0x18` | 9 | `queue_up` | `00948230` | `0x41` | 5 | `cheat_increase_buckets` | `00944c50` |
| `0x19` | `0x19` | `queue_up_build` | `00948110` | `0x42` | 5 | `cheat_zero_buckets` | `00944b80` |
| `0x1a` | `0x11` | `eject_all` | `00947fe0` | `0x43` | `0x11` | `cheat_init_unit` | `00944a80` |
| `0x1b` | 1 | `alarm` | `00947ef0` | `0x44` | `n*2+0x13` | `chat_set` | `009454f0` |
| `0x1c` | `0x19` | `flight` | `00947db0` | `0x45` | 9 | `chat_stats` | `009458e0` |
| `0x1d` | 1 | `stop_spell` | `00947cc0` | `0x46` | 5 | `resign` | `009438c0` |
| `0x1e` | `0x0d` | `follow` | `009479c0` | `0x47` | 7 | `quit` | `009439a0` |
| `0x1f` | `0x0d` | `guard` | `009478a0` | `0x48` | `0x0a` | `camera` | `00943b00` |
| `0x20` | 9 | `unitmask` | `00947790` | `0x49` | `0x21` | `leader_options` | `009441d0` |
| `0x21` | 9 | `buildmask` | `00947680` | `0x4a` | `0x0b` | `turn_data` | `00943d20` |
| `0x22` | `0x19` | `hotkey` | `009474d0` | `0x4b` | `0x35` | `rename_city` | `00944090` |
| `0x23` | 1 | `recall` | `00947bd0` | `0x4c` | 2 | `pause` | `00944160` |
| `0x24` | 1 | `scramble` | `00947ae0` | `0x4d` | 2 | `cannon_time` | `009464f0` |
| `0x25` | `0x0d` | `treaty` | `009473c0` | `0x4e` | `0x209` | `console_cmd` | `00943f30` |
| `0x26` | `0x0d` | `declare` | `009472b0` | `0x4f` | 9 | `player_speed` | `00943730` |
| `0x27` | 9 | `clear_tributes` | `009471b0` | `0x50` | 3 | `ungraceful_player_drop` | `00943ea0` |
| `0x28` | 9 | `clear_all` | `009470b0` | `0x51` | 2 | `marwan` | `00943660` |

Variable-length forms, taken verbatim from the handlers' `return` expressions:

- `0x00 group`: `return data[1] * 2 + 3` — `data[1]` = object count, `data[2]` = leader,
  then `count` × `int16` object ids (`FUN_0094a0c0`).
- `0x33 ping_line`: `return *(u16*)(data+4) * 8 + 6` (`FUN_00945140`).
- `0x44 chat_set`: `return *(i32*)(data+0x0d) * 2 + 0x13` — 19-byte header then a UTF-16
  string (`FUN_009454f0`).

Two worked examples of the field layout (from the handler bodies):

- `0x02 stance` (`FUN_00949ed0`): `u32 stance` at `+1`; total 5.
- `0x08 move_near` (`FUN_009495c0`): `u32` at `+1,+5,+9,+0x0d,+0x11`; `i8` at
  `+0x15,+0x16,+0x17,+0x18,+0x19`; total `0x1a`.
- `0x4f player_speed` (`FUN_00943730`): eight `u8` at `+1..+8`, each accumulated into
  `player[who]` at `+0x48,+0x4c,+0x50,+0x54,+0x58,+0x5c,+0x64,+0x68` — the
  `synced_frames_zoomed_in / synced_clicks / …` counters already in `schema/bindings.json`.

The default arm of the switch logs
`L"Unknown Command Packet: %d, from: %d, size left: %d, stamp: %d"`
(`0x00af7c00`, `0x00af7d20`) — proof that the size return is the parse cursor. [measured]

### `0x39 check_sums` — the important one [measured]

`FUN_009459d0` logs sixteen `u32`, at `data+1, +5, +9, …, +0x3d`, with the format strings
`L"  units: %u"` (`0x00afa8c0`) … `L"  total: %u"`. That is the exact channel list and
order of `check_all` (`FUN_00936560`) from `docs/derivation/checksum.md`:

```
units builds walls ammo deaths groups guys leaders
cities items goods world rules scenario_data script_run_time total
```

Size `0x41` = 1 + 16×4. Each value is an adler-32 over that channel's walked bytes, and
`total` is the plain 32-bit sum of the fifteen. Multiplayer replays carry these
continuously — in `mp2024`, 1,390 of them across 2,223 records. **This is a recorded,
per-turn, whole-sim-state oracle from the retail engine.**

`0x38 check_random` (5 bytes: opcode + `u32 seed`) is the RNG counterpart, logged as
`L"process_check_random seed: %d game.frame: %d"` (`0x00afa720`). Neither `0x38` nor `0x39`
appears in any single-player specimen I parsed. [measured]

---

## 6. The parser, and what it does on real files

`/Users/ember/dev/don/re/scripts/rcx_parse.py`. Reproduce:

```sh
cd /Users/ember/dev/don
uv run --quiet python re/scripts/rcx_parse.py ron-data/replays/today.rcx
uv run --quiet python re/scripts/rcx_parse.py ron-data/replays/today.rcx --checksums
uv run --quiet python re/scripts/rcx_parse.py ron-data/replays/today.rcx --seeds --commands
```

It gunzips, parses the header exactly, locates the record stream by finding the offset
whose record chain consumes the file to the last byte, and splits each payload with the
opcode table.

### `ron-data/replays/today.rcx` — parses completely [measured]

```
decompressed         1332849 bytes
version              '(Version: 00.2024.06.2000)'
players              cmr / Player 2 / Player 3   (slots 3..7 empty)
header ends at       0x165
stream starts at     0xfa439      (gap 1024724 bytes = the state blob)
records              10544
ends at              0x145671 of 0x145671   (0 trailing bytes)
frames               0 .. 10499        stamps 1 .. 10544
players seen         [0]               valid values [0, 1]
commands decoded     11946             records with garbage: 0

  0x48 camera                   10499
  0x4f player_speed              1313
  0x00 group                       63
  0x18 queue_up                    42
  0x19 queue_up_build              18
  0x36 speed_down                   6
  0x07 move_to                      2
  0x49 leader_options               1
  0x4c pause                        1
  0x20 unitmask                     1
```

Zero leftover bytes and zero undecodable packets over 10,544 records / 11,946 packets is
the strongest structural evidence available short of executing the reader. Note the first
two records: `leader_options` at frame 0 stamp 1, then `pause` — exactly what a game start
looks like. The last frame, 10499, against a known game length of 11:40 = 700 s, gives
15.0 turns/s.

### `solo2024` (14,197 records), `solo2020` (74 records) — parse completely [measured]

Same picture: `play == 0` only, zero trailing bytes, zero garbage.

### Multiplayer: framing parses, payloads do not [measured]

`mp2024`, `mp2020`, `mp2014` all have the same 18-byte framing, and the chain runs to the
last byte of the file — `mp2024` gives 2,223 records, frames 1…8457, `play ∈ {0,1}`,
`valid == 1`, stamps 1…1112 with two records per stamp (one per player); `mp2020` 280
records, `play ∈ {0,1}`; `mp2014` 12 records, `play ∈ {0,1,2,3}` — a four-human game, one
record per player per stamp, `valid == 1` throughout. Note `valid` is `1` in every
multiplayer record and takes both `0` and `1` in single-player ones. But the payload bytes
are **not** the plaintext command stream.

They are close to it. The payloads are dominated by a repeating 2-byte value that differs
per file (`bb 97` in `mp2024`, `3f 69` in `mp2020`, `53 70` in `mp2014`); XOR-ing with that
value recovers genuinely correct-looking packets:

```
mp2024, frame=1 play=0 stamp=1, XOR 97 bb:
  49 00 00 00 00 01 00 00 00 00 00 00 00 02 00 00 00 20 00 00 00 04 00 00 00 02 00 00 00 0b 00 00 00
  ^^ leader_options, 33 bytes, plausible fields
mp2024, frame=7 play=1 stamp=2, XOR 97 bb:
  39 fd 0e b1 65 55 64 38 e8 01 00 00 00 01 00 00 00 01 00 00 00 f5 f3 78 1c …
  ^^ check_sums; three channels reading 1 (empty at turn 2) is exactly right
```

and after the XOR the opcode histogram is a sane multiplayer game (`check_sums`,
`turn_data`, `begin`, `player_speed`, `group`, `attack`, `spell`, `resign`, `quit`). But
the recovered stream still slips by a byte in places (2,221 of 2,223 records fail an exact
size-sum check), and a brute force over all 2-byte keys resolves only 119 of the first 300
records — with 8 different winning keys. **So the transform is XOR-like with a
non-constant keystream, and I did not identify it.** See §7.

I left this in the parser as a *probe*, clearly labelled: `--xor` overrides the key, and
the report prints `payloads OBFUSCATED (multiplayer); probe key …`. Do not treat
multiplayer command decoding as parsed.

---

## 7. What I could NOT establish

1. **The multiplayer payload transform.** Framing plaintext, payload not. A 2-byte XOR
   gets most of the way and is definitely the dominant component, but is not the whole
   thing. Next step: find where `CommandManager` fills `pkg+0x12` on the send/receive path
   (`ecommandmanager.cpp` literals at `0x00af73c6`–`0x00af75a6`, `netsys.cpp` at
   `0x00b66858`, `connectiondata.cpp` at `0x00af9398`) and look for a keystream generator;
   zlib is linked and `FUN_00509140`/`FUN_00509350` are already in play, so a per-packet
   compressor is also plausible. **This blocks the highest-value use of the corpus** — the
   `check_sums` oracle lives in the multiplayer payloads.
2. **The 1 MB state block between the header and the stream.** I established that it is
   `SaveGame`-serialised, fixed-size per build, and contains the resource/rules tables;
   I did not field-decode it. `FUN_00589550` (`Rules::walk_data`) and `FUN_00669800` are
   the entry points; it is the same visitor `docs/derivation/checksum.md` already maps.
3. **The RNG seed, definitively.** I did *not* close this. What I have: the LCG at
   `FUN_00a39cf0` is `*this = *this * 0x19660d + 0x3c6ef35f`, i.e. exactly the
   `1664525 / 1013904223` of `docs/derivation/rng.md` — **confirmed independently here**
   [measured]. ⚠ **Corrected [measured, rise.pdb]:** the sim `Random` object is **not**
   `0x00eb697c` — that is `internal_random`, the secondary (graphics/water) stream. The
   simulation object is `game_random` at `0x00e37a8c`, reached via the static reference
   `GameAccess::game_random` at `[0x00c06184]`. And
   `0x38 check_random` carries a `u32` seed. GameInfo `+0x04` is a per-game 32-bit value
   and is the obvious seed candidate, but I have no specimen pair that isolates it and no
   solo replay contains a `check_random` packet to check the LCG relation against. The
   parser has a `--seeds` mode that, given any file containing `0x38`, tests whether
   consecutive seeds are related by ≤4096 LCG steps; running it on a decoded multiplayer
   payload would settle it.
4. **The tag-byte derivation.** `FUN_00a1b6b0` hashes the tag name into `String+0x0c` and
   writes a byte to `String+0x10` (via the `lea edx,[esi+0x10]` out-param at `0x0043d875`).
   The hash uses a 50-entry table at `0x00b14500`, a `mod 0x32`, and a call through
   `[0x00ac5690]`. I did not decode it, so I cannot yet *predict* that `0x16` = the tag for
   `game` — I only measured that those bytes are what the sections carry, consistently, in
   files spanning 2014–2026.
5. **`mp2014` (`03.02.03`) opcode numbering is unverified.** Its header parses, its stream
   ends exactly at EOF over 12 records, and under the `53 70` probe key the first packet of
   each record decodes as `leader_options` / `player_speed` / `next_check_sum` — the right
   shape for a 4-player game start. But 12 records is not enough to claim the 2003-era
   command set matches Extended Edition's.
6. **I did not execute the reader.** Everything in §3–§5 is static: decompiled structure
   plus arithmetic that lands exactly on real file contents. That is strong, but it is not
   Tier B. The oracle on hbox could call `FUN_00952d90` against a byte buffer and settle it.

---

## 8. Fidelity tiers

| claim | tier | basis |
|---|---|---|
| gzip container, single member, offset 0 | **C**, [measured] | `zlib.decompress` on 6 files, plus `FUN_00952b40` disassembly |
| header field layout §3 | **C**, [measured] | decompiled `FUN_005d6570` + disassembly, and the layout lands byte-exactly on 6 files across 3 engine builds |
| record framing §4 | **C**, [measured] | `FUN_00952fb0`/`FUN_00952d90`, and the chain consumes 6/6 files to the last byte |
| opcode table §5 | **C**, [measured] | handler return values from `FUN_0094a700`'s arms; 11,946 packets in `today.rcx` and 16,146 in `solo2024` split with zero residue |
| `0x39` = the `check_all` channels in order | **C**, [measured] | log-literal offsets in `FUN_009459d0` vs `FUN_00936560` |
| solo replays contain no AI commands | **C**, [measured] | `play == 0` for all 10,544 + 14,197 + 74 records in three solo files |
| GameInfo `+0x04` is the game seed | **hypothesis**, not measured | see §7.3 |
| multiplayer payload = 2-byte XOR | **refuted as stated**, partially true | see §6 / §7.1 |

Nothing here is Tier A or Tier B. There is no SMT proof and no differential test against
the retail reader; §7.6 says what it would take to earn Tier B.

---

## 9. Corpus

Five additional specimens were pulled from the VM for this lane, into
`/private/tmp/…/scratchpad/rcx/` (**not** committed — `ron-data/replays/` is gitignored
and these are game-derived user data):

| local name | source | bytes | decompressed |
|---|---|---|---|
| `mp2014.rcx` | `…\Recorded Games\multi\playback - 2014.04.26 16'11'55 (sat).rcx` | 61,068 | 1,026,493 |
| `mp2020.rcx` | `…\multi\Playback - 2020.07.25 19'30'12 (Sat).rcx` | 68,717 | 1,056,924 |
| `mp2024.rcx` | `…\multi\Playback - 2024.02.23 20'49'35 (Fri).rcx` | 117,966 | 1,285,149 |
| `solo2020.rcx` | `…\Playback - 2020.02.08 10'38'37 (Sat).rcx` | 59,004 | 1,027,348 |
| `solo2024.rcx` | `…\Playback - 2024.03.18 21'46'37 (Mon).rcx` | 134,710 | 1,439,992 |

The VM holds ~60 more spanning 2014, 2017, 2018, 2019, 2020 and 2024, most of them
multiplayer. Extraction script: `/private/tmp/…/scratchpad/fetch.py` (prlctl exec +
`certutil -encode` hop, one fresh temp name per file, as `docs/binary-ground-truth.md`
requires).
