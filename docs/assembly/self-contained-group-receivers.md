# Self-contained group receivers: HOTKEY, RECALL, RETURN — and why EJECT_ALL is not one

Lane `group-act`, megaswarm wave 2, 2026-08-11.

## What this tranche took, and by what rule

The closure ledger's `group_actions` domain had 33 red rows. This lane took the four whose
`ActionDef::delegates` set is **closed inside the cohort** — `hotkey` `[]`, `return` `[]`,
`eject_all` `[]`, `recall` `["return"]` — because a row whose delegate is another lane's red
row cannot honestly reach `Port::Complete` no matter how well its own body is recovered.
Three of the four closed. The fourth did not, and §5 says exactly what blocks it.

| row | VA | bytes | before | after |
|---|---|---:|---|---|
| `hotkey` | `0x006FA7A0` | 64 | `NotOnTheWire` | **`Complete`** |
| `recall` | `0x006FA7E0` | 1,373 | `StateWired` | **`Complete`** |
| `return` | `0x006FAD40` | 1,307 | `NotOnTheWire` | **`Complete`** |
| `eject_all` | `0x00710B40` | 766 | `StateWired` | `StateWired` (unchanged) |

Closure delta: `group_actions` 9/42 → 12/42, and the three promoted rows drop out of the
`opcodes` domain's red set through opcode 35. Port-class counts move
`[Complete, Orders, State, StateWired, Todo, NotOnTheWire]` from `[9, 14, 0, 12, 0, 7]` to
`[12, 14, 0, 11, 0, 5]`.

**Tier C.** Everything below is `[measured]` by capstone disassembly of
`ron-bin/riseofnations.exe` (sha256 `30478a44…625079`) on this Mac, named from
`ron-bin/sbl/rise.pdb`. Nothing here has been executed against retail.

## 1. `Group::action_hotkey` `0x006FA7A0`

The whole receiver:

```text
006fa7a4  imul esi, dword ptr [ebp + 8], 0x9fc   ; slot * sizeof(HotKeyGroup)
006fa7ab  mov  edx, ecx                          ; edx <- this (source Group)
006fa7ad  mov  ecx, dword ptr [0xc0afe0]         ; hot_key_groups
006fa7b6  call 0x715120                          ; HotKeyGroups::copy_group
006fa7c1  push 1
006fa7c6  call 0x7152f0                          ; HotKeyGroupOut::update_name
006fa7d0  mov  dword ptr [esi + eax + 0x9d8], 0  ; camera_valid <- 0
006fa7dd  ret  4
```

Three findings made this a 64-byte row rather than a research project.

**It is opcode 34's `clear == 0` arm, inlined.** `CommandPackage::process_hotkey`
`0x009474D0` does not call this receiver; at `0x009475B8..0x009475F2` it emits the identical
three operations against `groups.list[package.group]`. The already-green opcode-34 row's
`copy_hotkey_group` **is** this action's body, so the port shares it instead of restating it.

**`HotKeyGroup +0x9D8` is the camera-validity latch.** `process_hotkey`'s `clear != 0` arm at
`0x0094760B..0x0094766B` writes `+0x9D8 = 1` alongside `+0x9D0` (x), `+0x9D4` (y) and
`+0x9DC` (zoom). `HotKeySlot::camera = None` is therefore the exact model of the `= 0` store.

**`HotKeyGroupOut::update_name` `0x007152F0` is presentation.** Across its 3,404 bytes the
only store destinations are `+0x9E0` (a `String`) and `+0x9F4` (an icon selector); the only
calls are 31 `String::operator+=`, two `String::operator=`, `String::String`, `String::close`,
`memset`, and the read-only `ObjectData::is_worker` / `can_carry`. Its first branch
(`0x0071533E`) compares the group owner against the local display player `[[0xC06210]+0x298]`
and returns early for anybody else, so it is not a function of shared state at all.

`Console::on_key_down` `0x007CC80B` / `0x007CC8BA` are its only two call sites, measured over
every named procedure in `.text` — which is why the row was classified `NotOnTheWire`. The
port exposes it as `Bridge::action_hotkey(group, slot)`, the same receiver shape, addressing
`groups.list[group]`. Retail bound-checks the slot nowhere; the port refuses an out-of-range
slot rather than reproducing the out-of-bounds write.

The PDB declares `void HotKeyGroups::copy_group(Group*, const …)`. The emitted body reads its
destination from `ECX` and its source from `EDX` and cleans no stack (`ret` at `0x00715224`).
As usual, the disassembly is the authority.

## 2. `Group::action_recall` / `Group::action_return`: what was actually missing

