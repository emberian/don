# Every retail turn package carries a checksum — except the first one

Status: **read off the supported executable**, 2026-08-10. Corroborated by the recorded corpus.
This documents retail's emit path only; it is not a tier claim about any DoN Rust code.

## The question this answers

`tools/owned-peer` refused a live retail turn with

```text
first retail command package at stamp 1 has no checksum command;
refusing to synthesize or skip its slot-1 reply
```

and, one layer earlier,

```text
could not recover the first retail command package key at stamp 1
```

Both refusals have the same cause, and it is not a short payload.

## What builds a wire package

Two functions append to the one `CommandPackage` at `CommandManager+0x28`, and one sends it.

| function | VA | appends |
|---|---|---|
| `CommandManager::issue_check_sums` | `0x00940770` | opcode `0x39`, 65 bytes |
| `CommandManager::issue_turn_data` | `0x00940390` | opcode `0x4a`, 11 bytes |
| `CommandManager::issue_camera` | `0x00940a20` | opcode `0x48`, 10 bytes |
| `CommandManager::send_local_package` | `0x00940120` | — sends via `CommandPackage::send` `0x0094c1e0` |

`CommandPackage::add_command` `0x0094bae0` is the appender. Its first act is
`if (0x200 < size + this->size) return 0;` — a full package **silently sheds** a command, and
`issue_check_sums` ignores the return value. It also adds `Random::get(0, 2)` bytes of padding
after each command when the multiplayer flag is set, and that padding counts toward the same
512-byte ceiling.

## The checksum is unconditional once the prologue passes

`issue_check_sums` `0x00940770`, disassembled:

```text
00940795  mov byte ptr [ebp - 0x70], 0x39     ; the opcode
009407a1  jne 0x940a0e     ; replay-playback   ([game+0x820] & 0x10) must be clear
009407a9  je  0x940a0e     ; network game      ([game+0x820] & 0x04) must be set
009407c9  je  0x940a0e     ; local player live (player+0x74 & 0x0001)
009407d4  jne 0x940a0e     ;                   (player+0x74 & 0x0100) clear
009407dc  jne 0x940a0e     ; not resigned      (player+0x74 & 0x00d0) clear
009407e9  je  0x940a0e     ; Player::is_connection() 0x006edb90 nonzero
...
0094097e  je  0x94098e     ; the ONLY branch inside the body (a per-channel skip)
009409f8  push 0x41                            ; 65
009409fb  call 0x94bae0                        ; CommandPackage::add_command
```

Every conditional except `0x0094097e` is a prologue gate jumping to the early-out at
`0x00940a0e`, and that one lands **before** the append. There is no turn modulus, no window
counter, and no read of `GameInfo::checksum_window_size` / `checksum_deep` /
`checksum_failure_threshold` (`GameInfo` +12 / +8 / +16) anywhere on this path — those three
names bind only inside `GameInfo::log_data` `0x005D6040`. **So a peer that is in a network
game, alive, and connected emits exactly one 65-byte `0x39` per turn.**

`issue_check_sums` has exactly one caller: `CommandManager::process_turn` `0x0093EF10`, at
`0x0093F2E9`, which then sends at `0x0093F378`.

## The exception: `CommandManager::start`

`CommandManager::start` `0x00942E10` is the other producer of a wire package, and its
multiplayer arm is:

```text
00942f01  call 0x940390    ; CommandManager::issue_turn_data
00942f0b  call 0x940120    ; CommandManager::send_local_package
```

It never calls `0x00940770`. **The first package each peer puts on the wire in every match
therefore carries turn data and no checksum command.**

The corpus agrees exactly. `schema/replay-validation.json`, restricted to the 21 files that
contain any `0x39` at all: 488,603 packages decoded with zero anomalies, 488,557 checksum
packets — a difference of 46, which is precisely one per player per game (19 two-player files
× 2 plus 2 four-player files × 4). Each of those files reports `turns_checksummed == turns - 1`
with `first_turn: 1`.

The often-quoted "585,152 turns vs 488,557 checksum packets" gap is not evidence of a cadence:
the first number sums all 61 files while the second counts packets in only the 21 files that
have checksums. The other 40 are a solo recording and older builds.

## Consequences for an owned peer

1. **Ciphertext key ranking cannot settle the first turn.** `recover_game_key` accepts a
   candidate only when the decoded stream contains a valid `0x39`. The `CommandManager::start`
   package structurally cannot contain one, so no candidate is acceptable at that stamp — at
   any payload length. The key has to come from the match: see
   [`multiplayer-command-package-key.md`](multiplayer-command-package-key.md).
2. **A checksum-less package must be answered, not refused.** The lockstep gate consumes one
   package per slot per turn; refusing to answer the start turn stalls the match at turn one.
3. **A package carrying no commands is a legal retail record.** `send_local_package` sends
   unconditionally in its multiplayer arm; `CommandPackage::send` `0x0094c1e0` guards only its
   scramble loop with `if (size != 0)` and still sends `size + 8` bytes
   (`0094c3fd movsx edx, word ptr [ecx+0x10]` … `0094c407 add edx, 8`). So the empty package is
   an 8-byte header, and it is the one package a peer can always form correctly: an empty
   payload has no `u16` words to XOR and no inter-command pad to get wrong.
4. **A peer that reports no checksum is skipped, not desynced.** `process_turn` zeroes
   `DAT_00cbee90[0..8]` each turn, `CommandPackage::process_check_sums` `0x009459D0` stores
   each reporter's value, and `CommandPackage::end_process` `0x0094C800` compares only nonzero
   entries against the first reporter.

`don_net::msg` required 9 bytes to decode a `NETMSG_COMMANDPACKAGEDATA` because
`sizeof(NetMsg_CommandPackageData)` is 9 — but that 9 counts the one-element `unsigned char[1]`
flexible tail. The wire record is `8 + data_size`, so the empty package was being dropped one
byte short. Fixed, with a regression test.

## Not determined

- Whether the 512-byte `add_command` ceiling ever sheds a checksum in practice. It is
  reachable, and zero occurrences appear in 488,603 corpus packages, but those are 2- and
  4-player games at modest command rates.
- The semantics of the individual `player+0x74` bits `0x0100` and `0x00d0`; they are used
  positionally here.
- Where `GameInfo::checksum_window_size` is actually read. It provably does not gate emission;
  the likely consumer is the receive-side out-of-sync window around
  `CommandPackage::end_process` `0x0094C800` / `CommandManager::recover_from_oos` `0x0093E9E0`.
- Why the pre-2017 corpus builds show no `0x39` at all. Irrelevant to the supported binary.
