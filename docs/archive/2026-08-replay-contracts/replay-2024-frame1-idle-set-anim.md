# 2024 frame-1 idle `Unit::set_anim` capture boundary

Status: **source-bound authority implemented; replay closure remains red**. The repository does
not contain the supported-retail frame-1 entry/return capture required to issue a receipt, and
the 4,723-byte `Guy::set_anim` child remains data dependent. No Guy after-image or RNG draw was
inferred.

## Reached cohort

The canonical 2024 setup receipt allocates seven owner-0 Units in this stable order:

| setup ordinal / `o` | type | squad | crew | wrapper Guy order |
|---:|---:|---:|---:|---|
| 0 | 69 Scout | 1 | 1 | `0, 1` |
| 1, 2 | 62 Merchant | 1 | 2 | `0, 1, 2` |
| 3, 4, 5, 6 | 50 Citizen | 1 | 0 | `0` |

These are type/layout facts, not a claim about the live frame-1 Guy images. Frame zero already
processes the Scout and Merchants. Consequently the SetAnim entry must come from the same
supported-retail trace as the surrounding step-14 chronology. `IdleSetAnimRequest` carries the
complete entry `UnitGuys`, RNG state, stable `Handle`, authority revision and digest.

The Scout reaches an earlier source-dependent `Caster::process_spells` child. This boundary
therefore admits a Scout SetAnim receipt only if a real trace reaches it; it does not skip or
fabricate that earlier mutation.

## Instruction-exact wrapper

Measured against executable SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`:

| function | VA | bytes | body SHA-256 |
|---|---:|---:|---|
| `Unit::set_anim` | `0x00616f40` | 201 | `798f485753eb1853dc19ce55e43115674f6e3988370210dd9d5d4272d386f6ee` |
| `Guy::set_anim` | `0x005da300` | 4,723 | `be76d8eb8e4301d6c10888efa8b2ca1dde0ca02045f46b9c0c98b576d68f68b3` |

`Unit::set_anim(0,0,1)` first walks the live squad prefix `0..guy_mark`, then the crew suffix
`squad_size..guys.length`. Before every child call it reads **Guy 0's** current animation class.
If that lead class is not attack class 12, it clears the addressed Guy's `hold_attack`; it does
not select the clear from the addressed Guy's class. The two copies are at
`0x00616f60..0x00616f91` and `0x00616fc0..0x00616ff1`.

Each loop re-reads its bound after a child returns. The capture binder requires every child to
preserve pointer-array topology, so the validated call order is equivalent to the retail dynamic
walk. A trace claiming a topology mutation is refused rather than normalized into a different
call sequence.

## Authority and atomicity

`setup_2024_frame1_set_anim_capture::bind_captured_frame1_idle_set_anim` is read-only. It accepts
the frozen idle-prefix request, complete before/after `Sim` snapshots, and an ordered per-Guy
journal. Admission requires:

- the exact replay and executable identities, frame 1, arguments `(0,0,1)`, nonzero request and
  capture revisions, and matching request authority digest;
- the exact seven-Unit setup cohort rebound independently in both snapshots, including the same
  live Handle generation, owner/`o`, type, and Guy array shape;
- byte-derived DoNSave hashes for both complete snapshots and the request's complete entry
  `UnitGuys` plus game RNG state;
- one child journal per wrapper address in exact order, with complete entry/return `UnitGuys`,
  the lead-class `hold_attack` prefix, stable topology and identity, and exact RNG continuity;
- each declared draw count matching retail's affine LCG transition. The verifier exponentiates
  the transform in logarithmic time, so a hostile `u32::MAX` count cannot force a giant loop;
- after restoring only this Unit's entry `UnitGuys` and the entry RNG in a reload of the after
  snapshot, byte-identical equality with the complete before snapshot. Any unrelated Unit,
  Leader, map, order, cache, generation, or owner mutation rejects the whole receipt.

The receipt carries the exact request, capture revision/source, replay and executable hashes,
before/after whole-Sim hashes, stable setup ordinal/row/type, complete before/after `UnitGuys`,
RNG before/after, and the ordered child journals. Its composition digest includes every carried
Guy walk image and topology field. No candidate state is mutated during admission, so every
failure is an atomic stale rollback by construction.

## Remaining red boundary

A coherent supported-retail trace must still capture the immediate entry/return of each reached
frame-1 `Unit::set_anim` and its nested `Guy::set_anim` calls. Until that artifact exists, this
module can verify and bind authority but cannot emit a real receipt or advance the post-step-14
canonical Sim. Full `Guy::set_anim` execution, including captain/uber recursion, installed
graphics data and its data-selected game-RNG arm, remains charged rather than approximated.
