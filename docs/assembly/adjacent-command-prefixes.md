# Adjacent command-tail executable prefixes

Status: source-only recovery, 2026-08-09.  This cohort deliberately freezes both rows
as partial even though their pre-tail behavior is now executable.

| opcode | bytes | exact executable prefix | first open tail | closure |
|---:|---:|---|---|---|
| 73 | 33 | decode and copy the addressed 32-byte `LeaderOptionData`, preserving exact `BitMask<32>::operator=` behavior; compute the five observed change gates | object/unit cascades and local `my_leader_option` mirror | prefix exact / tail open / dispatcher open |
| 78 | 521 | signed coordinate decode, all 256 UTF-16 units, `ConsoleWin*` null gate, and stores to `ConsoleWin+0x518/+0x51C` | `ConsoleWin::parse_cmd(String, 1, 1)` | prefix exact / tail open / dispatcher open |

The same freeze is machine-readable in `ADJACENT_ROW_CLOSURE`; every row has both
`dispatcher_complete = false` and `whole_row_complete = false`.

## Opcode 73: leader options

`LeaderOptionsCommand+1` is a complete 32-byte `LeaderOptionData`:

| command offset | destination row offset | field |
|---:|---:|---|
| `+1` | `+0` | signed `who` |
| `+5` | `+4` | signed `peasants` |
| `+9` | `+8` | signed `peasants_wait` |
| `+13` | `+12` | signed `buildings` |
| `+17` | `+16` | 16-byte `BitMask<32>` |

The destination is `GameAccess::leader_options + 32*who`; the shipped PDB fixes this as a
ten-row `LeaderOption[10]`.  Retail snapshots old peasants,
buildings, and bits 1/3/4 of the inline flag byte, then stores the three scalar fields,
invokes `BitMask<32>::operator=` at `0x0042EF10`, and stores buildings last.
`BitMask<32>` is `{ i32 bits, i32 size, i32 flags, u8 inline[4] }`; its assignment copies
the three headers and exactly `size` inline bytes.  The safe prefix refuses sizes outside
`0..=4` because retail would otherwise overread the 33-byte command.

After the store, changed peasants/buildings and changed flag bits 1/3/4 launch multiple
object scans, stance calls, build/unit-data writes, and owner-dependent gates.  A local
player also copies the resulting row into `MiscAccess::my_leader_option` and replaces its
`who` with `-1`.  `LeaderOptionsCascadeRequest` freezes those branch facts but does not
pretend to execute the cascades.

Integration map: add a full LeaderOptions transaction to the world/`Fleet` boundary,
recover the object scans plus local mirror, then wire opcode 73 atomically.  The row store
must not be committed before a tail receipt is available when `open_tail` is present.

## Opcode 78: console command

The fixed body is `Coord mouse_x` at `+1`, `Coord mouse_y` at `+5`, and 256 raw UTF-16 units
at `+9`.  Retail always logs a `String` view.  Only when `MiscAccess::console_win` is non-null
does it store the two coordinates and invoke `ConsoleWin::parse_cmd(command, 1, 1)`.
`parse_cmd` can reach simulation-changing console actions, so this is not a UI no-op.

Integration map: product presentation owns the pointer/null gate and coordinate fields;
the console subsystem must return a typed parse transaction proving any simulation effects
before opcode 78 can become closure-green.

## Evidence

- wire/PDB: `schema/command-structs.txt`, `schema/command-wire.json`,
  `schema/pdb-types.json`, and `schema/rise-procs.tsv`;
- retail handlers: `0x009441D0..0x009449AB` and `0x00943F30..0x00944089`;
- first open callee for opcode 78: `ConsoleWin::parse_cmd` `0x007D6470`.