Both bodies were already recovered, into `recall_action_frontier.rs` and
`return_action_frontier.rs`, and opcode 35 already crossed
`Fleet::apply_recall_action_transaction` with a combined receipt that refuses to publish
recall's scenario prefix when the RETURN tail is missing
(`docs/assembly/recall-return-live-integration.md`). The row stayed red for one reason,
stated in that doc: *"The `ObjectTable` and default external `Fleet` implementations
deliberately return `Unavailable`: they do not yet own the complete scenario, launching-list,
path/action, and AirOrder columns."*

This tranche gives the reference host those columns.
`crates/don-sim/src/systems/air_containment_host.rs` adds one `AirWorld` side table to
`ObjectTable`, holding per object:

| column | retail source |
|---|---|
| `valid_wall` / `is_build` | object virtuals `+0x0C` / `+0x20` |
| `obj_masks` | `ObjectTypeData +0x1E4`; bit `0x08000000` excludes missiles |
| `inside` | `ObjectData::get_inside(&who)` `0x00651A80` |
| `launching` | `ObjectData::launching` `+0x44`; `None` is retail's null pointer |
| `air` | the `AirOrder` subobject (`oxx`, `whose`, `cruising_alt`, `sharp_turn`, `returning`) |
| `path_length` | `UnitData::path.length` at `Unit+0xC0` |
| `gather_points` | the `GatherPoint` list at `Build+0xCC` |
| `home_base` | `UnitData::home_base` `0x00609DC0` |
| `airbase_class` | `ObjectData::is(0x1BF, 0)` |

Facts are captured from those columns, `plan_recall` / `plan_return` are recomputed, and every
effect is applied under a snapshot: any step the host cannot apply restores the snapshot and
returns `Unavailable`, so a partial receiver is never published. That is the property
`RecallActionReceipt::validates` was written against.

`ignore_orders` is reported as `false` with an empty scenario selection because the reference
table holds no `ScenarioData` — the same truthful declaration `apply_stop_spell_transaction`
and `apply_follow_transaction` already make, and the same value ordinary multiplayer and
product execution carry. The planner rejects any fact set that disagrees with it.

### The children this host runs, and the two it does not

`RecallEffect::ClearPartialPath` and `UpdateAction` name `Unit::clear_partial_path`
`0x005E3920` (674 B) and `Unit::update_action` `0x0060A870` (485 B). Those are separate
receivers with their own owners and their own state columns, which this reference table does
not hold; the effects are recorded in the receipt and applied as no-ops. That is exactly the
treatment the already-`Port::Complete` `Group::action_stop_spell` row gives the same two
children, and `ObjectTable::take_air_unmodelled_children` makes each reached call visible to a
test rather than silent.

## 3. `Build::clear_gather` `0x00623180` — the one genuinely unread child

`action_recall`'s group-member pass ends in this call, so the row could not close without it.
Nobody had read past its first loop. The complete 390-byte body:

```text
while (this->gather_head (+0xCC) != 0) {                     ; 0x006231A9..0x006231E5
    node = list.current; list.remove_current(); free(node);
}
if (this->build_masks (+0x60) & 8) {                         ; 0x006231E7
    if (this->is(0x1BF, 0)) {                                ; 0x006231F3, devirt. 0x006231FA
        for (i = 0; i < players[who].num_units; i++) {       ; 0x00623215
            u = objects[who][i];
            if (!(u->flags(+8) & 1))              continue;  ; 0x0062324C
            if (u->type->domain(+0x218) != 2)     continue;  ; 0x00623259
            if (UnitData::home_base(u,&hw) != this->o) continue; ; 0x0062326D
            if (hw != this->who)                  continue;  ; 0x00623279
            if (u->is_on_map()) {                            ; 0x00623297, word +0x82 >> 15
                add_strafe_order(u, -1, -1, this->o, this->who, 0, QUEUE_NEW, 0);
            } else {
                Unit::clear_orders(u);                       ; 0x006232CA
                if (this->launching (+0x44)) launching.remove(i); ; 0x006232D7
            }
        }
    }
}
```

The `add_strafe_order` argument order at `0x006232C3` is the push sequence `-1, -1, o, who,
0, 2, 0` read bottom-up — the same seven-argument shape `action_recall` and `action_return`
install. Ghidra types `+0x44` as `Array<ScriptWatchWin *>`; it is the launching array, and the
off-map branch is the mirror of `action_recall`'s own `RemoveLaunchingSlot`.

`ObjectData::is` `0x00653790` is a 12-byte forwarder to `this->[+0x18]->vtable[+0x60]` — a
*type* query. Call sites comparing a vtable slot against `0x00653790` are devirtualizing it,
not testing a class.

## 4. Gates

`crates/don-sim/tests/air_containment_host.rs` drives every claim through
`Bridge::process_all` / `Bridge::process_one` with the real `ObjectTable` as the `Fleet`. No
test in that file constructs a receipt by hand.

