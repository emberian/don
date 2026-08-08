# Derivation: movement and pathfinding

Lane: `pathfinding`. All addresses are preferred-base VAs (image base `0x00400000`).
Everything below is marked **[measured]** (I verified it against
`ron-bin/riseofnations.exe`, `ron-data/`, or the oracle on hbox) or **[inference]**
(a reading of structure that I have not put under behaviour). Nothing here is
"verified" in the proof-assistant sense; the one Tier-B result states its sample count.

---

## Headline

**Rise of Nations' pathfinder is a pure-integer, 8-connected grid A\* over the tile
grid, and it calls the engine RNG once per edge relaxation.** [measured]

Whole-sim bit-exactness is *achievable*, and the reason is stronger than "probably":
the transitive call cone from the A\* driver (187 functions) contains **zero** of the
eight non-IEEE CRT transcendentals, **zero** x87 instructions, and exactly **five** SSE
instructions — all five inside the terrain-height sampler `FUN_008544a0`, which does
`(h_a + h_b) * 0.5f` then `cvttss2si`. That is plain IEEE binary32 add/mul/truncate and
maps 1:1 onto Rust `f32`. The A\* driver `FUN_00685990` and the cost function
`FUN_00686300` contain **no floating-point instruction at all**. [measured]

The determinism risk is therefore **not** floating point. It is the RNG: `FUN_00686300`
(the per-edge cost) calls `Random::range(0, 0xFFFF)` at `0x00686341` and takes the result
`mod 20` as a cost jitter. Pathfinding is a *consumer of the sim RNG stream*, so any
divergence in how many edges we relax — one extra node expanded, one different tie-break
— desynchronises every later RNG draw in the whole simulation. Reproducing the pathfinder
edge-for-edge is a hard prerequisite for reproducing combat, not an independent module.

The engine agrees that pathfinding is sim-critical: `PathfinderSync` is SyncLogger
channel **id 0x1E**, tag `Pthfd`, sitting in the same table as `UnitsSync`, `BuildsSync`,
`GuysSync`, `RulesSync`, `ChecksumSync`. [measured]

---

## 1. Class layout

RTTI descriptors, and the vtables reached from their complete-object locators: [measured]

| item | address |
|---|---|
| `.?AVPathFinder@@` TypeDescriptor | `0x00C98540` |
| `.?AVPathFinderData@@` TypeDescriptor | `0x00C98520` |
| `.?AVPathFinderOut@@` TypeDescriptor | `0x00C98504` |
| `PathFinder` COL / vtable | `0x00B7BACC` / **`0x00B4546C`** |
| `PathFinderOut` COL / vtable | `0x00B7BAB8` / **`0x00B45458`** |
| vbtable (`[-60, 0x8C, 0x90]`) | `0x00B4547C` |
| ASCII `"Pathfinding"` (the param-block name) | `0x00B45448` |

Hierarchy, read out of the base-class-descriptor arrays at `0x00B7B9D4` (7 entries) and
`0x00B7B9F4` (5 entries): [measured]

```
PathFinder : PathFinderOut : BaseParamRegister (mdisp 0), PathFinderData (mdisp 0x40)
PathFinderData : GameAccessConst (mdisp 0)
virtual bases: MiscAccess (vbtable[1] -> +0xC8), GameAccess (vbtable[2] -> +0xCC)
```

Sizes come from the scalar-deleting destructors (`push 0xCC` at `0x00479365`,
`push 0xC8` at `0x004793C1`): **`sizeof(PathFinder) = 0xCC` (204), `sizeof(PathFinderOut)
= 0xC8` (200)**. [measured]

The `PathFinder` vtable has only four slots:

| slot | fn | role |
|---|---|---|
| 0 | `0x00479320` | scalar deleting dtor |
| 1 | `0x0047A920` | returns `"Pathfinding"` (the `BaseParamRegister` block name) |
| 2 | `0x00687E40` | reset / per-game re-init |
| 3 | `0x0041BFF0` | `xor eax,eax; ret` |

**There is exactly one PathFinder in the process: a global at `0x00E85E40`**, constructed
by `FUN_0068A030`. [measured] The `PathFinderData` subobject therefore starts at
`0x00E85E80`, which is why the A\* code addresses `[0xE85E80]`, `[0xE85E84]`,
`[0xE85E88]` directly — those are `this+0x40/+0x44/+0x48`. `FUN_00688150` (ISLAND, 276 B)
zeroes `this+0x40 .. this+0xC0`, which brackets `PathFinderData` exactly.

`PathFinder.cpp` exists as an assert filename in both ASCII (`0x00ADD3A4`, 5 copies) and
UTF-16 (`0x00ADD2F0`, 3 copies), so the module is unambiguous. Two assert messages are
load-bearing: **`"crappy red-black tree"`** (`0x00ADD284` / `0x00ADD350`) and
**`"failed finding an open node somehow"`** (`0x00ADD370`). [measured]

---

## 2. Coordinate system — settled, and it matches `rules.xml`

