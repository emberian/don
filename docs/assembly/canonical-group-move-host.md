# Canonical Group → Move package host

Status: mounted on the canonical `Sim`, with a coherent DoNSave v13 receive-cache and order-node
metric boundary. Focused packet, atomicity, save/reload/resume, and compatibility gates are green.

## Evidence boundary

The admitted packet is the retail plaintext chronology

```text
[GroupCommand 0x00][MoveToCommand 0x07]
```

The finished retail replay contains 53 MoveTo commands. Every one is terminal and immediately
adjacent to Group; 41 packages are exactly this two-command shape. The remaining twelve carry a
Camera and, once, PlayerSpeed prefix. Those prefixes are evidence for the future whole-package
shell, not permission for this bounded transaction to discard bytes.

The replay also proves ten empty Group re-selections, including repeated empty selections which
continue to reference the last explicit selection. Static RE closes the owner: the shipped globals
are `last_num_received[8]`, `last_objects_received[8][128]`, and
`last_uids_received[8][128]`, indexed by package `play`. `process_group` separately verifies the
wire `who` against `GameInfo.player[play].who`. The cache therefore lives beside `Sim`, not in
`groups_guys::Groups`, and is keyed by seat rather than Leader owner.

The fresh retail SVX independently proves the canonical Group pool shape: 512 physical slots,
64 per owner, physical `Group.id`, eight `last_group` values, and a separate round-robin cursor.
It is a different match from the replay and is used only as an independent structural witness.

No replay/save bytes are committed. No live process mutation was used.

## One owner and one transaction

```text
bytes + play + serial
        |
        v
strict Group/Move decoder -- all nine Move fields
        |
        v
play -> who check ---- CommandPackageState[play]
        |
        v
World (who,o,uid,Handle) + exact member/type authority
        |
        v
fixed groups_guys::Groups allocator / formation plan
        |
        v
complete after-images:
  Groups + last_group
  Unit.group + unit_masks + OrderList + PathStack + orders_x/y
  selection cache
        |
        v
revalidate every exact before-image and Handle generation
        |
        v
assignment-only atomic commit
```

The public `Sim::process_command_package(play, lockstep_serial, bytes)` returns a typed receipt
containing frame, owner, canonical Group slot, selected stable identities, receive-state revision,
the retail Groups-channel checksum, and RNG before/after. Movement commands consume no RNG, so a
successful receipt requires the two RNG states to be identical.

`command::Bridge` and its private dynamic `command::Groups` are not dependencies. The fixed
save/checksum pool, canonical World registry, walked `OrderList`, and walked paths are the only
mutable gameplay owners.

## Admitted behavior

- Explicit Group lists cache the received `(o,uid)` sequence by `play`; duplicates remain in the
  cache while the effective Group is deduplicated.
- Empty Group lists read the cache without normalizing it. Dead, missing, or UID-mismatched rows
  are skipped for that invocation and stay cached.
- Every live selected Unit is resolved as `(who,o,uid,Handle)`. Commit repeats address lookup,
  Handle lookup, active, owner, object index, UID, and every affected before-image comparison.
- Fresh allocation uses the exact retail ascending preference for the first non-current empty
  slot. It mutates the scanned `get_num` images before publishing, as retail does.
- `QUEUE_NEW` replaces the order, clears the walked path, clears constructor/action mask bits, and
  publishes the new action coordinates. `QUEUE_LAST` appends and preserves the current path.
- Formation computation uses the recovered `GroupData::compute_form` transaction. `x`, `y`,
  `set_angle`, `angle`, `orders`, `queued`, `form`, `width`, and `disembark` are decoded and
  mutation-tested.
- Concrete Move/GroupMove orders carry a complete `MoveOrderState`. Constructor remainders,
  original click, group identity, member index, angle, facing, and flags are not dropped.

## Fail-closed residuals

The transaction refuses before mutation when it reaches an unmounted owner:

- Camera/PlayerSpeed package prefixes;
- scenario `ignore_orders` pruning;
- recursive `o_down` selection expansion or a split move-near result;
- building/all-band normalization during allocation;
- the full-pool one-captain LRU/fallback allocator arm;
- `QUEUE_FIRST`'s set-up/halt/reissue/finish-insert choreography;
- replacement of an ATTACK_TO special case which can delegate a target-domain child; or
- a queued targeted order whose final target location cannot be resolved from the Unit-only view.

These are typed errors and leave Groups, backlinks, cache, orders, paths, clocks, and RNG exactly
unchanged. They are not approximated by the shadow bridge.

## Save boundary

The selection cache must be saved or command execution must refuse across reload. DoNSave v13 adds
the required `COMMAND_PACKAGE_STATE` leaf containing eight length-prefixed `(i16 o,u16 uid)` rows;
the exact retail GROUPS leaf remains unextended. The typed-order threshold stays independently
frozen at v12, so v7–v12 order bytes remain exact. V13 also reserves the exact per-order-list-node
`metric:u8` immediately before the existing generic Order record. Until `OrderList` owns that
retail field, the producer emits zero and the reader rejects nonzero or truncated metrics instead
of silently dropping them. Versions 7–12 load an empty receive cache. A v13 nonempty cache requires
an installed PlayerTable both when saving and after loading; the Handle-bound content projection is
deliberately reinstalled after load before command execution can resume.
