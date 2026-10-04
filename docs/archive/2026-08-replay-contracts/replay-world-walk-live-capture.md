# Replay World byte-image live-capture frontier

## Result

This isolated tranche supplies the missing lawful capture path for checksum
channel 12. It does not contain a retail World image and does not claim that the
model agrees with retail. It makes the next replay run capable of producing the
exact `World::walk_data(-1)` byte stream whose first differing byte can be
localized.

The implementation is split deliberately:

- `tools/world-walk-capture/world_walk_capture_adapter.h` is an allocation-free,
  file-I/O-free PE32 adapter intended to be included by the retail controller;
- `crates/don-replay/src/world_walk_checkpoint_frontier.rs` extracts the exact
  same-group peer packets from the original RCX and hashes both their bytes and
  their `(group, play, stamp, packet)` evidence records;
- `crates/don-replay/src/bin/don-world-checkpoint.rs` writes that report with
  create-new and read-only semantics;
- `tools/world-walk-capture/seal_world_walk.py` authenticates and seals the raw
  capture, executable, checkpoint, controller/process context and optional model
  comparison into a content-addressed directory; and
- focused Rust and Python tests keep packet width, peer agreement, code identity,
  checksum binding, localization and immutability fail-closed.

None of the new replay files is added to `lib.rs`, and no replay schedule or
simulation state file is changed.

## Exact retail call path

The supported executable is SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Its relevant immutable code facts are:

| fact | value |
|---|---:|
| `World::walk_data(DataWalk*, int)` | `0x006b5cf0` |
| PDB body length | 903 bytes |
| body SHA-256 | `adf50b4197020da3932b562442b1cf7d8e80443da42f6181ebd23276bd080e01` |
| `check_all` World call | `0x00936a03` |
| original call bytes | `e8 e8 f2 d7 ff` |
| `CheckSum` vtable | `0x00b3f920` |
| `DataWalk` direction/checksum/byte-count | `+0x04` / `+0x10` / `+0x14` |

The call is a two-stack-argument callee which returns with `ret 8`; the complete
walker body ends at `0x006b6077`. The caller pushes section `-1` and the stack
`CheckSum`, then calls `0x006b5cf0`. The walker obtains the authoritative World
from `[0x00c06188]`; copying the 372-byte World owner would therefore miss the
pointer-owned WData, TData/fog, collision and Terrain arrays.

The adapter patches only the five-byte call at `0x00936a03`. Its bridge:

1. calls the original walker with retail's real `CheckSum` unchanged;
2. observes the exact result and byte count retail just produced;
3. when that value equals the armed peer checkpoint, immediately calls the same
   unmodified walker again with a read-only append visitor on the same retail
   main thread;
4. freezes only when the captured byte count and Adler value equal the original
   `CheckSum`; and
5. otherwise discards the image with a typed fault while leaving retail's
   original checksum call complete.

This intentionally does **not** replace the live stack visitor's vtable. It
also does not detour the 903-byte walker entry. The repeated traversal runs
before the main thread can advance simulation state, and every callback merely
copies `[begin,end)` and updates a private Adler accumulator.

## Controller integration and lifecycle handoff

The retail-controller owner should integrate the header, not load it as an
independent DLL. The following gates are mandatory before another retail retry:

1. On the worker, allocate a fixed capture buffer (2 MiB is sufficient for the
   current 780,168-byte case; the header permits at most 16 MiB) and call
   `wwc_initialize`. The header checks the exact walker prefix and trailer.
2. Authenticate the complete executable on the host as the controller already
   does. The sealer independently rechecks the full file SHA, PE machine,
   timestamp `0x6674863f`, entry RVA, image base/size, all 903 walker bytes and
   the five original callsite bytes.
3. Call `wwc_activate`, then arm target `0xd63a3a53` with a monotonically unique
   capture sequence before playing the lexically first corpus recording to
   lockstep group 2.
4. Extend the controller's hook transaction to own `0x00936a03`. Refuse the
   install unless all five original bytes match. Patch it to a rel32 call of
   `wwc_callsite_bridge` only after the existing generation/attempt/epoch fence
   is current.
5. Include the World callsite, the bridge code and `wwc_snapshot().inflight` in
   the controller's EIP/inflight audit. STOP must restore this callsite as well
   as the TurnControl callsite, prove its exact original bytes, wait for zero
   capture inflight, and only then call `wwc_deactivate` or release the buffer.
