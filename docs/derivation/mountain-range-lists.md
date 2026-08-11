# `Mountains::randomize_mountains` `0x0089ca70`, and where the three range lists come from

Lane: `oracle-mountains`. Every claim below is marked **[measured]** — I disassembled it
against `ron-bin/riseofnations.exe` sha256 `30478a44…625079` (image base `0x00400000`,
`objdump -d`, and the Ghidra bulk decompiles in `re/decomp-all/`), or parsed it out of the
shipped data with a real XML parser — or **[executed]**, meaning retail machine code ran
against the shipped Rust on hbox through `crates/oracle`, or **[external]**, meaning it
rests on a contract of a library that is not in this image. Nothing here is "verified" in
the proof-assistant sense; see `docs/CHARTER.md`.

---

## 0. Headline

`Mountains::randomize_mountains` is now **Tier B**: registered as oracle case
`randomize_mountains`, executing the retail bytes at `0x0089ca70` against the shipped
`don_sim::systems::mountains::Mountains::randomize_mountains`, and comparing the entire
post-call `MountainsData` list window plus every fabricated node byte plus the resulting
`game_random` word. It was Tier C — `docs/assembly/replay-place-all-boundary.md` §6 said
so explicitly: *"Nothing here is oracle-executed."* That sentence is now out of date for
this one function, and only this one.

Three things the case establishes that a static read could only assert:

- The **draw predicate** and therefore the **draw count**. The shipped `1 / 8 / 7` lengths
  cost exactly **two** `Random::get(0, 0xffff)` words, not three — and a one-bit mutation
  of that predicate in the shipped Rust makes the differential fail on its very first edge
  trial (§3.4).
- The **cursor writes**: `current_data` is a dword, `current_metric` is a **single byte** at
  `+4` (bytes `+5..+8` are not touched), `current_node` is a dword, and `length`/`head` are
  not written at all.
- That the call **writes nothing else** anywhere in `MountainsData +0 .. +0x60` — including
  the `ranges` free-slot counter at `+0x50`, which §2.4 shows is the field that decides
  whether a 17th mountain range corrupts the heap.

What it does **not** establish is which payload value sits at which list position. That is
`Mountains::init`'s XML walk, and §4 settles as much of it as this image can settle.

---

## 1. The body, in full

`TerrainGroups::place_all` `0x006a70d0` calls it at `0x006a7330`, as the first substantive
act of every generated map. The 222-byte body takes no arguments and never reads `ECX`
[measured]; it reaches its object through two fixed globals.

```text
0089ca70  mov  ecx, [0xe85f64]          ; Mountains+4 -- the virtual-base pointer
0089ca76  push esi
0089ca77  mov  eax, [ecx + 8]           ; vbtable[2] -- the virtual-base displacement
0089ca7a  mov  esi, [eax + 0xe85f74]    ; small_ranges.length
0089ca80  lea  eax, [esi - 1]
0089ca83  test eax, eax
0089ca85  jg   0x89ca8b                 ; draw only when length - 1 > 0
0089ca87  xor  edx, edx                 ; else index 0, and NO Random::get
0089ca89  jmp  0x89caa6
0089ca8b  mov  ecx, [0x00c06184]        ; GameAccess::game_random -- the MAIN sim LCG
0089ca91  push 0xffff ; push 0
0089ca98  call 0x00a39d70               ; Random::get(0, 0xffff)
0089ca9d  mov  ecx, [0xe85f64]
0089caa3  cdq ; idiv esi                ; SIGNED remainder by length
0089caa6  mov  ecx, [ecx + 8]
0089caa9  push edx
0089caaa  add  ecx, 0xe85f68            ; &small_ranges
0089cab0  call 0x0046f0e0               ; LinkListBase<int,unsigned char>::seek_index
```

and the same shape twice more, at `0x0089cab5` for **medium** (`add ecx, 0xe85f80`, length
at `[eax + 0xe85f8c]`) and `0x0089caf9` for **large** (`add ecx, 0xe85f98`, length at
`[eax + 0xe85fa4]`) [measured].

