# Script runtime checksum channel

Status: a standalone Rust primitive now models the complete checksum-visible traversal
reachable from `RunTimeEnv::walk_data`. It is not wired into `don-replay`, and there is no
builder yet which reconstructs its inputs from a replay or from DON's simulation. The
implementation therefore advances channel 15 from **absent** to **compiled structural
primitive**, not to a passing replay channel.

Implementation: `crates/don-replay/src/script_channel.rs`.

## What retail actually walks

`CheckSums::check_all` `0x00936560` creates a fresh `CheckSum` (adler-32 seed 1), resets
its byte counter, and calls `RunTimeEnv::walk_data` `0x009c41a0` as the fifteenth channel.
The caller then reads the channel checksum from `CheckSum+0x10`, logs it as
`script_run_time`, and adds it to the plain wrapping sum named `total`. [measured,
instruction trace at `0x00936af5..0x00936b49`]

`RunTimeEnv::walk_data` does **not** serialize the `RunTimeEnv` fields in the PDB. Its
checksum-side sequence is: [measured, disassembly `0x009c41a0..0x009c4213`]

1. call `DataWalk::walk_test` (zero bytes for `CheckSum`);
2. call `RunTimeEnv::close` `0x009c40a0`, clearing transient interpreter state;
3. hash the signed 32-bit count at `ScriptFile::script_files+0x04` (preferred VA
   `0x00c8cba4`);
4. for every entry in the global pointer vector at preferred VA `0x00c8cbb0`, call
   `ScriptFile::walk_data` `0x009c63b0` in index order.

The outer array's capacity, growth hint, flags, and pointer-presence bytes are not walked.
The function unconditionally dereferences each pointer covered by the count, which is why
the Rust root API accepts only non-null `ScriptFile` entries.

### `ScriptFile` checksum branch

The PDB gives `ScriptFile` size `0xe0`. Its walker calls these members in order:

| member | offset | checksum behavior |
|---|---:|---|
| `code: SimpleArray<u8>` | `+0x00` | full array walk |
| `scripts: PtrArray<Script>` | `+0x1c` | specialized pointer-array walk |
| `const_pool: PtrArray<ScriptType>` | `+0x38` | `ScriptType::walk_array` |
| `linked_files: SimpleArray<int>` | `+0xb8` | full array walk |

At `0x009c63fa`, `ScriptFile::walk_data` tests `DataWalk+0x08`. `CheckSum` sets that field
to one, so it skips `linked_file_names`, `source_file`, `line_to_op`, `break_lines`, the
two timestamps, and `file_flags`. [measured, `0x009c63b0..0x009c6444`] Those are source
and debugger fields, not channel-15 state.

### Container byte formats

All scalar bytes are little-endian. `walk_test` contributes no bytes to `CheckSum`.
[measured, container disassembly]

- `SimpleArray<T>` (`0x0049a090` for `u8`, `0x00473120` for `int`) hashes a signed
  32-bit count. If it is non-zero, it then hashes signed 32-bit capacity, two-byte growth
  hint, `flags & 0xbf`, and exactly `count` elements. Empty arrays hash only their zero
  count.
- `ObjectArray<String>` `0x00490fb0` uses the same header and then walks `count` strings.
- `String::walk_data` `0x00a1b2d0` hashes its `curr_len` as a zero-extended 32-bit value,
  then exactly `curr_len` UTF-16 code units. It does not hash the backing pointer, offset,
  flags, module id, or cached hashes.
- `PtrArray<Script>::walk_data` `0x004cccd0` hashes the non-zero array header, then one
  presence byte per slot. It next hashes capacity and growth **a second time** (the exact
  range `[array+0x08,array+0x0e)`), then walks each non-null script in index order. The
  flags byte is not repeated.
- `ScriptType::walk_array` `0x009d84a0` hashes only its signed 32-bit count before calling
  `ScriptType::walk_base` for each slot. It does not include pointer-array capacity,
  growth, flags, or a separate presence vector.

These metadata bytes are sim-critical. A Rust `Vec` with identical logical elements is
not a sufficient channel-15 input unless its retail capacity/growth/flags are also known.

### Scripts and dynamic values

`Script::walk_data` `0x009c5f30` walks, in order: [measured]

1. `static_vars` through `ScriptType::walk_array`;
2. `DynamicBitMask`'s two dwords (`bits`, `size`) and then `size` pointed-to bytes;
3. `params: SimpleArray<int>`;
4. `refs: SimpleArray<u8>`;
5. `trigger_names`, `var_names`, and `static_var_names` as three
   `ObjectArray<String>` values;
6. `name: String`;
7. the raw 12-byte range `[Script+0xc0, Script+0xcc)`: `offset`, `return_type`, and
   `script_type`.

