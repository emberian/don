# The multiplayer command-package key is `GameInfo::seed`

Status: **derived from the binary**, 2026-08-10. Now read live by `crates/netsys-shim`,
announced to peers, and consumed by `tools/owned-peer`. Still Tier C: no live match has
decoded a turn with it yet.

## The question this answers

A live owned peer joined a retail-hosted lobby, crossed the two-member all-ready gate, and
received real in-match traffic, then stopped:

```text
could not recover the first retail command package key at stamp 1;
refusing to skip the authoritative first turn (supply --game-key)
```

`recover_game_key` ranks candidate XOR keys by ciphertext word frequency. That heuristic
needs zero-heavy payloads; the first observed packages were five bytes, so it had nothing
to rank. Guessing harder is the wrong repair — the engine does not guess.

There is a second, harder reason, found while implementing this and recorded in
[`retail-command-package-cadence.md`](retail-command-package-cadence.md): ranking accepts a
candidate only when the decoded stream contains a valid `0x39` checksum command, and the first
package of every match comes from `CommandManager::start` `0x00942E10`, which never calls
`CommandManager::issue_check_sums` `0x00940770`. **So ranking could not have settled that stamp
at any payload length.** The live key is not an optimisation; it is the only path.

## The derivation

`CommandPackage::send` `0x0094c1e0` computes the key inline. From
`re/decomp-all/0094c1e0.c`:

```c
uVar12 = (ushort)((uint)*(undefined4 *)(PTR_DAT_00c061ec + 0x10) >> 8);
...
(&DAT_00cc00c8)[iVar8] = *(ushort *)(puVar9 + iVar8 * 2) ^ uVar12;   /* the XOR loop */
```

So the key is `*(u32 *)(<global 0x00c061ec> + 0x10) >> 8`, which is already the shape
`don_net::obfuscate::Obfuscation::xor_key` implements as `(g >> 8) as u16`. The open part
was `g`.

The PDB names the global and the field, independently of any prose in this repo:

| evidence | value |
|---|---|
| `0x00c061ec` | `public: static class Game &GameAccess::game` |
| `Game::info` | offset **12** (`0x0C`), type `GameInfo`, size 1348 |
| `GameInfo::seed` | offset **4**, `unsigned long` |

`0x0C + 4 = 0x10`. **`g` is `Game::info.seed`** — the same 32-bit value the `.rcx` prefix
parser already recovers and the same one `Map::make` writes into `World::seed` and the
`game_random` LCG state word.

The exact instructions, confirmed byte-for-byte against the supported executable
(SHA-256 `30478a44…625079`) at RVA `0x0054c312`:

```text
0094c312  a1 ec 61 c0 00   mov   eax, dword ptr [0xc061ec]   ; GameAccess::game (Game*)
0094c317  83 c1 12         add   ecx, 0x12                   ; -> CommandPackage::data
0094c31a  0f bf fa         movsx edi, dx
0094c31d  8b 40 10         mov   eax, dword ptr [eax + 0x10]  ; Game::info(+12).seed(+4)
0094c320  c1 e8 08         shr   eax, 8
0094c323  0f b7 d8         movzx ebx, ax                      ; the u16 XOR key
...
0094c41c  ff 50 54         call  dword ptr [eax + 0x54]       ; NetSys::send
0094c42e  ff 50 58         call  dword ptr [eax + 0x58]       ; NetSys::send_all
```

## Consequences

- The multiplayer payload key is not an independent secret. One seed drives map
  generation, the main RNG stream, and command-package obfuscation.
- Frequency ranking stays useful for recorded `.rcx` streams where no live `Game` exists,
  but it is the fallback, not the primary path.

## How the joining peer learns it

The shim runs inside `riseofnations.exe`, so it reads the value directly; the joining peer is
a separate process on another machine and cannot. The chosen handoff is a **DoN transport
extension** — `don_net::extension`, packet id `0xF0`, six bytes, carrying the whole 32-bit
seed plus a provenance byte. It sits above the shipped `InternalPacketType` range (128..=136)
in its own namespace, deliberately *not* inside `don_net::internal`, because every id there is
recovered from `CrossplayNetLib.pdb` and none of it is ours to extend. `Session` consumes the
extension itself exactly as it consumes the shipped internal range, so it never reaches the
game layer: a `riseofnations.exe` on the far end of our DLL never sees one.

Three decisions worth stating, because each had a plausible alternative:

1. **Read it at send time, not at load or lobby time.** `crates/netsys-shim` reads the seed
   inside `ns_send` / `ns_send_all` when the outgoing packet is a `NETMSG_COMMANDPACKAGEDATA`.
   Those are the two slots (`+0x54`, `+0x58`) that `CommandPackage::send` dispatches to, six
   instructions after its own read of the same address, on the same thread, in the same call.
   That is the strongest correspondence available: the announced word is the word retail just
   used. It also disposes of the "is the seed valid yet?" question without a heuristic —
   there is no `Game` at menu time, and a lobby-time read could hand over a previous match's
   seed.
2. **Announce before the package, once per peer per match.** The transport preserves order per
   peer, so the key is on the wire ahead of the first package that needs it; a late joiner is
   told when it appears; and `NetSys::init` clears the record so the next match re-announces
   rather than inheriting.
3. **Announce the whole `u32`, not the derived `u16`.** The same word seeds both transforms:
   the XOR key is bits 8..23 and the inter-command pad generator is seeded with the full word
   (only its low 16 bits can change a pad). Bits 0..23 are therefore the equivalence class the
   wire can distinguish, which is why `tools/owned-peer` compares an operator-supplied
   `--game-key` against an announcement with `game_keys_are_wire_equivalent` rather than by
   raw equality — a ranked key is a representative of that class and can never carry bits
   24..31.

The read is gated the same way the two retail constructor calls are: `retail_executable_base`
must accept the image, and the twenty instruction bytes at RVA `0x0054c312` must match the
pinned stream after relocation adjustment. So the offsets are never applied to a build that
does not contain the code they came from. The `Game*` is then checked with `VirtualQuery`
before the 20 bytes up to `info.seed` are read; nothing else in the 3184-byte object is
touched. A failed check returns "unknown", never a guess.

For a host that is not the pinned retail executable there is no `Game` at all; `DON_NET_GAME_KEY`
supplies a key in that case, announced with provenance `configured`. It never overrides a live
read.

Nothing here has been executed against a live match yet; the key derivation is read off the
disassembly and the PDB layout, and is Tier C until a decoded live turn confirms it.
