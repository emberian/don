# `CrossplayProxy.dll` — the replacement image

**Track owner:** crossplay-dll lane (wave 3). **Scope:** turning
[`crates/don-crossplay`](../../crates/don-crossplay/README.md) from a checked
ABI plus a local backend into a **loadable PE32 image** with the shipped export
surface. Companion tracks: [`crossplay-abi.md`](crossplay-abi.md) (the 58-slot
interface) and [`crossplay-local-backend.md`](crossplay-local-backend.md) (the
DoN-owned service behind it). Precedent:
[`crates/netsys-shim`](../../crates/netsys-shim/README.md), which did the same
for `CrossplayNetLib.dll`.

**[measured]** means read out of a shipped PE, a shipped PDB, a disassembly, or
observed in an executed process during this lane. **[DoN policy]** means a
decision this project made and nothing in the shipped binaries constrains.

**Nothing was loaded into retail.** No `riseofnations.exe` was started, no
process was read or written, and nothing was installed into a game directory.
Every runtime result below comes from a disposable Wine prefix running a
purpose-built PE32 loader. A live retail load needs explicit per-run
authorisation and is out of scope for this lane.

---

## 1. The export surface, and why two of the four are data

`ron-bin/dll/CrossplayProxy.dll` exports exactly four names. **[measured]**

| ord | name | RVA | kind |
|---|---|---|---|
| 1 | `?Logger@Logging@Crossplay@@YAPAVICrossplayLogger@12@XZ` | `0x12180` | code, `__cdecl`, bare `ret` |
| 2 | `?Service@Crossplay@@YAPAUICrossPlayService@1@XZ` | `0x130e0` | code, `__cdecl`, bare `ret` |
| 3 | `AmdPowerXpressRequestHighPerformance` | `0xb2048` | **data**, `.data`, `= 1` |
| 4 | `NvOptimusEnablement` | `0xb2044` | **data**, `.data`, `= 1` |

Ordinals 3 and 4 are the standard hybrid-GPU hints. A vendor driver finds them
by walking the process's export tables and reads a `DWORD`; publishing them as
functions would put the right address in the table under the wrong kind. The
replacement emits them as `static`s wrapping an `UnsafeCell<u32>`, which is what
places them in a writable `.data` section rather than `.rdata`, and
`build.rs` marks both `/EXPORT:…,DATA`.

`riseofnations.exe` imports ordinals 1 and 2 **by name** from its import
directory at `0xac5038` and `0xac503c` and resolves nothing here with
`GetProcAddress`. A `.text` scan for `call dword ptr [0xac5038]` finds **15**
call sites and for `[0xac503c]` finds **75**. **[measured]**

### 1a. Getting decorated C++ names out of a Rust cdylib

`#[export_name = "?…@Z"]` does not survive into a cdylib's export set on
`i686-pc-windows-msvc`; that was measured across all ten of `netsys-shim`'s
names. `crates/don-crossplay/dll/build.rs` emits explicit
`/EXPORT:exported=internal,@ordinal[,DATA]` link arguments instead. Two details:

* the alias target is spelled **without** the i386 leading underscore, because
  lld-link prepends it when resolving `/EXPORT:a=b`;
* the `@N` ordinals are explicit. lld-link assigns unspecified ordinals *after*
  the highest specified one, so the four `proxy_*` internal names Rust also
  exports land at 5..8 and cannot displace the shipped four.

`crates/don-crossplay/dll/check-exports.py` is the gate: name and ordinal parity
against the shipped DLL, PE32/i386/`IMAGE_FILE_DLL` identity, image base
`0x10000000`, both code exports in an executable section ending in a bare `ret`,
both data exports in a writable section holding `1` **in both images**, and the
shared-UCRT import described in §3.

---

## 2. The bug the runtime gate caught, which every static check passed

This is the headline result of the lane.

`abi.rs` declared

```rust
#[repr(C, align(8))]
pub struct MsvcFunction { pub storage: [u8; 36], pub target: u32 }
```

The `align(8)` was hand-written in the generator's prelude, not derived from any
PDB; every other MSVC aggregate in the file is `align(4)`. All 275 compile-time
layout assertions passed, `gen_abi.py --check-ret` still reported 54 slots
agreeing with the shipped `ret imm16`, the crate's 42 host tests were green, and
the DLL linked clean with exact export parity.

