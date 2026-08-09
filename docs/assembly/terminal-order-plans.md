# Terminal order planners: `CHANGE_FORM` and `THINK`

This lane recovers the two smallest still-red `Unit::do_job` bodies into
`systems::terminal_order_plans`. It deliberately does not edit the shared dispatcher.

## `CHANGE_FORM` — arm 18

Retail body: `Unit::do_form_change` `0x005E8670..0x005E86C3`, 86 bytes.

The PDB says `FormOrder` is 100 bytes. Its relevant flattened fields are
`MoveOrder::angle` at `+0x0C` and `FormOrder::newform` at `+0x50`; `delay` at `+0x54` is not
read by this executor.

The body is exactly:

1. Downcast the current order through virtual slot `+0x90` (`0x005E867D..0x005E8685`).
2. Read the order-list length at `UnitData+0xD8`, then store the low byte of `newform` in
   `UnitData::form` at `+0xAA` (`0x005E8685..0x005E868F`).
3. If the pre-retirement length equals one, call `Unit::set_angle(angle, ignored, 0)`
   (`0x005E8695..0x005E86A3`). The middle ABI value is dead: `Unit::set_angle`
   `0x00605400` never reads its second parameter.
4. Call `Unit::kill_current_order(0)` (`0x005E86A8..0x005E86AC`).
5. If the pre-retirement length was not one, call virtual `Unit::do_idle` at slot `+0x188`
   (`0x005E86B1..0x005E86B9`).

The apparently inverted idle condition is instruction-backed: a sole form order sets its
final angle before retirement and does not enter `do_idle`; a queued successor causes an idle
refresh after retirement.

## `THINK` — arm 27

Retail body: `Unit::do_think_order` `0x005E5BF0..0x005E5C63`, 116 bytes.

The body first calls `Unit::kill_current_order(0)` (`0x005E5BF2..0x005E5BF6`). If an order
remains, it performs the retail linked-list reset (`head->prev` into `current_node`, then node
data/metric into the cursor fields) and calls the successor's virtual `get_type` at `+0x10`
(`0x005E5BFB..0x005E5C37`). Any nonzero successor type returns immediately.

With no successor, or with a successor whose type is `NONE` (zero), it compares the actor's
exact `TypeIndex` against:

| value | PDB name |
|---:|---|
| 50 (`0x32`) | `PEASANTS` |
| 51 (`0x33`) | `PEASANTSKOREAN` |
| 52 (`0x34`) | `SCHOLARS` |
| 53 (`0x35`) | `SCHOLARSKOREAN` |

Only those four call `Unit::think_peasant(1)` (`0x005E5C3E..0x005E5C5C`). The adjacent type
IDs are not admitted.

## Atomic boundary and closure

`TerminalOrderRequest` binds the actor identity, exact queue-kind sequence, and the arm's
field reads. `TerminalOrderReceipt::validates` recomputes the ordered effect list. The
fail-closed `TerminalOrderHost` default returns `Unavailable` and performs no mutation.

Both arms can honestly move to `implemented` once the shared dispatcher calls one atomic host
transaction and accepts only a validated `Applied` receipt. That transaction must execute the
full nested effects:

- `CHANGE_FORM`: form store, exact `set_angle` side effects when selected, exact
  `kill_current_order(0)` lifecycle, and `do_idle` when selected.
- `THINK`: exact order retirement/list refresh and the complete `think_peasant(1)` call when
  selected.

Merely emitting these calls, or mutating the form byte before a callback can fail, is
`state_wired`, not complete. The expected closure delta after atomic dispatcher integration is
**+2 orders** (`CHANGE_FORM`, `THINK`), reducing the current order-red count by two.
