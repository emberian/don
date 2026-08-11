# don-crossplay — the checked ABI of `CrossplayProxy.dll`

`CrossplayProxy.dll` is Q-LOC's PlayFab bridge: lobby lifecycle, lobby
attributes, matchmaking, chat, and the Party P2P plumbing that
`CrossplayNetLib.dll` sends turns over. `riseofnations.exe` reaches all of it
through **two** imported free functions and then virtual dispatch.

This crate is that interface and **nothing else**. No method is implemented, no
DLL is built, nothing is replaced, and no live process is touched. It is the
foundation a future replacement would need, in the same shape
[`../netsys-shim/src/abi.rs`](../netsys-shim/src/abi.rs) took before that lane
built a DLL.

## The finding this lane exists to record

`CrossplayProxy.pdb` declares **two different interfaces both called
`ICrossPlayService`**, and only one of them is real:

| declaration | slots | offsets | is it the shipped vtable? |
|---|---|---|---|
| `CrossplayProxy::ICrossPlayService` | **58** | `0x000..=0x0e4` | **yes** |
| `Crossplay::ICrossPlayService` | 96 | `0x000..=0x17c` | no |

Three independent measurements agree that the 58-slot table is the one:

1. `CrossplayProxy::CrossPlayService` — the object the exported `Service()`
   returns — derives from `CrossplayProxy::ICrossPlayService` at offset 0.
2. `rise.pdb` and `CrossplayNetLib.pdb` declare *their*
   `Crossplay::ICrossPlayService` with a **slot-for-slot identical** layout:
   same 58 offsets, same names, same signatures, zero differences.
3. The pointer table the shipped DLL emits at `.rdata` RVA `0x9adc4` has
   **59 entries**: 58 that each resolve to the
   `CrossplayProxy::CrossPlayService::<declared name>` for that exact slot, then
   the class's own deleting destructor at `+0xe8`. Entry 60 is already outside
   `.text`.

The 96-slot record is a *newer vendor header* this build does not use. Its slot
0 is a destructor, slot 1 is `SetNew`, and `Init` — slot 0 in the real table —
sits at slot 3. Building an ABI from it would compile cleanly and be wrong in
every slot. It is emitted as `VendorICrossPlayServiceVtable` so the trap is
recorded in checked code rather than in prose; the extra `P2PEnableTcp`,
`SetP2PAllowedPorts` and matchmaking-ticket APIs it carries are what the vendor
SDK grew after RoN:EE shipped.

`docs/tracks/netcode-symbols.md` §3.2 already flagged this in prose. This lane
reproduces it from the PDBs independently and pins it with compile-time
assertions.

## What is in `src/abi.rs`

Everything is generated. Counts from the current run:

| | count | assertions |
|---|---|---|
| `ICrossPlayServiceVtable` | 58 slots, all with full `extern "thiscall"` signatures | slot index + x86 byte size |
| `ICrossplayLoggerVtable` | 4 slots | same |
| `ICrossplayPlayerVtable` | 14 slots (two `Send` overloads, kept distinct) | same |
| `INetworkClientVtable` | 15 slots | same |
| `VendorICrossPlayServiceVtable` | 96 slots, 3 opaque — **not the shipped ABI** | same |
| DTO structs | 15 | every field offset + total size |
| enums | 5, all 4-byte `int` | — |

275 compile-time assertions in total. The three opaque vendor slots are
`SetNew`, `SetDelete` and `P2PGetAllConnectedPlayers`: their by-value
`std::function` / `std::vector` parameter types are only forward-declared in
this PDB, so their sizes are not derivable here and are left explicitly absent
rather than assumed.

## Regenerating

Needs the local, gitignored `ron-bin/` (a legally owned install's binaries and
symbol store). The generator refuses to write if any check fails.

```sh
python3 crates/don-crossplay/gen/gen_abi.py

# adds a machine-code check of every derived argument size
uv run --with capstone python3 crates/don-crossplay/gen/gen_abi.py --check-ret
```

Checks the generator performs before writing:

- the 58-slot vtable is identical across `CrossplayProxy.pdb`, `rise.pdb` and
  `CrossplayNetLib.pdb`;
- slot offsets are contiguous and each holds exactly one method (a destructor
  slot may hold `~X` plus `__vecDelDtor`, which collapse to one pointer);
- every slot of the shipped `.rdata` vtable resolves to the matching
  implementation symbol, the deleting destructor follows it, and the next dword
  is outside `.text`;
- the 15 DTOs have identical size/field/base shape in every PDB that declares
  them;
- with `--check-ret`: each implementation's callee stack cleanup (`ret imm16`)
  equals the argument byte count derived from the PDB signature.
  **54 of 58 agree, 0 disagree, 4 indeterminate** (`SetJoinLobbyCallback`,
  `SetLeaveLobbyCallback`, `SetUpdateLobbyCallback`,
  `LobbyCancelPendingRequests` end in a tail jump, so a linear sweep finds no
  `ret`). This is the check that turns "the PDB says so" into "the shipped code
  agrees".

## Gates

```sh
tools/swarm-cargo crossplay-abi check --manifest-path crates/don-crossplay/Cargo.toml --lib
cd crates/don-crossplay && cargo check --lib --target i686-pc-windows-msvc
```

Both pass. The crate is excluded from the root workspace (see `../../Cargo.toml`)
exactly like `netsys-shim`, because `extern "thiscall"` only exists on x86.

The layout and slot-order assertions are written in `size_of::<*const ()>()`
units, so they are meaningful on the arm64 host too; the exact byte-offset and
byte-size assertions are additionally checked under `target_arch = "x86"`.

### The assertions are not vacuous

Mutation-tested by hand, each reverted immediately afterwards:

| mutation | result |
|---|---|
| swap the `StartGame` / `CancelGameStart` fields | fails on **both** i686 and arm64: `offset_of!(ICrossPlayServiceVtable, StartGame) == 22 * PTR_SIZE` |
| swap `LobbyDTO::_botCount` / `_attributeVersion` | fails: `offset_of!(LobbyDTO, bot_count) == 80` |
| shrink `MsvcUnorderedMap32` to 28 bytes | fails in three places, including `size_of::<LobbyDTO>() == 208` |
| delete the explicit 3-byte pad in `UpdateLobbyDTO` | **passes** — Rust's own `repr(C)` padding happens to coincide there |

The last row is the honest caveat: the emitted `_padN` fields document MSVC's
alignment gaps but are not themselves what pins the layout. The per-field
`offset_of!` and the total `size_of!` assertions are.

## What this crate deliberately does not claim

- **No behaviour.** A signature says how to call a method, not what it does.
- **Not verified.** These are PDB transcriptions cross-checked against shipped
  machine code — Tier C evidence about layout. Nothing here has been executed.
- **No live process** was read or modified, and nothing here authorises one.
