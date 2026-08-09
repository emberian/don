# RoNtoy host

RoNtoy host is the deliberately small Mac-side boundary for a live economy coach. It
accepts complete versioned readings, retains exactly one coherent snapshot, runs a
deterministic advisor, and pushes the newest result to a same-origin browser dashboard.
It does not attach to Rise of Nations itself.

The service uses only Python's standard library and refuses every non-loopback bind.
Each run creates a random ingest token. A Windows reader should default to 1 Hz while
we measure its effect on the game; the host's admission ceiling defaults to 20 Hz and
it allows at most 32 simultaneous HTTP connections. Every request must also carry an
exact `127.0.0.1:PORT` or `localhost:PORT` Host header, preventing a DNS-rebound web
origin from reading the otherwise unauthenticated local dashboard stream.

## Start it

From the repository root:

```sh
python3 tools/rontoy-host/server.py
```

The process prints a random `Snapshot ingest token`. Open
<http://127.0.0.1:17360/>. The host serves `web/public/rontoy.html` and its JavaScript
at the same origin as the event stream, with a restrictive CSP and no-store headers.
Nothing leaves the machine and the dashboard loads no external assets.

For dashboard/advisor work without a live process:

```sh
python3 tools/rontoy-host/server.py --demo
```

Demo readings say `session demo` and carry an explicit synthetic-data note. Their
numbers are examples, not reverse-engineered RoN constants.

Run all tests:

```sh
python3 -m unittest discover -s tools/rontoy-host -p 'test_*.py' -v
```

Verify that the committed cross-language fixture still equals the Rust encoder:

```sh
cargo run -q --manifest-path crates/donscan/Cargo.toml --example make_fixture
```

## Connect a reader

The canonical browser boundary is the host's JSON v1 envelope below. `donfeed` emits
strict `rontoy.observation` v1.0 NDJSON with coherent process, retail-image, mode,
pause, human, and economy evidence. [`bridge.py`](bridge.py) validates that envelope,
normalizes the engine's x16-per-30-game-second income to resources per game minute,
and POSTs canonical snapshots to the loopback host. It never records the raw feed.

The cross-language test consumes the Rust encoder's byte-checked golden at
`crates/donscan/fixtures/rontoy-observation-v1.ndjson` and pipes it through the real
bridge into the HTTP latest slot. This proves the reader-format → bridge → host path;
a live match still needs its own overhead/integrity check before being called proven.

The current Windows reader builds with:

```sh
cargo build --release --target aarch64-pc-windows-msvc \
  --manifest-path crates/donscan/Cargo.toml --bin donfeed
```

The safest first bridge has no guest network listener. Start the host, copy its
ephemeral token into a second Mac terminal, and pipe guest stdout directly:

```sh
export RONTOY_TOKEN='TOKEN_PRINTED_BY_THE_HOST'
prlctl exec "Windows 11" cmd.exe /d /s /c \
  '"C:\path\to\donfeed.exe" --hz 1 2>NUL' \
  | python3 tools/rontoy-host/bridge.py
unset RONTOY_TOKEN
```

Redirect guest stderr as shown so status lines cannot enter the NDJSON pipe. The
bridge accepts at most 64 KiB per line, stops after five consecutive failures, and
never writes captures to disk. `donfeed` itself defaults to 1 Hz and stops when the
target process identity changes.

If an HTTP bridge is more convenient, tunnel guest loopback to host loopback using
Windows OpenSSH (with macOS Remote Login enabled):

```powershell
ssh -N -L 17360:127.0.0.1:17360 ember@MAC_ADDRESS
```

Then, in another guest PowerShell:

```powershell
$token = 'TOKEN_PRINTED_BY_THE_HOST'
$headers = @{ 'X-RoNtoy-Token' = $token }
$body = Get-Content -Raw C:\path\to\snapshot.json
Invoke-RestMethod -Method Post -Uri http://127.0.0.1:17360/v1/snapshot `
  -Headers $headers -ContentType application/json -Body $body
