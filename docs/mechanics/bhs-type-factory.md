# BHS authoritative type-owner factory

Status: source-complete producer and fail-closed proof pack; the raw rules composer, canonical
session ownership, channel 13, and DoNSave persistence remain red. Implementation:
`crates/don-sim/src/systems/bhs_type_factory.rs`. Frozen tests:
`crates/don-sim/tests/bhs_type_factory.rs`.

## Result

`produce_type_builtin_state` is the sole setup projection for the landed BHS type owner. It
accepts three fully materialized inputs from one upstream rules composition:

1. exactly 806 ordered type rows;
2. exactly 24 ordered tribe lookup names;
3. exactly eight current Leader mask projections.

Each component carries a typed role, one common composition identity, the SHA-256 of the ordered
base-file/mod-overlay manifest, and its own post-composition SHA-256. The factory rejects a wrong
role, a zero identity, a zero digest, a mixed composition, or a mixed manifest before inspecting
row data. The successful output keeps the mutable `TypeBuiltinState` and immutable
`TypeBuiltinProvenance` together. `into_parts` yields both explicitly; there is no convenience API
that silently discards provenance.

This boundary deliberately consumes **composed values**, not arbitrary XML fragments. Retail
loads base rules, localization, tribe records, and mod overlays through multiple loaders before
scripts run. Synthesizing absent rows or guessing the result of a partial overlay here would make
the BHS owner disagree with production, research, and checksum consumers. The upstream composer
must hash the complete ordered input manifest and the three final components.

## Ground truth

The checked-in retail generation used for this contract is anchored by:

| artifact | SHA-256 |
|---|---|
| `ron-bin/riseofnations.exe` | `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079` |
| `ron-bin/sbl/rise.pdb` | `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5` |
| `schema/types.json` | `e020499517a9f6d9b5e0f349b623ec18032c04e6a68aa7b3902767b8e39ee849` |
| `ron-data/rules.xml` | `2cad6156f257c2faf79c3fa2de293a249f61ae245160b92fb5a76d0dbf3a9988` |
| `ron-data/unitrules.xml` | `09e0b35c20d149083fafac12c5d2182951d1f1878bfc372ad99d6004e2aceabb` |
| `ron-data/buildingrules.xml` | `b67ca00a8218c5fad2b125e7073e2aa87084b5cbb775028e003cadf9ec269dc1` |
| `ron-data/techrules.xml` | `e14dfebb4e0d7ba8bdf09c8f3d3e3ed7ac6a1c6b1f278f8b887aebdf23079f83` |
| `ron-data/typenames.xml` | `74c18fb89400c87690f5410a26b7b8f1bdd185ea3bbbb9ec3d9b94a614f75a4b` |

These are anchors, not a complete manifest. A production composition must also list every other
type-bearing base file and every mod overlay in application order.

The PDB-derived schema fixes the fields consumed by the projection:

- `TypeData` has `type +0x04`, `job_time +0x08`, `tribe_mask +0x10`, six costs at `+0x18`,
  three prerequisites at `+0x30`, `from +0x3C`, `where +0x40`, `modified +0x58`, grid bytes at
  `+0x5C/+0x5D`, internal `name +0x60`, display name `+0x74`, and `type_name +0xB0`;
- `TypeBak` is 760 bytes and holds the common restore prefix; the concrete backup tails and
  restore bodies are enumerated in `bhs-type-table.md`;
- `LeaderData::tech +0x6C0C` and `obs_flags +0x6CF4` are both 116-byte `BitMask<806>` values;
- `BitMask<806>` is `bits`, `size`, `flags`, then 101 inline bytes;
- `Tribes` owns an `ObjectArray<Tribe>` and `Tribes::find` is vtable slot `+0x0C`.

The executable handler loops establish eight Leader slots. `Tribes::find` at `0x006EF2D0`
walks 24 full-version records; `rules.xml` contains those 24 records in order before the separate
trial-version list. The fixed type domains are the executable/PDB table ranges already frozen in
the owner: Unit `50..414`, Build `414..543`, and Other everywhere else. Only ordinary Unit
`50..402` and Build `414..543` rows require restore backups.

The shipped data independently fits those fixed bands: `unitrules.xml` contains 364 `<UNIT>`
records, exactly the width of `50..414`; `buildingrules.xml` contains 129 `<BUILDING>` records,
exactly the width of `414..543`; and `techrules.xml` contains 85 `<TECH>` records, matching the
documented backup band `544..629`. These counts do not substitute for the canonical composed
index map, but they are a useful drift alarm for the upstream loader.

## Admission and backup capture

The factory refuses:

- wrong component counts and every `None` row/tribe/Leader slot;
- blank internal names, blank type names, and blank tribe keys;
- a missing non-strict relation, a relation that omits self, duplicate targets, or an index above
  805;
- a concrete body whose domain disagrees with its fixed retail slot;
- type tribe-mask bits above the 24 shipped tribes;
- an active Leader whose tribe is outside `0..24`;
- Leader masks whose header is not exactly 806 bits and 101 bytes, or whose two unused tail bits
  are set;
- any lower-level `TypeTable` invariant, including non-ASCII retail lookup names.

Only after every row is admitted does the factory capture `TypeBackup::capture_pristine` for all
481 restore candidates. The backups are owned separately inside `TypeTable` and cannot be
recaptured after a BHS mutation. The proof pack mutates a produced Unit, invokes registration
286's owner path, and freezes restoration to the post-composition pristine values.

## Frozen integration seam and honest boundary

`systems::bhs_type_factory` is the only shared-module addition. Production setup should:

1. complete one ordered base-rules plus mod-overlay composition;
2. materialize all 806 rows, canonical non-strict relations, 24 tribe keys, and eight live Leader
   mask records from that same composition;
3. hash the manifest and each component, then call `produce_type_builtin_state` once;
4. retain both returned parts in one session owner and install the state before the first script
   frame.

No raw XML parser, session install, save format, or checksum walk is claimed here. In particular,
the existing `ScriptRuntime::install_type_builtins` can accept only the state, so calling it while
discarding `TypeBuiltinProvenance` would violate this factory contract. The next integration must
place both under one opaque session/Sim owner, make channel 13 consume the same live rows, and
serialize the mutable state while using the retained provenance to reconstruct immutable backups.

Immediate executed shipped-call coverage therefore remains zero. Once the synchronized composer
and session seam use this producer, the already-routed 1,614 BHS calls can consume one exact owner
rather than fixture or zero-filled state.

## Source-only validation order

This lane was explicitly source-only: no compiler, formatter, Cargo command, remote harness, or
retail process was run. The frozen proof pack covers successful projection/provenance retention,
post-mutation backup restoration, mixed-source rejection, sparse and blank inputs, relation and
domain rejection, exact tribe/Leader cardinality, and Leader-mask header/padding checks. Static
validation is limited to `git diff --check` plus direct inspection of the source anchors above.

Root convergence passed the focused factory in both independent profiles on 2026-08-09 as part
of hbox `bhs-factory-special-state-20260809T222235Z-65491-19369-3437590ed646` and persvati
release `bhs-factory-special-state-release-20260809T222236Z-65494-7499-3437590ed646`.
Both exited 0 with 6/6 factory tests. This validates construction/refusal behavior, not the still
missing synchronized session installation.
