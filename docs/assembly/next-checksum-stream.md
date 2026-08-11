# `NextCheckSumCommand` `0x3a` — the corpus's second lockstep checksum stream

Lane: `nextsum`, 2026-08-11. Every number here is **[measured]** over the 61 recordings in
`ron-data/replays/` and reproducible with `cargo run --release -p don-replay -- nextsum
--corpus`. Nothing here is verified in the proof-assistant sense; see `docs/CHARTER.md`.

---

## 1. The finding, in one line

**39 of the 61 recordings carry a per-subsystem checksum stream that this repository had
never compared against, and not one of them carries a `CheckSumsCommand`.** The scoreboard's
"61 files, 21 with checksums" was true of `0x39` and false of the corpus: 60 of the 61
recordings carry lockstep checksums, in one of two mutually exclusive formats.

| stream | opcode | records | recordings | engine builds |
|---|---|---:|---:|---|
| `CheckSumsCommand` | `0x39` | 488,557 | 21 | `00.2017.11.2900`, `00.2024.06.2000` |
| `NextCheckSumCommand` | `0x3a` | **796,957** | **39** | `03.02.03.2905`, `00.2014.07.1000`, `00.2014.10.0200` |
| neither | — | — | 1 (`today.rcx`) | `00.2024.06.2000` (solo) |

The disjointness is measured, not assumed: `no_recording_carries_both_checksum_streams` in
`crates/don-replay/tests/next_checksum.rs` asserts `checksum_packets == 0` on all 39.

## 2. The record, and why the shipped binary never emits one

Six bytes — opcode, `checksum_type` (`u8`, `+1`), `checksum` (`u32`, `+2`).
`schema/command-wire.json` already carried the layout from the PDB type stream; nothing here
re-derives it.

`CommandPackage::process_next_check_sum` `0x00945e20` reads exactly those two fields, hands
them to `commandpackage.cpp`'s reporter, and — gated on `[0x00cc00b0] == 0` — stores the
value into `[0x00cbee90]` indexed by **the package's player**, `[this + 4]`, not by the type.
So the receive side keeps one slot per player, which is what a lockstep comparator needs and
not what a per-subsystem history would need.

The shipped `riseofnations.exe` still contains that receiver. It contains **no emitter**: a
full scan of the 102 call sites of the package-append helper `0x0094bae0` in
`re/decomp-all/` gives lengths 1, 2, 5, 7, 9, 10, `0xb`, `0xd`, `0xf`, `0x11`, `0x15`,
`0x19`, `0x35`, `0x41` and `0x209` — never 6. `CommandManager::issue_check_sums`
`0x00940770` is the only checksum issuer left and it appends `0x41` = 65 bytes, the `0x39`
tuple. That is why the two streams never coexist: the emitter was removed between the
`00.2014.10.0200` and `00.2017.11.2900` builds and replaced by the sixteen-word tuple.

## 3. `checksum_type` is `CheckSumTypes`, and it is not the wire word order

`schema/types.json` carries the PDB enum in full:

```text
CHECKSUM_ALL 0   CHECKSUM_RULES 1   CHECKSUM_UNITS 2   CHECKSUM_BUILDS 3
CHECKSUM_WALLS 4 CHECKSUM_AMMO 5    CHECKSUM_DEATHS 6  CHECKSUM_GROUPS 7
CHECKSUM_GUYS 8  CHECKSUM_LEADERS 9 CHECKSUM_WORLD 10  CHECKSUM_CITIES 11
CHECKSUM_ITEMS 12 CHECKSUM_GOODS 13 CHECKSUM_SCRIPT 14 CHECKSUM_NUM 15
```

Two traps in it. It **leads with `ALL` and puts `RULES` second**, so it is off by two from
`CheckSumsCommand`'s word order for most of its length and permuted at `world`/`cities` —
reading a `0x3a` record against the wire tuple's index silently compares the wrong channel.
And it has **no member for `scenario_data`**, which is word 13 of the tuple, so the enum is
not simply an older spelling of the same list. `CHECK_SUM_TYPES` in
`crates/don-replay/src/next_checksum.rs` is the binding; the test
`the_binding_covers_every_pdb_enumerator_and_no_more` pins both traps.