```

The SSH connection reaches the Mac service as loopback. Do not change the service to
bind `0.0.0.0`; it will refuse to start if asked.

The Windows probe is constrained to:

- request only `PROCESS_QUERY_INFORMATION` or
  `PROCESS_QUERY_LIMITED_INFORMATION`, plus `PROCESS_VM_READ`;
- fingerprint the exact `rise.exe` module before using offsets and stop when that
  process exits or restarts;
- read only named economy fields, atomically assemble one frame, and never persist or
  upload heap dumps;
- start at 1 Hz and report probe CPU time, byte count, read count, and dropped frames;
- remain explicitly single-player-only until live-read overhead and integrity have
  been measured at 1, 5, and 15 Hz.

## HTTP surface

| Method | Path | Purpose |
|---|---|---|
| `POST` | `/v1/snapshot` | Admit one complete JSON snapshot; requires `X-RoNtoy-Token` |
| `GET` | `/v1/latest` | Latest admitted snapshot and its deterministic analysis |
| `GET` | `/v1/stream` | Server-Sent Events; current revision, then newer revisions |
| `GET` | `/v1/status` | Schema, uptime, latest age, admission/drop counters |
| `GET` | `/healthz` | Minimal liveness probe |
| `GET` | `/`, `/rontoy.html` | Canonical same-origin economy dashboard |
| `GET` | `/js/rontoy.js` | Canonical dashboard adapter |

`POST` is limited to 64 KiB by default. Duplicate JSON keys, non-finite numbers,
unknown schema fields, incomplete or mixed-frame captures, requests above the rate
ceiling, and sequence/capture/frame rollback within a session are rejected. A new
`session_id` or changed process fingerprint is rejected after the first admission;
restart the host explicitly to attach a new process. Repeated equal frames are also
rejected unless `paused` is true, so a stuck reader cannot keep stale advice fresh.
Population above its reported cap is legal and is retained rather than treated as a
corrupt reading.

There is no snapshot queue or history. SSE readers that fall behind skip directly to
the one latest revision. The status `age_ms` and `stale` fields expose freshness;
the dashboard polls them and replaces advice with `DETACHED / STALE` after three
seconds without a new snapshot. `rejections_by_code` makes drops visible.

## Snapshot schema v1

See [`example-snapshot-v1.json`](example-snapshot-v1.json) for a complete specimen.
The required core is:

```json
{
  "schema_version": 1,
  "source": {
    "session_id": "stable-per-rise-process",
    "sequence": 1,
    "captured_at_ms": 1786221000000,
    "process_id": 4120,
    "process_started_100ns": "133994400000000000",
    "module_sha256": "1111111111111111111111111111111111111111111111111111111111111111",
    "module_size": 12345678,
    "image_entry_rva": 1431193,
    "image_size": 12271616
  },
  "capture": {
    "frame_start": 15,
    "frame_end": 15,
    "complete": true,
    "advice_allowed": true,
    "duration_us": 250,
    "read_count": 12,
    "bytes_read": 256
  },
  "game": {
    "frame": 15,
    "player_id": 1,
    "mode": "single_player",
    "paused": false,
    "seconds": 1,
    "human_count": 1,
    "human_selection_basis": "unique_active_in_play_console_flags"
  },
  "economy": {
    "resources": {
      "food": {"stock": 100, "income_per_min": 60}
    },
    "population": {"used": 20, "cap": 25},
    "rate_sample": {
      "basis": "engine_direct_gather_cache",
      "gather_stamp_raw": 12,
      "age_frames": 3,
      "confidence": "direct"
    }
  }
}
```

Resource names are always in retail order: `food`, `timber`, `wealth`, `knowledge`,
`metal`, and `oil`. Per-resource `gatherers` are optional and are accepted only with
`gatherers_basis: "direct_count"`; gather-slot capacity/occupancy must never be
presented as worker allocation. Idle citizens have the same direct-only rule.

The host independently checks the direct gather-cache age using the wrapping 32-bit
`game.frame - gather_stamp_raw` difference. Age remains in simulation frames because
speed and pause break any fixed relationship to wall time. Sampled stock deltas are
spend-confounded and therefore observation-only. Goal advice requires a fresh
`engine_direct_gather_cache` reading with direct confidence, explicit reader
permission, and `single_player` mode.

The direct cache can legitimately remain unchanged for hundreds of frames; the
current 45-frame ETA cutoff is deliberately conservative and will often hide ETA.
`income_rate_too_old_for_eta` means “too old for this model,” not corrupt telemetry.
Population observations remain eligible when only rate-dependent advice is hidden.

Optional priority goals let the advisor estimate time-to-afford and identify
the modeled limiting resource. The v1 schema deliberately has no stock `capacity`:
RoN's commerce cap limits income, not stored stockpile, and pretending otherwise
would reject legal states and produce false advice. A commerce-cap field should only
enter a future schema after its live meaning and units are verified.

The advisor is a pure function of the admitted snapshot. It observes idle citizens
without pretending a safe work site exists, conditions population/queue pressure on
reported queue data, and identifies the highest-priority goal's modeled resource
bottleneck. Goal ETA is explicitly an optimistic `constant reported income, no future
spending` model—not an exact forecast. It uses no clock, random state, history,
network, or undocumented game constants, so the same snapshot produces
byte-equivalent advice.

This advisor is stateless scaffolding: it has no multi-frame confirmation, cooldown,
hysteresis, or in-game acknowledgement lifecycle yet. The browser should present its
output as observational/model-based. A non-single-player, paused, or unknown-pause
feed is instrument-only; no advice is emitted.

Production is optional and only accepted with
`queue_basis: "direct_build_queue"`. The current live aggregate attacking-unit
counts are not producer queues and must never be mapped to `queue_depth`.

## Operational checks

Before a live match:

1. Run the unit tests and start the host without `--demo`.
2. Confirm `/v1/status` has `latest: null` and save the printed ingest token only in
   the ephemeral collector process.
3. Start the exact-module-fingerprinted reader/bridge pipe above at 1 Hz.
4. Confirm sequence and frame move, host-monotonic `age_ms` stays fresh, and rejection
   counters stay flat. Guest `captured_at_ms` is informational and must never drive
   freshness across Windows/Mac clocks.
5. Compare Rise of Nations p95 frame time and probe CPU/bytes/read counts with the
   probe off, then at 1 Hz. Only test 5/15 Hz after the 1 Hz baseline is clean.
6. Stop the reader automatically when the process handle signals or its module
   identity changes. Restart this host before attaching another process. Stop it with
   Ctrl-C; no snapshot history remains to clean.
