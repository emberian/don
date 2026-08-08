# Static Rules checksum channel

Status: the traversal is implemented and independently live-validated.  The repository
still lacks builders for two input families (`Types::list`'s pointed-to arrays and the 24
Tribe records), so the new module deliberately does not claim that checked-in data alone
can produce the retail channel yet.

Implementation: `crates/don-replay/src/rules_channel.rs`.

## Exact retail traversal

`Game::walk_rules_data` at `0x00589550` performs these checksum-visible operations in
order:

1. `walk_test` (no bytes for `CheckSum`);
2. `Types::walk_rules_data` at `0x00669800`: 806 pointers from `types.list`, each virtual
   call through vtable slot `+0xc4`;
3. `Constants + 0x000 .. + 0xd40` (3,392 bytes);
4. `Constants + 0x804 .. + 0x808` (the dword is intentionally hashed a second time);
5. `Balance::walk_rules_data` at `0x00582cc0`: 493 x 493 individual two-byte calls over
   `final_balance_table` at preferred VA `0x00c12bf4`;
6. 24 contiguous Tribe records, stride `0x5f0`: a no-op `walk_test`, then
   `[Tribe+0x54, Tribe+0x6c)` and `[Tribe+0x70, Tribe+0x5f0)`.

The virtual type mix in the shipped data is exactly:

| effective walker | records | concrete registry |
|---|---:|---|
| `UnitType` | 364 | `UnitType` |
| `BuildType` | 129 | `BuildType` |
| `TechType` | 85 | `TechType` |
| `ObjectType` | 1 | `ItemType` inherits it |
| `SpellType` | 55 | `SpellType` |
| `Type` | 122 | `BonusType` inherits it |
| `GoodType` | 50 | `GoodType` |

The implementations' direct ranges come from disassembly and
`schema/state-schema.json`.  `String::walk_data` calls are skipped because
`DataWalk::is_checksum` is non-zero.  Each ObjectType also walks two
`SimpleArray<unsigned short>` members.  On the checksum path that array emits count; if
non-empty, capacity, the two-byte growth hint, `flags & 0xbf`, and `count` little-endian
elements.  This is why a flat type-record dump alone is insufficient: it contains the
array headers and backing pointers, not the pointed-to elements.

## Live validation

On 2026-08-08 the standalone read-only probe walked the user's running solo match:

- PID `5236`, module base `0x00d60000`, preferred-base delta `+0x00960000`;
- `types.list` pointer field runtime `0x017e5ddc` -> `0x1a81b320`;
- Constants pointer field runtime `0x015661f0` -> `0x01798b88`;
- balance runtime base `0x01572bf4`;
- Tribe pointer field runtime `0x017dfa34` -> `0x0c6cb9b4`.

The probe requested only `PROCESS_QUERY_INFORMATION | PROCESS_VM_READ`.  It read the
three root pointers again after the walk; all were stable.  It performed 11,551 reads
covering 997,846 checksum bytes.  No unrelated heap bytes or raw process-memory dump was
persisted, and the temporary guest executable and HTTP transfer server were removed.

| cumulative point | adler-32 | bytes walked |
|---|---:|---:|
| after Types | `0x72e0c3b6` | 473,984 |
| after Constants | `0x50625668` | 477,380 |
| after Balance | `0x56daabc1` | 963,478 |
| after 24 Tribes | **`0x12ba3104`** | **997,846** |

The final word exactly matches every checksummed replay in the corpus.  This validates
the traversal code and resolves the previously unknown type dispatch and array semantics;
it does not turn the one live specimen into a checked-in shipped-data builder.

## Checked-in inputs and remaining integration

Already usable:

- `repository_constants()` decodes `schema/live/rules-block-pid14644.txt`, validates its
  length, and returns the exact 3,392-byte Constants block;
- `repository_balance()` uses the corrected
  `schema/live/final-balance-runtime.bin` (486,098 bytes);
- the checksum walker, dynamic-type validation, ObjectType arrays, duplicate Constants
  dword, Tribe slices, live checkpoints, completeness checks, and mutation tests are all
  in the standalone module.

Still required before `don-replay` can compute the target without a running game:

1. build all 806 most-derived type images in global type-id order from the shipped XML
   and canonicalisation/grafting rules, including the two ObjectType u16 array payloads;
2. build the 24 Tribe images, especially the 352-entry graft arrays;
3. independently compare those builders to a narrow live capture (values, not merely the
   final checksum);
4. feed a completed `StaticRules` into the replay simulation and set channel 13 from
   `RulesCheckpoints::after_tribes`;
5. keep `matches_retail()` as the shipped-data admission check.  A modded ruleset should
   use a future non-shipped shape mode rather than weakening this check.

Until (1)-(3) land, a caller gets a structural error for missing or substituted data;
there is no zero-filled fallback.
