# The multiplayer command-package key is `GameInfo::seed`

Status: **derived from the binary**, 2026-08-10. Not yet consumed by `tools/owned-peer`.

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

## Consequences

- The multiplayer payload key is not an independent secret. One seed drives map
  generation, the main RNG stream, and command-package obfuscation.
- `tools/owned-peer` should take the key from the match's `GameInfo::seed` rather than from
  ciphertext ranking. `--game-key` already exists for supplying it.
- The shim runs inside `riseofnations.exe`, so it can read
  `*(u32 *)(*(u32 *)0x00c061ec + 0x10)` directly after rebasing by the ASLR delta — the
  same bounded-read technique used in
  [`netsys-retail-identity-relocation.md`](netsys-retail-identity-relocation.md). Reporting
  it through the existing trace would let a joining peer be handed the real key instead of
  inferring one.
- Frequency ranking stays useful for recorded `.rcx` streams where no live `Game` exists,
  but it should be the fallback, not the primary path.

Nothing here has been executed against a live match yet; the key derivation is read off the
disassembly and the PDB layout, and is Tier C until a decoded live turn confirms it.
