# Canonical replay Build spawn identity transaction

Lane: `replay-build-spawn-runtime` · Builds/Cities prerequisite · 2026-08-11.

`crates/don-replay/src/build_spawn_runtime.rs` closes the smallest mutation gap shared by
the exact Builds and Cities adapters. Given a fully staged unlinked `BuildData`, playable
owner, current ptype index, and already-snapped center position, it atomically:

1. proves the current dense and sparse Build registries are the supported gap-free phase;
2. obtains the next Build object id from the canonical owner band;
3. stamps owner, object id, and XOR-encoded X/Y into the staged body;
4. calls `Sim::spawn_build`, which appends both registry views;
5. registers the current ptype in `production_runtime.build_types`; and
6. reads every resulting join back into a typed receipt.

No recorded checksum enters this transaction. It does not execute the remainder of
`Build::init`, create a City, choose a map start, or claim a Builds/Cities channel match.

## Shipped allocation and initializer chain

The authority is the supported `ron-bin/riseofnations.exe` (SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`) and its GUID-matched
`ron-bin/sbl/rise.pdb`. Names/sizes come from `schema/rise-procs.tsv` and the PDB extraction;
the instructions below were independently decoded from the PE32 image with Capstone.

| VA | shipped PDB name | relevant identity behavior |
|---:|---|---|
| `0x0065d190` | `Objects::init_build` | calls `find_free(owner,2000,3000,...)`, snaps coordinates, calls Build vtable `+0x1a0` with returned id |
| `0x0065ad60` | `Objects::find_free` | selects/reuses a free object id and advances the owner Build mark |
| `0x00629740` | `Build::init` | derived initializer; calls `Wall::init` at `0x0062979d` |
| `0x0063e9b0` | `Wall::init` | calls `Object::init` at `0x0063e9ca` |
| `0x00647750` | `Object::init` | calls `SubObject::init` at `0x00647768` |
| `0x00662300` | `SubObject::init` | writes owner/id/type/position identity |

The load-bearing `SubObject::init` stores are literal:

```text
0066230d  mov byte ptr [esi+09], al       ; who
00662318  mov byte ptr [esi+08], 1        ; valid at base-init point
0066231c  mov word ptr [esi+0a], ax       ; allocated object id
0066233c  call dword ptr [eax+84]         ; bind TypeIndex/ptype
00662347  xor eax, 00063637
0066234f  mov dword ptr [esi+10], eax     ; encoded X
00662357  xor edx, 00063637
00662360  mov dword ptr [esi+14], edx     ; encoded Y
```

The same body derives encoded Z from terrain after the X/Y stores. This isolated transaction
does not own terrain elevation and therefore does not claim the complete SubObject initializer.
The staged Build must already contain every field outside the four joined facts.

`Objects::init_build` proves the object-id provenance. At `0x0065d1a3` and `0x0065d1a8` it
pushes `3000` then `2000` before `Objects::find_free`; the returned EAX is retained in ESI.
At `0x0065d209` it pushes ESI into Build's `+0x1a0` initializer. The caller does not get to
invent or repair an id after checksum construction.

## Supported canonical owner phase

DoN currently dual-writes two object-address views:

- legacy `World::objects`, with per-owner dense Build rows at `[2000, build_mark)`; and
- `World::object_bands()`, the sparse owner whose live identity is
  `WorldObjectIdentity::BuildRow(row)`.

`Sim::spawn_build` already appends the same row to both. Its intentionally generic input
does not set the `BuildData + 0x0a` short, position, or current ptype. The transaction wraps
that append only after `World::object_bands_are_dense_equivalent()` succeeds and after every
existing `Sim::builds` row is proved to appear exactly once with matching owner and id.

This is exact only for the current gap-free phase-1 allocator. The next id is the Build mark,
`2000 + band length`. A mark at 3000 refuses. The wrapper does not pretend to implement
retail tombstone reuse from the full 1,101-byte `Objects::find_free`; sparse gaps remain a
separate migration boundary.

Owners 8 and 9 are refused because the shipped Build/Wall loops cover only the eight playable
leader slots. A nonempty Build band there also invalidates the existing-owner preflight.

## Request and receipt contract

`CanonicalBuildSpawnRequest` contains:

- `owner: u8` in `0..8`;
- `type_index: i32` whose row exists in the installed ptype table;
- independently produced `snapped_x/snapped_y`; and
- a complete staged `BuildData` with `flags & VALID != 0` and `city == -1`.

Requiring `city == -1` is deliberate. Accepting a pre-linked row would bypass the atomic City
transaction and make rollback impossible to audit. The setup producer may link the committed
Build only after it has independently staged and validated the exact City record.

The transaction overwrites stale incoming `who`, object id and encoded X/Y because it owns
those fields. It does not choose `flags`, type, snapped coordinates, City index, Build queue,
mining/gather state, Wall fields, or any remaining byte.

`CanonicalBuildSpawnReceipt` reads back:

- dense row, owner, allocated object id and mark transition;
- snapped and encoded positions;
- active-owner transition;
- dense registry row and sparse stable identity;
- committed body owner/id/decoded position; and
- registered current ptype.

The ptype is a semantic type index sidecar. A process-local C++ pointer is neither persisted
nor checksummed; the exact Builds/Cities adapters consume the index reached through that
pointer in retail.

## Atomicity and refusals

All fallible conditions are checked before the first mutation:

- owner and type-table domain;
- valid staged body and unlinked City sentinel;
- dense/sparse registry equivalence;
- unsupported owner bands;
- row coverage, uniqueness, owner and object-id identity for all prior Builds;
- Build-band capacity; and
- an already-owned ptype at the future dense row.

After preflight, both called methods are infallible dense appends. Tests snapshot Build count,
ptype rows, every dense Build band, all owner-active bits, and the complete sparse registry,
then prove each refusal leaves that snapshot unchanged.

## Validation

`crates/don-replay/tests/build_spawn_runtime.rs` covers:

- PE/PDB VA and XOR constants;
- complete first-spawn receipt and both registry views;
- immediate admission into `builds_runtime` as a 131-byte live Build;
- consecutive same-owner ids and independent per-owner marks despite dense row order;
- owner/type/flags/pre-linked-City refusals with no mutation;
- the exact current raw-`Sim::spawn_build` object-id gap;
- dense/sparse split refusal; and
- premature ptype ownership refusal.

## Integration hook

The active `replay-setup-cities-builds` lane owns the first consumer. Its staged setup
transaction should call `spawn_canonical_build`, validate the receipt against the chosen
start/placement facts, stage and link the exact City, and commit both or neither. It must not
duplicate identity stamping or feed checksum words into this request.

Registration is one future `pub mod build_spawn_runtime;` line in `don-replay/src/lib.rs`.
No `don-sim/src/tick.rs`, production runtime, replay harness/state, or setup file was edited
by this lane.
