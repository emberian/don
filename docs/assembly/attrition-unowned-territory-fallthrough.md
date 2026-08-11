# `Unit::process_attrition`'s unowned-territory arm reads outside `leaders`

**Function:** `Unit::process_attrition` `0x005E11A0` ·
**Arm:** `0x005E1294..0x005E12E3` ·
**Owner:** `crates/don-sim/src/systems/borders_fog.rs`
(`UnownedTerritoryFallthrough`, `unowned_territory_fallthrough`)

A sibling lane recovering the ordered attrition transaction found that the unowned-territory
arm **does not return**, and stopped at a typed `UnownedTerritoryFallthrough` blocker because
the continuation was not derivable from what it had. This note derives it. The result is that
retail performs a genuine out-of-bounds read of `leaders`, that for the *unowned* index that
read resolves to the **Win32 keyboard-state buffer**, and that the resulting control flow
always returns — for a reason that has nothing to do with leaders, diplomacy, or attrition.

Fidelity: this is **`[measured]` disassembly plus PE/PDB layout**, not behaviour. It settles
where the read lands and what the two flag gates can do with it; it is not an oracle run and
nothing here is verified. `don-sim` does not reproduce the read.

## 1. The arm

`re/decomp-all/005e11a0.c` renders this as an `if` that stores `neutral_attrition` and
returns. Disassembling it shows otherwise — the store is followed by a fall-through into the
owned-territory chain with the territory index **still negative**:

```asm
005e11fe  0fbe7cb10f    movsx edi, byte ptr [ecx + esi*4 + 0xf]  ; WData::who, signed char
005e1203  0fb67309      movzx esi, byte ptr [ebx + 9]            ; the unit's owner
...
005e1294  85ff          test  edi, edi
005e1296  792d          jns   0x5e12c5                  ; owned -> the owned chain
005e1298  69ceec6e0000  imul  ecx, esi, 0x6eec          ; leaders.list[unit owner]
005e129e  83b990abe30000 cmp  dword ptr [ecx + 0xe3ab90], 0   ; neutral_attrition +0x800
005e12a5  0f841b070000  je    0x5e19c6                  ; zero -> return
005e12ab  8b4318        mov   eax, dword ptr [ebx + 0x18]
005e12ae  83b81802000001 cmp  dword ptr [eax + 0x218], 1       ; domain == 1 (sea)
005e12b5  740e          je    0x5e12c5
005e12b7  668b8190abe300 mov  ax, word ptr [ecx + 0xe3ab90]
005e12be  6689839e000000 mov  word ptr [ebx + 0x9e], ax        ; the period write
005e12c5  3bfe          cmp   edi, esi                  ; <- FALL-THROUGH, edi still < 0
005e12c7  0f84f9060000  je    0x5e19c6
005e12cd  69d7ec6e0000  imul  edx, edi, 0x6eec
005e12d3  8b8290a3e300  mov   eax, dword ptr [edx + 0xe3a390]  ; leaders.list[-1].leader_flags
005e12d9  a801          test  al, 1
005e12db  0f84e5060000  je    0x5e19c6
005e12e1  a802          test  al, 2
005e12e3  0f84dd060000  je    0x5e19c6
```

Two facts make the index negative and keep it negative:

* `WData::who` is `char` — signed — at `WData +0x0F` (`schema/types.json`; the `movsx` at
  `0x005E11FE` and the `[ecx + esi*4]` stride with `esi = 7 * index` confirm both the sign
  and `sizeof(WData) == 28`).
* `-1` is unowned. `-2` also occurs: `World::compute_reg_territory` loads the immediate
  `mov ecx, 0xfffffffe` at `0x006B14E8` and `0x006B1515` and stores it through
  `mov byte ptr [eax + ecx*4 + 0xf], dl` at `0x006B1731` — the marker for a claim whose
  `City +0x5F` disagrees with the leader slot the city is filed under
  (`docs/mechanics/borders-fog.md` §4.3).

## 2. There is no hidden neutral slot before `leaders`

`Leaders` begins with its array and nothing else:

| fact | evidence |
|---|---|
| `leaders` is at `0x00E3A390` | PDB public `class Leaders leaders`; **and** the immediate `add ecx, 0xE3A390` that `Leader::calc_anti_attrition` materialises five times (`0x006CDCF6`, `0x006CDD1B`, `0x006CDD3D`, `0x006CDDA2`, `0x006CDE03`) |
| `Leaders::list` is `Leader[10]` at offset `0` | `schema/types.json` |
| `sizeof(Leader) == 0x6EEC` | every `imul … 0x6EEC` index; and `10 * 0x6EEC == 0x45538`, which is exactly the offset of the next member `Leaders::prod_script_path` — so the array is contiguous with no header and no leading slot |
| the array ends at `0x00E7F8C8` | `0x00E3A390 + 0x45538`; independently named in `crates/don-sim/src/systems/unit_inctime.rs` as the step-15 leader walk bound |

