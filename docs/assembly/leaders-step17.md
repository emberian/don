# Leaders step 17 — end-of-process population feedback

`Leaders::end_process_all` at retail VA `0x006ED070` is now executable from tick step 17.
The port is in `crates/don-sim/src/systems/leaders.rs`; the real-schedule adapter is the
single `Sim::leaders_end_process_all` method in `crates/don-sim/src/tick.rs`.

## Recovered dispatcher

The 549-byte function walks the eight `LeaderData` records at `0x00E3A390`, stride
`0x6EEC`, and gates each record on `LeaderData::flags & 2`. The inlined per-leader body is
the same behavior as `Leader::end_process` at `0x006B8690`:

1. When PDB field `LeaderData::pop_issues` (`+0x7E8`) is zero, scan all eight
   `GameInfo::Player` records. A record with `flags & 1` and byte `who` equal to
   `LeaderData +0x08` receives `flags &= 0xF7FF`, clearing warning bit `0x800`.
2. With nonzero `pop_issues`, only the leader matching `Console::who` (`+0x298`) continues.
3. The selected `Console::play` (`+0x2A0`) Player is due only when signed wrapping
   `Game::frame - Player::pop_cap_frame > 0x1C1`; the first due elapsed value is 450.
4. Retail writes `Player::pop_cap_frame = Game::frame` before reading the selected
   population limit. That write therefore occurs even when `LeaderData::pop_cap`
   (`+0x7E4`) is already at the cap.
5. Below the cap, retail copies localized text at `text +0xC878`, calls
   `MessageWin::add_feedback` (`0x007E9AB0`), and calls `SoundGlobal::play`
   (`0x0097F770`) with category `0x5B`.

The shipped `poplimits` category in `ron-data/rules.xml` supplies, in order, the exact
values `[50, 75, 100, 125, 150, 200]`. The table base/stride recovered from the function is
`Categories::list + index*0x58`, reading `Category::data[0]` at `+0x3C`.

## Headless boundary and missing facts

Player flags and feedback stamps are deterministic state and are mutated directly. The
localized message window and audio call are presentation, so the core emits an inspectable
`PopulationCapFeedback` containing the leader/player identity and sound category. The most
recent call's events remain in `Leaders::end.last_feedback`; they are cleared on every call,
so an old notification cannot replay.

`Console::who`, `Console::play`, and the match's population-limit index are external inputs.
Invalid indices produce an explicit `EndProcessMissingFact` after all preceding retail-order
writes; no negative answer or fallback player is invented. The normal lightweight tick can
still execute the zero-issues cleanup path because its adapter refreshes only Player `who`
and valid bit from live Leader state while preserving warning flags and timestamps.

## Verification

Focused unit tests mutation-pin the PROCESS gate, all-eight Player scan, valid/matching
filters, 449/450 interval edge, stamp-before-cap ordering, nonlocal silence, missing-fact
behavior, shipped sound category, and no stale feedback replay. Integration tests in
`crates/don-sim/tests/tick_step17_end_process.rs` prove the real 29-step driver executes row
17 for an active leader and reports it vacuous for an empty population.

Evidence tier remains C: PDB/disassembly/decompiler plus checked-in shipped data, not a
retail oracle comparison.
