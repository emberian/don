# Retail save boundary: Groups tail

Status: **complete tagged tail after Array<Group>**. This lane begins at the
caller tag immediately following the landed `Array<Group>`, consumes the
eight `last_group` integers and `proc_group`, and stops before
`Objects::walk_data`. It is an exclusive parser/test/doc tranche developed in
an isolated worktree and does not touch shared Rust or the shared checkout.

## Fresh-SVX splice

The fresh SVX compressed and decompressed SHA-256 values are
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
The exact Groups-tail stage is:

| range | owner | fresh value |
|---|---|---:|
| `0x27e6e..0x27e6f` | Groups tag, StringTable[2920] | 0 |
| `0x27e6f..0x27e8f` | `last_group[8]` | all 0 |
| `0x27e8f..0x27e93` | `proc_group` | 0 |
| next at `0x27e93` | `Objects::walk_data` | excluded |

The exact 37-byte section SHA-256 is
`ab24a95f44ceca5d2aed4b6d056adddd8539f44c6cd6ca506534e830c82ea8a8`.
The installed-artifact test chains every landed helper through the
PathFinder+Array<Group> tranche to reach `0x27e6e`, then proves that mutating
`0x27e93` cannot affect this owner. SVX seed `0x014810ac` and RCX seed
`0x007f93e0` remain distinct; RCX SHA-256 is
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`.

## Exact grammar and PDB mapping

Caller `0x005a2c38` emits `walk_test` at StringTable byte offset `0xe420`,
exact index 2920, then performs two direct walks:

```text
u8  Groups tag                       # StringTable[2920]
i32 last_group[8]
i32 proc_group
```

The PDB public `Groups groups` is at `0x00e85f10`. The first direct PE range,
`0x00e85f2c..0x00e85f4c`, is therefore Groups +28..+60 and exactly matches
PDB `last_group[8]`. The second, `0x00e85f50..0x00e85f54`, is Groups
+64..+68 and exactly matches `proc_group`.

PDB `GroupsData` is 68 bytes and lays out:

| offset | field | tail behavior |
|---:|---|---|
| +0 | `Array<Group> list` (28 bytes) | walked by the preceding tranche |
| +28 | `last_group[8]` | walked here |
| +60 | `const_last_group` pointer | excluded |
| +64 | `proc_group` | walked here |

PDB `GroupsOut` is 72 bytes and PDB `Groups` is 76 bytes due to virtual-base
state. Neither virtual-base state nor object padding/tail is serialized here.
All nine walked values are signed 32-bit integers and are preserved without
range normalization.

Independent `Groups::walk_data` `0x00713e30` proves the full class order:
first `Array<Group>`, then this tag, the +28..+60 direct range, and finally
the +64..+68 direct range. This is why the runtime pointer at +60 is skipped
rather than treated as a serialized scalar.

## Next owner and frozen evidence

After profiling, caller `0x005a2c77` invokes `Objects::walk_data`
`0x006541e0`. Its first byte defines the exact boundary `0x27e93`; there is no
alignment or delimiter inferred from the all-zero fresh specimen.

The matching PDB and schema-export SHA-256 values are
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`
and `399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The three-class layout receipt is
`e3837df2200337b43c2d74847af1493eb9448c3ac5e3c2d8ce1acf51ceba2196`.
The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Tests freeze Groups, the main caller handoff, and the following Objects body.

The synthetic fixture uses alternating signed `last_group` values and a
negative `proc_group`. Every owned-byte mutation fails or changes the
receipt, every truncation is rejected, the exact tag gate is tested alongside
an explicit permissive receipt mode, and the next Objects byte is excluded.

## Reproduction

```sh
python3 re/scripts/test_savegame_groups_tail.py

python3 re/scripts/savegame_groups_tail.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x27e6e
```

The returned `end` is the exact start of `Objects::walk_data`.
