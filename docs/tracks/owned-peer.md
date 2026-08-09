# Owned peer acceptance lane

Status: runnable synthetic second-peer acceptance. It does **not** join or mutate a retail
lobby.

## Run it

From the repository root:

```sh
cargo run --quiet --manifest-path tools/owned-peer/Cargo.toml -- --turns 50
```

Success is one JSON line with `"status":"pass"`. Failure is a JSON error on stderr and a
non-zero exit. The tool binds only `127.0.0.1`, creates two peers whose visible name is
exactly `Ai`, carries no credentials, and never reads or contacts the running retail game.

The acceptance is more than a socket ping. It fails closed unless all of these hold:

- the PDB-derived 70-byte `IPT_ADDPLAYER`, 34-byte `IPT_PLAYERLIST`, and 2-byte
  `IPT_READYFLAG` records encode/decode exactly;
- host-authoritative membership gives unique numeric IDs in slots 0 and 1 while both
  visible labels remain `Ai`;
- both peers cross the all-ready gate;
- the 46-byte `GameConnectionData` record round-trips, and the fields which retail really
  transports round-trip through the recovered PlayFab lobby-attribute schema;
- every turn crosses real TCP framing as two 73-byte `NETMSG_COMMANDPACKAGEDATA` messages;
- each message carries an exact 65-byte opcode-`0x39` `CheckSumsCommand`: 15 genuine
  Adler-32 values over deterministic synthetic preimages followed by their wrapping sum;
- both peers receive one package from each slot for every turn and finish with the same
  transcript hash.

The setup assertion is intentionally described as an **offline lobby-attribute roundtrip**.
The shipped dispatcher treats old setup message IDs 1–4 as dead; retail setup is PlayFab
lobby state, so sending the 46-byte record as a made-up TCP packet would overclaim fidelity.

## Why this cannot occupy the open retail Friend Game slot yet

The shipped PDB maps `CrossplayProxy::CrossPlayService::JoinLobby` to `0x10017360`, and the
DLL contains a real implementation there. Joining requires a second authenticated PlayFab
entity. This build obtains that identity through Steam login, so a second human member needs
a distinct Steam auth ticket/account. Reusing the already signed-in host identity is not a
second lobby member, and this repository does not collect, synthesize, or print credentials.

The two apparent credential-free shortcuts are conclusively absent in the shipped DLL:

| PDB symbol | VA | shipped body |
|---|---:|---|
| `CrossPlayService::P2PStartConnection` | `0x1001dc60` | `c2 08 00` (`ret 8`) |
| `CrossPlayService::CreateLocalPlayerLoopback` | `0x1001e350` | `c2 00 00` (`ret 0`) |

Those addresses come from `schema/CrossplayProxy-symbols.tsv`; the bodies are directly
disassembled from `ron-bin/dll/CrossplayProxy.dll`. The remaining blocker is therefore
exact: **a distinct owned Steam-authenticated PlayFab entity, plus the still-untested retail
shim/load ABI, is required for an evidentiary direct retail join.** No community-derived
value is used here.

## Evidentiary boundary

Passing this command establishes that the owned implementation can form a two-member
roster, synchronize readiness, exchange lockstep turns, and validate the retail checksum
shape through a real socket. It does not establish PlayFab discovery, Party transport,
retail match launch, or simulation equivalence. Those stay red until an explicitly approved
second-account run can test them without strangers.

