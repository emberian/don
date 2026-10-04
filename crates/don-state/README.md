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
   Currently green: 100.0% consumed on all 53 captured saves.
2. **Checksums**: the 15 `CheckSums::check_*` channels (adler-32 word +
   bytes-walked) equal `manifest.json` for every captured frame —
   `tests/check_all.rs`. Currently **13/15 exact** on all 53 captured
   frames.

   Closed (exact): units, builds, walls, ammo, deaths, groups, guys,
   leaders, cities, items, goods, world, **rules** (typed
   `Rules`/`TypeRec`/`TribeRec` grammar now wired into load/save — the
   section is located by a self-authenticating scan: `0x92` tag, first
   record's serialized `type_index` == 0, full-width parse).

   Open (`OPEN` list in the test — asserted to still mismatch so a fix must
   remove them):
   - `scenario_data` — retail's `ScenarioData::walk_data` (0x00997ad0)
     checksums the *initialized* `Game::init` scenario state, not just the
     serialized `.svx` bytes. Ours: 8,320 B; retail: 8,453 B.
   - `script_run_time` — `RunTimeEnv::walk_data` (0x009c41a0) checksums
     initialized `ScriptFile` state (`FUN_009c63b0` records: tag +
     SimpleArray\<u8\> bytecode + pointer array + tagged hash list +
     SimpleArray\<int\> + gated path/name data). Ours: 4 B; retail: 52,029 B.

## Open problem: the tail interior

The tail after `final_globals` is now tri-partitioned (`RulesTail` =
`pre_rules` + `rules` + `post_rules`), byte-exact on all 53 saves. For
donf2 (decompressed offsets):

- `0x16d7f1..0x172821`: ~20.5 KB of pure zeros — owner section unidentified.
- `0x172822..0x25e9cf`: ~1.83 MB of script-VM serialization — a
  `ScriptFile` body parses forward at `0x172820` (tag + SimpleArray\<u8\>
  code + …); interior strings include `editor_scratch_file.svx`
  (`0x24f3d1`), `./scenario/scriptlibrary/general_powers` (`0x24f40f`),
  and trigger names (`city_build`, `one_farm`, …). Both the
  `ScenarioData::walk_data` virtual collections (`FUN_004c7060`,
  `FUN_004c7270`, `FUN_004c8070`, `FUN_004c8420`×8, `FUN_004c7db0`,
  `FUN_004c75e0`, `FUN_004c78b0`×16, `FUN_00473120`×16) and
  `RunTimeEnv`'s `ScriptFile` records live here — their serialized element
  bodies (`FUN_009d7ea0` virtual elements: Int/Float/String/Object/Array)
  are the remaining decode. Chained file parse drifts at file 1 — grammar
  is close but not exact.
- `0x25e9d0`: `RunTimeEnv` tag + count = **0** (empty on the save stream;
  retail's `script_run_time` channel walks 52,029 B of *initialized live*
  state — same files, checksum projection only).
- `0x25e9d5`: `final_globals` (tag `0xc2`, count 1, 341 B) ending exactly
  at `0x25eb34`.
- `0x25eb34..0x358c11`: **Rules** — 1,024,221 B, unique `0x92` candidate
  in the tail; all 806 serialized `type_index` values validate.
- `0x358c11..EOF`: ~840 KB trailer of dense i32-grid-like data
  (`0x20001` × 75 K, `0x420021` × 47 K). No walk after
  `Game::walk_rules_data` accounts for it — `do_save` 0x005a81f0 calls
  `WalkDataGame::walk_data` then only a flush. Owner unidentified; kept as
  `post_rules` opaque span.

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
