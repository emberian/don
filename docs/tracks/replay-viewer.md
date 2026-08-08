# Replay viewer — recovered command-stream frontend

Status: **landed and corpus-tested, command-only**. The viewer decodes real `.rcx` files in
the browser and presents GameInfo/player metadata, player commands, camera paths, coordinate
orders, and the recorded checksum ledger. It does **not** reconstruct or execute the retail
world.

## Boundary

The page at `web/public/replay.html` has three deliberately separate layers:

1. `web/public/js/rcx.js` opens the gzip-or-raw container, parses the independent
   `GameInfo::walk_data`/Player prefix, finds the 18-byte package chain, applies the command
   transform, and splits all 82 wire opcodes.
2. `web/public/js/replay.js` builds a reversible-by-rebuild browser model of selection,
   camera, input counters, pause/speed, diplomacy, chat, resignation, coordinate markers,
   and recorded checksums.
3. `schema/replay-validation.json`, when packed, supplies the separate don-sim divergence
   result drawn in the lower half of the checksum strip.

The ~1 MB state/rules block between the Player prefix and command stream remains undecoded.
There is therefore no object table, terrain, starting world, unit motion, combat, economy,
or shipped-AI execution. Nine coordinate-bearing order shapes are plotted at their recorded
coordinates. Object-id orders are only annotated. The page does not import
`web/wasm/src/real.rs`; four opcodes have handlers in that separate prototype, but the viewer
labels that fact as **handler present, not run**.

## Evidence carried into the implementation

All format claims here are Tier C [measured], with their binary provenance cited at the use
site in `rcx.js`:

- container: gzip from offset zero when the gzip magic is present; older unfinished captures
  can be raw;
- GameInfo: `GameInfo::walk_data` `0x005d6570`, including the 30 setup bytes and eight Player
  slots;
- record writer/reader: `0x00952fb0` / `0x00952d90`;
- command transform: `CommandPackage::process_all` `0x0094c500`;
- opcode lengths: the 82-arm `CommandPackage::process` switch `0x0094a700` and the shipped
  PDB sizes.

The disk header is now named independently of the in-memory prefix. Word one is the global
`Game::frame`; word four is the monotone `CommandPackage::stamp`. Checksum reporters are
joined on the fourth word, while the timeline and seek transport use the first. Median
frames-per-turn is measured as frame delta / stamp delta.

Multiplayer decoding first tries the transformation derived independently from the plaintext
`GameInfo::seed`: `key = (seed >> 8) & 0xffff`, with the local padding RNG reset to the whole
seed per package. Several 2014-build files do not obey that relationship. They remain
readable through a frequency-ranked/padding-fit fallback, but the UI and checker label those
files `payload-fit fallback`; that result is structural recovery, not independent key
confirmation.

The old `roundTripExact` number was removed. It copied command slices from the decoded
plaintext and then applied XOR back to the same bytes, so it could not establish a true
round trip or validate unknown padding contents. The replacement evidence is stated at its
actual strength:

- package payload structurally tiles into known wire commands;
- header-derived versus payload-fit key provenance is explicit;
- every checksum packet independently satisfies `all == wrapping_sum(channels[0..15])`;
- every component checksum has Adler-32-shaped halves (`< 65521`);
- cross-player tuples joined on package stamp agree channel by channel.

The checksum row order is:

```
units builds walls ammo deaths groups guys leaders
cities items goods world rules scenario_data script_run_time
```

The sixteenth wire word is the aggregate `all` value and is checked, not drawn as a separate
DataWalk component.

## Corpus result — 2026-08-08

Command:

```sh
node web/tools/rcx-check.mjs
```

Measured on the 63 files under `ron-data/replays/`:

| measure | result |
|---|---:|
| actual command streams | 61 |
| packages | 1,296,194 |
| structurally tiled packages | 1,296,192 (99.9998%) |
| turns | 585,152 |
| commands | 5,055,253 |
| distinct opcodes observed | 59 of 82 |
| checksum packets with correct aggregate | 488,557 / 488,557 |
| checksum packets Adler-shaped | 488,557 / 488,557 |
| cross-player tuple comparisons identical | 265,619 / 265,619 |
| framing residue | 0 bytes |

The frames-per-turn distribution is `1:1, 2:1, 4:6, 6:37, 8:16`. The 1-frame file is the
named solo control `today.rcx`; the multiplayer distribution is therefore exactly
`2/4/6/8 = 1/6/37/16`.

Two corpus artifacts are not command streams. The 801-byte
`playback___2014.08.08_22_02_25__fri_.rcx` is an aborted header whose tail happens to form a
two-record/zero-command structural chain; it is shown as header-only and excluded from
command totals. The 4.7 MB raw
`playback___2014.08.12_19_59_21__tue_.rcx` ends in repeating state bytes and has no package
chain. The default corpus command recognizes that exact known artifact as non-fatal and exits
zero; passing it explicitly still exits one, so the allowlist cannot hide a new parser
failure.

The two untiled packages are the already measured short records at stamp 19,879 in
`playback___2014.04.26_15_32_51__sat_.rcx` and stamp 57,043 in
`playback___2014.08.08_20_54_48__fri_.rcx`. Each is one byte sequence too short for the
opcode it names; they remain visible anomalies and set the package-rate gate to 99.999%.

`rcx-check.mjs` also gates zero residue, metadata structure, checksum aggregate/shape,
cross-player equality, package tiling ≥99.999%, and a 30-second corpus budget. The observed
wall time was 3.0–10.9 seconds in consecutive local runs while other swarm lanes were active.

## Browser result

Commands:

```sh
node web/tools/pack-replays.mjs
node web/serve.mjs 8787
node web/tools/replay-smoke.mjs --json /tmp/don-replay-smoke.json
```

The default smoke chooses `today.rcx` by name (never by `xorKey`) and two multiplayer files
by the presence of recorded checksum packets. On this run all three pages passed:

| file | packages | commands | browser decode |
|---|---:|---:|---:|
| `today.rcx` | 10,544 | 11,946 | 271 ms |
| `Playback___2018.11.17_13_21_42__Sat_.rcx` | 36,140 | 140,666 | 269 ms |
| `Playback___2025.02.10_21_26_50__Mon_.rcx` | 22,646 | 87,647 | 276 ms |

The smoke navigates a real headless Chrome page, checks browser exceptions and console
errors, verifies coverage accounting and checksum invariants, seeks forward to 35%, forward
to 90%, then backward to 35%, and requires the rebuilt model counters to be byte-for-byte
identical. It also plays from frame zero. Chrome is closed and awaited before its temporary
profile is removed; a four-second TERM timeout escalates to KILL.

Replay-controlled filenames, player names, chat, setting labels, map/scenario names, and
decoder anomalies are escaped or inserted with `textContent`. Remaining `innerHTML` uses
only static markup, numeric fields, fixed generated opcode names, or strings passed through
the local escaper.

## Remaining limits

- Decode the save-state block and object table before calling this a world replay.
- Execute retail-equivalent orders in don-sim/WASM before claiming simulation playback.
- Replace the 2014 payload-fit fallback with a build-specific binary derivation if those
  recordings become validation inputs.
- A structural stream search can find a tiny empty chain in aborted data; the packer marks
  the known specimen `commandStream: false`, but the underlying locator is not a proof of
  semantic stream identity.
- The page visualizes the validation report; it does not itself recompute don-sim checksums.
- No claim here is Tier A or Tier B. The browser and corpus runs are behavioral/structural
  evidence against recorded retail output, not execution of the retail reader.
