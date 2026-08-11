# Web canonical Group → Move package bridge

Date: 2026-08-11. Scope: browser and loopback local-match preparation for the canonical
Sim-owned package transaction. This tranche does not rebuild `don_web.wasm`, register a Sim
system, change a save format, or claim the shipped native relay accepts the new package yet.
The existing one-byte Halt turn remains the only advertised live browser cohort.

## Exact admitted browser package

`web/public/js/play/canonical-command-package.mjs` owns two plaintext shapes:

| kind | bytes | commands |
|---|---:|---|
| existing Halt | 1 | `0x0c` |
| prepared direct move | 27 | singleton `GroupCommand 0x00` followed by complete `MoveToCommand 0x07` |

The direct move builder writes:

```text
00 01 who o:i16
07 x:i32 y:i32 set_angle=0:i32 angle=0:i32
   orders=1 queued=2 form=0 width=50 disembark=0
```

The exact compact retail fixture from package 6,284 of the finished 2026-08-11 replay is
reconstructed byte for byte:

```text
0001000100074bb900007eb9000000000000000000000102003200
```

That is one package. The frontend must never submit Group and Move through two independent
`game_submit` calls: another command, tick, or failure between those calls would create a state
retail never exposes and would defeat the Sim transaction's atomicity.

The validator rebuilds the complete byte sequence and rejects wrong length, residue, changed
opcode chronology, an invalid owner/object, or any changed Move tail. It does not silently
normalize a near miss. The separate replay host will eventually admit other measured Move tails;
the 27-byte browser cohort remains deliberately exact.

## Identity boundary

The browser renderer currently selects a global adapter `id`. Retail Group does not carry that
id. It carries owner-local `(who,o)`, and the receiver resolves a live Unit and checks its `uid`.
The prepared discovery helper therefore requires an authoritative callback returning exactly:

```text
{ who: u8 in 0..7, o: nonnegative i16, uid: u16 }
```

The renderer id is passed only as the callback lookup key. It is never compared with, returned
as, truncated into, or encoded as `o`. A missing resolver, stale/dead answer, extra shadow field,
owner mismatch, multiple selection, or absent UID refuses package construction. UID remains a
generation witness; it is not an invented Group wire field. `Handle` is also absent from the
browser wire identity.

The exact C ABI requested from the Wasm adapter is source-only until the Sim host lands:

```text
game_command_identity(game, renderer_id) -> 1 found / 0 stale-or-missing
game_command_identity_ptr(game) -> three i32 words [who, o, uid]

// caller stages plaintext bytes in the existing bounded game_cmd_ptr allocation
game_process_command_package(game, play:u32, lockstep_serial:i32, len:u32)
    -> 1 applied / 0 typed refusal
game_package_receipt_ptr(game) -> receipt words
game_package_receipt_words(game) -> exact receipt word count
```

The package entry point is immediate and atomic; it does not enqueue Group and Move in the
existing split command buffer. It refuses if split browser commands are pending. The JS wrapper
copies identity/receipt words only after each call (which may grow Wasm memory) and uses the
existing error-text export for a typed refusal.

The receipt word image is fixed-width header plus five words per selected Unit:

```text
header = [play, lockstep_serial, frame, who, group_slot, selected_len,
          command_state_revision_lo, command_state_revision_hi,
          groups_checksum, random_state_before, random_state_after]
unit   = [handle.id, handle.generation, who, o, uid]
words  = 11 + selected_len * 5
```

All words are little-endian Wasm `i32`/`u32`; revision halves reconstruct the receipt's `u64`.
The Handle pair is receipt evidence only and never becomes the next command's wire identity.

The Sim signature and receipt frozen by the movement host are:

```rust,ignore
Sim::process_command_package(
    play: usize,
    lockstep_serial: i32,
    bytes: &[u8],
) -> Result<GroupMovePackageReceipt, PackageError>

GroupMovePackageReceipt {
    play,
    lockstep_serial,
    frame,
    who,
    group_slot,
    selected: Vec<UnitIdentity { handle, who, o, uid }>,
    command_state_revision,
    groups_checksum,
    random_state_before,
    random_state_after,
}
```

