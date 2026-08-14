# 2024 frame-1 LeaderOptions chronology

Status: exact source and synchronized mutation plan; canonical Sim commit still open,
2026-08-13.

The actual Rust replay classifier, rather than the older loose Python package survey, finds
exactly two simulation commands before the selected replay's serial-64 frame-379 Group+Move:

| serial | live frame | play/who | opcode | exact effect selectors |
|---:|---:|---:|---:|---|
| 1 | 1 | 0 | 73 | peasants 1, buildings 2, flags byte `0x0b` |
| 1 | 1 | 1 | 73 | peasants 1, buildings 0, flags byte `0x2a` |

Both packages have exact shell `[LeaderOptions, TurnData, TurnData, Camera]`. The next
classified simulation package is serial 64 at live frame 379, whose exact shell contains the
already-owned Group+Move pair. There is no classified frame-205 Follow command in this replay.

`setup_2024_frame1_leader_options::discover_frame1_leader_options_source` pins the retail file
SHA-256, both complete 33-byte command wires and package coordinates, the shells, the absence of
any other pre-379 simulation command, and the frame-379 handoff. It uses no recorded checksum as
an input.

## Reached retail body

`LeaderOptions::init` `0x006F1D40` initializes all ten rows to:

```text
who = row ordinal
peasants = 0
peasants_wait = 2
buildings = 0
flags = { bits: 32, size: 4, flags: 0, inline: [0x0a,0,0,0] }
```

The existing executable prefix reproduces `process_leader_options`' row assignment and exact
`BitMask<32>` copy. Against that constructor image, play 0 changes peasants and buildings, play 1
changes peasants, and neither command changes flag bits 1, 3, or 4. Consequently none of the three
flag cascades is reached.

The remaining object scans are resolved from the shipped handler `0x009441D0` and
`UnitTypeData::get_stance_type` `0x0061D350`. Replay-carried Rules give:

| setup type | type index | stance type |
|---|---:|---:|
| Scout | 69 | 2 |
| Dutch Merchant | 62 | -1 |
| Citizen | 50 | 1 |
| starting Village | 414 | 1 (`BuildTypeData::get_stance_type`'s Village arm) |

The synchronized tail therefore writes stance 1 to Citizens `(who=0,o=3..6)` and the starting
Village `(who=0,o=2000)`. The buildings-change type-zero scan reaches no member of this setup,
owner 1 has no setup objects, and no other synchronized field changes. Copying the local player's
row to `MiscAccess::my_leader_option` remains presentation-only and is explicitly excluded from
the synchronized plan.

## Exact boundary

`plan_frame1_setup_leader_options` returns those five writes plus the before/after ten-row state.
It does not mutate a caller Sim, advance frame zero to frame one, or manufacture a frame-379
chronology authority. `bind_captured_frame1_command_entry` admits the one-tick retail capture only
when both whole-DoNSave hashes match, the executable/replay identities are exact, the first image
is the completed seven-call setup receipt, and all seven generation-bound Units survive into live
frame 1. It produces the command-entry authority; it does not derive that tick from replay bytes.

`mount_frame1_setup_leader_options` consumes that authority, rebinds all four Citizens by
generation and the Village through both object registries, validates every zero-stance
before-image, and commits the five writes atomically. It cannot relabel the setup receipt's
frame-zero snapshot.

After the serial-1 commands execute at live frame 1, the next exact boundary is the
**post-command frame-1 tick**, followed by entry ticks 2 through 378 to reach the frame-379
command boundary. Calling that interval merely “frames 2..379” would omit the first tick and
confuse the frame-379 command entry with its later frame-384 Groups pass.

Focused tests pin the real source, both row after-images and all five writes, and bite mutations
in either command wire, the classified simulation inventory, and every reached
`get_stance_type` decision gate.