> `docs/assembly/replay-place-all-boundary.md` §1 quotes the medium and large addresses as
> `+0xe85f80` and `+0xe85f98` alongside the small **length** address `+0xe85f74`. Those are
> not the same kind of address: `0xe85f74` is a length field and the other two are list
> bases. The consistent triples are bases `0xe85f68 / 0xe85f80 / 0xe85f98` (stride `0x18`)
> and lengths `0xe85f74 / 0xe85f8c / 0xe85fa4` (base `+0xC`) [measured]. The lane's
> conclusions are unaffected; the transcription is corrected here.

### 1.1 The virtual-base displacement is real data, not an assumption

`Mountains` is at `0x00e85f60` and its virtual base `MountainsData` is reached through the
vbptr at `0x00e85f64`. That word is **runtime-initialised** — the `.data` section holds no
raw bytes that far in (raw data stops at VA `0x00caa000`), so a file-image reader sees zero.
The constructor's store is in `.text`:

```text
file offset 0x3396f   c7 05 64 5f e8 00  c8 45 b2 00
                      mov dword ptr [0xe85f64], 0xb245c8
```

and the vbtable at `0x00b245c8` reads `00000000 6c010000 6c010000 54020000`, so
**`vbtable[2] = 0x16c`** [measured]. Therefore:

| | VA |
|---|---|
| `MountainsData` = `0x00e85f64 + 0x16c` | `0x00e860d0` |
| `small_ranges` = `MountainsData + 0x04` | `0x00e860d4` |
| `medium_ranges` = `MountainsData + 0x1c` | `0x00e860ec` |
| `large_ranges` = `MountainsData + 0x34` | `0x00e86104` |
| `ranges` free-slot counter = `MountainsData + 0x50` | `0x00e86120` |

which is exactly the `+4 / +28 / +52` layout `crates/don-sim/src/systems/mountains.rs`
documents. The oracle case reads the displacement out of the mapped vbtable rather than
hard-coding it, and fails if it is not `0x16c` (§3.2).

All sixteen absolute operands in the body carry base relocations [measured], so the case can
run against a relocated mapping at all.

---

## 2. `LinkList<int, unsigned char>` — layout and insertion order

### 2.1 The struct, from its two consumers

| offset | field | evidence |
|---|---|---|
| `+0x00` | `current_data` (`int`) | `seek_index` `0x0046f0fb  mov [edi], eax` |
| `+0x04` | `current_metric` (`unsigned char`) | `0x0046f0ff  mov al, [edx+0xc]` / `0x0046f102  mov [edi+4], al` — **one byte** |
| `+0x08` | `current_node` | `0x0046f0f4  mov [edi+8], edx` |
| `+0x0c` | `length` | `0x0046f10b  mov ebx, [edi+0xc]` (the walk clamp) |
| `+0x10` | `head_node` | `0x0046f0e6  mov edx, [edi+0x10]` |
| `+0x14` | unread by either routine | — |

Node layout, from `LinkListBase::add` `0x004a4af0`: `next` at `+0`, `prev` at `+4`, `data`
at `+8`, `metric` (one byte) at `+0xC` [measured].

### 2.2 `seek_index` `0x0046f0e0` restarts from the head, and clamps

```text
0046f0e6  mov  edx, [edi+0x10]      ; head
0046f0e9  test edx, edx
0046f0eb  jne  0x46f0f4
0046f0ed  xor  eax, eax             ; EMPTY LIST: return 0, write NOTHING
...
0046f10b  mov  ebx, [edi+0xc] ; dec ebx
0046f110  cmp  esi, ebx ; je 0x46f12a   ; stop at length - 1
```

So any cursor carried in from a previous game is unobservable, and an out-of-range index is
clamped to the tail rather than running off the ring [measured]. The shipped Rust models the
clamp by direct indexing, which agrees on the domain `randomize_mountains` produces
(`index = draw % length`, always `< length`).