Observed values are exactly `0..=14` over all 796,957 records. `CHECKSUM_NUM` never appears.

## 4. The sweep, measured

Every `0x3a` recording emits **one record per player per turn, every turn, starting on turn
2**. Corpus-wide there are **0 non-contiguous turn steps** across 440 runs. The
`checksum_type` is non-decreasing in **38 of the 39** files, so the stream is a single sweep
`CHECKSUM_ALL → … → CHECKSUM_SCRIPT` that clamps on the last type and stays there for the
rest of the recording. The one exception restarts its sweep, and it is the recording that
desyncs on turn 3 (§6).

Each type occupies a contiguous **run** of turns. The run lengths, over every non-terminal
run in the corpus:

| type | channel | run length in turns |
|---:|---|---|
| 0 | `all` | **1 in 39 of 39** |
| 1 | `rules` | **1 in 38 of 38** |
| 2 | `units` | 19 – 2,119 |
| 3 | `builds` | 4 – 863 |
| 4 | `walls` | **1 in 30 of 30** |
| 5 | `ammo` | 1 in 23, else 10 – 8,944 |
| 6 | `deaths` | 1 in 23, else 49 – 5,349 |
| 7 | `groups` | **1 in 27 of 27** |
| 8 | `guys` | 19 – 2,485 |
| 9 | `leaders` | **9 in 25 of 25** |
| 10 | `world` | **1 in 25 of 25** |
| 11 | `cities` | 3 – 17 |
| 12 | `items` | 1 – 56 |
| 13 | `goods` | 21 – 223 |

### The run-length law: `elements + 1`

Two independent pins, and they are the reason this module can compare anything at all.

- **`walls` is empty in every recorded game.** The `0x39` corpus measures retail's own
  `walls` word at `1` on 222,938 of 222,938 comparisons. Zero elements, and the run is
  exactly one turn — so a subsystem with no elements still gets one turn.
- **`CheckSums::check_leaders` `0x009375a0` iterates exactly eight leaders.** Its loop runs
  `[0x00e3a390, 0x00e71af0)` in `0x6eec` strides, and `(0xe71af0 − 0xe3a390) / 0x6eec = 8`
  exactly. The `leaders` run is **nine** turns in all 25 recordings that reach it — in
  two-, three- and four-player games alike, so the 9 is not a player count.

8 + 1 and 0 + 1. The `ammo` and `deaths` runs behave the same way from the other side: one
turn in the 23 recordings where the subsystem is still empty when the sweep arrives, and
tens to thousands of turns where it is not.

**The one consequence this repo uses:** a run of length 1 contains no per-element record, so
its single record is the **whole channel**. That is confirmed twice — `walls`' single record
is `1`, the adler of nothing, in all 30 files; and `rules`' single record is `0x12ba3104`,
which `crates/don-replay/src/rules_channel.rs` independently produces by walking 997,846
bytes of the recording's own carried Rules SaveGame section (§5).

Whether the **last** record of a *longer* run is likewise the whole channel is **not**
established here, and nothing in the code assumes it. Runs longer than one turn are counted
as `per_element_records` and never compared. That is 796,489 of the 796,957 records, and
leaving them uninterpreted is the honest boundary: an element-indexed cursor over a list that
grows while it is being swept is a plausible reading of the long runs, but it is a reading,
and a wrong one would silently compare a single `Unit`'s walk against a whole channel.

## 5. What the stream says about the `rules` producer

