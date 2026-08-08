# Adversarial audit — the replay lanes (`replay-io`, `replay-stream`, `replay-checksum`)

Auditor lane. I read the three lane reports myself, re-derived the load-bearing claims from
`ron-bin/riseofnations.exe` with capstone, and re-parsed the specimens with my own code
written from the disassembly rather than from the lanes' parsers. Every number below that I
produced is **[measured]**; where I am repeating a lane's number I say so.

Nothing in any of the three lanes is Tier A or Tier B under `docs/CHARTER.md`. See §5.

---

## 0. Verdict

**The replay work is substantially correct and the headline finding is real.** The command
stream, the 82-opcode table, the 18-byte framing, the 65-byte `check_sums` packet, and the
per-turn cross-player checksum oracle all reproduce independently. I did not find folklore
anywhere in these three lanes — the whole thing traces to addresses.

But the three lanes disagree with each other on the record header, two of the three are
wrong about it, one lane's central "100.000000%, zero exceptions" statistic is a category
error, one lane's record count for the reference specimen is off by 12, the evidence phrase
all three lean on ("parses exactly to EOF, zero residue") is nearly worthless on its own,
and the summary handed upward carries a **Tier A** label on a claim that has no proof.

**And the lanes' stated blocker is not a blocker.** `replay-io` closed with "THE BLOCKER:
multiplayer command payloads are obfuscated… This blocks the highest-value use of the
corpus", and `replay-stream` concluded "MP command streams are **NOT** statically decodable".
Both are **refuted [measured]**: I decoded **25,279 of 25,279** multiplayer command packages
across four specimens **exactly, zero residue, zero undecodable commands**, using a key that
is sitting in the replay's own plaintext header. §2.

`cargo test` at `/Users/ember/dev/don`: **passes**, exit 0 — 73 tests across 6 binaries
(8 + 5 + 5 + 18 + 37), plus 5 harnesses running 0 tests. No failures, no ignored tests.
None of them touch replay code; the replay lanes shipped Python, so `cargo test` is not
evidence about anything audited here.

---

## 1. What I re-derived and confirm

All of this I produced myself from the binary or from the files, not from the reports.

### 1.1 Container and header

| claim | lane | my result |
|---|---|---|
| whole file is one gzip member, 0 unused bytes | io | **confirmed** — `today.rcx` 108,241 B → 1,332,849 B, `unused_data` 0, `unconsumed_tail` 0 |
| `0x28` is `'('`, not a length; `0x1a`=26 is the UTF-16 char count | io | **confirmed** — bytes 0..5 are `16 42 1a 00 00 00`, and `p[6:58].decode('utf-16le')` == `'(Version: 00.2024.06.2000)'`, exactly 26 units, ending at `0x3a`. The standing note in `docs/replay-format.md` is refuted, and the `ESTABLISHED` line in the lane brief ("then UTF-16LE `Version: 00.2024.06.20`") is an under-read of the same field |
| GameInfo layout, header end offsets | io | **confirmed** — I walked `4 + 16 + 4 + 29 + 1` then 8 slots of `tag(1) + u16 flags + (flags&1 ? 57 + u32 len + 2*len : 0)` and landed on `0x165` / `0x114` / `0x1b0` for today / mp2024 / mp2020, matching their table byte for byte. Slot tag byte is `0x50` [measured at `0x70`, `0xb6`, `0x106`] |
| GameInfo `+0x00` build stamp, `+0x04` per-game u32 | io | **confirmed** — today `0x06ac012c` / 557,986 |
| blob is fixed-size per build | io | **confirmed** — today 1,024,724; h1 1,024,729; mp2024 1,024,719; mp2020 1,024,729. (mp2024a is **1,024,739**, outside the "1,024,719–1,024,729" range the lane states for that build — a small overclaim) |
| stream is preceded by a u32 table ending `0x00000191` | stream | **confirmed and generalised** — the 8 bytes before the stream are `90 01 00 00 91 01 00 00` in **all five** specimens I checked, two engine builds |