### 2.3 `add` makes each new node the head, so payload order is reverse insertion order

`LinkListBase::add` `0x004a4af0` writes `metric = 0` unconditionally, stores its argument as
the node payload, splices the node into the circular ring **and assigns `head = new`** on
both the empty and the non-empty path [measured]. Head-first traversal is therefore reverse
insertion order, and every metric in a retail-built list is zero.

### 2.4 The `ranges` array saturates at exactly 16, and the overflow diagnostic cannot fire

`Mountains::add_range` `0x008992b0` allocates the range object, parks it in the first free
slot of the 16-entry `ranges` array at `MountainsData + 0x50`, and **returns that slot
index** — which is the value `Mountains::init` then pushes into the area list. Its guard:

```text
8992d5  inc  dword ptr [eax + 0xe85fb4]   ; count++  -- BEFORE the scan
8992f0  cmp  dword ptr [eax], 0           ; scan for a free slot
8992f9  cmp  esi, 0x10 ; jl 0x8992f0      ; esi saturates at 16
8992fe  cmp  esi, [ecx + 0xe85fb4]
899304  jge  0x899383                     ; "Too Many Ranges"
```

**[measured] the "Too Many Ranges" branch is unreachable.** On the *N*-th call the counter
is already *N* and the scan yields `min(N-1, 16)`, so `esi >= count` is never true — and the
17th range is written to `ranges[16]`, one dword past a `malloc(0x40)`. `Mountains::init`
sizes the array by `if (capacity < 0x10) { capacity = 0x10; ranges = malloc(0x40); }`, so
there are exactly 16 slots. The shipped data has exactly 16 `<MOUNTAIN>` elements and all
16 reach `add_range` (§4), i.e. **the shipped section saturates the array exactly** and a
mod that adds a seventeenth mountain corrupts the heap rather than getting the diagnostic.
Not exploited, not fixed here — recorded because anyone extending the mountain section will
meet it.

---

## 3. The oracle case

`crates/oracle/src/registry.rs` case id `randomize_mountains`, executor
`Plan::RandomizeMountains` in `crates/oracle/src/run.rs`.

### 3.1 Both sides are the shipped code

Retail side: the real bytes at `0x0089ca70`, entered with `call`, including the real
`Random::in_range` `0x00a39d70` (a fake TEB is installed for its SEH prologue) and the real
`seek_index` `0x0046f0e0`. Model side: `don_sim::systems::mountains::Mountains::
randomize_mountains`, which draws through `don_sim::rng::Random::get`. Nothing is
transcribed into the harness.

**A side effect worth naming:** `don_sim::rng::Random` had no oracle case at all — the
registry's two RNG cases (`rng_next_float`, `rng_in_range`) point at
`oracle::models::rng`, copies that live only in the harness, and `crates/don-sim/src/rng.rs`
says in its own header that it is *"Tier C. Transcribed from the disassembly and self-tested
here; no oracle execution has compared it against retail."* This case executes retail's
`Random::get(0, 0xffff)` and compares the resulting state against the shipped
`don_sim::rng::Random`. Over the full run that is **363,603 in-call draws**
(`randomize_draw_counts: zero=10672 one=45164 two=114125 three=30063`), every one of them
compared state-for-state. That header is now understated for the `(0, 0xffff)` call shape.

### 3.2 The fixture

- `[0x00e85f64]` ← the mapped address of the real vbtable at `0x00b245c8`, i.e. what the
  constructor writes. The displacement is then **read back** and the case fails if it is not
  `0x16c` or if the three list bases are not `MountainsData + 4 + 0x18*i` with each length
  exactly `base + 0xC`. That is the `structure` phase: four trials that make the layout an
  observation rather than a premise.
- The three `LinkList` states land at their **real** `.data` addresses. Only the circular
  node rings and the `Random` word live in a private arena.
- `[0x00c06184]` ← a pointer to that arena word, so the draw runs on the main stream.

### 3.3 What is compared, per trial