- the air-leader branch reaching RETURN's helicopter route, pinning the targetless STRAFE,
  the `0x04000000` mask clear, `path.length = 0`, and both unmodelled children in order;
- the main body: a selected airbase drains its gather list, its contained aircraft loses its
  orders and its launching slot, and its airborne aircraft gets `returning = 1`, a STRAFE
  home, and its `cruising_alt` / `sharp_turn` preserved across the reinstall;
- `Build::clear_gather`'s second arm swept over both of its gates — dropping either
  `build_masks & 8` or the `0x1BF` class changes the outcome, so neither is decorative;
- fail-closed behaviour: a plan the planner refuses (`OwnerOutOfRange`) leaves the group
  sentinel, the order queues and the air columns byte-identical;
- `action_hotkey` copying the group and clearing a camera parked through the wire row, and
  refusing an out-of-range slot.

Two module tests carry the atomicity claim honestly. The snapshot restore in
`apply_recall_action` is a safety net rather than a live branch: on this host **every effect
the planner can emit is applicable**, which is precisely what lets the row be `Complete`, and
that is asserted directly over a real main-body plan. The two variants the planner cannot emit
here (`ScenarioKill`, `OpenActionReturnTail`) are asserted to refuse rather than silently
succeed, so the restore stays reachable if that ever changes.

Mutation-tested: no-oping the `0x04000000` mask clear, deleting `Build::clear_gather`'s second
arm, and dropping the camera invalidation each turn tests red. (Deleting the checkpoint restore
does *not* — see the paragraph above; that is a property of the host, not a hole in the tests.)

## 5. `Group::action_eject_all` `0x00710B40` — recovered, and deliberately still red

The receiver itself is not the problem; its body is short and now fully read:

```text
scenario ignore_orders prelude -> this->kill(o, who, 0, 0) per nonnegative entry ; 0x00710B47
this->action_begin()                                                             ; 0x00710BB6
BULK if (arg4 < 0 && arg2 == this->who) || arg2 < 0:                             ; 0x00710BBF
    form = -1; for each selected member, descending list order:                  ; 0x00710BD9
        require object_flags & 1, num_inside(1) > 0, can_carry(2) == 0
        eject_contents(0, arg1 ? 0x32 : -1, 0, arg1 ? 0 : 1)                      ; 0x00710C95
        if is(0x140,0) || is(0x13E,0):  if num_inside(0) == 0: die(0, -1, 0)      ; 0x00710D70
SINGLE else if arg3 >= 0:                                                        ; 0x00710D92
    walk (arg3, arg4)'s inside_down/inside_down_who chain (+0x28 / +0x3E); the first
    node owned by arg2 gets Unit::come_out(0); restart from the container         ; 0x00710E1E
```

Both arms bottom out in code that is **not** recovered, and inventing it is exactly the
failure mode this project forbids:

- `Object::eject_contents` `0x0064CD20` (2,962 B). `docs/mechanics/step8-eject-contents.md`
  recovered only the step-8-reachable slice, under fixed arguments
  `(kill_failed=1, filter=-1, transfer=0, reset=1)` and a Build carrier, and *explicitly*
  excluded the non-negative type/filter selector, the `reset == 0` arm, the `kill_failed == 0`
  arm, and the whole carrier-`Unit` suffix. `action_eject_all` calls it with
  `kill_failed = 0`, `filter = 0x32` or `-1`, and `reset` on both settings, over carriers that
  are usually units. Every one of the excluded subtrees is live here.
- `Unit::come_out` `0x00617C10` (9,925 B). `docs/assembly/unit-come-out-full-frontier.md`
  models `0x00617C10..0x006186B3` and stops at the common-release tail, leaving 7,201 bytes
  unrecovered.

So the row stays `StateWired` with its existing exact prefix. The critical path for whoever
takes it next is `Unit::come_out`'s common-release tail, not `action_eject_all` itself — and
that same tail also gates `transport` and `alarm`.

## Files

| path | what |
|---|---|
| `crates/don-sim/src/systems/hotkey_group_action.rs` | new — `Group::action_hotkey` plan |
| `crates/don-sim/src/systems/air_containment_host.rs` | new — RECALL/RETURN reference host + `Build::clear_gather` |
| `crates/don-sim/tests/air_containment_host.rs` | new — wire-driven gates |
| `crates/don-sim/src/command.rs` | minimal hunks: module decl, two `ObjectTable` fields + three accessors, `apply_recall_action_transaction`, `Bridge::action_hotkey` |
| `crates/don-sim/src/systems/mod.rs` | one export line |
| `crates/don-sim/src/command_tables.rs` | `port` of `hotkey` / `recall` / `return` |
| `crates/don-replay/src/bin/don-closure.rs` | port-class count assertions |
