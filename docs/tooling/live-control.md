# Bidirectional live retail control

Status: **implemented and attach-probed; gameplay command exercise awaits a live match.**
Target: the one supported `riseofnations.exe`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.

## Why this path

Calling `CommandManager` from an injector-created worker would race the main thread. The
bridge instead redirects the sole direct `TurnControl::do_frame` call in `Game::loop`, at
`0x00591686`, to a wrapper that performs pre/post observation around the original method.
The five replaced bytes are exactly:

```text
e8 45 67 3c 00    call TurnControl::do_frame (0x00957DD0)
```

The DLL refuses any different bytes and also checks the PE machine (`0x14c`), entry RVA
(`0x15d699`), and image size (`0xbb4000`). `retailctl.py deploy` additionally hashes the
target file before injecting. Runtime addresses are always `module base + measured RVA`;
there are no guessed offsets.

The worker parses `request.txt`; a tiny main-thread callback consumes an atomic request.
That callback calls the shipped entry points and snapshots the live state. The worker only
serializes those fixed records to `events.ndjson`. Thus file I/O and parsing never occur
on the game thread.

## Supported retail calls

| request | shipped entry | proof recorded |
|---|---:|---|
| `pause 0|1` | `CommandManager::issue_pause` `0x00940BA0` | exact opcode `0x4c` bytes + pause bit transition |
| `speed N` (`0..4`) | `issue_speed_set` `0x00940B60` | exact opcode `0x34` bytes + `TurnControl+0x30` state |
| `speed-up` / `speed-down` | `0x00940B30` / `0x00940B00` | exact `0x35` / `0x36` bytes |
| `checksum` | `issue_check_sums` `0x00940770` | exact 65-byte opcode `0x39` packet when retail's multiplayer gates allow it |
| `halt WHO IDS...` | `issue_halt` `0x009418D0` | retail-generated group+halt bytes; selected unit order list becomes empty |
| `move WHO X Y QUEUED ORDER FORM WIDTH DISEMBARK IDS...` | `issue_move_to` `0x00941720` | retail-generated group+move bytes; current order pointer/vtable transition |
| `attack WHO TARGET_WHO TARGET_ID FLAGS QUEUED IDS...` | `issue_attack` `0x009415E0` | retail-generated group+attack bytes; current order pointer/vtable transition |

`WHO` and IDs are retail object owner/index coordinates, not DoN entity IDs. `X` and `Y`
are retail `Coord` integers, exactly as exposed by `donscan`; the control layer performs no
invented scaling. `QUEUED` is the shipped `QueuePos` enum (`0 first`, `1 last`, `2 new`).
`ORDER` is the shipped `OrderIndex`; ordinary move-to is `1`.

The synthetic `GroupOut` passed to retail contains only fields that
`CommandPackage::add_group` (`0x0094BB60`) demonstrably reads: `num +0x0c`, `who +0x4a`,
and `short list[128] +0x8cc`. Retail itself resolves each object, checks liveness/type,
converts indices to UIDs, enforces package capacity, and rejects empty/invalid groups.
The probe does not bypass those gates.

## Use

```sh
python3 tools/retail-control/retailctl.py deploy --pid 5236
python3 tools/retail-control/retailctl.py send observe
python3 tools/retail-control/retailctl.py send pause 1
python3 tools/retail-control/retailctl.py send pause 0
python3 tools/retail-control/retailctl.py send move 0 16000 12000 2 1 -1 -1 0 12 13
python3 tools/retail-control/retailctl.py stop
python3 tools/retail-control/retailctl.py rearm
```

Every response is NDJSON with `queued`, then (where state evidence exists) `applied` or
`timeout`. `command_hex` is copied from the exact range appended to retail's live
`CommandPackage`; it includes multiplayer padding when retail inserts it. Unit order
evidence reads the PDB-defined `UnitData::orderlist` at `+0xc8`, specifically its current
order pointer (`Unit+0xcc`) and length (`Unit+0xd8`). Vtable values can be resolved through
`schema/vtables.json` (for example `MoveOrder` `0x00B4A12C`, rebased at runtime).

## Reversibility and current exercise

`STOP` first prevents callbacks, then suspends other threads long enough to restore the
five original call bytes and flushes the emulator instruction cache through both
`FlushInstructionCache` and `WriteProcessMemory`. The DLL deliberately remains loaded and
parked; this avoids unloading code while another thread could still have a return address
inside it. Deleting `STOP` rechecks the prologue and reinstalls the detour.

On 2026-08-08, PID `5236` was inspected read-only before this probe was built:

- module base `0x00D60000`, ASLR delta `0x00960000`;
- `GameAccess::game` resolved to `0x01797EC0`;
- five observations of `Game::frame` at 500 ms intervals all read `0`;
- the process had no main window and no older `donhook`/`donhook2` module loaded.

The built DLL was then exercised against that exact process. Host preflight reproduced the
expected executable SHA-256; `donject` verified identical local/remote `kernel32` bases,
`LoadLibraryA` returned module base `0x6AFB0000`, and the DLL reported `state=armed` with
runtime call site `0x00EF1686`. An `observe` request correctly produced no response because
the dormant process never reached `Game::loop`. `STOP` then reported `state=parked`, and a
fresh external read proved the retail call bytes were restored exactly:

```text
00EF1686: E8 45 67 3C 00 85 C0 74 6D 8B ...
```

This proves image gating, deployment, attachment, hook installation, the no-loop failure
mode, and reversible removal. It is not evidence that a gameplay command was applied. The
controlled live-match exercise must wait for a match loop; the
required operator action is simply to launch/relaunch retail and enter a solo skirmish,
then run `send observe`, `pause 1`, `pause 0`, and one known-unit `move`. Do not label the
bridge gameplay-validated until those events contain both the retail packet and the
corresponding `applied` observation.
