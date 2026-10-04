# don-state

Retail-faithful container for *Rise of Nations* simulation/save state, built
around the engine's own `DataWalk` architecture: one set of per-class
`walk_data` functions implements `.svx` load, save, and lockstep checksum,
exactly as retail does. State objects are byte-image-backed typed records
(not Rust field structs); each `walk_data` is a literal transcription of the
retail function's op sequence so it can be audited line-by-line against the
disassembly (`re/decomp-all/`).

Zero third-party dependencies. The gzip container is decompressed by shelling
out to `gzip` (same pattern as `don-replay`).

## Gates

1. **Round-trip**: parse `donf2/3/4.svx` (decompressed) to EOF with zero
   residue and re-emit byte-identical bytes — `tests/roundtrip.rs`.
   Currently green: 100.0% consumed on all 55 captured saves.
2. **Checksums**: the 15 `CheckSums::check_*` channels (adler-32 word +
   bytes-walked) equal `manifest.json` for every captured frame —
   `tests/check_all.rs`. Currently **15/15 exact** on all 55 captured
   frames; the `OPEN` list is empty.

## Tail map (donf2, decompressed offsets)

Every section boundary from `GraphicEvents` to EOF is now reached
structurally and self-validated: each `walk_test` byte is checked against
`section_tag(name)` — the low byte of `String::generate_hash`'s
case-insensitive hash of the `internal_strings.xml` entry retail passes
(`SaveGame::walk_test` 0x0043d840 writes it, `LoadGame::walk_test`
0x0043da60 compares it). The misdecode that produced the earlier "zeros /
scan" tail was a 15,368 `GraphicEvents` slot count; the shipped install has
**60,415** slots (`first_ammo_piece + ammo_names.length`), and the
`AmbienceStruct` row is 23 B, not 24.

- `0x15f9f6` `GraphicEvents` (tag `0x7c`) … `0x1728da` `Scene` (`0xb7`),
  `Farms` (`0x26`), `UnbuiltWonders/Cities/Forts`, `ConquestGame` (`0x0c`,
  with 25 `Tribe` rows), then `DAT_00c0623c` (4 B) + **Camera** (`0x94`,
  228 + 468 B), `SelectGroups` (`0x76`), `Options` (`0xa8`),
  `CommandManager` (`0x59`), `Rivers`.
- `Terrain::walk_coord_data` 0x00850ef0: `2*(xs*ys-1)` B — one 2-byte
  `CoordInfo` flag word per tile, except the single tile in the list
  `TerrainOut::generate_land_lists` files at index `xs+ys-1`, which the
  walk's `xs-1+ys` bound skips.
- `MessageWin` (`0x58`), `Terrain::walk_data` (height floats + road lists),
  `CliffsData` (two 8-B heads each followed by a `count` payload — 20,000 B
  per-tile planes here), `Doober` (`0x66`; four trailing
  `SimpleArray<int>`), 8 B regions globals, `ObjectArray<Region>` (128),
  `Array<WCoordData>`, `Terrain::walk_roads` (flag byte per tile, 256 B
  when nonzero), 12 B, `Achieve` (`0xc1`, 6 `AchieveData` `0x95`).
- `0x24d422` `ScenarioData` (`0x04`): 8,102 B of direct globals, 6 Strings
  (`temp_save = editor_scratch_file.svx`, `general_powers_script_file =
  ./scenario/scriptlibrary/general_powers.bhs`), colours, and the
  component/message/objective/group/reveal collections.
- `0x24f531` `RunTimeEnv` (`"Script RunTimeEnv"` → `0x99`) + count **3** +
  3 × `ScriptFile` (`0x5c`): code `SimpleArray<u8>`, `PtrArray<Script>`
  (`"Script"` `0x50`: static vars, DynamicBitMask, params, refs, three
  name arrays, name, 12 B), const pool via `ScriptType::walk_array`
  (`"Script Vars Walk Array"`; elements `"Script Vars Walk"` + data_type /
  scope / ref_count + Int `0x57bad` / Float `0x12f35f` / String `0x168174`
  / Object / Array (scope bit 0x80) payloads, each with its own tag),
  linked files, then (save/load only) names, source path, `line_to_op`,
  break lines, 9 B. Ends at `0x25e9d5` exactly — this is the 52,029-B
  `script_run_time` channel.
- `0x25e9d5` `TurnControl` (`0xc2`) globals, 341 B tail.
- `0x25eb34..0x358c11` **Rules** (`"Game Rules"` `0x92`), 1,024,221 B,
  reached directly (the former tag scan is gone).
- `0x358c11..EOF` (840,784 B): `SaveGame::verify_save` 0x005a76b0 — called
  by `do_save` 0x005a81f0 through vtable slot +0x10 after `walk_data`. It
  drives a stack `CheckSum` (seed 1, mask -1) through each state walker and
  appends the 4-byte adler word after every sub-walk; per-tile (`wdata` rows,
  21 B), per-`tdata` u16 and per-fog `check_seen` loops make it 210 K words.
  **Transcribed** as `check_all::verify_save_words` (62 labelled steps):
  `save()` regenerates the trailer from the state tree and the loader
  recomputes it and rejects a stream whose trailer it cannot reproduce. Two
  CheckSum projections surfaced here and are now in the grammars:
  `GameInfo` under CheckSum skips the version string and the gated leading
  dword and hashes the +0x14 flags with bit 0 cleared; `Game` skips the
  semaphore/graphic_tick; `Docks` rows walk `[+0,+6)`, `[+8,+10)` then the
  gated `[+6,+8)`.

There are no opaque spans left: every byte of every capture is produced by a
typed walker, and the trailer is a function of the typed state.

## Tests

    cargo test -p don-state

`tests/check_all.rs` skips when `schema/live/frame-pairs/` (gitignored
proprietary captures) is absent.

## Simulation tick (`src/tick.rs`)

`tick::do_frame(&mut Save)` runs the retail `Game::do_frame` `0x00591ef0`
29-step schedule over the canonical state. Each `STEPS` entry is `Ported`
(whole body transcribed from the disassembly), `Partial` (some writes
transcribed), or `Stub` (no-op). Currently ported: `Game::frame++`
(`0x005924BF`), `Game::tick++` on `frame % 15 == 0` (`0x005924CF`), and
inside `GameDaemon::process_all` the `Groups::process` tail
`proc_group = (proc_group + 1) % 64` (`FUN_006fa210`). `Game::market_tick`
(`+0x564`) is owned by `FUN_00732180` and stays stubbed pending the rules
constants at `[0x00c061f0]+0xcd8..0xcec`. `Game::graphic_tick` is excluded
as nondeterministic — `FUN_00591570` derives it from `timeGetTime()`
66 ms quanta.

`src/bin/frame-burndown.rs <capture-dir>...` diffs each stride-1 pair
through `spandiff`: `retail_changed` / `explained` / `unexplained` /
`introduced` (must stay 0), the `game_random` LCG draw count per frame
(`FUN_00a39d70`, seed at post-World +40), channel matches, and writes
`frame-burndown.json` into the capture dir. `src/spandiff.rs` holds the
shared span alignment/attribution factored out of `svx-diff`.