For each `ScriptType*`, `ScriptType::walk_base` `0x009d7ea0` hashes a zero dword for null.
For non-null it hashes `data_type` (dword), the low word of `scope` after OR-ing `0x80`
when virtual `is_ref()` is true, `ref_count` (word), and then calls the most-derived
`walk_data` vtable slot. The statically recovered payload walkers are:

| Rust shape | retail walker | additional traversal |
|---|---:|---|
| `Base` | `ScriptType::walk_data` `0x009d83d0` | none |
| `Int` | `ScriptInt::walk_data` `0x009d7720` | raw four-byte integer |
| `FloatBits` | `ScriptFloat::walk_data` `0x009d7000` | raw four-byte float bits |
| `String` | `ScriptString::walk_data` `0x009d6ae0` | `String::walk_data` |
| `Object` | `ScriptObject::walk_data` `0x009d6500` | nested `ScriptType::walk_array` |
| `Array` | `ScriptArray::walk_data` `0x009d5bc0` | `blank_base`, then nested values |

Named subclasses such as `ScriptUnitGroup`, `ScriptVector`, and
`ScriptConquestDiploOffer` inherit the `ScriptObject` layout/walker according to the PDB.
The Rust primitive carries float payloads as `u32` bits to avoid changing NaN payloads.
It deliberately makes the dynamic shape explicit; mapping a captured vtable/data-type
pair to that shape belongs in a future runtime adapter.

## Completeness boundary and fidelity

The public primitive returns an error when a count cannot fit retail's signed dword,
non-empty array capacity is negative or below count, a string exceeds retail's `u16`
length, or `DynamicBitMask.size` is negative or disagrees with the supplied payload.
There is no zero-fill or “unknown means empty” path. Nullable script and `ScriptType`
pointers are represented explicitly and emit the retail discriminator bytes.

The function graph, order, widths, masks, and conditional branches above are structural
**[measured]** facts from the exact shipped executable and GUID-matched PDB. The Rust
primitive has byte-order, mutation, raw-float-bit, and rejection tests, but it has not
been run differentially against retail over generated states. It is therefore **not yet
Tier B** and is not described as verified or proven.

No BHS instructions are executed. `RunTimeEnv::close` is not emulated; the adapter
boundary is a complete projection of the global script-file state after retail's close
call. This is enough to define the checksum primitive, not enough to recreate script state
from `.rcx`, `.svx`, source BHS, or DON's current empty simulation.

## Narrow live observation

On 2026-08-08, a read-only `donscan --read` against the user's running solo match (PID
5236, module base `0x00d60000`, preferred-base delta `+0x00960000`) read only 44 bytes of
root metadata:

- runtime `ScriptFile::script_files` header `0x015ecba0`: count 3, capacity 4, growth
  `0xffff`, data pointer `0x38becba8`;
- the 12-byte pointed-to vector contained three non-null `ScriptFile*` values.

This establishes that the channel was non-empty in that match and that the rebased root
matches the static layout. It was a single narrow observation, not a coherent double-read
or a walk of the pointed-to payloads, so it is **not** live checksum validation. No script
contents, source paths, broad memory regions, or private data were captured or persisted.

The replay corpus already supplies dynamic channel targets. For example, the first
checksummed turn in the exact-build sample shown by `tools/replay-validate.sh` carries
`script_run_time = 0x6a91bf5d`; the current `NullSim` emits 1 only because it walks zero
bytes. The new primitive correctly hashes at least the four-byte root count, but no claim
is made that a synthetic fixture reaches that recorded value.

## Tests and integration gaps

Run the standalone tests without changing the workspace module graph:

```sh
rustc --edition 2021 --test crates/don-replay/src/script_channel.rs \
  -o /tmp/don-script-channel-test
/tmp/don-script-channel-test
```

Five tests cover an empty root count, exact instruction-order byte transcript, a
one-byte mutation which changes the checksum, rejection of incomplete array/bitmask
captures, and preservation of distinct NaN payload bits.

Before channel 15 can become non-trivial in replay validation:

1. export this module from `don-replay` and point the channel walker at it;
2. build a complete `ScriptRuntime` from replay/save setup or from DON's future BHS VM,
   preserving retail container metadata and dynamic value identity;
3. independently map retail vtables/data-type tags to the six recovered dynamic payload
   shapes and reject unknown walkers;
4. compare complete captures against retail channel words over multiple script states,
   with a mutation test proving that the differential harness bites;
5. model BHS execution separately. This checksum primitive must not become a substitute
   for the interpreter state transitions which produce its next input.

Until those land, callers should report channel 15 as structurally implemented but
runtime-unavailable, never as `1`, zero, or a guessed replay target.