Every byte of `MountainsData +0 .. +0x60` (all three list structs plus a guard word before
and 20 bytes of guard after, including the `ranges` counter at `+0x50`); every byte of all
48 fabricated nodes; the final `game_random` word; and the shipped receipt's
`selected_indices` and `draws` against the observed LCG step count. Everything not written
by the call is pre-patterned with a per-trial pattern, so a stray write is a mismatch rather
than a coincidence.

### 3.4 Result, and the mutation

| | |
|---|---|
| trials | 200,028 (`structure` 4, `edges` 24, `shipped-configuration` 50,000, `random` 150,000) |
| mismatches | 0 |
| excluded | 0 — no trial in this case is discarded |
| draw-count split | `zero=10672 one=45164 two=114125 three=30063` |
| suite after this case | 25 cases, 19,022,634 trials, zero mismatches, exit 0 |

Measured on hbox, `i686-unknown-linux-musl`, image sha256 `30478a44…625079`, through
`tools/oracle-regress.sh` with no `--only` and no `--scale`.

**Which tree the record describes.** `crates/don-sim` was red in the shared working tree for
most of this lane — several sibling lanes were mid-migration in `command.rs`,
`systems/mod.rs` and `leader_process_taunt.rs`, and `HEAD` itself does not build (its
`command.rs` declares `air_containment_host` / `economy_group_actions`, whose files are
still untracked, and imports `hotkey_group_action`, which `HEAD`'s `systems/mod.rs` does not
declare). The record was produced from `git archive HEAD` plus a whole-directory snapshot of
the working tree's `crates/don-sim` taken at a green moment, plus this lane's two
`crates/oracle` files. None of the seven don-sim files the siblings are editing is a model
any registered case points at, so the delta from `HEAD` is invisible to the measurement —
but the run is a green of that snapshot, not of the shared working tree, and it is recorded
that way rather than implied to be more.

Distributions and exclusions are in the case's `caveat` and `distribution` strings and are
copied verbatim into `schema/oracle-regression.json`. The exclusions in one line: lengths
`0..=16` per list with `length` and the ring always consistent and `head` null exactly when
`length == 0` (so retail's **negative-length** arm is UNTESTED and desynchronised
length/head is unrepresentable); an empty list's cursor triple installed as zero because
that is the only empty state the shipped Rust can express; node metrics randomised even
though retail's `add` only ever produces zero.

**Mutation [executed].** On the hbox copy of `don-sim` only — the shared working tree was
never modified — `crates/don-sim/src/systems/mountains.rs:149` was changed from
`if ranges.len() > 1` to `if ranges.len() > 0`: one bit of one literal, and exactly the
predicate this lane is here to pin. Rebuilt and re-ran at `--scale 0.01`:

```text
FAIL  randomize_mountains  0x0089ca70  2028 trials  1490 mismatches
  shipped-1-8-7 zero-seed lens=[1, 8, 7] seed=0x00000000
    model=(idx [0, 1, 5], draws 3, state 0xd1ccf6e9)
    retail_state=0x47502932 first_window_byte=Some(28) draws_agree=false
```

The first edge trial fails: the mutant draws three main-stream words where retail draws two,
and the first divergent byte is offset 28 into the window — `medium_ranges.current_data`.
The 538 trials that still passed are the ones where no list had length exactly 1, which is
the mutation's exact blind spot. The remote copy was restored from the unmodified local file
and the full suite re-run green.

---

## 4. The list order, settled as far as this image settles it

`docs/assembly/replay-place-all-boundary.md` §6 lists this as inherited and unverified:
*"that `Mountains::init` walks the `<MOUNTAIN>` elements in document order was reported by a
sibling research pass and not re-derived."* Here is what each half rests on.

### 4.1 The shipped census, re-measured