It is still the wrong calling convention. On `i686-pc-windows-msvc` rustc passes
an aggregate whose alignment exceeds 4 **indirectly, as a pointer**, and one
with alignment 4 **pushed on the stack**. MSVC does the same. So the emitted
`ICrossplayLogger::Register` opened with

```text
0x176e4  mov edi, dword ptr [esp + 0x18]     ; load a pointer from the argument
...
0x176e?  ret 8
```

where the shipped `Register(LogLevel, std::function)` pops **44**
(`4 + 40`). **[measured — `CrossplayProxy.pdb`]** The first time
`riseofnations.exe` installed a log sink — `0x0056143d`, which builds the
40-byte `std::function` on its own stack and calls vtable `+0x04` — the callee
would have popped 8 of 44 bytes and returned into a destroyed frame.

`crossplay-load-smoke` found it as a page fault on read of `0x00000024`
(`MsvcFunction::target`'s offset, read off the zero word the callee mistook for
a pointer). After correcting the alignment, the same thunk opens
`lea edi, [esp+0x18]` and ends `ret 0x2c` = 44, matching the PDB, and the smoke
passes. **[measured, both states]**

Three things now hold the line:

* `align_of` assertions on `MsvcFunction`, `MsvcWstring`, `MsvcString` and
  `MsvcVector`, in `src/abi.rs` **and** in `gen/gen_abi.py`, so a regeneration
  cannot reintroduce it;
* the note above each one saying the alignment is part of the calling
  convention;
* the PE32 smoke, which crosses by-value `std::function` and by-value `wstring`
  parameters with an ESP check on every call.

The general lesson is the one this project keeps re-learning: a layout assertion
constrains the *bytes of a type*, not the *convention a signature is lowered
to*. Only executing the call tests the second.

---

## 3. `std::function` ownership: the open item, closed

The backend lane left this open, correctly: retaining a caller's
`std::function` by copying its 40 bytes is right only when the target lives in
the object's own inline buffer, and closing it means executing guest code —
`_Copy` (vtable slot 0) and `_Delete_this` (slot 4).

### 3a. Who destroys what, measured on both ends

MSVC x86 is a **callee-destroys-parameters** ABI:

* `riseofnations.exe` `0x0056147c` constructs a `std::function` in a 40-byte
  stack slot with unwind state `0x1d`, writes state `4` — "not mine to unwind" —
  and only then executes `call dword ptr [eax+4]`, `ICrossplayLogger::Register`.
  The same idiom at `0x004fefa3` releases the by-value `wstring` one instruction
  before `call dword ptr [eax+0xc]`, `Log`. **[measured]**
* the shipped `CrossplayNetLibSys::set_p2p_callbacks` at `0x10017420` does the
  mirror image and returns `ret 0x78` = three 40-byte objects. **[measured, by
  the netsys-shim lane]**

