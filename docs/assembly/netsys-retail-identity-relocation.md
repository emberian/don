# The generation-7 retail identity gate cannot match a live image

Status: **defect, reproduced live**. Recorded 2026-08-10 from an authorized
`DON_NET_LOAD_ONLY=1` run of the supported executable with the generation-7
`CrossplayNetLib.dll` installed.

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
executable's own constructor at RVA `0x39E80`. **That fix is inert on the real game**,
because the constructor is only invoked inside the `Some(exe)` arm.

The live trace records it plainly:

```text
seq=3 factory=ready abi=netsys-v65 role=Host load_only=true retail_identity=false
seq=4 factory=lobby-dto-skipped reason=non-retail-load-only-executable
```

`load_only` masks the severity here — it merely skips. Outside load-only the same `None`
takes the `factory=inert reason=retail-executable-identity` arm, so a host or join attempt
would refuse before any transport. Fixing this gate is a prerequisite for any generation-7
match attempt.

The Wine PE32 smoke cannot catch this: its disposable executable is deliberately not the
retail image, so it exercises the `None` path by design and passes.

## The fix this implies

Compare only relocation-invariant bytes, or relocation-adjust the operand before comparing
(the delta is `actual_base - RETAIL_IMAGE_BASE`, both already known at the call site).
Masking the four operand bytes is the smaller change; adjusting them keeps the check
sensitive to a different callee. Either way the prefix constants must record which byte
ranges are relocatable rather than assuming none are. Not yet implemented.

## Capture protocol

1. Install the generation-7 DLL over
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