6. Copy the frozen bytes and snapshot from the worker only after phase `FROZEN`
   and inflight zero. Main-thread file I/O remains forbidden.

The adapter intentionally does not install or remove its own patch. That keeps
one owner for both hook lifecycles and prevents a helper DLL from escaping the
controller's quarantine/detach proof.

The controller must emit a `don.retail-world-walk-context.v1` JSON file with the
exact field set enforced by `seal_world_walk.py`: controller DLL/ready/load
manifest identities; PID and process creation time; attempt and epoch; main
thread; original callsite and walker identities; capture sequence/group; target,
original and captured checksum/byte counts; callback counts; thread id; phase,
fault and raw-image SHA-256. The `restoration_owner` value is exactly
`retail-controller-lifecycle`; the file also attests exact callsite restoration,
adapter deactivation and zero inflight at the moment the worker copied bytes,
and repeats the raw/decompressed replay SHA-256 identities from the checkpoint.

## Peer checkpoint extraction

The first checkpoint must come from the original replay, not from
`schema/replay-validation.json` and not from a manually typed checksum:

```sh
cargo run --release -p don-replay --bin don-world-checkpoint -- \
  /absolute/path/Playback___2018.11.17_13_21_42__Sat_.rcx \
  2 /absolute/evidence/world-turn-2.checkpoint.json
```

The extractor decodes group 2, requires one exact 65-byte opcode-`0x39` packet
from each of at least two distinct player records, re-parses all sixteen words,
requires each tuple's wrapping-total and Adler-shape self-checks, and requires
all sixteen words to agree between same-group peers. It records raw and
decompressed replay SHA-256 identities, each exact packet hash, and a distinct
header-bound evidence hash over `(group, play, stamp, packet)`.

For the target recording this report must name World `0xd63a3a53`. The report
explicitly writes `byte_agreement_claimed: false`: even unanimous peer hashes
do not disclose one World byte.

## Sealing and localization

After the controller has stopped, restored both hook sites and copied the raw
buffer and context to the host:

```sh
python3 tools/world-walk-capture/seal_world_walk.py \
  --image /absolute/evidence/world-turn-2.bin \
  --checkpoint /absolute/evidence/world-turn-2.checkpoint.json \
  --context /absolute/evidence/world-turn-2.context.json \
  --executable /Users/ember/dev/don/ron-bin/riseofnations.exe \
  --output /absolute/evidence/sealed-world-walks
```

The sealer reads each source through a no-symlink, stable-inode/size/mtime
snapshot. It refuses unless `adler32(1, image)` equals the peer-agreed World
word and all three adapter values. It then writes `image.bin`, the checkpoint,
the context and a manifest into a directory named by the SHA-256 of the complete
manifest core, using create-new files, fsync, atomic rename and read-only modes.
The executable and replay themselves are not copied.

Without a model image the manifest says localization is unavailable. It never
derives an offset from the checksum. Once the convergence owner emits the
`WorldOwnerSnapshot.image` for the identical replay point, add
`--model-image`; optionally provide a contiguous `don.world-walk-sections.v1`
section map to receive both the first global offset and its retail section-local
offset. `byte_image_equal` is then a direct byte comparison result, while
`simulation_agreement_claimed` remains false.

## Claims not made

This tranche does not claim a captured retail image, a channel-12 match, a
specific first differing section, correct generated terrain, or any closure
increase. The landed checksum corpus remains a comparator only. The first
localizable mismatch requires a future main-thread capture whose raw bytes pass
every gate above.

## Validation boundary

Root convergence validated the isolated pack without touching the VM or retail:

- persvati job `world-walk-checkpoint-v2-20260809T230547Z-520-19038-71d773e61575`
  passed all three Rust checkpoint tests (the first submission failed closed only because the
  required pinned balance asset was omitted);
- the Python sealer passed all four focused tests and `py_compile`;
- Zig's PE32 target compiled the adapter header as an Intel 80386 COFF object under
  `-Wall -Wextra -Werror` (with header-only unused static functions exempted).

This validates the isolated parser/sealer/adapter shape, not the still-unintegrated second
controller callsite or a live retail capture.