`FUN_00681DB0` builds the global coordinate table pointed to by `[0x00CAE5FC]`: [measured]

```
FUN_00681DB0(ecx = N):
    span   = 24*N
    buffer = malloc(192*N)                  ; 48*N dwords
    T      = buffer + 96*N                  ; centred: valid index range [-24N, 24N)
    [0x00CAE5FC] = T
    for i in 0 .. span-1:   T[i] = i / 3    ; magic 0xAAAAAAAB, shr 1  (unsigned)
    for i in -1 .. -span:   T[i] = i / 3    ; magic 0x55555556         (signed, trunc)
```

So `T[i] = i / 3`, and every use of the table is one of two idioms:

- `T[pos >> 6]` = `pos / 192` → **tile index (TCoord)**
- `T[pos >> 8]` = `pos / 768` → **4×4-tile block index (WCoord)**

**1 tile = 192 world units. 1 WCoord = 4 tiles = 768 world units. Positions are `int32`.**
[measured]

This is independently corroborated twice:

1. `ron-data/rules.xml:15-21` — *"Map distances and speeds are specified as multiples or
   fractions of a 'tile' (aka 'TCoord'). A farm is 4 TCoords wide, or 1 WCoord wide … Largest
   denominator allowed is 192"* and `<UNIT_MOVE_SPEED value="1/192 tile (granularity for
   unit movement speeds)"/>`. The 192-denominator limit in the parser exists because
   **1/192 tile is the world unit**; every legal rule distance lands on an exact integer.
   [measured]
