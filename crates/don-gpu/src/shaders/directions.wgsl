// Flow-direction extraction: for every cell, the index into the fixed neighbour order of
// the neighbour that realises this cell's distance. Ties break to the lowest index, and
// the order matches `don_gpu::field::NEIGHBOURS` exactly, so CPU and GPU break ties the
// same way. Integer only, therefore order-independent and exactly reproducible.

struct Params {
    width: u32,
    height: u32,
    fields: u32,
    inner_steps: u32,
    field_offset: u32,
    stride: u32,        // threads per y-row of the dispatch = workgroups_x * 64
    pad0: u32,
    pad1: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> cost: array<u32>;
@group(0) @binding(2) var<storage, read> dist: array<u32>;
@group(0) @binding(3) var<storage, read_write> dirs: array<u32>;

const INF: u32 = 0x7fffffffu;
const BLOCKED: u32 = 0xffffffffu;
const W_CARD: u32 = 2u;
const W_DIAG: u32 = 3u;
const DIR_NONE: u32 = 255u;

@compute @workgroup_size(64, 1, 1)
fn directions(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cells = params.width * params.height;
    let total = cells * params.fields;
    let i = gid.x + gid.y * params.stride;
    if (i >= total) {
        return;
    }
    let d = dist[i];
    if (d == 0u || d >= INF) {
        dirs[i] = DIR_NONE;
        return;
    }
    let within = i % cells;
    let base = i - within;
    let x = i32(within % params.width);
    let y = i32(within / params.width);

    var c2: u32 = INF;
    var c3: u32 = INF;
    let c = cost[i];
    if (c != BLOCKED) {
        c2 = W_CARD * c;
        c3 = W_DIAG * c;
    }

    var best: u32 = 0xffffffffu;
    var arg: u32 = DIR_NONE;
    for (var k: u32 = 0u; k < 8u; k = k + 1u) {
        var dx: i32 = 0;
        var dy: i32 = 0;
        var cw: u32 = c2;
        switch (k) {
            case 0u:      { dx =  0; dy = -1; cw = c2; }
            case 1u:      { dx =  1; dy = -1; cw = c3; }
            case 2u:      { dx =  1; dy =  0; cw = c2; }
            case 3u:      { dx =  1; dy =  1; cw = c3; }
            case 4u:      { dx =  0; dy =  1; cw = c2; }
            case 5u:      { dx = -1; dy =  1; cw = c3; }
            case 6u:      { dx = -1; dy =  0; cw = c2; }
            default:      { dx = -1; dy = -1; cw = c3; }
        }
        let nx = x + dx;
        let ny = y + dy;
        if (nx >= 0 && ny >= 0 && nx < i32(params.width) && ny < i32(params.height)) {
            let nd = dist[base + u32(ny) * params.width + u32(nx)];
            let cand = nd + cw;
            if (cand < best) {
                best = cand;
                arg = k;
            }
        }
    }
    dirs[i] = arg;
}