So `leaders.list[-1]` is `0x00E3A390 - 0x6EEC = 0x00E334A4`, genuinely before the object, and
`leaders.list[-2]` is `0x00E2C5B8`.

## 3. Where `-1` lands: `Window::key_states`

`0x00E334A4` is 228 (`0xE4`) bytes into `Window::key_states` at `0x00E333C0`.

`key_states` carries the PDB decoration `?key_states@Window@@2PAEA` (`unsigned char *`), but
it is an **array of 256 bytes**, on two pieces of evidence:

```asm
; Window::get_key_state 0x0051AEB0
0051aeb3  0fb64508      movzx eax, byte ptr [ebp + 8]
0051aeb7  8a80c033e300  mov   al, byte ptr [eax + 0xe333c0]   ; indexed by a virtual key code
```

```asm
; Window::update_key_states 0x00A4DC00
00a4dc03  68c033e300    push  0xe333c0
00a4dc08  ff15e453ac00  call  dword ptr [0xac53e4]            ; USER32!GetKeyboardState
```

and the next static symbol, `code_buffer`, begins at `0x00E334C0` — exactly `0x100` later.
`GetKeyboardState` is documented to fill a 256-byte array.

Both `.data` addresses are past the section's raw size (`.data` VA `0x00C06000`, raw
`0xA4000`, so anything at or above `0x00CAA000` is zero-initialised at load): this is a live
read of unrelated global state, not a fault, and it is stable under ASLR because both
addresses move together.

`0x00E334A4` has **zero** immediate references anywhere in `.text` (`tools/pdb/xref.py`), and
`0x00E333C0` has exactly three: `Window::get_key_state` (read), `Window::get_lbutton_state`
(read) and `Window::update_key_states` (the `GetKeyboardState` call). Nothing else writes
that buffer.

## 4. What retail therefore does at `-1`

Both tests read the **same byte**, `AL` = `key_states[0xE4]`:

* `0x005E12D9` `test al, 1` — `GetKeyboardState`'s *toggled* bit;
* `0x005E12E1` `test al, 2` — a bit `GetKeyboardState` does not define. Its contract is the
  high-order bit for "key is down" and the low-order bit for "key is toggled"; nothing sets
  `0x02`.

So `test al, 2` cannot pass, and `Unit::process_attrition` **always takes the `je` at
`0x005E12E3` back to `0x005E19C6` and returns**. The *observable* behaviour of the unowned
arm is "write `LeaderData::neutral_attrition` as the period, then return" — which is what the
decompiled C suggested — but it arrives there through a keyboard byte rather than through a
`ret`, and that distinction is the whole reason to record it:

* it is contingent on the image layout (`leaders - 0x6EEC` landing inside `key_states`) and
  on an OS buffer's bit assignment, not on any simulation invariant;
* a port that "simplifies" it to a return has quietly assumed both;
* and it is not reachable in a normal game anyway: the arm requires
  `LeaderData::neutral_attrition != 0`, which only scenario and Conquer-the-World paths write.

## 5. What `-2` does is **not derived**

`leaders.list[-2]` is `0x00E2C5B8`. The shipped PDB has no symbol — public, global or static
— anywhere in `[0x00CC4BDC, 0x00E32F38)`, a 1.7 MB unnamed zero-initialised span, so what
occupies that dword and whether anything writes it is unknown here. `-2` is not a rare case
either: it is the ordinary contested-claim marker.

`don-sim` reports this as `UnownedTerritoryFallthrough::UnnamedStatic { va: 0x00E2C5B8 }` and
`admits_attrition() == None`. Answering `Some(false)` for it — folding it into the `-1`
answer because "it is probably also zero" — is precisely the plausible substitution
`docs/CHARTER.md` forbids, and the test
`the_contested_marker_lands_somewhere_we_have_not_derived` exists to fail if someone does it.

## 6. Open

* What global occupies `0x00E2C5B8`. A live-process read of that dword in the Windows VM
  during a scenario game would settle it in one sample; a fuller PDB static dump might name
  it without running anything.
* Whether `WData::who == -2` can coexist with a non-zero `LeaderData::neutral_attrition` in
  practice. If it cannot, the `-2` branch is unreachable and the gap closes for free.

## 7. Tests

`crates/don-sim/tests/attrition_kernel_returns.rs`:

* `an_unowned_tile_reads_the_win32_keyboard_state_buffer`
* `the_contested_marker_lands_somewhere_we_have_not_derived`
* `an_owned_tile_is_not_a_fallthrough_at_all`

All three were mutation-checked: changing `LEADER_STRIDE` by one byte, or making every
negative index claim the keyboard answer, fails them.
