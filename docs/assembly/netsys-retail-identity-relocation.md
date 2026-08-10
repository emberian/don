# The retail identity gate could not match a live rebased image

Status: **two defects, reproduced live and fixed**. Recorded 2026-08-10 from authorized
`DON_NET_LOAD_ONLY=1` runs of the supported executable. Generation 7 reported
`retail_identity=false`; after both fixes the same run reports `retail_identity=true` and
`factory=lobby-dto-constructed`.

Both defects are the same mistake: a value recorded at the **preferred** base compared
against a **loaded** image that the Windows loader rewrote.

## What happens

`retail_executable_base` (`crates/netsys-shim/src/netsys.rs`) gates every hard-coded
executable RVA behind an identity check. Its last two clauses compare **in-memory** bytes
against two pinned prefixes:

```rust
core::slice::from_raw_parts(base.add(LOBBY_DTO_CTOR_RVA), LOBBY_DTO_CTOR_PREFIX.len())
    != LOBBY_DTO_CTOR_PREFIX
```

`LOBBY_DTO_CTOR_PREFIX` is the **preferred-base** form of the instruction stream and its
sixth byte begins a `push imm32` carrying an absolute address:

```text
55           push ebp
8b ec        mov  ebp, esp
6a ff        push -1
68 2a fe a5 00   push 0x00a5fe2a      <-- relocated at load time
64 a1 ...    mov  eax, fs:[0]
```

The supported executable has ASLR enabled, so the loader rewrites that operand. The
comparison is therefore between relocated memory and unrelocated constants, and it can
never succeed on a live rebased image.

## The measurement

Bounded `ReadProcessMemory` of 16 bytes at each pinned RVA in the live process:

```text
module_base = 0x00190000        preferred = 0x00400000      delta = -0x270000

rva=0x4b1f0  in memory  55 8b ec 6a ff 68 2a fe 7e 00 64 a1 00 00 00 00
             pinned     55 8b ec 6a ff 68 2a fe a5 00 64 a1 00 00 00 00
                                          ^^ differs

rva=0x39e80  in memory  56 8b f1 83 c8 ff 66 89 46 0c c7 46 04 00 00 00
             pinned     56 8b f1 83 c8 ff 66 89 46 0c c7 46 04 00 00 00   (equal)
```

`0x00a5fe2a - 0x270000 = 0x007efe2a`, exactly the operand observed in memory. The single
differing byte is the high byte of that relocated address. The second prefix contains no
absolute operand and matches unchanged, which isolates relocation as the cause rather than
a wrong constant or a different build.

Every other identity clause passes against this executable, measured from the shipped file:
machine `0x014c`, timestamp `0x6674863f`, magic `0x010b`, image base `0x00400000`,
size-of-image `0x00bb4000`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079` — the supported
executable named in `README-LLM.md`.

## Why it matters

`retail_executable_base` returning `None` makes the factory skip **both** retail
constructor calls:

- `LobbyDTO` at concrete `+0xD0` is never constructed.
- `ip_addresses` at concrete `+0x1A0` stays `MsvcObjectArrayString::empty_unconstructed()`.

The second is the whole reason generation 7 exists. Generation 5 crashed at
`SetupWin::draw_ip_address` (`0x005BD635`) dereferencing the array returned by
`NetSys::get_ip_addresses`; generation 7 fixes that by constructing the array through the
executable's own constructor at RVA `0x39E80`. **That fix was inert on the real game**,
because the constructor is only invoked inside the `Some(exe)` arm.

The live trace records it plainly:

```text
seq=3 factory=ready abi=netsys-v65 role=Host load_only=true retail_identity=false
seq=4 factory=lobby-dto-skipped reason=non-retail-load-only-executable
```

`load_only` masks the severity here — it merely skips. Outside load-only the same `None`
takes the `factory=inert reason=retail-executable-identity` arm, so a host or join attempt
would refuse before any transport. Fixing this gate was a prerequisite for any match attempt.

The Wine PE32 smoke cannot catch this: its disposable executable is deliberately not the
retail image, so it exercises the `None` path by design and passes.

## Defect 2: the mapped `ImageBase` field

Fixing the prefix alone did **not** restore identity. A second run still reported
`retail_identity=false`, isolating `pinned_retail_pe_headers`, which required

```rust
read_u32(headers, pe + 24 + 28) == Some(RETAIL_IMAGE_BASE)
```

The loader rewrites `OptionalHeader.ImageBase` in the mapped image to the address it chose.
Read from the live process at the same module base:

```text
IN-MEMORY imagebase    = 0x00190000     (the file on disk says 0x00400000)
IN-MEMORY sizeofimage  = 0x00bb4000     machine 0x014c, timestamp 0x6674863f, magic 0x010b
```

So **that field carries no identity once the image is rebased**: the preferred base is not
recoverable from a rebased header, and demanding the pinned value can only fail. It is now a
mapping sanity check — it must equal either the pinned preferred base or the actual load
base — and identity rests on the fields the loader does not touch (machine, timestamp,
magic, size-of-image) plus the two code prefixes.

## The fixes

`prefix_matches_relocated` compares bytes outside a recorded relocation list exactly, and
each listed dword as `loaded == pinned.wrapping_add(delta)`. Adjusting rather than masking
keeps the check sensitive to a *different* callee at the same RVA, which masking the four
operand bytes would have accepted. `LOBBY_DTO_CTOR_RELOCS = [6]`;
`OBJECT_ARRAY_STRING_CTOR_RELOCS` is empty because that prefix is position independent.

`pinned_retail_pe_headers` takes the loaded base and accepts the `ImageBase` field at either
the pinned or the actual value, rejecting any third value.

Both are pinned by unit tests built on the **measured** live bytes rather than on
recomputed arithmetic, including negative cases for a different callee, a corrupted opcode,
a wrong delta, and a header claiming an unrelated base.

## Capture protocol

1. Install the shim DLL over
   `C:\Program Files (x86)\Steam\steamapps\common\Rise of Nations\CrossplayNetLib.dll`;
   the shipped original is preserved at
   `C:\Users\Public\don-netsys-experiment\CrossplayNetLib.shipped.dll`, SHA-256
   `d716caafa565fbe9a914ae912b981573d14e7fa19efb73d500bbd5016de6ab60`, 1,198,592 bytes.
2. Launch `launch-load-only.cmd` (sets `DON_NET_LOAD_ONLY=1`, `DON_NET_TRACE`) as the
   console user through a one-shot scheduled task, because `prlctl exec` is SYSTEM in
   session 0 and retail must run in the interactive session.
3. Read the flushed trace, take the bounded 16-byte reads above, stop the process, and
   remove the task.

The run reached the menu and exercised `ns_init`, `set_p2p_callbacks`, `ns_set_ip_override`,
`ns_log_connection`, `ns_check_pulse`, `ns_process_system_messages` and `ns_get` before
termination. No lobby was entered and no transport was enabled.

## Result

The same protocol against the fixed shim:

```text
seq=3 factory=ready abi=netsys-v65 role=Host load_only=true retail_identity=true
seq=4 factory=lobby-dto-constructed offset=0xd0 size=0xd0 ctor_rva=0x4b1f0 \
      ip_array_offset=0x1a0 ip_array_ctor_rva=0x39e80
```

Both retail constructors execute, so `ip_addresses` at `+0x1A0` is a constructed
`ObjectArray<String>` rather than the unconstructed shape that faulted generation 5. This is
the identity gate and the two constructors only — it is **not** evidence that a match, a
lobby, or any transport works.