The browser validates the receipt but does not retain its Handle as future wire identity.
`random_state_before == random_state_after` is mandatory for this package cohort. A typed
refusal is displayed and produces no ACK.

## Two-seat ordering and serials

The loopback gateway still admits exactly one command package per seat per open stamp. It now
validates either canonical shape, sends each seat's complete payload to the two native owners,
requires both native owners to return byte-identical ordered package sets, and publishes no
agreement until they do.

Network `NetMsg_CommandPackageData` carries `(stamp,play,payload)` and omits the replay-file
`group` field. Before execution that file field is a monotone package serial; it is not a Groups
slot. The two-seat browser owner derives it without conflating any of those domains:

```text
lockstep_serial = stamp * 2 + play + 1
```

Thus turn zero produces serials 1 and 2 in player order, turn one produces 3 and 4, and so on.
The gateway refuses before the result exceeds positive `i32`. The derived serial is attached to
each agreed package, so both tabs invoke the Sim host with the same exact arguments.

The fake two-process peer mutation tests return distinct 27-byte P0/P1 payloads and prove the
gateway preserves their bytes, order, and serials. This is a Node/gateway contract test, not a
claim that the compiled Rust `service-match-peer` has widened: its current validator remains
Halt-only. The public handoff consequently remains `canonical-halt-v1` until the native validator
and Wasm hooks both land and a browser smoke proves the complete path.

## Pause lock and ACK witness

The existing lock remains in force:

1. both browser worlds stop at the same frame;
2. ordinary commands and free-running steps are refused;
3. each seat submits one complete canonical package;
4. both native peers must publish the same ordered set;
5. each browser applies P0 then P1 through the whole-package Sim API;
6. each package receipt must match `(play,lockstep_serial,frame)` and the discovered
   `(who,o,uid)`, must carry the canonical Groups checksum witness, and must leave RNG unchanged;
7. only then does the browser advance exactly one frame; and
8. each browser ACKs the exact agreement hash plus post-frame state.

The ACK schema is now:

```text
{ stamp, agreementHash, frame: stamp + 1, digest: 16 lowercase hex, rngState: u32 }
```

The gateway rejects an ACK whose `agreementHash` is not the currently agreed native package-set
hash. It then requires both full ACK objects to be byte-equal before opening the next stamp. This
prevents a correct-looking state witness from acknowledging a different package set at the same
frame. Contradictory ACKs close the relay, as before.

## Gates and residual activation work

Focused local gates:

```text
node --test web/tools/canonical-command-package.test.mjs web/tools/local-match.test.mjs
15 passed, 0 failed
```

The five codec tests cover the exact retail fixture, owner-local discovery, signed coordinate
endpoints, Halt preservation, and mutation/refusal cases. The ten local-match tests include two
distinct 27-byte packages through both fake native owners and the complete gateway ACK barrier.

The full real-Chrome smoke also passes with `--local-match`. Chrome executed the served codec,
returned the exact 27-byte retail fixture, proved renderer id `7,405,568` remained distinct from
decoded `o=1`, and refused a width-byte mutation. The compiled Rust Halt relay then advanced both
real tabs from frame 0 to frame 1 with identical native hash `a688f2a5a1b1dc93`, digest
`85b325c89034b4b0`, RNG `309747810`, agreement-bound ACKs, and no console errors. This preserves
the existing live cohort while the Group+Move product path remains gated.

Activation remains evidence-gated on all of the following:

- the read-only renderer-id → `(who,o,uid)` Wasm export;
- a Wasm wrapper around the frozen one-shot Sim API and its complete receipt;
- widening the real native relay validator with an explicit capability/version handshake;
- two real browser tabs applying distinct packages, reaching equal receipt/state witnesses, and
  remaining pause-locked; and
- preserving the existing real-browser Halt smoke unchanged.

Until those are green, the frontend does not expose a synchronized move button and does not
overstate the prepared gateway as a live Group+Move service.