Parsed with `xml.etree` (not a regex — see the standing board finding):
`ron-data/effects_graphics.xml` has exactly one `<MOUNTAINS>` block with exactly 16
`<MOUNTAIN>` children and no `<MOUNTAIN>` anywhere else in the document. `area` runs
`lg` × 7 at document positions 0–6, `med` × 8 at 7–14, `sm` × 1 at 15. Every one of the 16
carries non-empty `TEMPLATE_TEX`, `MAIN_ALPHA_TEX` and `RING_ALPHA_TEX` children with a
`file` attribute [measured].

That last clause is load-bearing and was not previously stated. `Mountains::init`
`0x0089ad70` calls `add_range` **only** when all three of those strings are non-empty, and
when it does not, the stale previous slot index is still pushed into the area list [measured
from the decompile's `if ((local_ce._2_2_ != 0) && (local_ba._2_2_ != 0) && (local_a6._2_2_
!= 0))` guard and the `iVar3 = local_14` carry]. In the shipped data the guard passes 16
times out of 16, so all 16 slots fill in document order and the payload of the *k*-th
document element is *k*.

The internal-string ordinals are re-decoded here against `ron-data/internal_strings.xml`
(7,630 `STRING` elements, 20-byte records, `add eax, N` ⇒ ordinal `N/20`):

| `.text` offset | ordinal | value |
|---|---|---|
| `+0x181dc` | 4939 | `MOUNTAINS` |
| `+0x181f0` | 4940 | `MOUNTAIN` |
| `+0x18204` | 4941 | `TEMPLATE_TEX` |
| `+0x18218` | 4942 | `MAIN_ALPHA_TEX` |
| `+0x1822c` | 4943 | `RING_ALPHA_TEX` |
| `+0x18240` | 4944 | `area` |
| `+0x18254`…`+0x18290` | 4945–4948 | `sm`, `sml`, `med`, `lg` |
| `+0x3a0c` | 743 | `file` |

`docs/assembly/replay-place-all-boundary.md` §1 reports `area` at ordinal 4944 (right) but
implicitly at offset `+0x18204` (wrong — that is `TEMPLATE_TEX`). Corrected here.

`sm` and `sml` both select the small list; `med` the medium; `lg` the large; anything else is
a `mountains.cpp` diagnostic [measured, `0x0089b8fd`–`0x0089b9c7`].

### 4.2 The in-image half of the order claim: **[measured]**

`XMLNode::get_elements` `0x00a27720` is not a tree walk of its own. It is:

```text
a277db  call [ecx + 0x90]   ; IXMLDOMNode::selectNodes(BSTR "MOUNTAIN", &nodeList)
a27802  call [ecx + 0x20]   ; IXMLDOMNodeList::get_length(&n)
a27827  call [ecx + 0x24]   ; IXMLDOMNodeList::nextNode(&node)   -- n times
        call [node + 0x28]  ; get_nodeType; keep only NODE_ELEMENT (1)
