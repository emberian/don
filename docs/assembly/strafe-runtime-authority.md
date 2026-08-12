# STRAFE runtime payload authority

Status: source-only, unregistered, strict row 16 remains red.

`crates/don-sim/src/systems/strafe_runtime_authority.rs` reuses the canonical
`patrol::StrafeOrder`; it does not introduce a second gameplay payload. It freezes two exact
images needed by the detached STRAFE executor transaction:

- the 57-byte `StrafeOrder::walk_data` image, including both visits to the shared order-flags
  byte, the target triple, all five attack-state bytes, six `AirOrder` words, and `xx/yy`;
- the fixed 47-byte DoNSave v13 tag-8/version-1 leaf for fields not already present in the
  generic order header.

The leaf layout is:

```text
tag:u8 = 8, version:u8 = 1
def_x:i32, def_y:i32
mandatory:u8, defensive:u8, in_range:u8, ever_in_range:u8, new_ord:u8
home_o:i32, home_who:i32, cruising_alt:i32, sharp_turn:i32, old:i32, returning:i32
xx:i32, yy:i32
```

Target `(o,who,uid)` and common flags remain in the generic order header and are passed to the
decoder explicitly. Targetless returning sorties preserve the old UID exactly; retail clears
only the target pair. The codec rejects incoherent target/home address pairs, noncanonical
attack booleans, wrong tag/version, truncation, and trailing bytes. A mutation sweep proves that
all fifteen fixed leaf fields own distinct bytes.

This does not admit tag 8 into `save_load`, register a module, alter `Order`, wire dispatcher row
16, or claim a tick/packet closure. Those remain a coordinated shared-owner step after the
detached executor transaction and save/resume after-image are both proven.

The clean-HEAD hbox overlay job
`strafe-runtime-authority-20260812T003029Z-50262-16517-e7ef5c68be83` passes all seven tests,
including the fifteen-field distinct-byte sweep and malformed-envelope mutations. Persvati was
unavailable for this gate because its filesystem reported zero free bytes; no failed test was
observed there.
