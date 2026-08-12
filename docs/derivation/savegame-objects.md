# Retail save boundary: Objects

Status: **complete `Objects::walk_data` owner**. This isolated tranche begins
at the Objects tag after the landed Groups tail, covers every direct range and
the complete dynamic grammars delegated to the nine object arrays, Ammo array,
optional Spline children, and contiguous DeathObj array, and stops before the
caller-owned HotKeyGroup tag. It does not edit a shared parser or any Rust.

## Fresh-SVX splice and next owner

The fresh SVX compressed and decompressed SHA-256 values are
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
The landed Groups tail ends at `0x27e93`; the exact Objects stage is:

| range | Objects owner | fresh value |
|---|---|---:|
| `0x27e93..0x27e94` | tag, StringTable[5066] | 0 |
| `0x27e94..0x27e9c` | `valid`, `ammo_index` | 0, 0 |
| `0x27e9c..0x27ea4` | `good_mark`, `rare_mark` | 0, 0 |
| `0x27ea4..0x27ec8` | `unit_mark[0..9)` | all 0 |
| `0x27ec8..0x27eec` | `build_mark[0..9)` | all 0 |
| `0x27eec..0x27f10` | `wall_mark[0..9)` | all 0 |
| `0x27f10..0x27f22` | `obj_ctr[0..9)` | all 0 |
| `0x27f22..0x27f46` | nine `MultiPtrArray<Object>` lengths | all 0 |
| `0x27f46..0x27f4a` | `PtrArray<Ammo>` length | 0 |
| `0x27f4a..0x27f4e` | `ObjectArray<DeathObj>` length | 0 |
| next at `0x27f4e` | caller tag, StringTable[3963] | excluded |

The exact 187-byte section SHA-256 is
`9708fd0c3a64591c024c02167f3b7cad1dcda62d1d41b6d0a27b96989b63864f`.
The installed-artifact test chains every landed helper from Leaders at
`0x9a5a` through Groups and reaches Objects structurally; it performs no zero
search. Mutating `0x27f4e` cannot affect the Objects receipt.

The main caller returns from Objects at `0x005a2c83`, performs load-only object
index reconstruction through `0x005a2d7e`, and then emits a tag using
StringTable byte offset `0x1359c`, exact index 3963. It invokes
`Array<HotKeyGroup>::walk_data` `0x00480290` at `0x005a2d9f`. Therefore
`0x27f4e` is an executable-owned handoff, not a delimiter inferred from the
all-zero fresh specimen.

## Direct Objects image

`Objects::walk_data` is `0x006541e0` (469 bytes). Its ordered direct walks are:

```text
u8  Objects tag                      # StringTable[5066]
i32 valid
i32 ammo_index
i32 good_mark
i32 rare_mark
i32 unit_mark[9]
i32 build_mark[9]
i32 wall_mark[9]
u16 obj_ctr[9]
```

PDB `ObjectsData` is 544 bytes. These executable ranges map exactly to:

| PE object range | PDB fields |
|---|---|
| +500..+508 | `valid`, `ammo_index` |
| +340..+348 | `good_mark`, `rare_mark` |
| +348..+384 | first nine `unit_mark` entries |
| +388..+424 | first nine `build_mark` entries |
| +428..+464 | first nine `wall_mark` entries |
| +468..+486 | first nine `obj_ctr` entries |

The class owns ten 28-byte `ObjectsArray` objects at +4, but the executable
loops exactly nine times (`edi = 9`, stride 28). Likewise, it omits the tenth
element of each marker plane. The parser preserves this exact player-band
choice rather than serializing the full PDB allocation. Runtime pointers
`obj_mark`, `const_ammo_objs`, `find_list`, and output-only state are excluded.

## Nine `MultiPtrArray<Object>` histories

Each owner calls `MultiPtrArray<Object>::walk_data` `0x0045d550` in order. Its
save grammar is:

```text
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags                  # save path clears bit 0x40 before walking
    u8  present[length]
    i32 concrete_type[sum(present)]
    i32 repeated_capacity
    i16 repeated_increment
    virtual Object::walk_data body for each present slot
```

