# `schema/types.json` drops 208 definitions to name collisions — the decision

**Lane:** vtables (megaswarm wave 4). **Date:** 2026-08-11.
**Ground truth:** `ron-bin/sbl/rise.pdb` through `tools/pdb-extract`; every number below is
now emitted by the tool itself into `schema/types.json`'s `_meta`, so it is checkable
without rerunning anything. Reported by lane `decomp-backfill`; independently re-derived
here, and one of its numbers is extended.

## The mechanism

`tools/pdb-extract` walks the TPI stream and builds two indices:

- **`defs`** — bare tag name → type index, `entry(name).or_insert(idx)`, i.e. **first record
  wins**.
- **`udefs`** — COMDAT *unique* name (`.?AUtagRECT@@`) → type index. Unique names do not
  collide.

`types.json`'s `classes` / `enums` objects are keyed by bare tag name and are built from
`defs`, one entry per name. So when the PDB defines a tag more than once — the normal state
of affairs for an SDK header pulled into many translation units — the extra definitions are
**not emitted**, silently.

## The measurement

[measured, `schema/types.json` `_meta.counts`]

| | |
|---|---|
| class/struct/union definitions (non-forward-ref) | **20,095** |
| distinct class/union names emitted | 19,914 |
| enum definitions (non-forward-ref) | **2,884** |
| distinct enum names emitted | 2,857 |
| **definitions dropped to name collision** | **208** |
| colliding names | **194** (170 class/union, 24 enum) |
| colliding names whose duplicates genuinely disagree | **13** |

All 194 are Win32 / COM / CRT / zlib headers: `tagRECT`, `tagPOINT`, `_GUID`, `IUnknown`,
`HWND__`, `_LARGE_INTEGER`, `_FILETIME`, the `IEnum*`/`IOle*` COM interfaces, plus
`<unnamed-tag>` ×7 and `__unnamed` ×7. **No engine class is affected.** The only colliding
name that even has a vftable in the image is `_com_error`, which is CRT.

### The 13 whose duplicates disagree

The relayed finding said "seven have genuinely divergent layouts". That is right for
class/union records and it is the number that matters for a layout read, but the same test
applied to enums finds six more. Shapes are byte size for a class/union, enumerator count
for an enum:

| name | kind | defs | shapes |
|---|---|---|---|
| `internal_state` | class/union | 2 | 4, **5816** |
| `_IMAGE_LOAD_CONFIG_DIRECTORY32` | class/union | 2 | 92, 164 |
| `_NOTIFYICONDATAW` | class/union | 2 | 952, 956 |
| `_PROPSHEETPAGEW` | class/union | 2 | 52, 56 |
| `static_tree_desc_s` | class/union | 2 | 4, 20 |
| `<unnamed-tag>` | class/union | 7 | 2, 4, 8 |
| `__unnamed` | class/union | 7 | 2, 8, 16, 24, 80 |
| `tagBINDSTATUS` | enum | 3 | 47, 73, 78 |
| `tagBINDSTRING` | enum | 3 | 17, 23, 26 |
| `ReplacesCorHdrNumericDefines` | enum | 3 | 21, 24, 25 |
| `_tagQUERYOPTION` | enum | 2 | 14, 16 |
| `tagURLZONE` | enum | 2 | 9, 10 |
| `__MIDL_ICodeInstall_0001` | enum | 2 | 9, 10 |

These are the same header compiled against different SDK versions in different static
libraries. `internal_state` (zlib's opaque `z_stream` state: a 4-byte forward declaration in
one TU, the real 5,816-byte struct in another) is the one where reading the emitted record
would be actively misleading.

## What it does **not** break

**Field-type resolution does not go through the colliding map.** `TypeCtx::resolve`
(`tools/pdb-extract/src/main.rs`) resolves a forward reference through `udefs` — the unique
name — and only falls back to `defs` when the record has no unique name. So a field declared
`tagRECT` gets its size and name from *its own* translation unit's record, not from whichever
one won the bare-name race. That matters, because **914 fields across `types.json` do declare
a colliding name as their type** (`_GUID` ×159, `HWND__` ×103, `_LARGE_INTEGER` ×79,
`tagRECT` ×43, …). The narrower relayed statement — that no field has an
`<unnamed-tag>`/`__unnamed` type — is true but is not the reason nothing is corrupted; the
unique-name preference is.

What a dropped record *can* do is give the wrong answer to a direct catalogue lookup:
`types.json["classes"]["internal_state"]` returns one of the two arbitrarily.

## Decision: **declare it, do not restructure**

Fixing it properly means keying `classes`/`enums` by something collision-free — the unique
name, or a list of records per name. Either changes the shape of a 13 MB artifact that
`crates/don-net`, `crates/don-sim`, `crates/don-crossplay/gen/*.py` and a dozen docs read as
`classes[<bare name>]`, in exchange for correctly emitting 208 Win32/CRT header duplicates
that this project does not read. That is a worse trade than the defect.

What was actually wrong was that the loss was **silent**. It now is not:

- `_meta.counts` carries `class_union_definitions`, `enum_definitions`, `definition_names`,
  `definitions_dropped_to_name_collision`, `colliding_names` and
  `colliding_names_with_divergent_shapes`.
- `_meta.collisions` lists **every** affected name with its definition count and its distinct
  shapes, so the 13 divergent ones are discoverable from the artifact alone.
- `_meta.collision_note` states the first-record-wins rule and the unique-name resolution
  preference in the file itself.

`schema/types.json` was regenerated to carry this. **The `classes` and `enums` bodies are
byte-identical to the previous file** (verified by structural comparison after popping
`_meta`); the entire diff is the new `_meta` keys. `schema/symbols.json` regenerates
identically and was left untouched.

Reproduce:

```sh
cargo build --release --offline --manifest-path tools/pdb-extract/Cargo.toml
tools/pdb-extract/target/release/pdb-extract \
    /Users/ember/dev/don/ron-bin/sbl/rise.pdb 0x00400000 \
    schema/symbols.json schema/types.json schema/vtables.json
```

## The one place this still bites

Reading a **zlib or Windows shell** struct out of `types.json` by bare name. Check
`_meta.collisions` first; if the name is there with more than one shape, take the layout
from the PDB by unique name instead of from the catalogue.
