# RL environment performance

This track measures engineering cost in `don-env`; it does not claim or raise a simulation
fidelity tier. Optimizations here must preserve simulation dynamics, action legality, output
bits, deterministic reset/step behavior, and the product-readiness gate.

## Reproducible workload

`crates/don-env/src/bin/envbench.rs` times three complete release-mode operations:

- `step`: action decoding, one simulation frame, rewards, entity selection, all observation
  planes, and all packed masks;
- `reset`: deterministic world reconstruction plus complete output refresh;
- `sample`: the reference masked action sampler over every unit and player head.

The fixed workload is a 64x64 grid, 64 entity rows, 32 controlled rows, two agents, 16
starting units, one frame per step, no episode cutoff, and fog disabled. The timed step uses
legal NOOP actions so every output writer runs without conflating this benchmark with a
particular bot policy. Run it with:

```sh
DON_ENV_BENCH_THREADS=8 DON_ENV_BENCH_BUDGET=0.50 \
  DON_ENV_BENCH_SAMPLES=7 cargo run --release -p don-env --bin envbench
```

The numbers below were captured on an Apple M2 Max (8 performance plus 4 efficiency cores,
96 GiB, arm64, macOS 26.6). Eight workers avoid using the efficiency cores. Each number is
the median of per-operation timings from independent half-second samples; the tool also
prints best-to-worst spread so noisy runs are visible. The 256-world result was measured as
an adjacent optimized/baseline/optimized/baseline sequence and averages the two medians for
each executable.

The frozen baseline executable was built at `fc7e2b0` immediately before the hot-path edits,
with the same benchmark and memory instrumentation, and has SHA-256
`85351a0c61a6796f540128daf4b90648e29cf082e02e91b30d4ca8c129cd179d`.

## Profile-guided candidate capture

The following isolated capture guided the implementation. It included an optimization that
left source-less spatial planes at their allocation-time zeroes. A final host-alias audit
found that Python's zero-copy arrays are writable, so that optimization was rejected: every
plane is again reconstructed on every call. These numbers are retained as experiment data,
not mislabeled as the current product's throughput. Reset, sampler, scratch-reuse, and
determinism changes remain in the final source.

| worlds | operation | baseline ms/batch | optimized ms/batch | optimized env/s | result |
|---:|---|---:|---:|---:|---:|
| 1 | step | 0.048 | 0.014 | 71,400 | 3.43x |
| 1 | reset | 0.048 | 0.019 | 52,600 | 2.53x |
| 1 | sample | 0.029 | 0.009 | 111,100 | 3.22x |
| 16 | step | 0.234 | 0.161 | 99,300 | 1.45x |
| 16 | reset | 0.273 | 0.254 | 63,100 | 1.07x |
| 16 | sample | 0.164 | 0.124 | 129,000 | 1.32x |
| 64 | step | 0.437 | 0.430 | 148,800 | 1.02x |
| 64 | reset | 0.831 | 0.525 | 121,900 | 1.58x |
| 64 | sample | 0.269 | 0.214 | 299,100 | 1.26x |
| 256 | step | 1.607 | 1.207 | 212,100 | 1.33x |
| 256 | reset | 2.949 | 1.859 | 137,700 | 1.59x |
| 256 | sample | 0.607 | 0.591 | 433,200 | 1.03x |

The one- and 64-world rows are adjacent five-sample captures. The 16-world row is a
seven-sample retest after moving small resets back to the serial path; its spread was at
most 1.12x. The 256-world ABBA medians were 1.153/1.260 ms optimized versus
1.669/1.545 ms baseline for step, and 1.819/1.899 versus 2.956/2.942 ms for reset. Masked
sampling at 256 worlds was effectively unchanged within run noise and is reported that way,
not as a meaningful speedup.

After restoring full output reconstruction, the final one-world source measured
0.017/0.023/0.009 ms for step/reset/sample versus the baseline's
0.048/0.048/0.029 ms. A trustworthy final eight-worker batch capture is still required on
an idle host: the shared development machine was at load 13-18 with the arena tests, retail
VM, and decompiler lanes active, and observed sample spreads reached 6.38x. Those contended
measurements are intentionally not promoted into an “authoritative” after table.

The optimized performance snapshot reserved 135.35 MiB at 256 worlds: 33.62 MiB of worlds,
100.36 MiB of returned outputs, 0.63 MiB of sampler buffers, 0.14 MiB of worker workspace,
and 0.60 MiB of shared rules. Caller-owned action arrays add 0.63 MiB. The baseline reported
135.03 MiB, so retaining scratch costs 0.32 MiB (0.24%) while eliminating transient hot-path
allocations. The executable patrol queue landed concurrently after that isolated snapshot;
the current benchmark accounts for each queue and nested waypoint allocation under `world`,
rather than incorrectly attributing that product state to the performance workspace.
With those queues present, the current product reports 139.94 MiB of reserved payload and a
short `/usr/bin/time -l` page-warming run peaks at 155.7 MiB RSS, versus 150.4 MiB for the
frozen baseline. The 4.77 MiB `world` increase, not the 0.14 MiB performance workspace,
explains nearly all of that later product-memory difference.

## What changed

An Apple `sample` profile of a 256-world step found observation/mask writers and entity
selection at the top of worker stacks, with repeated allocator traffic. The main thread also
spent material samples in thread creation. The optimized implementation therefore:

- retains one mask writer, entity-selection buffers, occupancy plane, and statistics block
  per configured worker;
- reuses control/observation handle vectors and fixed-size reward snapshots;
- removes the reward path's temporary list of living players;
- executes one-worker step and sampling calls directly, without creating a scope/thread;
- parallelizes reset only above the measured crossover and reuses sampler-shaped buffers for
  reset's zero actions;
- seeds reset and masked sampling by global world index, so worker chunking cannot alter the
  random stream.

A Rayon conversion was built and measured, then removed: at eight workers it regressed a
64-world step from 0.528 to 0.739 ms and a 256-world step from 2.077 to 2.295 ms in adjacent
runs. There is deliberately no new runtime dependency in this lane.

## Semantic gates

`don-env` tests compare complete environment images across one, two, four, and eight workers:
simulation digests, entity columns and handles, every floating-point output by exact bits,
packed masks, sampled actions, flags, and action statistics. A reset test independently
compares serial and parallel reconstruction. A captured pre-optimization fingerprint locks
the changed spatial and unit-mask writers to:

```text
spatial = 0xd9ea9dc5
unit masks = 0x5dc29c21
```

The standalone benchmark repeats those checksums for every batch size; old and new
executables matched at 1, 16, 64, and 256 worlds. These are equivalence gates, not evidence
that an incomplete simulation subsystem is retail-faithful; the normal fidelity/readiness
gate remains authoritative.

## Remaining limits

Returned observations are about 74% of reserved batch memory at 256 worlds, so future shape
growth matters more than worker scratch. Multi-worker calls still create scoped native
threads per operation; a persistent executor is worth revisiting only with a design that
beats the measured implementation without changing scheduling semantics. The wide 806-type
mask head remains the sampler's dominant scan. Fog-enabled observation cost, Python/NumPy
view overhead, policy inference, and host-to-accelerator transfer are intentionally outside
this benchmark and need their own end-to-end measurements.
