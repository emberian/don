# 2024 frame-one Scout caster child

Status: **exact adjacent-call contract; production capture not yet installed.**

The supported 2024 Great Lakes replay's first setup Unit is owner 0/object 0, with base Scout
type 69. The shipped unit table gives that base row `unit_flags2 = 18` (`0x12`). The live
effective type remains source-owned because `current_upgrade` can replace the base row. The
captured effective row must retain caster bit `0x02` and Special bit `0x10` while clearing Hero
bit `0x20`; the instruction sequence in `Unit::process` then reaches the Special caster table and calls
`Caster::process_spells(o, who, 0)`:

```text
0x00610d47  test type.unit_flags2, 0x02     ; caster prelude is present
0x00610d63  type.unit_flags2 & 0x20         ; is_hero = false
0x00610d9b  type.unit_flags2 & 0x10         ; is_special = true
0x00610db1  who = unit.who
0x00610dbe  owner_specials = Specials[who]
0x00610dc5  o = unit.o
0x00610dcd  caster_index = unit.special (+0x86)
0x00610dd4  caster = owner_specials[caster_index]
0x00610dd7  Caster::process_spells(o, who, 0)
```

This is not a call on storage embedded in `UnitData`.  The receiver is the indexed `Special`
object, whose `CasterData` base owns `Array<ActiveSpell>` at `+0x04`.

## Exact captured owner

[`frame1_caster_process`](../../crates/don-sim/src/systems/frame1_caster_process.rs) defines the
child-local call-boundary authority.  The PDB fixes the complete engine array shape:

| array field | array offset | captured form |
|---|---:|---|
| length | `+0x04` | signed 32-bit |
| allocated size | `+0x08` | signed 32-bit |
| increment | `+0x0c` | signed 16-bit |
| list | `+0x10` | ordered pointee values, never the process pointer |
| flags | `+0x14` | unsigned 8-bit |
| current index | `+0x18` | signed 32-bit |

Each `ActiveSpell` is the PDB-exact 12-byte triplet `{type, start, end_frame}`.  The authority
retains every field above before and after the adjacent call; it never reconstructs the engine
container from a Rust `Vec`'s capacity.

The same local envelope binds the Scout `Handle` generation and `{who,o,uid,type}`, the captured
`unit.special` index, header flags, both Unit mask words, the spell-visibility dirty word, and the
main RNG state.  Its entry and exit digests are explicitly *child-local envelope* digests.  They
are not called whole-frame hashes: commands and sibling Unit work can change a whole frame on
either side of this call.  The opened request separately binds this envelope into the canonical
post-command frame-one chronology through a nonzero authority revision and digest.

## Admitted execution

`mount_captured_frame1_scout_caster_process` completes only when all of the following are true:

1. replay and executable hashes match the supported revisions;
2. the request is exactly frame 1, owner 0/object 0, normal mode zero, with the captured
   `unit.special` index;
3. a nonzero canonical setup-member/type join proves base Scout 69 and the live effective
   type/`unit_flags2`; both call-boundary images must match that effective row and take the
   Special rather than Hero dispatch;
4. both array images are structurally complete and the caller's live projection equals the
   captured entry image;
5. the entry array is empty;
6. the full child-local entry and exit images are equal; and
7. their adjacent envelope digests are equal.

That path reports zero removed spells, Jam Radar pulses, RNG draws, and simulation mutations.
It preserves nondefault empty-array allocation history rather than normalizing `size`,
`increment`, `flags`, or `cur_index`.

A valid nonempty entry is
`Frame1CasterChildResidual::NonEmptyActiveSpellQueue`, carrying the exact before/after lengths
and capture composition digest.  It is not silently processed by the existing logical
`Vec<ActiveSpell>` helper, because retail removal also owns engine container metadata and may
reach Unit masks, Leader spell-flag verification, the global dirty word, and presentation
effects.  Malformed images, changed identity, stale state, or an allegedly empty call with any
changed field are capture errors rather than residual execution.

The mount preflights the entire capture before publication.  Errors and open residuals leave the
caller-owned projection unchanged; the successful publication is itself the captured identical
after-image.  This gives the frame-one Unit-process parent an atomic child boundary without
guessing post-frame-zero caster state.

## Evidence and remaining join

- executable SHA-256: `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`;
- replay SHA-256: `1690431a5ef19b38a3425d3dd7311e8e83ca0d27c56fabe49d776a9f1421b251`;
- `Caster::process_spells`: `0x00739AD0`, 481 bytes;
- PDB types: `Caster`, `CasterData`, `Array<ActiveSpell>`, and `ActiveSpell`;
- Scout row: `schema/live/live-tables-unit.tsv`, type 69, `unit_flags2=18`.

The remaining product join is deliberately explicit: a supported retail adjacent-call capture
must populate this authority and bind it to the canonical post-command frame-one Scout identity.
Neither the replay wire nor the frame-zero setup receipt proves that runtime queue image.
