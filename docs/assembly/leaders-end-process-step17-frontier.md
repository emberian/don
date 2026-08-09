# Tick step 17 — `Leaders::end_process_all` retail frontier

The isolated reconstruction in
`crates/don-sim/src/systems/leaders_end_process_step17.rs` models the complete deterministic
portion of retail `Leaders::end_process_all` at `0x006ED070`. Mutation-sensitive,
path-import tests live in `crates/don-sim/tests/leaders_end_process_step17.rs`; the module
does not depend on the broad `systems` module and is not yet wired into `tick.rs`.

## Authority and identity

The authority is the shipped PE32 executable and its GUID/age-matched private PDB:

| artifact | SHA-256 |
|---|---|
| `ron-bin/riseofnations.exe` | `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079` |
| `ron-bin/sbl/rise.pdb` | `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5` |

The PDB procedure is 549 bytes (`0x006ED070..0x006ED294` inclusive). Those bytes hash to
`03e7359cb131a49e11dee2a7019e227e525f3742a7556e9ffa478e6b6de0f961`.
The PDB also names the reached calls:

| VA | PDB symbol |
|---:|---|
| `0x00A1D590` | `String::String(String const&)` |
| `0x007E9AB0` | `MessageWin::add_feedback(String,int,Color&)` |
| `0x0097F770` | `SoundGlobal::play(SoundGlobalCat) const` |

## Exact cardinality and layouts

The PDB `Leaders` object contains ten `Leader` records, but this scheduled dispatcher walks
exactly **eight**. It starts with a field cursor at `leaders + 8 = 0x00E3A398`, adds the PDB
`sizeof(Leader) = 0x6EEC`, and loops while that cursor is below `0x00E71AF8`. Thus the
visited bases are `0x00E3A390 + i*0x6EEC` for `i = 0..7`; neither auxiliary record is
visited. Each slot is gated by `LeaderData::leader_flags & 2`.

The reached PDB fields are:

| owner | offset | field |
|---|---:|---|
| `LeaderData` | `0x08` | `who` |
| `LeaderData` | `0x7E4` | `pop_cap` |
| `LeaderData` | `0x7E8` | `pop_issues` |
| `Game` | `0x0C` | `info` |
| `GameInfo` | `0x25` | `pop_limit` (therefore `Game + 0x31`) |
| `GameInfo` | `0x38` | `player` (therefore `Game + 0x44`) |
| `Player` | `0x2C` | `pop_cap_frame` |
| `Player` | `0x30` | `flags` |
| `Player` | `0x33` | `who` |
| `Game` | `0x550` | `frame` |
| `Console` | `0x298` | `who` |
| `Console` | `0x2A0` | `play` |

`sizeof(Player) = 0x8C`. The zero-issue arm is physically unrolled across all eight records
at `Game + 0x44 + i*0x8C`, in index order. A record is selected by `(flags & 1) != 0` and
zero-extended byte `Player::who == LeaderData::who`. Retail always stores
`flags & 0xF7FF` for a match, even when bit `0x800` was already clear; the port and tests
retain that observable store.

## Population-warning ordering

When `pop_issues != 0`, only the leader matching `Console::who` continues. Retail indexes
the Player array directly with `Console::play`; the isolated headless form reports an
out-of-range value as a typed residual instead of reading arbitrary memory.

The cadence gate is exactly:

```text
elapsed = Game::frame wrapping-sub Player::pop_cap_frame
if signed(elapsed) < 450: skip
```

This comes from `sub; cmp 0x1C2; jl`, so 449 is blocked and 450 is due. Once due, retail
writes `Player::pop_cap_frame = Game::frame` **before** loading `GameInfo::pop_limit`,
resolving the category list, or comparing `LeaderData::pop_cap`. The model shares one
sequence counter across deterministic mutations, product reads, host-tail receipts, and
residuals so this ordering is machine-checkable. A missing category row therefore leaves
the due timestamp write intact.

The population row address is selected from `pop_limits`: `Categories::list` contains an
`ArrayBase<Category>::list` pointer at `+0x10`; `sizeof(Category) = 0x58`; and
`Category::data[0]` is at `+0x3C`. The shipped `rules.xml` values are
`[50, 75, 100, 125, 150, 200]`. Equality is capped: presentation is reached only when
`LeaderData::pop_cap < selected_limit`.

## Product-host boundary and residual

The below-cap tail is represented, not executed, by three typed receipts in exact retail
order:

1. copy the localized `String` at product text offset `0xC878` via `0x00A1D590`;
2. call `MessageWin::add_feedback` at `0x007E9AB0` with duration `-1` and PDB global
   `RED` at `0x00C8D248`;
3. call `SoundGlobal::play` at `0x0097F770` with category `0x5B`.

These receipts deliberately do not claim that a headless core owns localization, window
state, or audio. The two open product invariants are also explicit: `Console::play` must
select one of the eight Players, and `GameInfo::pop_limit` must select a supplied Category
row. No fallback Player, population cap, message, or sound result is invented.

## Verification and honest closure

The path-import suite mutation-pins the PE/PDB sizes and offsets, eight-address visit order,
outer gate, valid/identity Player selection, unconditional masked store, nonlocal branch,
449/450 and wrapping cadence behavior, timestamp-before-category ordering, equality cap,
the exact three-call host tail, continuation into later leaders after that tail, and typed
failure for both missing product invariants.

No compiler, formatter, or test runner was invoked in this reversal lane. Static
`git diff --check` is the lane-local validation; build and runtime validation belong to the
convergence lane. Runtime closure delta is therefore **zero** until this isolated module is
reviewed and integrated. Even after integration, the three presentation calls remain
intentional product-host receipts rather than headless implementations.

Root convergence passed both independent profiles on 2026-08-09: hbox
`tick-step17-frontier-20260809T223814Z-77934-6942-fede3d11b0b3` and persvati release
`tick-step17-frontier-release-20260809T223814Z-77937-9563-fede3d11b0b3`, each with 10/10
focused tests and exit 0. The module remains unwired, so the tick row remains red.