`rules` is the only channel with both a one-turn run and a producer, so it is the only one
this stream can currently score. It gives a genuinely new test, because unlike the `0x39`
corpus — where all 21 recordings carry the identical `0x12ba3104` — the `0x3a` recordings
carry **nine distinct rulesets** on their uncontested records: `0x12ba3104`, `0x7e703235`,
`0x15d52ff2`, `0x6ff321f6`, `0xc356323a`, `0x5eb6449b`, `0x519d43c9`, `0xcced43d9`,
`0xf98de811` — plus `0x330a44eb`, which appears only in a recording whose own clients
disagree (§6).

| outcome | whole-channel turns |
|---|---:|
| our parser admitted the carried section, and it equals the wire value | **5** |
| our parser refused, and the wire value is **not** the shipped ruleset | 29 |
| our parser refused although the wire value **is** the shipped ruleset | 3 |
| retail's own clients disagreed on the value, so it was excluded | 1 |

**Zero false positives.** The producer never admitted a Rules section for a recording whose
own client reported a different ruleset — 29 opportunities to be wrong, none taken — and
never disagreed where it did admit. That is a fidelity statement about
`crates/don-replay/src/rules_channel.rs` measured on 39 recordings disjoint from the 21 it
was built against.

**And a named gap.** Three recordings (`Playback___2017.07.20_20_02_35`, `…_20_24_10`,
`…_20_46_23`) report `0x12ba3104` on the wire while `Replay::open` finds no Rules section at
all in them. They carry the shipped rules and we do not locate the block. Closing that is a
container-layout problem in the older recorder, and it can only raise the agreement count;
it is recorded here rather than papered over.

## 6. The corpus contains retail desyncs after all

`docs/tracks/replay-validation.md` records, correctly, that under the right join key the
`0x39` corpus shows **no** cross-player disagreement — 265,619 of 265,619 identical. That
statement is about `0x39`, and it stays true. The `0x3a` stream is a second control
experiment on a disjoint 39 recordings, and it does **not** come out clean:

```
445,347 comparisons, 445,332 identical, 15 disagreements
```

Every one of the 15 is within **two turns of its recording's last turn**, and they group
into three coherent events:

| recording | turns | first disagreement | channel |
|---|---:|---|---|
| `Playback___2017.07.14_23_34_08` | 4 | turn 2 `all`, turn 3 `rules` | p0 has a different ruleset |
| `playback___2014.04.26_16_11_55` | 3 | turn 3 `rules` | p2 has a different ruleset |
| `Playback___2017.07.14_23_35_56` | 33 | turn 31 `units` | simulation divergence |
| `Playback___2017.07.14_23_37_35` | 39 | turn 37 `units` | simulation divergence |
| five 2014 files | 1,467 – 14,623 | the recording's final turn, `script_run_time` | terminal record |

The two `rules` cases are the cleanest object lesson in the corpus: **two clients with
different rules data, detected on turn 3, and the recording is over by turn 4.** The
`0x3a` stream is a working desync detector and the recordings show it working.

The consequence for this repo is a scoping correction, not a retraction: "there is no retail
desync in this corpus" must read "there is no retail desync in the 21 `CheckSumsCommand`
recordings". On the other 39 there are four games that ended in one.

## 7. `world` reads `1` on this stream, in all 25 recordings that reach it

Retail's `world` word is **never** `1` in the 21 `0x39` recordings
(`retail_empty_compares = 0` over 222,938 comparisons). On the `0x3a` stream it is `1` in
every one of the 25 recordings whose sweep reaches `CHECKSUM_WORLD`.

The shipped mechanism that permits this is in `CommandManager::issue_check_sums`
`0x00940770` itself: the world walk is the one channel behind a guard,

```c
local_47 = 1;
if (*(int *)(PTR_DAT_00c06188 + 0x134) != 0) {
    FUN_006b5cf0(local_28, 0xffffffff);   // World::walk_data 0x006b5cf0
    local_47 = local_18;
}
```

so a null `[[0x00c06188] + 0x134]` leaves the channel at its initialised `1` without the
world being empty. Which of "the older builds gated the channel off" and "the older builds
had a different world walker" is true is **not** established here; the measurement is that
25 of 25 read `1`, and the guard is where to look.