```

and each kept node is `QueryInterface`d to `IXMLDOMElement`, wrapped, and **appended** to the
caller's `ObjectArray<XMLElement>` in iteration order [measured]. `Mountains::init` then
walks that array **forward from index 0** — its cursor starts at `items + 8` dwords and
advances by 10 dwords per iteration (element stride `0x28`), for exactly `count` iterations —
inserting each element's `add_range` slot index with `LinkListBase::add`, which makes it the
head [measured]. The area dispatch is a four-way string compare in the order
`sm`, `sml`, `med`, `lg`, with `sm` **and** `sml` both reaching the small list
(`0x0089bb00`), `med` the medium (`0x0089b95b`, type 2) and `lg` the large (`0x0089b9df`,
type 3); anything else is a `mountains.cpp` diagnostic [measured].

So, entirely within this image: **head-first list order is the reverse of the order MSXML's
node list yields.**

### 4.3 The residual: **[external]**

Whether that node-list order is document order is `IXMLDOMNode::selectNodes`' contract, and
MSXML is not in this binary. Its documented behaviour is that a node list from an
XPath/XSLPattern query is in document order; nothing in `riseofnations.exe` can be read to
confirm or refute it, and this lane did not run a live capture.

**Net position.** The claim "payload order is `small = [15]`, `medium = [14…7]`,
`large = [6…0]`" is now: the census, the range-index assignment, the forward array walk and
the head-first insertion are all **[measured]**; the single remaining dependency is MSXML
returning `selectNodes` matches in document order, which is **[external]** and named as
such. That is strictly narrower than "reported by a sibling pass", and it is as far as
static reading of this image can go. Closing it needs a live capture of `MountainsData`
after `Mountains::init`, not more disassembly.

---

## 5. What is deliberately not claimed

- **Nothing about `Mountains::get_range` `0x0089cb50`, `Mountains::add_mountain`
  `0x0089c2e0`, or `TerrainGroups::place_all` itself.** Those remain Tier C, and
  `add_mountain` still needs the `.tga` displacement art `ron-data/` does not hold.
- **Nothing about `Mountains::init` executing.** §4 reads its instructions and its input; it
  was not run.
- **Nothing about negative `length`.** The shipped Rust cannot express it, so the third arm
  of `lea eax,[esi-1]; test eax,eax; jg` is untested.
- **The `ranges` overflow in §2.4 is a static reading**, not something the oracle executed;
  `add_range` allocates and calls a constructor, which the harness does not fabricate.
- **Tier B is testing, not verification.** 200,028 trials over the stated distribution say
  nothing about inputs outside it.

---

## 6. Ledger rows to add to `docs/provenance-ledger.md`

| mechanic | source | tier | evidence |
|---|---|---|---|
| `Mountains::randomize_mountains` — draw predicate, draw count, signed remainder, head-relative seek, and the exact three cursor writes | retail `0x0089ca70` executed against `don_sim::systems::mountains::Mountains::randomize_mountains` | **B** | oracle case `randomize_mountains`, 200,028 trials, 0 mismatches; full `MountainsData +0..+0x60` window, all 48 nodes and the `game_random` word compared byte-for-byte; one-bit predicate mutation caught on the first edge trial |
| `don_sim::rng::Random::get(0, 0xffff)` reproduces retail `Random::in_range` `0x00a39d70` state-for-state | same case, 363,603 in-call draws | **B** (for that call shape only) | previously Tier C by `crates/don-sim/src/rng.rs`'s own header; the registry's RNG cases test harness-local copies, not the shipped `Random` |
| `MountainsData` is at `0x00e860d0`, from vbtable `0x00b245c8` entry [2] = `0x16c`, with the three `LinkList` states at `+4/+0x1c/+0x34` | constructor store at file offset `0x3396f`; vbtable read | structural [measured] + **B** | the oracle `structure` phase reads the displacement back and fails if it is not `0x16c` |
| `LinkList<int,unsigned char>` writes `current_metric` as **one byte** at `+4`, leaving `+5..+8` untouched | `seek_index` `0x0046f102 mov [edi+4], al` | **B** | byte-for-byte window comparison over 200,024 calls |
| `Mountains::add_range`'s "Too Many Ranges" diagnostic is unreachable; the 17th range writes one dword past a 64-byte allocation | `0x008992d5` / `0x008992f9` / `0x008992fe` | structural [measured] | the counter is incremented before a scan that saturates at 16, so `esi >= count` is never true |
| The shipped `<MOUNTAINS>` section is 16 elements, `lg`×7 / `med`×8 / `sm`×1, all three texture children non-empty, exactly saturating the 16-slot `ranges` array | `ron-data/effects_graphics.xml`, parsed | structural [measured] | 1 block, 16 direct `<MOUNTAIN>` children, no `<MOUNTAIN>` elsewhere in the document |
| `XMLNode::get_elements` is `selectNodes` + `nextNode`, appending in node-list order; `Mountains::init` walks the result forward and inserts head-first | `0x00a277db` / `0x00a27802` / `0x00a27827`; the `get_elements` call in `init` returning to `0x0089b1e4`, and the element loop after it | structural [measured], with the document-order half **[external]** to MSXML | replaces the inherited "walks in document order" claim with its measured half plus a named library contract |
