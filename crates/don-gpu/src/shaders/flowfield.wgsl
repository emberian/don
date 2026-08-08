// Batched integer flow-field relaxation.
//
// Everything here is u32 min-plus arithmetic. There is not one floating point value in
// this file, and that is the entire determinism argument: `min` and integer `+` are exact,
// `min` is associative/commutative/idempotent, so the fixed point does not depend on the
// order the GPU happens to schedule anything.
//
// One workgroup owns one TILE x TILE tile of one field. It loads the tile plus a one-cell
// halo into workgroup memory and runs `inner_steps` Jacobi relaxations there before
// writing back — temporal blocking, which trades a stale halo for a factor of
// `inner_steps` less global traffic. A stale halo is *sound* here because every step is
// `d = min(d, neighbour + w)`: values only ever decrease and can never fall below the true
// distance, so an under-informed step is a slower step, never a wrong one.

struct Params {
    width: u32,
    height: u32,
    fields: u32,
    inner_steps: u32,
    field_offset: u32,   // first field this dispatch covers (z is capped at 65535)
    stride: u32,         // linear kernels only: threads per y-row of the dispatch
    pad0: u32,
    pad1: u32,
};

struct Flags {
    changed: atomic<u32>,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> cost: array<u32>;
@group(0) @binding(2) var<storage, read> src: array<u32>;
@group(0) @binding(3) var<storage, read_write> dst: array<u32>;
@group(0) @binding(4) var<storage, read_write> flags: Flags;

const TILE: u32 = 8u;
const HALO: u32 = 10u;      // TILE + 2
const HALO_CELLS: u32 = 100u;
const THREADS: u32 = 64u;   // TILE * TILE

const INF: u32 = 0x7fffffffu;
const BLOCKED: u32 = 0xffffffffu;
const W_CARD: u32 = 2u;
const W_DIAG: u32 = 3u;

var<workgroup> sd: array<u32, 100>;
var<workgroup> wg_changed: atomic<u32>;

@compute @workgroup_size(8, 8, 1)
fn relax(
    @builtin(workgroup_id) wg: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(local_invocation_index) li: u32,
) {
    let field = wg.z + params.field_offset;
    let base = field * params.width * params.height;
    let tx = wg.x * TILE;   // tile origin in field coordinates
    let ty = wg.y * TILE;

    if (li == 0u) {
        atomicStore(&wg_changed, 0u);
    }

    // ---- load tile + halo, out-of-grid reads become INF -------------------------------
    // 100 cells, 64 threads: two strided passes.
    for (var k: u32 = li; k < HALO_CELLS; k = k + THREADS) {
        let hx = k % HALO;
        let hy = k / HALO;
        // halo cell (hx,hy) maps to field cell (tx + hx - 1, ty + hy - 1)
        let gx = i32(tx) + i32(hx) - 1;
        let gy = i32(ty) + i32(hy) - 1;
        var v: u32 = INF;
        if (gx >= 0 && gy >= 0 && gx < i32(params.width) && gy < i32(params.height)) {
            v = src[base + u32(gy) * params.width + u32(gx)];
        }
        sd[k] = v;
    }

    // ---- this thread's own interior cell ----------------------------------------------
    let cx = tx + lid.x;
    let cy = ty + lid.y;
    let inside = cx < params.width && cy < params.height;
    var gi: u32 = 0u;
    if (inside) {
        gi = base + cy * params.width + cx;
    }

    var c2: u32 = INF;
    var c3: u32 = INF;
    if (inside) {
        let c = cost[gi];
        if (c != BLOCKED) {
            c2 = W_CARD * c;
            c3 = W_DIAG * c;
        }
    }
    let idx = (lid.y + 1u) * HALO + (lid.x + 1u);

    workgroupBarrier();
    let original = sd[idx];

    // ---- temporally blocked Jacobi -----------------------------------------------------
    // `params.inner_steps` is uniform across the whole dispatch, so both barriers sit in
    // uniform control flow, as WGSL requires.
    for (var s: u32 = 0u; s < params.inner_steps; s = s + 1u) {
        let card = min(min(sd[idx - HALO], sd[idx - 1u]), min(sd[idx + 1u], sd[idx + HALO]));
        let diag = min(min(sd[idx - HALO - 1u], sd[idx - HALO + 1u]),
                       min(sd[idx + HALO - 1u], sd[idx + HALO + 1u]));
        // No overflow: distances are <= INF (2^31-1), c3 <= 3*MAX_COST (< 2^26), and a
        // blocked cell carries c2 = c3 = INF, so the largest sum is < 2^32.
        let nd = min(sd[idx], min(card + c2, diag + c3));
        workgroupBarrier();
        sd[idx] = nd;
        workgroupBarrier();
    }

    let final_d = sd[idx];
    if (inside) {
        dst[gi] = final_d;
        if (final_d != original) {
            atomicOr(&wg_changed, 1u);
        }
    }
    workgroupBarrier();
    if (li == 0u && atomicLoad(&wg_changed) != 0u) {
        atomicOr(&flags.changed, 1u);
    }
}
