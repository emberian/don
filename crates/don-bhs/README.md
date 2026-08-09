# `don-bhs`

The Big Huge Script bytecode VM, recovered from Rise of Nations: Extended Edition.

Full derivation, provenance and open questions: **`docs/tracks/bhs-engine.md`**.
Using BHS as a live instrument instead: `docs/tooling/bhs-bridge.md`.

## Why

`script_run_time` is one of the sixteen lockstep checksum channels
(`CheckSums::check_script_run_time`), and `RunTimeEnv::walk_data` /
`Script::walk_data` put script state on the same `DataWalk` interface as
`CheckSum`/`SaveGame`/`LoadGame`. Script state is therefore sim-critical, and no
scripted game can be validated against a replay without a faithful interpreter.

## Shape

BHS is source → bytecode → stack VM. The compiler runs once, offline, outside the
tick and can be *borrowed* from retail rather than rebuilt. The VM runs inside
`Game::do_frame` once per frame and must be ours. The builtins are the boundary to
`don-sim` and are owed regardless.

- **1 opcode byte, 0–2 unaligned little-endian `u32` operands.** 73 opcodes.
- **Variable references are tagged**: `0x20000000` = constant pool,
  `0x40000000` = script static, otherwise a local frame slot.
- **`static` locals are the entire cross-frame memory**, guarded by
  `OP_JUMP_IF_INITED`.
- **All arithmetic funnels through one virtual call**, `ScriptType::do_operator`.

## Regenerating the builtin table

`src/builtin_table.rs` is generated from `schema/bhs-builtins.json`, which is itself
a mechanical decode of the five `*FuncSet::init_funcs` bodies in
`ron-bin/riseofnations.exe`. 873 registrations, 813 distinct names, 36 overloaded.
Do not hand-edit either file; see `docs/tracks/bhs-engine.md` §4.1 for the decode.

## Fidelity

Tier C. Layouts, the opcode enum, operand counts and the builtin table are
`[measured]` from the binary and PDB. The arithmetic in `src/ops.rs` has **never been
run against the retail evaluator** and is the weakest part; `docs/tracks/bhs-engine.md`
§6 layer 2 is the design that fixes it. Nothing here is verified or proven.

## Coverage

Unimplemented builtins are recorded, not fatal: `Host::call` returns
`HostError::Unimplemented`, the VM logs `(index, name, count)` and substitutes the
engine's error return, so a workload yields an exact debt list instead of stopping at
the first gap.

```rust
let mut vm = Vm::new(&mut prog, &mut host);
vm.run_script(0, "tick")?;
print!("{}", vm.coverage.report());
```