In the shipped 58-slot table every `Set*Callback` setter takes
`const std::function&` and every request-completion pair takes it **by value**
(`CreateLobby`, `GetLobby`, `FindLobbies`, `StartGame`, `CancelGameStart`,
`UpdateLobby`'s eighth argument, and the stats/leaderboard family).
**[measured]**

### 3b. Why a cross-module `operator delete` is sound here

`_Delete_this` is the *caller's* code: the vtable in the target belongs to
whichever module constructed the object, so calling it runs that module's
deallocator. It is additionally safe because the process has one heap:
`riseofnations.exe`, `CrossplayProxy.dll` and `CrossplayNetLib.dll` all import
`MSVCP140.dll`, `VCRUNTIME140.dll` and `api-ms-win-crt-heap-l1-1-0.dll` — the
shared UCRT, not a statically linked CRT each. **[measured — import directories
of all three shipped images]** The replacement imports the same
`api-ms-win-crt-heap-l1-1-0.dll`, and `check-exports.py` gates on it.

### 3c. What was built

`crates/don-crossplay/src/func.rs`:

* the six measured `_Func_base` slots as named constants;
* `retain` (by reference: `_Copy` into our storage, `_Move` if the copy landed
  inline, destroy whatever we held), `destroy` (`_Delete_this` with MSVC's own
  `target != &self` deallocate test), and `adopt` (by value: retain then destroy
  the argument);
* `OwnedFunction`, a `Box`ed 40 bytes whose **address never moves**. This is not
  ergonomics: an inline target is a pointer into the object's own buffer, so
  every `BTreeMap::insert`, `Vec::push` and `let` binding that moves an
  `MsvcFunction` by value invalidates it. MSVC never hits this because its move
  constructor calls `_Move`; Rust's bitwise move does not. Dropping an
  `OwnedFunction` runs the destructor, so the discipline is a type invariant;
* a **stack guard**: `destroy` never passes `deallocate = true` for a target
  inside the current thread's stack, read from the x86 TIB at `fs:[4]`/`fs:[8]`.
  MSVC's inline test is an address comparison, which is only correct if `&self`
  really is the address the caller pushed. It is — the emitted thunk opens
  `lea edi, [esp+0x18]`, the caller's own slot, not a spilled local
  **[measured]** — but that is a fact about one rustc, not a guarantee, and the
  failure mode without the guard is `free()` on a stack address.

`src/service.rs` now takes the by-reference setters with `retain` and the
by-value completion pairs with `adopt`, holds all 17 retained setter callbacks
and every pending request pair as `OwnedFunction`, and releases them by drop —
including `LobbyCancelPendingRequests`, which now really does destroy what it
cancels.

`src/logger.rs` is new: the 4-slot `ICrossplayLogger` behind export ordinal 1,
which retail dereferences with **no null check** two instructions after the
call. Its `Register` adopts the by-value `std::function`, its `UnregisterAll`
releases every copy, and its `Log` decodes the by-value `wstring` and releases
its heap buffer when `_Myres >= 8` (the SSO boundary measured at
`CrossplayProxy.dll` RVA `0x35c0`, `cmp dword ptr [ebx+0x1c], 8`).

### 3d. What the smoke actually executed

`crossplay-load-smoke` supplies **guest code**: two synthetic `_Func_impl`
vtables built to the measured six-slot layout, one whose `_Copy` allocates
(heap-target behaviour) and one whose `_Copy` places into `where` and returns it
(inline-target behaviour). Every slot counts its own invocations, so the DLL's
behaviour is observed from the caller's side rather than self-reported.

Observed in the passing run:

| | |
|---|---|
| `_Copy` invocations | 9 |
| `_Move` invocations | 1 (the inline copy relocated into owned storage) |
| `_Delete_this` invocations | 18 — 16 with `deallocate=true`, 2 in place |
| `_Do_call` invocations | 3 (a `StartSession` completion, a `StartGame` failure, a refused `JoinChat`) |
| targets outstanding at exit | 1 — the `SetServiceErrorCallback` copy the process-lifetime service still holds |
| ESP checks | 36, all balanced |

The assertions that make those numbers mean something:

* `Register` on a heap target runs `_Copy` exactly once and destroys the
  by-value argument with `deallocate=true`; `UnregisterAll` releases the
  retained copy; the outstanding-target count returns to its pre-test value.
* `Register` on an inline target runs `_Copy`, then `_Move` into owned storage,
  destroys the heap original once with `deallocate=true` and the relocated
  temporary in place with `deallocate=false`.
* `SetServiceErrorCallback` — by reference — runs `_Copy` and does **not**
  destroy the caller's object.
* `StartGame` takes both by-value callbacks and releases both; answering it on
  `Tick` releases the two retained copies.
* a refused slot (`JoinChat`) releases its by-value callbacks *and* answers on
  the caller's error callback.

That is the open item closed with executed evidence rather than reasoning.

**What it is not:** a claim that MSVC's real `_Func_impl` behaves like these
probes. It is a claim that the DLL calls the right slots in the right order with
the right `deallocate` flag and leaves nothing undestroyed.

---

## 4. Two smaller findings

**`Crossplay::Logging::Logger()` is not optional and must never return null.**
Fifteen retail sites call it and immediately `mov ecx, eax` into a member call.
Two of rise.exe's own wrappers (`0x004fef40` at level 8 `Error`, `0x004fee90` at
level 2 `Info`) build a 24-byte `wstring` in place on the stack
(`sub esp, 0x18`; `_Myres = 7`; `_Mysize = 0`; `_Buf[0] = 0`) and end in
`call dword ptr [eax + 0x0c]` — vtable slot 3. One site, `0x0056143d`, builds a
40-byte `std::function` and calls slot 1 with level 2. **[measured]**