### 1.2 Record framing — settled from the writer, not from a parse

`FUN_00952fb0` @ `0x00952fb0`, disassembled by me. Six writes, in this order:

```
00952fb3  mov  edx, [0xc061ec]      ; game object
00952fbe  add  edx, 0x550           ; -> 1st field on disk is game+0x550, a GLOBAL
00952fff  lea  edx, [edi + 4]       ; 2nd: pkg+0x04
0095302c  lea  edx, [edi + 8]       ; 3rd: pkg+0x08
0095305c  push edi                  ; 4th: pkg+0x00
00953086  lea  ebx, [edi + 0x10]    ; 5th: pkg+0x10, 2 bytes
009530b5  movsx edx, word ptr [ebx] ; 6th: pkg+0x12, that many bytes
```

The reader `FUN_00952d90` mirrors it exactly, and reads the first field into a **local**
(`[ebp-4]`) that it compares against the requested frame — it never becomes a
`CommandPackage` field at all. It then seeks back 4 if that frame is ahead
(`0x00952e2d sub eax,4`) and back 8 on player mismatch (`0x00952e74 sub eax,8`).

Field names from `FUN_0094b7c0`, which I disassembled: `push 0xaf76b8` = `L"stamp"` paired
with `push dword ptr [ebx]`, and `push 0xaf7718` = `L"play"` paired with
`push dword ptr [ebx+4]`. So `pkg+0x00` **is** `stamp` and `pkg+0x04` **is** `play`.

**Therefore the on-disk header is:**

| disk offset | width | source | correct name |
|---|---|---|---|
| `+0x00` | u32 | `[[0x00c061ec]+0x550]` | game **frame** — a global, not a package field |
| `+0x04` | u32 | `pkg+0x04` | `play` (player index) |
| `+0x08` | u32 | `pkg+0x08` | unnamed; 0/1 |
| `+0x0c` | u32 | `pkg+0x00` | `stamp` — a per-player package serial |
| `+0x10` | u16 | `pkg+0x10` | `size` |
| `+0x12` | | `pkg+0x12` | body |

`replay-io` has this **exactly right**, including the "stamp is written 4th, not 1st" note.
`replay-stream` (`[stamp][from][?][serial]`) and `replay-checksum`
(`stamp, from, ?, turn`) both name field 0 `stamp` and field 3 something else, which is
backwards. `replay-checksum` compounds it by asserting "the record layout **is** the
in-memory `CommandPackage` layout" — refuted at `0x00952fb3`, where the first thing written
is a global that is not in the structure.

### 1.3 Opcode table

I extracted the `case → handler` map from `re/decomp-all/0094a700.c` and pulled each
handler's return constant from its own decompilation, independently of both lanes' tables.
**82 cases, `0x00`–`0x51`, none missing.** 79 constant-size; 3 variable, and my extractions
agree with both lanes: `0x00 = data[1]*2+3`, `0x33 = u16@+4 * 8 + 6`,
`0x44 = i32@+0x0d * 2 + 0x13`. (`replay-checksum` lists `0x33`/`0x44` as unresolved — they
are resolvable from the same bulk decompilation, so that open item is a thoroughness gap,
not a real one.)

`0x39` size verified from **disassembly**, not decompilation: `00945e0e mov eax, 0x41` /
`00945e17 ret 4`. **65 bytes.** `docs/derivation/checksum.md` §4's `0x3d` = 61 is refuted.
The 16th u32 is read at `00945e02 mov eax, [edi+0x3d]` and stored to `[ecx*4+0xcbee90]`.

### 1.4 Channel order

