# Object/selection command opcode recovery

Status: source-only proof pack, 2026-08-09.  No row in this document is counted as
dispatcher-complete until `CommandBridge` invokes it.

## Opcode 75: `RenameCityCommand`

`CommandPackage::process_rename_city` at `0x00944090` is 204 bytes and returns the fixed
wire size `0x35`.  PDB layout and instruction reads agree on the body:

| offset | type | field |
|---:|---|---|
| `+0` | `u8` | opcode `75` |
| `+1` | `i32` | owner `who` |
| `+5` | `i32` | object index `o` |
| `+9` | `wchar_t[22]` | raw city name |

The recovered instruction order is:

1. emit the sync diagnostic with references to `who`, `o`, and the name buffer;
2. load `Objects[who][o]`;
3. when `Object+0x08 & 0x20`, call the object's virtual slot `+0xac`, sign-extend
   `TypeData+0x72`, and assign the name into `Cities[who][city_type]+0x90`;
4. call `HotKeyGroups::find_group(o, who, ignored_third_argument)`;
5. if it returns a non-negative slot, call `HotKeyGroupOut::update_name(0)`.

The last two lines hide a serialized selection mutation.  `find_group` at `0x00714D00`
scans exactly eighteen slots starting at `who * 18`.  Before inspecting each slot it writes
`Group+0x30 = 1` and calls `Group::normalize` at `0x00711540`.  It accepts the first
post-normalize group whose `who` matches, whose addressed object has flag `0x01`, and whose
live signed-`i16` member prefix contains the full signed `o`.  A match stores current
`Game+0x550` at `Group+0x14` and calls `update_name(0)` inside `find_group`; the command
handler then calls `update_name(0)` a second time.

`systems/object_command_plans.rs` therefore keeps the raw 22 UTF-16 units, the `0x20` city
gate, the independent `0x01` selection-valid gate, all visited normalization receipts, the
frame stamp, and both presentation receipts.  It rejects reordered, missing, or surplus
host receipts.  Bounds refusal is explicitly a safe adapter availability rule: the retail
routine itself performs unchecked object-array indexing.

Current status: **planner-complete, dispatcher-partial**.  The remaining integration is an
atomic host method which executes and echoes the full `Group::normalize` mutations and the
city-name store, followed by a narrow opcode-75 arm in the shared bridge.  The planner does
not claim that its projected `NormalizedSelectionView` is the full serialized `GroupData`.

## Adjacent tail-row audit

The nearby rows are not relabelled as UI no-ops:

| opcode | handler | measured boundary | honest status |
|---:|---|---|---|
| 73 | `process_leader_options` `0x009441D0` | copies a 32-byte owner row, then enters technology/object recalculation and presentation tails | partial |
| 75 | `process_rename_city` `0x00944090` | city-type name mutation plus normalized hot-key selection refresh | planner-complete / dispatcher-partial |
| 77 | `process_cannon_time` `0x009464F0` | byte `+1` drives log/script work, then `TurnControl::start_cannon_time(Game.player[local]+0x77)` | partial |
| 78 | `process_console_cmd` `0x00943F30` | signed coords `+1/+5`, 256 UTF-16 units at `+9`, and `ConsoleWin::parse_cmd` | partial |
| 80 | `process_ungraceful_player_drop` `0x00943EA0` | bytes `+1/+2`; under `Game+0x820 & 0x10`, calls `DropControl::process_drop` | partial |

Opcode 77 reaches deterministic turn-control state, opcode 78 can reach arbitrary console
actions, and opcode 80 crosses the drop controller.  Typed boundary receipts alone would
not prove those callees, so they remain open rather than being converted into presentation
receipts prematurely.

## Evidence

- shipped PDB symbols and sizes: `schema/rise-procs.tsv`, `schema/rise-symbols.tsv`;
- field layout: `schema/command-structs.txt`, `schema/command-wire.json`;
- retail x86: `riseofnations.exe` ranges `0x00944090..0x0094415B` and
  `0x00714D00..0x00714E2B`;
- exact callees: `String::operator=` `0x00A1DC60`, `Group::normalize` `0x00711540`,
  `HotKeyGroups::find_group` `0x00714D00`, and `HotKeyGroupOut::update_name`
  `0x007152F0`;
- `cv search process_rename_city` found earlier wire/PDB inventory work but no prior
  behavioral implementation to recover.
