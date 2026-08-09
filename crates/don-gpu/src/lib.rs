//! Batched integer flow-field relaxation: CPU reference kernels and a wgpu compute port.
//!
//! # What this crate is
//!
//! A prototype of the one simulation work class that is an unambiguous GPU fit: a **local,
//! iterative, convergent** field solve over a tile grid, batched across many independent
//! worlds. It exists to answer a measurable question — *at what batch size, if any, does a
//! GPU beat the CPU on this kernel* — and to establish the determinism discipline that any
//! GPU kernel in this project has to follow.
//!
//! It implements **no Rise of Nations mechanic**. The engine's own pathfinder is not
//! derived (`docs/provenance-ledger.md` lists it under "not yet derived"), `PathFinder` and
//! `BorderSpline` are still just RTTI class names, and nothing here should be read as a
//! claim about how the game computes anything. The cost model, the chamfer weights and the
//! synthetic terrain are **ours**, chosen to exercise the hardware. When the real
//! pathfinder is derived, the *shape* here — batched, integer, relaxation-based — is what
//! survives; the numbers are all replaceable.
//!
//! # The determinism argument
//!
//! GPU floating point is not the problem people usually think it is: a single `f32` add is
//! exactly rounded on any conformant device. The problem is **order**. A parallel reduction
//! sums in whatever order the scheduler picks, and `f32` addition is not associative, so
//! the same input can give different bits on different runs, let alone different vendors.
//! For a lockstep-faithful RTS simulation that is fatal.
//!
//! This kernel sidesteps the whole issue rather than mitigating it. It computes the least
//! fixed point of a **min-plus (tropical) relaxation** over `u32`:
//!
//! ```text
//! d[c] = min( d[c],  min over neighbours n of ( d[n] + w(n,c) * cost[c] ) )
//! ```
//!
//! - `min` on `u32` is associative, commutative **and idempotent**;
//! - integer `+` is associative and exact, and the operand ranges are chosen so nothing
//!   wraps (see [`cpu::MAX_COST`]);
//! - every step is monotone decreasing and bounded below by the true distance.
//!
//! Three consequences, and they are *structural* properties of the operator, not
//! observations from a test run:
//!
//! 1. **The fixed point is unique.** Any schedule that keeps relaxing until nothing changes
//!    lands on the same array of bits. Jacobi, Gauss-Seidel, Dijkstra, one thread or forty
//!    thousand — same answer.
//! 2. **Partial and stale information is safe.** The GPU kernel deliberately relaxes
//!    against a *stale halo* in workgroup memory. That can only slow convergence; it can
//!    never produce a value below the true distance.
//! 3. **Convergence detection does not break reproducibility.** Overshooting the fixed
//!    point by a few rounds is a no-op, so the "run until the changed-flag stays clear"
//!    loop is reproducible even though the round count is not part of the answer.
//!
//! The crate's tests assert exactly this: three independent CPU implementations and the GPU
//! agree bit-for-bit, and the GPU result is invariant under changes to `inner_steps` and
//! `rounds_per_poll` — i.e. under changes to the parallel schedule.
//!
//! **The generalisation, and its limit.** This works because the operator is an idempotent
//! semiring `min`. It carries over to influence maps built from `max`, to fog-of-war
//! visibility (boolean `or`), to territory ownership (argmax with a fixed tie-break), and
//! to any integer or fixed-point accumulation. It does **not** carry over to an `f32` sum
//! reduction, and no amount of care makes it. Where a system needs a sum — economy income
//! aggregated over gatherers, for instance — the answer is fixed-point integer
//! accumulation, or keeping that system on the CPU. See
//! `docs/derivation/gpu-architecture.md`.
//!
//! # The other half: the ECS factoring that makes batching possible at all
//!
//! [`arena`], [`orders`], [`partition`] and [`reduce`] are not about the flow field. They
//! answer a different objection — *"order execution must stay CPU-bound because it is
//! branchy"* — which is false as stated: **branchiness is a property of the layout, not of
//! the problem.** The engine dispatches per-unit behaviour through 28 `Order` archetypes
//! ([`orders`], names [measured] from `schema/symbols.json`); ported directly that is one
//! indirect call and one mispredict per entity per tick. Bucketed by order *first*
//! ([`partition`]) the same computation is 28 dense homogeneous kernels with no dispatch
//! inside any of them.
//!
//! [`reduce`] is the companion rule: **never use atomics for accumulation.** Many-to-one
//! conflicts resolve by segmented reduce over a sorted key, which is deterministic by
//! construction. Integer `wrapping_add` is the abelian group `Z/2^32`, so reduction order
//! cannot move a bit — and the sim *is* integers, so this is the normal case rather than a
//! lucky one. Where order genuinely matters (drawing from a pool that can run dry) a
//! segmented exclusive **scan** reproduces the sequential answer exactly instead. The module
//! docs list, explicitly, the cases where none of this holds.
//!
//! # Layout
//!
//! [`field::FieldBatch`] holds one flat column per quantity, spanning every field in the
//! batch, with the field index implicit in the offset. That is the Madrona column-store
//! shape: the column *is* the device buffer and *is* the observation tensor, so there is no
//! per-world allocation, no gather on export, and the GPU upload is one `write_buffer`.

pub mod arena;
pub mod cpu;
pub mod field;
pub mod gpu;
pub mod orders;
pub mod partition;
pub mod reduce;

pub use arena::{Arena, Mix, PhaseA, Resolve, StepPlan};
pub use cpu::{solve_batch_parallel, solve_batch_serial, CpuKernel, DialSolver};
pub use field::{FieldBatch, COST_BLOCKED, DIR_NONE, INF, NEIGHBOURS};
pub use gpu::{solve_batch_gpu, FlowSolver, Gpu, GpuUnavailable, SolveOptions, SolveStats};
pub use orders::{Order, Shape, ORDER_COUNT, ORDER_NAMES, ORDER_PARAMS};
pub use partition::{KeySort, Partition};
pub use reduce::{accumulate_i32, draw_from_pools, ReduceScratch};