`check_all` `FUN_00936560` logs, in order: units, builds, walls, ammo, deaths, groups, guys,
leaders, cities, items, goods, world, rules, scenario_data, script_run_time, **total** (16
literals, last at `0x00936b7d`). `FUN_009459d0` logs the first **fifteen** of those in the
same order (first at `0x00945a4b`) and reads the sixteenth at `+0x3d` without logging it.
So `replay-io`'s wire order is correct — but its cited evidence is not: it claims the
handler carries literals "`L"  units: %u"` … through `L"  total: %u"`". `L"  total: %u"`
lives at `0x00af7210` and is referenced from `check_all`, **not** from the `check_sums`
handler. Right conclusion, wrong citation.

### 1.5 Packet builder and gate

`FUN_00940770`: I scanned all of `.text` for `E8` rel32 targets and found **exactly one**
caller, `0x0093f2e9` — `replay-checksum`'s claim confirmed. `00940795 mov byte [ebp-0x70],
0x39`; gate `00940799 mov al,[edx+0x820]` / `test al,0x10` / `jne skip` / `test al,4` /
`je skip`; per-player flag `[edx+0x44 + player*0x8c + 0x30]` via `imul eax,[eax+0x2a0],0x8c`.
All confirmed as stated.

### 1.6 Solo specimen `today.rcx`

Parsed with my own code:

- stream start **`0xfa439`**, **10,544 records**, exact consumption to EOF
- `play == 0` for **all 10,544** — AI players emit nothing. Confirmed.
- field 0 (frame): 0…10,499, 10,500 distinct, non-decreasing
- field 3 (`stamp`): **exactly index+1 for all 10,544**
- field 2: nonzero on **record 0 only** (value 1) — `replay-stream`'s self-correction is real
- **11,946 commands, 0 parse failures**; histogram `{0x00:63, 0x07:2, 0x18:42, 0x19:18,
  0x20:1, 0x36:6, 0x48:10499, 0x49:1, 0x4c:1, 0x4f:1313}`
- **0 `check_sums` packets** in solo. Confirmed.
- `0x4f`: 1,313 packets, **all** at frames ≡ 1 (mod 8); accumulator totals
  `[7774, 2722, 125, 57, 1, 75, 0, 0]` — `replay-stream`'s "125 clicks, 57 hotkeys, 1
  minimap, 75 mainmap" reproduce exactly
- `0x48` camera: 10,499 packets, zoom ∈ {4,5,6}, **297** position changes — exact match
- `0x00` group: 63, of which **41** have `num == 0`, and **all 63** are immediately
  followed by a non-group command — exact match
- nation byte at `name_len − 7`: `0x16` for `cmr`, `0x18` for both `Player 2`/`Player 3` —
  exact match; slot index byte at `name_len − 4` runs 0,1,2

Three players (`cmr` + two AI) is consistent with the specimen's known "two AI opponents".

---

## 2. NEW RESULT — the multiplayer stream is fully decodable, and the seed is closed

This overturns the single most consequential "open" item in the swarm.

### 2.1 The mechanism, from `FUN_0094c500`

I disassembled `CommandPackage::process_all` @ `0x0094c500`. Three things matter, and only
`replay-stream` found two of them, and it drew the wrong conclusion from the second.

**(a) The XOR key** — `0x0094c5e0`:
```
0094c5e0  mov   eax, [ecx + 0x10]     ; ecx = [0x00c061ec], the game object
0094c5e6  shr   eax, 8
0094c5eb  movzx eax, ax                ; K = (u16)(game[0x10] >> 8)
```
gated on `test dl,4` (`game[0x820] & 0x04`), applied to `size/2` u16 words from `pkg+0x12`
(SSE-unrolled at `0x0094c651`, scalar tail at `0x0094c690`).

**(b) The padding RNG is a per-package LOCAL, not the sim RNG** — `0x0094c6a4`:
```
0094c6a4  mov   eax, [0xc061ec]
0094c6a9  test  byte [eax + 0x820], 4
0094c6b2  mov   ecx, [eax + 0x10]
0094c6b7  mov   [ebp - 0x18], eax'     ; [ebp-0x18] := game[0x10]   (the xor/xor pair is a no-op)
...
0094c6fa  push  2 / push 0 / lea ecx,[ebp-0x18] / call 0xa39d70   ; in_range(0,2)
0094c70f  lea   eax, [ecx + edx]       ; advance = command_size + skip
0094c717  sub   word [edi], ax         ; remaining -= advance
```
`[ebp-0x18]` is a **4-byte stack local**, seeded to `game[0x10]` once per `process_all`
call — i.e. **once per package** — and advanced only by the skip draws. It is *not* the
shared sim `Random`. So the padding sequence restarts identically at every package and is
completely reproducible offline.

`in_range` (`0x00a39d70`, tail at `0x00a39ea5`): `s = s*0x19660d + 0x3c6ef35f;
return lo + (((s & 0xffff) * (hi-lo)) >> 16)`. For `(0,2)` that is `(s>>15) & 1`.

**(c)** There is also an un-scrambled fast path nobody noted, at `0x0094c5a2`: if
`(game[0x820] & 0x14) == 0x14` and `!(game[0x823] & 2)` and `size != 0` and
`pkg->data[0] == 0x50` and `size <= 4`, the XOR loop is skipped entirely. It did not fire in
my corpus, but any decoder that assumes universal scrambling has a latent bug here.

### 2.2 `game[0x10]` **is** `GameInfo+0x04`

I first recovered the 32-bit seed by fixing bits 8..23 from the modal body word and brute
forcing the other 16. Then I noticed the recovered values shared their high bits with the
header field, and tested the header value directly. Result, using
`seed = GameInfo+0x04` read from the plaintext header and `K = (u16)(seed >> 8)`:

| specimen | build | `GameInfo+0x04` | K | **packages consumed exactly** |
|---|---|---|---|---|
| `h1` (2025 MP) | 00.2024.06.20 | 51,697 | `0x00c9` | **22,646 / 22,646** |
| `mp2024` | 00.2017.11.29 | 12,294,099 | `0xbb97` | **2,223 / 2,223** |
| `mp2020` | 00.2017.11.29 | 188,705,235 | `0x3f69` | **280 / 280** |
| `mp2024a` | 00.2017.11.29 | 9,356,832 | `0x8ec6` | **130 / 130** |
| | | | **total** | **25,279 / 25,279, zero residue** [measured] |

98,117 commands decoded, zero undecodable. Reproduce:
`/private/tmp/claude-501/-Users-ember-dev-don/62b78482-846c-4ffd-a44c-2199d3744a8e/scratchpad/{gi4test.py,final.py}`
(specimens `rc/h1.rcx`, `rcx/mp2024.rcx`, `rcx/mp2020.rcx`, `mp2024a.rcx` in that scratchpad).

**Consequences:**

1. `replay-io`'s "THE BLOCKER" is **refuted**. Nothing is blocked.
2. `replay-stream`'s "MP command streams are NOT statically decodable — a faithful reader
   must advance the same Random in lockstep" is **refuted**, both by the disassembly (the
   Random is a per-package local) and by 25,279/25,279.
3. `replay-io`'s "The RNG seed is NOT closed. GameInfo+0x04 is a per-game u32 and the
   obvious candidate" — **closed**, with a mechanism: `GameInfo+0x04` is the value the
   engine reads from `game[0x10]`, it is the sole source of the XOR key and of the padding
   keystream, and using it as-is decodes every package in four games across two engine
   builds. That is far stronger than the `--seeds` LCG-distance heuristic that lane
   proposed.
4. `replay-io`'s diagnosis "the keystream is not constant" was **wrong**. The keystream is
   perfectly constant; the missing piece was the per-command `in_range(0,2)` padding, which
   shifts alignment without touching the XOR.
5. Caution on method: my brute-force step found **1,280–4,864** seeds consistent with a
   30-package sample per file. Only `GameInfo+0x04` survives the whole file. A search that
   stops at the first consistent candidate would have shipped a wrong constant.

### 2.3 Independent checksum numbers

Extracted from the fully decoded streams (mine, not the lanes'):

| | h1 | mp2024 | mp2020 | mp2024a | total |
|---|---|---|---|---|---|
| packages | 22,646 | 2,223 | 280 | 130 | 25,279 |
| `check_sums` packets | 22,644 | 2,221 | 278 | 128 | **25,271** |
| `total == Σ(15) mod 2³²` | 22,644/22,644 | 2,221/2,221 | 278/278 | 128/128 | **25,271/25,271** |
| 15 real channels adler-32-shaped | 100% | 100% | 100% | 100% | **379,065/379,065** |
| frames with ≥2 reporters | 11,322 | 1,110 | 107 | 50 | **12,589** |
| divergent | 0 | 0 | 0 | 0 | **0** |
| `rules` channel | `0x12ba3104` | `0x12ba3104` | `0x12ba3104` | `0x12ba3104` | constant |
| `walls` channel | 1 | 1 | 1 | 1 | constant |
| checksum frame spacing | 6 | 6, 8 | 6 | 6 | — |

`replay-checksum`'s substance holds. Its `rules = 0x12ba3104` is exactly right, and I get it
from files it did not use for that number. `mp2024` at 2,221 matches its 2,221 and matches
`replay-io`'s 2,223-record count.

---

## 3. Findings against the lanes, worst first

### F1 — `replay-checksum`: "824,205 channel values, adler-32-shaped, 100.000000%, zero exceptions" is a category error, and false

`total` is a **wrapping sum of fifteen adler-32s**. It has no reason to look like an
adler-32 and it frequently does not. Measured on `h1` alone: **14 packets** carry a `total`
whose halves are not both `< 65521` — e.g. frame 16,741 `total = 0xfff8be13`, frame 64,753
`total = 0x7915ffff`, each reported identically by both players, each satisfying
`total == Σ(15)`. Both the count ("zero exceptions") and the framing are wrong: folding
`total` into an "all values are adler-shaped" test manufactures 1/16 of the sample from a
field where the test is meaningless. The defensible claim is:

> the **fifteen** channels are adler-32-shaped (379,065/379,065 measured), and the
> sixteenth is their wrapping sum (25,271/25,271 measured).

This matters because the "100.000000%" was presented as one of two headline validations.

### F2 — Tier inflation in the summary handed upward

The structured claim delivered to the orchestrator labels *"The `CheckSumsCommand` is
`0x41` = 65 bytes"* as **tier `"A"`**, with an evidence string that contradicts its own
label ("Structural claim measured off disassembly; the corpus check is tier B"). Charter
Tier A is an SMT equivalence over the entire input domain. **Nothing in any replay lane is
Tier A.** The doc's own ledger says "structural [measured] + B", so the inflation happened
in the summary, which is exactly where an orchestrator would read it.

Separately, `replay-checksum` labels corpus statistics **Tier B** throughout ("MP replays
embed a `check_sums` command in every package — B"; "`rules` = `0x12ba3104` — B"). Tier B
per the charter is *our Rust agreeing with the shipped code on N generated inputs*. No
replay lane executed shipped code at all. Reading N files is **Tier C / [measured]
observation**. Four ledger rows are affected. `replay-io` is honest about this ("Nothing
here is Tier B") and `replay-stream` mostly uses C/structural; the misuse is localised.

### F3 — "Parses exactly to EOF with zero residue" is not evidence, and all three lanes lead with it

I brute-forced it: in `today.rcx`, of the 8,192 candidate start offsets in
`[0xf9000, 0xfb000)`, **7,440** produce an 18-byte-header chain that terminates *exactly* on
the last byte with >500 records. The framing is self-synchronising, so landing on EOF is the
default outcome, not a discriminator. Exactly one offset (`0xfa439`) survives semantic
filters (`frame₀ = 0`, `stamp₀ = 1`, `play ∈ 0..7` for every record).

The conclusions are still right — because the *disassembly* fixes the framing, and I
confirmed it there. But "0 residue" appears as headline evidence in all three reports
("consumes all 6 specimens to the last byte with 0 trailing bytes"; "tile to EOF with ZERO
residual bytes"; "lands EXACTLY on the last byte in 4/4 files"), and it is carrying weight
it cannot bear. Any future lane citing zero-residue as proof of a framing should be sent
back.

### F4 — Three lanes, three different names for the same four fields; two are wrong

Settled in §1.2 from `FUN_00952fb0` / `FUN_00952d90` / `FUN_0094b7c0`. `replay-io` is
correct. `replay-stream` and `replay-checksum` both invert `stamp`. `replay-checksum`
additionally asserts the disk record *is* the in-memory layout, which the writer refutes.
This is not cosmetic: a `don-replay` crate written from `replay-checksum`'s table would
label the game frame `stamp` and the package serial `turn`, and its "turn" would be a
per-player counter that in `today.rcx` reaches 10,544 for a 10,500-frame game.

### F5 — `replay-checksum` parsed the reference specimen 12 records short, twice, with different numbers

It reports `today.rcx` as **10,532** packages in four places and **10,541** in its §7 table.
Measured: **10,544**. Its own §5 gives it away — "10,532 clean turns, turn 13…10544": its
anchor landed 12 records late and it read the pkg serial (max 10,544) as a turn number. Two
different wrong counts in one document, both stated `[measured]`, is a discipline failure
even though the lane's conclusion (0 `check_sums` in solo) is unaffected. Its
`0xf9e8c` anchor is 1,453 bytes before the true `0xfa439`.

### F6 — `replay-io`, three factual errors around `check_sums`

- "Multiplayer replays carry **~one per two turns**" — **false**. Measured: 25,271 of
  25,279 packages carry one, i.e. essentially every package, every turn, every player.
  `replay-checksum` has this right.
- "**1,390** such packets found in mp2024" — **false**. Measured: **2,221** in the same
  file. The lane's partial XOR probe undercounted by 37%, and it reported the undercount as
  a finding rather than as a symptom.
- "the first three channels reading 1 at turn 2 exactly as an empty adler-32 should" —
  **false**. The channels that read 1 are indices **2, 3, 4** (`walls`, `ammo`, `deaths`);
  `units` and `builds` are large from the first packet (frame 7, `units = 0x9EFA... `).
  `replay-checksum`'s "`walls` constant at 1" is the accurate version.
- minor: the `L"  total: %u"` literal it cites for `FUN_009459d0` is at `0x00af7210` and
  belongs to `check_all`.

### F7 — `replay-stream`, over-precise invariants

- "MP payloads … 97/130 packets parsed under a brute-force 0–2 byte tolerance" — superseded:
  **130/130 with no tolerance** once the seed is right. The "tolerance" was papering over a
  missing constant.
- "`frames_zoomed_in + frames_zoomed_out == 8` on **every** packet, totalling 10,496" — the
  total is right; the universal is **false** on exactly 1 of 1,313 packets (frame 1, all
  eight accumulators zero). 1,313 × 8 = 10,504 ≠ 10,496 is visible in the lane's own numbers.
- "63 selections / 63 orders in **exact 1:1 pairing**" — the direction is right (all 63
  `0x00` commands are immediately followed by a non-group command), but there are **134**
  non-camera non-speed commands, i.e. 71 orders, so 8 are unpaired.
- "10,500 frames ÷ **15 fps** = 700.000 s = 11:40, matching the specimen exactly … an
  independent confirmation of *both* the frame-stamp interpretation and the 15 fps tick" —
  this is one equation in two unknowns. The frame count is `[measured]`; 15 fps is
  **`[reported]`** (it comes in via the lane brief, not from any address). The match is a
  real and valuable cross-check of the frame interpretation *given* 15 fps; it is not
  independent evidence for 15 fps. I could not find the tick period in the binary in the
  time available, so it remains `[reported]`.
- Nation: the lane is appropriately careful and I confirm its bytes exactly (`0x16` human,
  `0x18` both AI), but the `.rdata` index→nation table ordering is unproven and the lane
  says so. Do not implement a nation id from this yet.

### F8 — nobody validated "score 766"

The brief lists four ground-truth facts for `today.rcx`. Dutch (weakly, via an unproven
table), 11:40 (via an assumed fps), two AI opponents (via three player slots) are addressed.
**Score 766 is validated by no lane and by nothing in the header I could find.** It is the
cheapest remaining independent check on the header decode and it is unspent.

---

## 4. Residual open items (mine, not theirs)

- **My decoder is not proven, it is measured.** 25,279/25,279 packages is a corpus
  statistic, not a theorem. `mp2014` (engine 03.02.03) does not yield a sane stream start
  under my filters at all, so the 2003-era opcode numbering remains untested — `replay-io`'s
  own caveat stands.
- **The un-scrambled `0x50`/`size ≤ 4` path** at `0x0094c5a2` never fired in my corpus. A
  decoder is incomplete until it handles it.
- **`[[0x00c061ec]+0x550]` is "the frame" by inference**, not by a write site. I found only
  one static write (`0x00458410`) and it is walked as part of `Game::walk_data`
  (`FUN_00589600` walks `+0x550`…`+0x6e4`). The inference is strong — 10,500 distinct
  values, one `0x48` camera packet per value, MP checksum spacing of 6 matching the MP turn
  length — but it is inference.
- **The 8 packages without a `check_sums`** (25,271 of 25,279) mean "every command package"
  is 99.97%, not 100%. Whether those are the per-player first packages is unchecked.
- **`FUN_00952d90` was never run.** `replay-io` correctly names this as the route to a real
  Tier B (call the retail reader on a controlled buffer via the hbox oracle and diff against
  the Python). Nobody did it, so the framing remains structural-plus-corpus. Now that the MP
  streams decode exactly, that oracle run is cheap and would upgrade the highest-value claim
  in the swarm from "it lands on real files" to actual differential testing.

---

## 5. Ledger corrections to make

1. `docs/derivation/checksum.md` §4: `CheckSumsCommand` is `0x41` = **65** bytes, `total` at
   `+0x3d`. (Both lanes right; I confirm from `00945e0e mov eax,0x41`.)
2. `docs/replay-format.md`: `0x28` is `'('`; the length is the u32 at `+2`.
3. `docs/derivation/replay-checksum.md`: `today.rcx` has **10,544** records, not 10,532/10,541;
   the header field at disk `+0x00` is the game frame and at `+0x0c` is `stamp`; drop the
   "all channel values adler-32-shaped, zero exceptions" claim in favour of the 15-channel
   version; demote four Tier B rows to Tier C / [measured].
4. `docs/derivation/replay-io.md`: remove "THE BLOCKER"; `check_sums` is one per package, not
   one per two turns; mp2024 has 2,221 check_sums, not 1,390; the channels reading 1 are
   `walls`/`ammo`/`deaths`.
5. `docs/derivation/replay-stream.md`: MP streams **are** statically decodable; mark 15 fps
   `[reported]`; soften the zoom and pairing universals.
6. New ledger entry: **`GameInfo+0x04` == `game[0x10]` is the per-game seed**; `K =
   (u16)(seed >> 8)` scrambles MP bodies as u16 words from `pkg+0x12`; a per-package local
   LCG seeded to the same value emits `in_range(0,2)` padding after each command. Source
   `FUN_0094c500` @ `0x0094c5e0` / `0x0094c6a4`; `in_range` @ `0x00a39ea5`. Tier C
   [measured], 25,279/25,279 packages across 4 specimens and 2 engine builds.
