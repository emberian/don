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
   Currently green: 100.0% consumed on all three frames.
2. **Checksums**: the 15 `CheckSums::check_*` channels (adler-32 word +
   bytes-walked) equal `manifest.json` for frames 2–4 —
   `tests/check_all.rs`. Currently **12/15 exact** on all three frames.

   Closed (exact): units, builds, walls, ammo, deaths, groups, guys,
   leaders, cities, items, goods, world.

   Open (`OPEN` list in the test — asserted to still mismatch so a fix must
   remove them):
   - `rules` — the typed `Rules`/`TypeRec`/`TribeRec` grammar in
     `src/sections.rs` (tag + 806 dispatched type records + Constants 0xd40 +
     dup + Balance 493²×2 + 24 tribes) is fully transcribed from the
     disassembly but **not yet wired into load/save**: the stream offset
     where it starts is unknown because the preceding region is undecoded.
     The tail (`rules_tail`) is still an EOF-bounded opaque span.
   - `scenario_data` — retail's `ScenarioData::walk_data` (0x00997ad0)
     checksums the *initialized* `Game::init` scenario state, not just the
     serialized `.svx` bytes. Ours: 8,320 B; retail: 8,453 B.
   - `script_run_time` — `RunTimeEnv::walk_data` (0x009c41a0) checksums
     initialized `ScriptFile` state (`FUN_009c63b0` records: tag +
     SimpleArray\<u8\> bytecode + pointer array + tagged hash list +
     SimpleArray\<int\> + gated path/name data). Ours: 4 B; retail: 52,029 B.

## Open problem: the tail boundary

After `final_globals`, the decompressed stream contains (donf2 offsets):
- `0x169f22..0x172821`: ~35 KB of pure zeros — owner section unidentified
  (counted structures can't sit inside pure zeros; likely a fixed-size walk
  we under-model, possibly in Achieve/Scenario/RunTimeEnv or the conditional
  object at `WalkDataGame::walk_data`'s `local_58`).
- `0x172822..~0x253xxx`: script-state content — UTF-16 script/function names
  (`city_build`, `one_farm`, …) and path strings
  (`editor_scratch_file.svx` at `0x24f3cd`,
  `./scenario/scriptlibrary/general_powers.bhs` at `0x24f40b`), consistent
  with serialized `ScriptFile` records.
- `0x2d9ae7`: a verbatim copy of the runtime `Balance` table
  (`schema/live/final-balance-runtime.bin`, 486,098 B) — but ~878 KB of
  section data follows to EOF, so the rules tail is more than the
  checksum-order `types + constants + balance + tribes` layout or contains
  additional serialized region(s).

Resolving this boundary needs the `ScriptFile`/`RunTimeEnv` element
grammars (`FUN_009c5f30`, `FUN_009d7ea0`'s virtual element walk) decoded
far enough to parse records forward; once the runtime region is measured,
`Rules::walk` can replace the opaque span for load/save and drive the
`rules` checksum through the same function.

## Tests

    cargo test -p don-state

`tests/check_all.rs` skips when `schema/live/frame-pairs/` (gitignored
proprietary captures) is absent.