The first length is the current logical length. Capacity and increment are
walked once as array history and again by the generic tail; both images must
agree. On save, each present pointer is converted by virtual slot +4 to its
factory type code, then each child is walked through virtual slot +0x7c.
On load, virtual slot +8 reconstructs the concrete type before the body pass.

Concrete Object bodies are polymorphic owners, not a fixed Objects POD range.
The helper accepts decoders keyed by the exact serialized type code. A present
type without a decoder fails at its structural body boundary. This keeps the
generic Objects parser independent of the already-landed Unit/Build/Good
census work and prevents cross-match inference. The non-empty synthetic test
uses two exact length-prefixed child decoders solely to prove ordering,
history, slot/type association, body chaining, mutation, and truncation.

## `PtrArray<Ammo>` and `AmmoData`

After all nine object arrays, Objects calls `PtrArray<Ammo>::walk_data`
`0x00473fe0`. Its header/presence/repeated-history grammar is identical except
there is no type plane: every present slot constructs an `Ammo`, then invokes
its first virtual walker.

`AmmoData::walk_data` `0x0067ab50` has exact grammar:

```text
u8  Ammo tag                       # StringTable[126]
u8  flags                          # PDB AmmoData +4
if flags & 3:
    bytes AmmoData[+5..+104)       # 99 bytes
u8  ammo_path_present
if ammo_path_present:
    SplineData::walk_data
```

The +104 `Spline*` itself is never serialized. The independent presence byte
is followed by `SplineData::walk_data` `0x009132b0` when set.

Spline emits StringTable[6209], directly walks +64..+100 (36 bytes), then
walks six nested arrays in executable order:

```text
u8  Spline tag
bytes SplineData[+64..+100)
Array<Vector<float>> control_verts     # element = 12 bytes
SimpleArray<float>   knots             # element = 4 bytes
SimpleArray<float>   spline_knots      # PE order: +184 before +156
SimpleArray<float>   weights
Array<Vector<float>> spline_verts
Array<Vector<float>> spline_normals
```

Each POD array is `i32 length`; when nonzero it adds `i32 capacity`, `i16
increment`, the writer-cleared `u8 flags`, and `length * element_size` bytes.
The executable order of `spline_knots` before `weights` is retained even
though PDB address order puts weights first.

## `ObjectArray<DeathObj>`

The final child is `ObjectArray<DeathObj>::walk_data` `0x00474420`:

```text
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags
    repeat length times:
        u8  DeathObj tag              # StringTable[2610]
        i32 valid
        if valid != 0:
            bytes DeathObjData[+4..+75)  # 71 bytes
```

This is a contiguous object array: there is no presence plane and no repeated
history tail. PDB `DeathObjData` is 76 bytes; the PE walks +0..+4 first and
only walks +4..+75 when valid. Byte +75 is padding and `DeathObjOut` fields at
+80 and above are not part of this save image.

## Frozen evidence and gates

The matching executable, PDB, and schema SHA-256 values are
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`,
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`,
and `399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The twelve-class PDB layout receipt is
`ee1e7b37234ffadfc5426c90de61c1dacbd285fabf3312d501f8e9bf2146f886`.
Tests freeze executable spans for Objects, all three child containers,
AmmoData, SplineData, both Spline array walkers, the main caller handoffs, and
the next HotKeyGroup walker.

The synthetic fixture covers nonempty object histories, holes and type planes,
empty and nonempty Ammo records, an optional Spline with all nested array
forms, and inactive/active DeathObj rows. Every owned byte mutation is rejected
or changes the exact section receipt; every truncation is rejected. Tag gates,
writer-cleared flag bit, repeated-history equality, capacity/length bounds,
missing virtual child decoders, PDB mutation, and next-owner exclusion each
have independent tests.

SVX seed `0x014810ac` and RCX seed `0x007f93e0` remain distinct. RCX SHA-256 is
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`;
the replay is checked independently and never joined to the save stream.

## Reproduction

```sh
python3 re/scripts/test_savegame_objects.py

python3 re/scripts/savegame_objects.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x27e93
```

The returned `end` is the exact caller-owned HotKeyGroup tag.
