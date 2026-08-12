# Canonical fresh-SVX Fishermen DEPLOY work

Status: **production-mounted bounded executor**. `Sim::unit_work` now advances the exact
already-open Fishermen DEPLOY wait branch observed in the fresh retail v16 save. The branch
is atomic, save/reload/resume-tested, revision-bound to installed spell/type facts, and fails
closed before every adjacent target, payment, opening, siege/general, completion, or effect
cone.

## Why CAST precedes BUILD_AT

The complete fresh-SVX Unit census contains 29 Gather, eight ExploreTo, four BuildAt, one
CastSpell, and one MoveTo orders. Gather is already production-mounted. The four BuildAt
orders enter the construction lifecycle, whose placement/start/activation and builder host
have separate ownership. The single CAST witness instead has a lossless v13 payload owner,
an exact no-target sentinel, and a state-writing child that closes without world or effect
calls. That makes it the larger immediately exact executor tranche, despite the lower specimen
count.

## Fresh witness

Source: `new save game 2026.08.11 15'42'57 (Tue).SVX`, compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7`, with the structurally
derived `Objects::walk_data` boundary `0x4F21F`.

The census reaches owner 0 / Unit slot 16 without pattern matching:

| fact | exact value |
|---|---|
| Unit identity | `(who=0,o=16)`, active |
| `ptype` | 317, shipped `Fishermen` / `FISHERMEN` |
| Unit body | `[0x520F5,0x5227B)`, SHA-256 `1f2afb9d1cb4697bb0546a16adbb6f6d75d8d03f7e0d3764e163e9396065e612` |
| saved `spell_time` | 12 |
| Path | `(capacity=10,length=0,increment=10)` |
| order node | `[0x521AE,0x521CE)`, metric 0 |
| Guys | exact `(1,1,1,0)` array; Guy 0 SHA-256 `a1192bce1ac9672a768c59e685cb4162ec8a49ec12efe3abe5c3fa5274991687` |

The complete 27-byte `CastOrder::walk_data` image is:

```text
00 ffffffff ffffffff ffff ffffffff ffffffff 01000000 92020000
```

That is flags 0; target `(-1,-1,65535)`; `x=-1`; `y=-1`; `paid=1`; spell
`658 / 0x292`. Its SHA-256 is
`81cdba3f17e0185b23b1179e44b0b40fd841e2d0b819baa965a44d577bf62ab9`.
`EconomyOrderPayload::CastSpell` and DoNSave v13 retain every field losslessly.

Installed shipped content joins the witness exactly: type 317 is Fishermen; spell 658 is
`Deploy4`; `craftrules.xml` gives Deploy4 `JOB_TIME=40`, `FROM=Fishermen`, no target flags,
zero mana/cost/range, and `FROM2=None`.

## Exact executable child

Authority is the matched `riseofnations.exe` SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079` and PDB GUID
`{51d4f219-61c6-4f84-9d5b-c3361b0d291f}`. `Unit::do_cast` is
`0x005EBFE0`, 4,191 PDB bytes, ending at `0x005ED03F`; its exact PE body SHA-256 is
`e6ddf5fe099b840ec1241f9b8d7aba81884646f2cf8ab45992ccdfce4602da7a`.

For the fresh image the executed chain is:

```text
get CastOrder
  -> spell virtual +0x48 says real SpellType
  -> paid == 1, so no pay_cast_costs call
  -> SpellTypeData.spell_flags & 0x0E == 0, so untargeted
  -> spell_time == 12 != 0, so no first-frame animation/opening
  -> virtual +0x50 says not PACK
  -> virtual +0x54 says DEPLOY
  -> actor UnitTypeData virtual +0x10C says not siege
  -> skip has_general and every contained Guy timer write
  -> wrapping spell_time++: 12 -> 13
  -> get_job_time(actor o=16, who=0) == 40
  -> signed 13 < 40, return
```

Thus the complete observable mutation set is one checksum/save-owned `UnitData::spell_time`
write. The current order, paid latch, Guys, RNG, paths, and all world/effect owners remain
byte-identical. The production adapter admits later waiting frames of this same installed
branch until the next increment would reach job time; the completion frame fails closed.

## Atomic authority and fail-closed boundary

`CastWorkAuthority` is not saved. It binds the stable actor Handle and retail identity to
type 317 plus the real-spell, zero-flags, non-pack/deploy, non-siege, and exact returned
job-time facts under a nonzero composition digest and revision. Save load restores the
canonical actor/order image and deliberately leaves authority empty until content reinstalls
it.

Prepare reads the full actor/order/type image and plans without mutation. Commit recomputes
the same plan and compares the whole prepared value, including authority digest/revision,
before assigning `spell_time`. Mutated payload bytes, target state, type identity, active bit,
authority result, job time, or timer all authorize no partial write.

Explicit unowned arms are payment; any targeted spell; untargeted first-frame opening and
animation; PACK; non-DEPLOY; siege/general detection plus contained Guy timers; and
completion/`SpellType::cast`/retirement effects. No scalar approximation is used for any of
them.

## Gates

The focused suite freezes the binary/payload identities, pure-plan single-field delta,
mutations for every adjacent cone, stale compare behavior, missing-authority behavior, and
production direct versus save/reload/reinstall/resumed `do_frame` equality:

```sh
cargo test -p don-sim --test canonical_cast_runtime
```

The remote hbox overlay gate uses the same focused target so the production mount is checked
from a clean HEAD plus only this isolated tranche's files.