## 8. What went on the scoreboard, and what deliberately did not

`schema/replay-validation.json` gains a `totals.next_checksum` section and a per-file
`next_checksum` object. **No existing number changed** — the regeneration was diffed field by
field against the previous record and the only difference is the new key.

Scoring rule, and it is deliberately narrow:

1. only one-turn, non-terminal runs are compared (§4);
2. a turn where retail's own clients disagreed is excluded, exactly as the `0x39` path does;
3. **a channel whose producer is not installed for that recording is not compared at all.**

Rule 3 is the one that matters. `walls`, `ammo` and `deaths` all read `1` on this stream in
the recordings where they are empty, and our absent producer also reads `1` — 76
whole-channel turns that would agree, every one of them vacuous. They are counted as
`no_producer`, not as matches. `an_absent_producer_is_never_credited_with_a_match` fails if
that ever changes.

| channel | records | whole-channel turns | compares | matches | note |
|---|---:|---:|---:|---:|---|
| `rules` | 88 | 38 | **5** | **5** | 997,846 bytes walked per compare |
| `groups` | 62 | 27 | 27 | 0 | frozen `Game::init` image vs a turn-300+ record |
| `world` | 58 | 25 | 18 | 0 | retail reads `1`, we walk 780k bytes |
| `walls` | 68 | 30 | 0 | 0 | no producer |
| `ammo` | 22,549 | 23 | 0 | 0 | no producer |
| `deaths` | 33,507 | 23 | 0 | 0 | no producer |
| `items` | 993 | 1 | 0 | 0 | producer not installed on that recording |
| `all` | 90 | 39 | 0 | 0 | no producer; see below |
| `units`, `builds`, `guys`, `leaders`, `cities`, `goods`, `script_run_time` | 739,542 | 0 | 0 | 0 | every run longer than one turn |

The five `rules` agreements are **substantive**: the walker hands the visitor 997,846 real
bytes on each, retail's own value there is never `1`, and the claim is falsifiable — it is
false for any recording carrying one of the other eight rulesets, which is exactly why the
producer refuses those. They are also **five**, on a channel that already had 222,938
comparisons, so they are not a headline; the fail-closed record over 29 foreign rulesets is
the part worth quoting.

The 27 `groups` and 18 `world` comparisons are **divergences, recorded on purpose**. The
`groups` producer is frozen at `Game::init` and the sweep reaches `CHECKSUM_GROUPS` several
hundred turns in, long after the first `GroupCommand`; it cannot hold and it does not. That
is a falsification opportunity taken, not a failure to report.

**`CHECKSUM_ALL` is deliberately uninterpreted.** `crates/don-replay/src/check_all.rs`
measured that `CheckSums::check_all` `0x00936560` *returns* its fifteenth channel while the
wire tuple's sixteenth word is the wrapping **sum**. Which of the two the older builds' `all`
record carries is not established, so all 39 of its whole-channel records are counted and
none is compared. Its values are in the per-file `next_checksum.sweep` record for whoever
settles it.

## 9. What this does not license

- It is **not** 796,957 new comparisons. It is 50, on three channels, plus 796,489 records
  whose meaning is deliberately left open.
- A one-turn-run record is the whole channel; a record inside a longer run is **not known
  to be**, and treating one as such would compare a single element's walk against a
  channel — the exact class of error the `Array<T>`-header hazard belongs to.
- The `rules` agreement is evidence about the projection and about those five recordings'
  carried bytes. It is no evidence about a rules *loader*: nothing here reads
  `ron-data/rules.xml`.
- The 15 cross-player disagreements are evidence that retail detected a divergence and
  stopped. They say nothing about *our* simulation, which took part in none of them.

## 10. Reproduce

```sh
cargo run --release -p don-replay -- nextsum --corpus          # stream structure + desyncs
cargo test --release -p don-replay --test next_checksum -- --nocapture
tools/replay-validate.sh                                        # regenerates the record
```