**An associated `const` vtable is not one table.**
`LocalCrossPlayService::<M>::VTABLE` is an associated `const`, so each separate
`&`-of-it is its own promoted allocation. With two use sites the object
published one table and the crate's own accessor returned a different address
with identical contents; the host build happened to merge them and the
`i686-pc-windows-msvc` build did not, so the existing
`the_vtable_pointer_is_the_object_address` test only failed once the suite ran
on the real target. Fixed by making the accessor `#[inline(never)]` and the only
place `VTABLE` is named. Harmless to retail, which never compares vtables, and a
lie in any diagnostic that does.

---

## 5. Gates

```sh
# host, arm64 — the layout and semantics suite
cd crates/don-crossplay && cargo test --lib                      # 57 passed

# the retail target, compiled and then executed under Wine
cd crates/don-crossplay
XWIN_CACHE_DIR=/Users/ember/Library/Caches/cargo-xwin-x86 XWIN_ARCH=x86 \
  cargo xwin test --release --lib --no-run --target i686-pc-windows-msvc
wine target/i686-pc-windows-msvc/release/deps/don_crossplay-*.exe   # 57 passed

# the image
cd crates/don-crossplay/dll
XWIN_CACHE_DIR=/Users/ember/Library/Caches/cargo-xwin-x86 XWIN_ARCH=x86 \
  cargo xwin build --release
uv run --with pefile --with capstone python3 crates/don-crossplay/dll/check-exports.py   # PASS

# the PE32 loader gate, in a fresh disposable Wine prefix
crates/don-crossplay/dll/run-wine-smoke.sh \
  crates/don-crossplay/dll/target/i686-pc-windows-msvc/release/CrossplayProxy.dll \
  crates/don-crossplay/dll/target/i686-pc-windows-msvc/release/crossplay-load-smoke.exe \
  /tmp/don-crossplay-evidence
```

The runner seeds both Wine module trees before `wineboot`, waits for
`wineserver -w` and then polls for `system.reg`, refuses to overwrite evidence,
and writes a receipt. Both lessons are inherited verbatim from
`crates/netsys-shim/run-gen7-wine-smoke.sh`.

## 6. Status — read this before believing anything

| claim | evidence |
|---|---|
| a PE32/i386 DLL exists with the four shipped names at ordinals 1..4 | `check-exports.py`, PASS; extra `proxy_*` alias targets reported, not hidden |
| image base, PE32 magic, `IMAGE_FILE_DLL`, `__cdecl` factories, both GPU hints as writable `.data` holding `1` | `check-exports.py`, PASS, checked against the shipped image as well as ours |
| Windows loads the image, starts the Rust runtime inside it, and both factories return stable singletons | `don.crossplay-load-smoke.v1`, PASS under Wine Devel 11.10 in a fresh pre-seeded prefix |
| the `__thiscall` boundary is crossed with the stack balanced | 36 ESP checks across both interfaces, all balanced |
| all 58 `ICrossPlayService` slots and all 4 `ICrossplayLogger` slots are non-null and reachable through the object's own vtable | same run, `null_slots=0` |
| the `std::function` ownership hole is closed for heap **and** inline targets, in both the by-value and by-reference directions | same run; counts and assertions in §3d |
| the by-value `wstring` heap buffer is released on the shared UCRT heap | the smoke hands over a `malloc`'d buffer past the SSO bound and the DLL decodes and frees it; block-reuse after the free was **not** observed under Wine and is reported as an observation, not a proof |
| the 58-slot ABI and the local backend behave identically on the host and on the retail target | the same 57 tests pass native on arm64 and as PE32 under Wine |
| `MsvcFunction` had the wrong alignment and therefore the wrong calling convention | §2, measured in both states |
| **the retail game loads this DLL** | **no.** Never attempted. Nothing installed into a game directory, no retail process read or written. |
| **a retail match runs over this service** | **no.** The service has never been called by `riseofnations.exe`. Its semantics are DoN's, and the lobby/setup architecture note in `crossplay-local-backend.md` still stands. |

The honest gap is unchanged in shape from `netsys-shim`'s: the loader, the
export surface, the calling convention and the object lifetime are now
established in a disposable process, and retail has still never executed a
single one of these slots. The next step needs explicit authorisation for a
load-only run.