2. `FUN_00688740` (the pathfinder's step-legality check) bounds-checks candidate positions
   against `192 * mapWidthTiles` and `192 * mapHeightTiles`
   (`lea ecx,[ebx+ebx*2]; shl ecx,6` at `0x00688770` and `0x00688781`, with
   `mapWidthTiles = [[0x00C061D0]+0x18]`). [measured]

This **closes open question #2 in `docs/binary-ground-truth.md`** for positions: parsed
rule distances are integers in 1/192-tile units, not `f32`, and not a separate fixed-point
scheme.

### Object position is stored XOR-obfuscated

`FUN_00662680` is the canonical `Object::setPosition(x, y)`: [measured]

```
this->[0x10] = x ^ 0x00063637          ; X, int32
this->[0x14] = y ^ 0x00063637          ; Y, int32
FUN_008544A0(&z, T[x>>6], T[y>>6], 1)  ; terrain elevation at that tile
this->[0x0C] = z ^ 0x00063637          ; Z, int32, DERIVED not integrated
```

The immediate `0x00063637` appears **2,548 times in `.text`** [measured]. It is a
compile-time constant, not a per-run cookie, so it is trivially reproducible — but our
`XData` mirror must know that raw position dwords are XOR-masked, or every offset we
recover will look like noise. Ground elevation is *derived from the heightmap on every
position write*, not simulated.

---

## 3. The algorithm

Grid A\*, 8-connected (or 4-connected, see mode below), on the tile grid, with a
red-black-tree open list and a second index keyed by cell id. [measured]

### Search node — 36 bytes (`malloc(0x24)` at `0x00685B83`)

| off | field |
|---|---|
| `+0x00` | x (world units) |
| `+0x04` | y (world units) |
| `+0x08` | **g** — cost so far |
| `+0x0C` | **h** — heuristic |
| `+0x10` | **f = g + h** — the tree key |
| `+0x14` | depth (parent depth + 1) |
| `+0x18` | cell index = `stride * T[y>>6] + T[x>>6]` |
| `+0x1C` | `int16` cached terrain elevation at this node |
| `+0x20` | parent pointer |

Nodes come from a free-list pool: array `[0x00C8DA70]`, capacity `[0x00C8DA74]`,
count `[0x00C8DA78]`, grow-hint byte `[0x00C8DA7C]`. Recycled LIFO. Deterministic.
[measured]

### Containers (`PathFinderData` first three members)

| field | address | role |
|---|---|---|
| `+0x40` | `0x00E85E80` | **open list** — red-black tree ordered by `f` (`node+0x10`) |
| `+0x44` | `0x00E85E84` | index keyed by cell id (`node+0x18`) |
| `+0x48` | `0x00E85E88` | closed/visited index keyed by cell id |

`FUN_00687970` pops the best open node: it walks `left` pointers from the root to the
leftmost node (`while ([n] != 0) n = [n]`), i.e. **min-`f`**, then erases from `+0x44` by
key and from `+0x40`. If the tree is empty it fires the
`"failed finding an open node somehow"` assert. [measured]
`FUN_00688360` is the tree consistency check that carries the `"crappy red-black tree"`
assert (it computes and compares subtree depths). [measured]

### Neighbours

Direction tables in `.rdata`, indexed 0..8: [measured]

```
dx[] @ 0x00ADCAF0 = { 0, -1,  0, +1, +1, +1,  0, -1, -1 }
dy[] @ 0x00ADC400 = { 0, -1, -1, -1,  0, +1, +1, +1,  0 }
                      -   NW  N   NE  E   SE  S   SW  W
```

So **odd indices are the four diagonals**. Child position is
`parent + dir*192` (`lea eax,[edx+edx*2]; shl eax,6` at `0x00685DDC`), i.e. exactly one
tile per step; the child cell index is maintained incrementally as
`parent.cell + stride*dy + dx`.

The 8 directions are always tried in increasing index order with wraparound, starting
from the cardinal that points at the goal (`0x00685CB5`–`0x00685D02`): [measured]

```
if |dx| > |dy|:  seed = (x > goalX) ? 7 : 3      ; loop starts at seed+1 = W or E
else:            seed = (y > goalY) ? 1 : 5      ; loop starts at seed+1 = N or S
```

**Tie-breaking is fully determined**: equal-`f` order is red-black-tree insertion order,
and insertion order is this fixed rotation. There is no hash map and no pointer-ordered
container anywhere in the search. [measured]

### Cost of one step — `FUN_00686300` (`__stdcall`, 5 args, `ret 0x14`)

Arguments: `(childNode, playerIndex, capabilityFlags, direction, unused)`. [measured]

```
cost  = Random::range(0, 0xFFFF) % 20            ; 0..19   <-- RNG, see §6
cost += PF_BASE                                  ; [0x00E85EE0]

tile  = (T[x>>6], T[y>>6]);  flags = terrainFlags16[stride*ty + tx]

if (flags & 0x30) == 0x20:                       ; water
    if transportModeActive [0x00E85ECC]:
        cost += PF_WATER_TRANSPORT               ; [0x00E85EE4]
    else if parent tile is not water:
        cost += PF_WATER_ENTRY                   ; [0x00E85EE8]
else:                                            ; land
    region = regionArray[ (ty>>2)*W + (tx>>2) ]  ; 28-byte records, [[..]+0x134]
    owner  = (int8)region[0x0F]
    if owner < 0:              cost += PF_NO_REGION      ; [0x00E85EF0]
    else if not passable(owner, playerIndex):
                               cost += PF_FOREIGN_TERR   ; [0x00E85EEC]
    if (flags & 0x6000) == 0x2000 and (flags & 0x30) != 0x10:
                               cost += PF_TERRAIN        ; [0x00E85EF4]
    if region[0] & 8:          cost += 2 * PF_ROUGH      ; [0x00E85EF8]
    if flags & 0x800:          cost +=     PF_ROUGH
    dz    = | z(child) - |z(parent)| |
    cost += 3 * min(dz, PF_MAX_SLOPE)            ; [0x00E85EFC]
    ... further (flags & 0x30)==0x10 shore adjustments using PF_TERRAIN/8, *4 ...

if FUN_006B53F0(tx>>1, ty>>1, ...) == 0:  cost *= 2
if direction & 1:                         cost = (7 * cost) / 5     ; diagonal, 7/5 ~ sqrt(2)
```

The diagonal multiplier is **7/5 = 1.4** exactly (`lea ecx,[eax*8]; sub ecx,eax` then
signed magic `0x66666667 >> 1` = ÷5), not `sqrt(2)`. [measured]

### The eight cost constants — recovered

`FUN_00689EC0` installs them with two `movaps` from `.rdata` (`0x00689F29` and
`0x00689FF2`): [measured]

| global | offset in `PathFinder` | value | source |
|---|---|---|---|
| `0x00E85EE0` | `+0xA0` | **55** | `.rdata 0x00B69A30 +0` |
| `0x00E85EE4` | `+0xA4` | **100** | `.rdata 0x00B69A30 +4` |
| `0x00E85EE8` | `+0xA8` | **100** | `.rdata 0x00B69A30 +8` |
| `0x00E85EEC` | `+0xAC` | **540** | `.rdata 0x00B69A30 +12` |
| `0x00E85EF0` | `+0xB0` | **240** | `.rdata 0x00B69A60 +0` |
| `0x00E85EF4` | `+0xB4` | **200** | `.rdata 0x00B69A60 +4` |
| `0x00E85EF8` | `+0xB8` | **60** | `.rdata 0x00B69A60 +8` |
| `0x00E85EFC` | `+0xBC` | **600** | `.rdata 0x00B69A60 +12` |
| `0x00E85F00` | `+0xC0` | **8** | immediate at `0x00689FF9` |

The `PathFinder` constructor zeroes all of these; `FUN_00689EC0` is what gives them
values. They are also exposed by name through `BaseParamRegister` under the block name
`"Pathfinding"`. **They do not appear in any shipped XML** — I grepped all 45 files in
`ron-data/`; `"Pathfinder"` there is only a unit name. So these `.rdata` values are the
shipped defaults and, absent a dev-console override, the live values. [measured]

Candidate parameter names sit in the same `PathFinder.cpp` UTF-16 string pool and are
almost certainly the registered names of this block, but I did not find their binding
sites and so **cannot map name → offset**: `value` (`0x00ADD1D4`), `estimate`
(`0x00ADD224`), `transport` (`0x00ADD238`), `building` (`0x00ADD264`), `z_val`
(`0x00ADD278`), `length` (`0x00ADD2C0`), `metric` (`0x00ADD2D0`), `timeout`
(`0x00ADD2E0`), ASCII `cost` (`0x00ADD368`). [measured that the strings exist and are
adjacent; **not** measured that they name these eight slots]

### The heuristic — and a retail bug worth replicating

Both heuristics call the same integer distance kernel `FUN_0046CFF0` (§4), then scale.
Raw bytes checked by hand, because the two shift amounts differ by one and that is exactly
the kind of thing a decompiler smooths over:

```
0x00685C3A: b8 ab aa aa 2a  c1 e1 02  f7 e9  ...  c1 fa 05     ; start node: (60*d) / 192
0x0068601C:                 c1 e1 02  f7 e9       c1 fa 06     ; child node: (60*d) / 384
```
[measured]

- **start node** (`0x00685C2E`): `h = 60 * pf_dist(|dx|,|dy|) / 192` = **60 per tile**.
- **child node, mode B** (`0x0068600B`): `h = 60 * pf_dist(|dx|,|dy|) / 384` = **30 per tile**.
  With a mean step cost of `55 + 9.5 = 64.5`, this is roughly a 2× underestimate —
  admissible, and deliberately weak.
- **child node, mode A** (`0x00685FD9`): the code is

  ```
  ecx = child.x - goalX
  edx = -goalY                       ; f7 da  =  neg edx, verified in raw bytes
  eax = pf_dist(ecx, edx)
  h   = (40 * eax) / 16              ; = 2.5 * eax
  ```

  `pf_dist` takes `abs` of both arguments, so this is `2.5 * pf_dist(dx, goalY)`.
  The second argument should obviously have been `child.y - goalY`. [measured that the
  instructions are these; **[inference]** that it is a bug]

  Because `goalY` is a per-search constant and normally far larger than `dx`,
  `pf_dist(dx, goalY) ≈ goalY + dx²/(2·goalY)`, so `h` is a large constant plus a term of
  order `dx²/goalY` — for a goal 100 tiles down the map, `2.5/(2·19200) ≈ 6.5e-5` per unit
  of `dx²`. A constant added to `h` for every node does not change A\* ordering. **In mode
  A the search is, to within a negligible term, plain Dijkstra.** That is a large finding
  for both fidelity and throughput, and it must be reproduced, not corrected.

### Two modes, selected by the sign of argument 6

`FUN_00685990` is `__stdcall` with 7 arguments (`ret 0x1C`). Argument 6 (`[ebp+0x1C]`) is
a search-slot id; argument 7 (`[ebp+0x20]`) selects a per-owner table at `0x00E3A2A0`
(stride 28). [measured]

| | arg6 < 0 (mode B) | arg6 ≥ 0 (mode A) |
|---|---|---|
| connectivity | **4** — odd (diagonal) directions skipped (`test cl,1; jne skip`, `0x00685D78`) | **8** |
| heuristic | `60·d/384` (30/tile) | the degenerate one above |
| resumable | no | **yes** |

### Time-slicing: searches are suspended and resumed across frames

The node-expansion budget is **`0xC80` = 3200 expansions** (`cmp [ebp-0x28], 0xC80`
at `0x00685D44` / `0x006860AF`). [measured] On exhausting it in mode A, the search does
not fail — it *parks itself* in the slot record and returns `-1`:

```
slot = [0x00E3A2A0 + 28*arg7][arg6]
slot[0x20] = 1          ; suspended
slot[0x28] = open tree      slot[0x2C] = cell index      slot[0x30] = closed index
slot[0x34] = direction      slot[0x38] = goalX           slot[0x3C] = goalY
slot[0x40] = expansions used
```

and on the next call with the same `(arg6, arg7)` it swaps those containers back in and
continues. Fresh tree objects are allocated from pools (`FUN_0047A370`, `FUN_0047A2C0`,
`FUN_0047A160`). [measured]

Return values: `0` = open list exhausted (no path), `-1` = suspended or over budget,
`1` = path found. [measured]

### Path extraction and waypoint output

On success (`0x00686203`) the parent chain is walked from the goal node and written into
the caller's array as **16-byte records** `{ int32 x, int32 y, 0x60, 0 }`. A node is
**skipped** if its tile has terrain flag `0x4000` set — i.e. the emitted path is filtered,
not one waypoint per expanded cell. The constant `0x60` = 96 = **half a tile**, plausibly
an arrival tolerance. [measured; the meaning of `0x60` is [inference]]

The output container is `{ ptr, capacity, count, growByte }` with growth via
`FUN_0046E8C0`. Note the same 16-byte record layout is what `FUN_00685990` *reads* for
its start/goal: it takes the **last** record as the start and the **second-to-last** as
the goal (`0x00685B27`–`0x00685B5D`), i.e. it extends a path being built backwards.
[measured]

---

## 4. `FUN_0046CFF0` — the engine's integer distance kernel — **Tier B**

`__fastcall(ecx, edx) -> eax`, 105 bytes, classified **ISLAND**, called from **105
distinct functions** across the image. It is not pathfinder-specific: it is the engine's
general point-to-point distance primitive (`FUN_0046D060` and `FUN_0046F020` are thin
wrappers that take coordinate-pair pointers). [measured]

```
hi = |a| ; lo = |b|                       ; wrapping abs (cdq/xor/sub)
if hi > lo (signed):
    if hi == 0: return 0
    if (u32)lo <  0xEA60: return  (u32)(lo*lo) / (u32)(2*hi)  + hi     ; unsigned div
    else:                 return ((u32)lo + (u32)(2*hi)) >> 1
else:
    if lo == 0: return 0
    if (u32)hi <  0xEA60: return  (u32)(hi*hi) / (u32)(2*lo)  + lo
    else:                 return ((u32)hi + (u32)(2*lo)) >> 1
```

This is `max + min²/(2·max)`, the two-term expansion of `sqrt(max² + min²)`, with an
overflow guard at `min ≥ 60000` (`60000² < 2³²`) that falls back to `max + min/2`. There
is **no `sqrt` and no float** anywhere in it. [measured]

**Tier B evidence: 4,000,026 inputs, 0 mismatches.** [measured]
Distribution: 26 hand-chosen edge cases (`0`, `±1`, the `0xEA60` guard from both sides,
`i32::MIN`/`i32::MAX` in every combination) + 2,000,000 uniform full-32-bit pairs +
1,000,000 pairs in `±65536` (realistic map deltas: a 340-tile map is 65280 world units)
+ 1,000,000 pairs straddling the `60000` guard within `±31` and with mixed signs.
Seeded xorshift64, fixed seed `0x2545F4914F6CDD1D`, so any failure replays exactly.

Captured retail vectors (**captured, not calculated** — printed by the harness from the
mapped retail code):

```
pf_dist(0,0)=0        pf_dist(1,1)=1        pf_dist(3,4)=5        pf_dist(5,12)=13
pf_dist(100,100)=150  pf_dist(192,192)=288  pf_dist(1000,1)=1000  pf_dist(-3,-4)=5
pf_dist(59999,59999)=89998   pf_dist(60000,60000)=90000   pf_dist(60000,1)=60000
pf_dist(2147483647,1)=2147483647            pf_dist(-2147483648,1)=1073741825
```

Note `(3,4) -> 5` and `(5,12) -> 13` are exact, but `(100,100) -> 150` overestimates the
true `141.42` by 6.1%, and `(1,1) -> 1` underestimates. The error is **not** bounded on
one side, so this is not an admissible heuristic in the textbook sense either.

### Reproducing the difftest

I deliberately did **not** edit `crates/oracle/src/main.rs` — other lanes share it. The
harness is standalone:

```sh
# local copy (this lane's scratchpad):
#   .../scratchpad/lane/{Cargo.toml,src/main.rs}
ssh hbox 'mkdir -p ~/lane-pathfinding/src'
scp lane/Cargo.toml   hbox:~/lane-pathfinding/Cargo.toml
scp lane/src/main.rs  hbox:~/lane-pathfinding/src/main.rs
ssh hbox 'ln -sfn ~/don-oracle/data ~/lane-pathfinding/data'
ssh hbox 'cd ~/lane-pathfinding && nice -n 15 taskset -c 0-3 \
          cargo build --target i686-unknown-linux-musl -q'
ssh hbox 'cd ~/lane-pathfinding && nice -n 15 taskset -c 0-3 \
          ./target/i686-unknown-linux-musl/debug/lane-pathfinding difftest 4000000'
#   -> PASS  0x0046cff0  pf_dist(dx,dy)  4000026 trials, 0 mismatches
```

It depends only on `don-pe` by path and calls `__fastcall` through inline asm
(`in("ecx") a, in("edx") b`). The model, verbatim, for whoever lands it in `don-sim`:

```rust
/// FUN_0046CFF0 — the engine's integer distance primitive. Tier B, 4,000,026 samples.
pub fn pf_dist(a: i32, b: i32) -> i32 {
    let hi = a.wrapping_abs();
    let lo = b.wrapping_abs();
    if hi > lo {                                  // signed compare
        if hi == 0 { return 0; }
        if (lo as u32) < 0xEA60 {                 // unsigned compare
            let num = (lo as u32).wrapping_mul(lo as u32);
            let den = (hi as u32).wrapping_mul(2);
            return ((num / den) as i32).wrapping_add(hi);
        }
        return (((lo as u32).wrapping_add((hi as u32).wrapping_mul(2))) >> 1) as i32;
    }
    if lo == 0 { return 0; }
    if (hi as u32) < 0xEA60 {
        let num = (hi as u32).wrapping_mul(hi as u32);
        let den = (lo as u32).wrapping_mul(2);
        return ((num / den) as i32).wrapping_add(lo);
    }
    (((hi as u32).wrapping_add((lo as u32).wrapping_mul(2))) >> 1) as i32
}
```

Traps that a careless port falls into, all of them exercised by the edge cases: the
comparison `hi > lo` is **signed** while the `0xEA60` guard is **unsigned**; the divide is
**unsigned** `div`, not `idiv`; the `>> 1` is **`shr`**, not `sar`, so it must be done in
`u32`; and `abs` is the wrapping kind, so `i32::MIN` stays `i32::MIN`.

---

## 5. Movement integration — partial, with an honest gap

**Established** [measured]:

- Object position is `int32` in 1/192-tile units, stored XOR `0x00063637` at
  `+0x0C` (Z), `+0x10` (X), `+0x14` (Y). `FUN_00662680` and `FUN_00662300` are the setters.
- Z is **not integrated**: it is re-derived from the terrain heightmap on every position
  write, and re-derived in bulk for every object by `FUN_005ABF90` after terrain changes.
- Terrain elevation `FUN_008544A0(out*, tileX, tileY, flag)` is the **only float in the
  pathfinding cone**: `z = (int)((h[i] + h[j]) * 0.5f)` over an `f32` heightmap at
  `[this+0x4A4]`, with `cvttss2si` truncation. If the tile is water
  (`(flags & 0x30) == 0x20`) it returns `(int)*(f32*)0x00CBE54C` instead. Bit-exact in
  Rust `f32` with an explicit `as i32` (which is also truncate-toward-zero).
- Speed comes from `UnitType+0x2C0` (`moves`, bound by `FUN_0061C490`, already in
  `schema/bindings.json`) and is copied to the object at `+0x314` by `FUN_0061DE20`.
  `ron-data/unitrules.xml` gives Citizen `<MOVES>25</MOVES>`; with `UNIT_MOVE_SPEED =
  1/192 tile` that is **25 world units per frame**, i.e. 25/192 tile/frame ≈ 1.95
  tiles/second at 15 fps. The unit is integral by construction.
- Formation and collision parameters are already bound in `schema/bindings.json` from
  `FUN_0065FC00`: `guy_spacing` (+548), `x_spacing` (+552), `y_spacing` (+556),
  `guy_radius` (+572), `block_radius` (+576), `big_radius` (+580), `new_block_radius`
  (+584); and from `FUN_0061C490`: `push_size` (+760), `push_circles` (+764),
  `target_size` (+768), `uber_size` (+776), `crew_size` (+780), `base_form` (+784),
  `turn_speed` (+708). `rules.xml` supplies the calibrations:
  `UNIT_FORMATION_SPACING = 1/16 tile`, `UNIT_GUY_SPACING = 1/16 tile`,
  `UNIT_BLOCK_RADIUS = 1 UCoord`, `UNIT_TURN_SPEED = 1/1 rate`,
  `UNIT_PACK_TURN_BONUS = 2x`. `unitrules.xml` documents `PUSH_SIZE`/`PUSH_CIRCLES` as
  *"their collision profile is described by a number of circles … spaced evenly along the
  forward-to-back axis, each circle tangential to the next"* — so pushing is
  circle-vs-circle, and the parameters for it are already recovered.

**Not established, and I will not guess it**: I did **not** locate the per-frame position
integrator — the code that reads `moves`, consumes the waypoint array, and writes the new
`x`/`y`. `MoveOrder`'s vtable (`0x00B4A12C`) turned out to be four adjusted-`this` thunks
into serialization/`DataWalk` machinery (`0x00487E70` builds a `"MOVEORDER"` descriptor
record), not an execute method, so the obvious entry point was a dead end. Everything I
found writing `+0x10`/`+0x14` was a *teleport* or a bulk Z-refresh, not an integrator.
Consequently:

- whether the integrator is exactly `pos += (delta * speed) / dist` in integers,
- how facing/turn rate gates lateral motion,
- the collision/push resolution order (which is a determinism-relevant iteration order),
- and formation slot assignment,

are **open**. What I can say is bounded and useful: the *representation* is integer, the
inputs are integer, and there is no float in the pathfinder that feeds it, so the
integrator is very unlikely to be float — but that is an inference and the next lane
should settle it by finding the writer, not by assuming.

---

## 6. Determinism risk — the headline result

### Non-IEEE CRT transcendentals: **none reachable**

I built a direct-call graph over all 46,564 functions in `schema/islands.jsonl` (E8/E9
rel32 scan, byte-aligned) plus every `call dword ptr [IAT]` site, and walked the transitive
closure. [measured]

- Cone from `FUN_00685990` (A\* driver): **187 functions**. Transcendental imports used:
  **0**. x87 instructions: **0**. SSE instructions: **5**, all in `FUN_008544A0`
  (`movss`/`addss`/`mulss`/`cvttss2si` ×2 — the height average described above).
- Wider cone from all six pathfinder entry points (`0x00685990`, `0x00688A40`,
  `0x00688FC0`, `0x006897D0`, `0x0068BC90`, `0x00687E40`, `0x00685950`): **859 functions**.
  Transcendental imports used: **0**.

IAT slots checked: `0x00AC5518` `acos`, `0x00AC551C` `asin`, `0x00AC5520` `atan`,
`0x00AC5524` `cos`, `0x00AC5528` `pow`, `0x00AC552C` `sin`, `0x00AC5530` `sqrt`,
`0x00AC5534` `tan`.

**Caveat, stated plainly:** this closure follows *direct* calls only. The 187-function
cone contains 247 indirect call sites, nearly all of them CRT allocator/string/EH calls
reached via the assert and logging paths. A virtual dispatch from inside pathfinding into
transcendental-using code is not excluded by this analysis. The result is strong evidence,
not a proof.

### RNG: **yes, and it is the whole problem**

`FUN_00686300` calls `FUN_00A39D70` at `0x00686341` with `(0, 0xFFFF)` and `ecx` = the
global at `[0x00C06184]`, then takes `% 20`. `FUN_00A39D70` is confirmed as the engine
RNG by its own assert filename `L"random.cpp"` at `0x00B1A840`, referenced at
`0x00A39DF0`. [measured] Across the 859-function pathfinder cone there are **20 RNG call
sites**; the one inside the cost function is on the hot path — **once per edge
relaxation**, up to 8 times per expanded node, up to 3200 nodes per slice.

Consequences we have to design around, not discover later:

1. Reproducing the RNG algorithm exactly is a **prerequisite for pathfinding**, and
   reproducing pathfinding edge-for-edge is a prerequisite for every later RNG consumer.
   The coupling runs one way and it is total.
2. Path cost is not a function of the map alone. Two identical queries return different
   paths depending on RNG position. Any cache we build must be keyed accordingly, and any
   "optimisation" that skips an edge evaluation is a desync.
3. For batch RL this is the dominant throughput constraint on the movement system: the
   RNG draw serialises the inner loop of the single most expensive subsystem. Worth
   measuring early whether the draw can be vectorised across worlds (it can, if the RNG is
   counter-based or trivially strideable — which the RNG lane should report).

### Iteration-order-dependent containers: **none found**

The open list is a red-black tree keyed on `f`; the two indexes are keyed on cell id
(a deterministic function of position); the node pool is a LIFO array; the neighbour loop
is a fixed rotation. No hash map, no pointer-keyed set, no pointer comparison anywhere in
the 187-function cone. [measured] **Pathfinding does not introduce container-order
nondeterminism.**

### Verdict

Whole-sim bit-exactness through the movement subsystem is achievable. The blocker is
the RNG (a solved problem once its algorithm is recovered) and the sheer exactness demand
on the search — including reproducing the mode-A heuristic bug. It is *not* floating
point.

---

## 7. Function inventory

| VA | size | class | role |
|---|---|---|---|
| `0x00685990` | 2411 | SELF_CALL | **A\* driver.** `__stdcall`, 7 args (`ret 0x1C`) |
| `0x00686300` | 908 | SELF_CALL | **per-edge cost.** 5 args (`ret 0x14`). Calls the RNG |
| `0x00688740` | 689 | SELF_CALL | step legality / passability; bounds-checks vs `192*W`, `192*H` |
| `0x00687970` | 289 | SELF_CALL | pop min-`f` open node |
| `0x006882B0` | 95 | SELF_CALL | find node by cell index |
| `0x00688360` | 961 | SELF_CALL | red-black tree consistency assert |
| `0x00687E40` | 776 | SELF_CALL | `PathFinder` reset (vtable slot 2) |
| `0x00689EC0` | 356 | SELF_CALL | `PathFinder` init — installs the 8 cost constants |
| `0x00688150` | 276 | **ISLAND** | zero `PathFinderData` (`+0x40..+0xC0`) |
| `0x0068A030` | 121 | SELF_CALL | ctor of the global `PathFinder` at `0x00E85E40` |
| `0x00688A40` | 976 | SELF_CALL | path-request entry (unit/player), calls the driver |
| `0x00688FC0` | 2055 | SELF_CALL | further pathfinder entry (not analysed) |
| `0x006897D0` | 1169 | SELF_CALL | further pathfinder entry (not analysed) |
| `0x0046CFF0` | 105 | **ISLAND** | **integer distance kernel — Tier B, 4,000,026 samples** |
| `0x0046D060` / `0x0046F020` | 41 / 37 | SELF_CALL | distance wrappers over coordinate pairs |
| `0x00681DB0` | 125 | SELF_CALL | builds the `T[i]=i/3` coordinate table at `[0x00CAE5FC]` |
| `0x008544A0` | 196 | WRITES_GLOBAL | terrain elevation sampler (the only float) |
| `0x006B53F0` | 241 | SELF_CALL | 2×2-tile grid query; cost doubles if it returns 0 |
| `0x006EDB50` | 55 | WRITES_GLOBAL | territory passability for a player |
| `0x00A39D70` | 356 | SELF_CALL | `Random::range(lo,hi)` (`random.cpp`) |
| `0x004796F0` / `0x00479770` | 126 / 422 | SELF_CALL | open-tree insert / erase-min |
| `0x00479920` / `0x00479B90` / `0x00479C00` | 610 / 111 / 610 | SELF_CALL, **ISLAND**, SELF_CALL | cell-index map insert / find-erase / insert |
| `0x0047A160` / `0x0047A2C0` / `0x0047A370` | 166 each | SELF_CALL | tree allocators (pooled) |
| `0x0047A820` | 125 | SELF_CALL | node-pool grow |
| `0x00662680` / `0x00662300` | 94 / 147 | SELF_CALL | `Object::setPosition` (XOR `0x63637`) |

Key globals: `0x00E85E40` PathFinder; `0x00E85E80/84/88` its three containers;
`0x00E85EE0..0x00E85F00` cost constants; `0x00C8DA70..7C` node pool;
`0x00E3A2A0` suspended-search slots (stride 28); `0x00CAE5FC` coordinate table;
`0x00C06188` / `0x00C061D0` map/terrain (`+0x18` stride, `+0x134` region array,
`+0x138` `u16` terrain-flag grid); `0x00C06184` RNG; `0x00C06218` terrain object used by
`FUN_008544A0`.

Terrain flag bits observed in use by pathfinding: `0x30` mask (`0x20` = water, `0x10` =
shore/other), `0x6000` mask (`0x2000` = a surcharged class), `0x800`, `0x4000` (waypoint
suppression). [measured that these are the tested bits; **the semantic labels are
[inference]**]

---

## 8. What I could not establish

1. **The per-frame movement integrator.** See §5. This is the biggest gap in the lane.
2. **Name → offset mapping for the eight cost constants.** The names exist in the
   `PathFinder.cpp` string pool; the `BaseParamRegister` binding sites do not appear in
   `schema/bindings.json` and I did not find them by xref (the strings have no 4-byte
   `.text` reference, suggesting `/GF` pooling merged them with canonical copies
   elsewhere). Until that is closed, the constants are known by *value and use site*, not
   by name.
3. **Meanings of the terrain flag bits.** I know exactly which bits are tested and what
   each branch costs; I do not know what the bits are called.
4. **`FUN_006B53F0`** (the query whose falsehood doubles the cost) and **`FUN_006EDB50`**
   (territory passability) are unread.
5. **`FUN_00688FC0` (2055 B) and `FUN_006897D0` (1169 B)** — two more pathfinder entry
   points, likely path smoothing / local avoidance. Unread.
6. **Whether the eight cost constants can be overridden at runtime.** They come from
   `BaseParamRegister`, which implies a settable path; nothing in `ron-data/` sets them.
7. **Indirect-call reachability** for the transcendental sweep (§6), stated as a caveat
   there rather than papered over.
8. **The `-goalY` heuristic** is [measured] as an instruction sequence and [inference] as
   a bug. Confirming it behaviourally needs the pathfinder driven under the oracle with
   live globals, which is a fallback-path (minidump) job, not fabricated inputs.

## 9. Proposed provenance-ledger entries

```
### pf_dist(a, b) — FUN_0046CFF0, the engine integer distance primitive
source     riseofnations.exe VA 0x0046CFF0 (__fastcall ecx,edx; 105 bytes; ISLAND)
tier       B
evidence   4,000,026 inputs — 26 hand-chosen edge cases (0, +/-1, the 0xEA60 guard from
           both sides, i32::MIN/i32::MAX in all combinations) + 2,000,000 uniform 32-bit
           + 1,000,000 in +/-65536 + 1,000,000 straddling 60000 — 0 mismatches
harness    ~/lane-pathfinding on hbox, i686-unknown-linux-musl, seed 0x2545F4914F6CDD1D
note       used by 105 call sites engine-wide, not pathfinding-specific

### World coordinate scale — 1 tile = 192 units, positions int32
source     FUN_00681DB0 (T[i] = i/3 at [0x00CAE5FC]); bounds check at 0x00688770;
           ron-data/rules.xml:15-21 and UNIT_MOVE_SPEED = "1/192 tile"
tier       structural (two independent derivations agree; no differential test applies)

### Object position storage — int32 XOR 0x00063637 at +0x0C/+0x10/+0x14
source     FUN_00662680, FUN_00662300; immediate appears 2,548x in .text
tier       structural

### Pathfinding cost constants (8) — 55, 100, 100, 540, 240, 200, 60, 600 (+8)
source     .rdata 0x00B69A30 and 0x00B69A60, installed by FUN_00689EC0 at 0x00689F29
           and 0x00689FF2 into PathFinder+0xA0..+0xBC (globals 0x00E85EE0..0x00E85EFC)
tier       structural (values read from the image; not yet exercised under the oracle)
```
