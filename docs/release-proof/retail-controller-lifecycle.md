# Retail controller lifecycle release evidence

This proof closes two narrow distribution-evidence gates with compact records derived only from
the repository's already documented 2026-08-09 measurement. It does not access a retail process,
retain retail bytes beyond the five-byte call instruction, infer why the earlier process exited,
or claim whole-product readiness.

## Active STOP/rearm lifecycle

`schema/live/retail-control-active-stop-cycles-v1.json` binds the exact
`docs/tooling/live-control.md` source by SHA-256 and records only the aggregate facts stated there:

- supported retail executable SHA-256 and runtime base;
- immutable `relaunch-v22` controller DLL SHA-256, one load base, and image size;
- the active paused-solo PID and scope;
- five consecutive STOP/rearm cycles plus a final acknowledged park on the retail main-thread
  boundary;
- `dropped_events=0` after every acknowledgement;
- the host's independent external read of `E8 45 67 3C 00` after every STOP; and
- zero new files in the scoped WER dump directory.

The source prose does not publish per-cycle timestamps, request identifiers, or separate read
digests. The compact record therefore does not invent them or expand one aggregate measurement
into five synthetic rows. Its exact cycle count and “after every” coverage are machine fields and
the verifier requires the matching measured statements in the hash-bound source.

## Incident closure without causal attribution

`schema/live/retail-control-stop-incident-closure-v1.json` binds both the original incident record
and the later lifecycle record. The original record remains unchanged: its dump was not retained,
the faulting stack is unavailable, and the preceding query or retired worker-thread unhook is not
claimed as a uniquely proved cause.

The closure conclusion is correspondingly limited. It proves that a SHA-bound later controller
candidate exercised the missing active-main-thread boundary for five consecutive STOP/rearm
cycles, restored the exact original bytes after every STOP, acknowledged the final park, dropped
no events, and produced no new file in the scoped WER dump directory. This is candidate-bound
reversibility soak evidence. It is not a retrospective root-cause finding and it does not turn an
absent minidump into stack evidence.

## Verification

Run the strict offline verifier:

```sh
python3 tools/release-proof/retail_control_evidence.py
```

Run its negative regressions:

```sh
python3 -m unittest tools/release-proof/test_retail_control_evidence.py
```

The main release-proof checker loads the same verifier. It rejects unknown fields, type aliases
such as integer `1` for boolean `true`, changed cycle counts or bytes, source/incident/lifecycle
hash drift, a changed controller DLL identity, a closure record without the lifecycle record, and
any closure that claims the original incident causality is resolved.
