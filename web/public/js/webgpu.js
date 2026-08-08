// WebGPU backend.
//
// # The whole design in one sentence
//
// The simulation's structure-of-arrays columns are bound *as separate vertex buffers with
// `stepMode: 'instance'`, so there is no interleaved per-instance struct anywhere in the
// system and therefore no repack step.
//
// That is the piece worth arguing for. The reflex when rendering N entities is to build an
// array of `{x, y, colour}` structs and upload it, which costs a full pass over every
// entity every frame, on the CPU, in the wrong language. A vertex buffer does not have to
// be interleaved: `pos_x` alone can be one buffer with `format: 'sint32'`, `pos_y` another,
// the tag column a third. The GPU's vertex fetch gathers them. So `don-sim`'s column layout
// — chosen for SIMD and cache reasons that have nothing to do with graphics — turns out to
// be *exactly* the layout the vertex stage wants, and the CPU-side per-frame work drops to
// `queue.writeBuffer(gpuBuf, 0, wasmMemory, byteOffset, byteLength)` per column.
//
// Corollaries that fall out:
//   - Positions stay `i32` in engine subtile units all the way to the vertex shader. No
//     `f32` conversion pass, no normalisation pass; the divide by `MAP_SPAN` happens once
//     per instance on the GPU.
//   - The tag column (owner + occupancy) changes only when a world's population changes,
//     so it is uploaded on a dirty flag rather than every frame.
//   - Quad corners come from `@builtin(vertex_index)`, so there is no geometry vertex
//     buffer at all — four vertices, `triangle-strip`, no index buffer.
//
// The one copy that remains is `queue.writeBuffer`. It cannot be removed: no browser API
// hands the GPU a pointer to a page the CPU is still writing. `mapAsync` + `getMappedRange`
// only relocates the same memcpy into user code and adds a round trip, and a persistently
// mapped ring would still need the sim to write *into* the mapping — which the sim cannot
// do, because it writes into wasm linear memory.

const TEAM_COLOURS = `
const TEAM: array<vec3f, 8> = array<vec3f, 8>(
  vec3f(0.35, 0.72, 1.00), vec3f(1.00, 0.45, 0.38), vec3f(0.55, 0.90, 0.55),
  vec3f(1.00, 0.82, 0.36), vec3f(0.78, 0.58, 1.00), vec3f(0.40, 0.94, 0.86),
  vec3f(1.00, 0.60, 0.85), vec3f(0.72, 0.76, 0.82));
`;

const SHADER = `
struct U {
  gridCols : u32,
  gridRows : u32,
  stride   : u32,
  mapSpan  : u32,
  panZoom  : vec4f,   // xy = pan in clip units, z = zoom, w = point size in device px
  viewport : vec2f,
  worlds   : u32,
  flags    : u32,
  extra    : vec4f,   // x = plate brightness, y = plate inset
};
@group(0) @binding(0) var<uniform> u : U;
${TEAM_COLOURS}

struct VS { @builtin(position) pos : vec4f, @location(0) uv : vec2f, @location(1) rgb : vec3f };

// Corner of the 4-vertex triangle strip, in [0,1]^2. No geometry buffer exists.
fn corner(vi : u32) -> vec2f {
  return vec2f(f32(vi & 1u), f32((vi >> 1u) & 1u));
}

// Off-screen position for an instance that must not be drawn. Both clip planes reject it,
// and a degenerate quad costs no fragments.
fn cull() -> vec4f { return vec4f(1e6, 1e6, 0.5, 1.0); }

@vertex
fn vs_units(@builtin(vertex_index) vi : u32,
            @builtin(instance_index) ii : u32,
            @location(0) px : i32,
            @location(1) py : i32,
            @location(2) tag : u32) -> VS {
  var out : VS;
  if ((tag & 0x80000000u) == 0u) { out.pos = cull(); out.uv = vec2f(0.0); out.rgb = vec3f(0.0); return out; }

  let world = ii / u.stride;
  let cell  = vec2f(f32(world % u.gridCols), f32(world / u.gridCols));
  // Engine subtile integers -> [0,1] inside the world. This divide is the only arithmetic
  // the position data receives anywhere in the pipeline.
  let inWorld = vec2f(f32(px), f32(py)) / f32(u.mapSpan);
  let g = (cell + inWorld) / vec2f(f32(u.gridCols), f32(u.gridRows));

  var p = vec2f(g.x * 2.0 - 1.0, 1.0 - g.y * 2.0);
  p = (p - u.panZoom.xy) * u.panZoom.z;
  let c = corner(vi) - vec2f(0.5);
  p = p + c * (u.panZoom.w * 2.0) / u.viewport;

  out.pos = vec4f(p, 0.5, 1.0);
  out.uv = corner(vi);
  out.rgb = TEAM[tag & 7u];
  return out;
}

@fragment
fn fs_units(in : VS) -> @location(0) vec4f {
  // Round sprites only when a sprite is big enough for the shape to be visible. The branch
  // is uniform across the draw, so it costs nothing per fragment in practice.
  if (u.panZoom.w > 2.5) {
    let d = in.uv - vec2f(0.5);
    let r = dot(d, d);
    if (r > 0.25) { discard; }
    let shade = 1.0 - r * 1.2;
    return vec4f(in.rgb * shade, 1.0);
  }
  return vec4f(in.rgb, 1.0);
}

// ---- aggregate view: one quad per world, coloured by that world's statistics ----------
struct AVS { @builtin(position) pos : vec4f, @location(0) uv : vec2f, @location(1) rgb : vec3f };

// Perceptually-ordered ramp; monotone in luminance so it still reads in greyscale.
fn ramp(t : f32) -> vec3f {
  let x = clamp(t, 0.0, 1.0);
  let a = vec3f(0.05, 0.06, 0.12);
  let b = vec3f(0.13, 0.35, 0.62);
  let c = vec3f(0.36, 0.74, 0.78);
  let d = vec3f(0.86, 0.80, 0.55);
  if (x < 0.34) { return mix(a, b, x / 0.34); }
  if (x < 0.67) { return mix(b, c, (x - 0.34) / 0.33); }
  return mix(c, d, (x - 0.67) / 0.33);
}

@vertex
fn vs_agg(@builtin(vertex_index) vi : u32,
          @builtin(instance_index) ii : u32,
          @location(0) st : vec4u) -> AVS {
  var out : AVS;
  if (ii >= u.worlds) { out.pos = cull(); out.uv = vec2f(0.0); out.rgb = vec3f(0.0); return out; }
  let cell = vec2f(f32(ii % u.gridCols), f32(ii / u.gridCols));
  let inset = u.extra.y;
  let k = corner(vi) * (1.0 - 2.0 * inset) + vec2f(inset);
  let g = (cell + k) / vec2f(f32(u.gridCols), f32(u.gridRows));
  var p = vec2f(g.x * 2.0 - 1.0, 1.0 - g.y * 2.0);
  p = (p - u.panZoom.xy) * u.panZoom.z;
  out.pos = vec4f(p, 0.5, 1.0);
  out.uv = corner(vi);
  // st.x = live units, st.y = summed hit points, st.z = digest low bits, st.w = frame.
  // flags bit 0 picks which scalar drives the ramp.
  let metric = select(f32(st.x) / f32(max(u.stride, 1u)),
                      f32(st.z & 0xffffu) / 65535.0,
                      (u.flags & 1u) != 0u);
  // extra.x is how much of the ramp to show. 1.0 is the aggregate view; ~0.3 makes the
  // same quad a dim *plate* under the units, so a grid of thousands of worlds reads as a
  // grid without the tiles out-shouting the units drawn on top. Same pipeline, same
  // buffer, one uniform apart -- which is why the cluster view costs one extra instanced
  // draw and nothing else.
  // (No backticks in this string: it is inside a JS template literal.)
  out.rgb = mix(vec3f(0.062, 0.070, 0.094), ramp(metric), u.extra.x);
  return out;
}

@fragment
fn fs_agg(in : AVS) -> @location(0) vec4f { return vec4f(in.rgb, 1.0); }
`;

export class WebGpuBackend {
  static get name() { return 'webgpu'; }

  #device; #ctx; #format; #uni; #bind; #unitPipe; #aggPipe; #uniStride = 256;
  #bufX = null; #bufY = null; #bufTag = null; #bufStats = null;
  #cells = 0; #worldsCap = 0;
  #canvas;
  #querySet = null; #queryBuf = null; #queryRead = null; #tsSupported = false;
  #lastGpuNs = 0; #queryBusy = false;

  static async create(canvas) {
    if (!navigator.gpu) throw new Error('navigator.gpu missing');
    const adapter = await navigator.gpu.requestAdapter({ powerPreference: 'high-performance' });
    if (!adapter) throw new Error('no WebGPU adapter');
    // Timestamp queries turn "the loop ran at N fps" into "the GPU was busy for M
    // microseconds", which is the number that actually says whether there is headroom.
    // Optional: it is a feature, not a guarantee, and the renderer must work without it.
    const wanted = adapter.features.has('timestamp-query') ? ['timestamp-query'] : [];
    const device = await adapter.requestDevice({
      requiredFeatures: wanted,
      // Ask for the adapter's own ceiling rather than the spec default: a cluster of 4096
      // worlds at full capacity is a 67 MB column, and the default `maxBufferSize` would
      // reject it. Requesting the adapter value can never fail.
      requiredLimits: { maxBufferSize: adapter.limits.maxBufferSize },
    });
    return new WebGpuBackend(device, canvas, wanted.length > 0, adapter);
  }

  constructor(device, canvas, ts, adapter) {
    this.#device = device;
    this.#canvas = canvas;
    this.#tsSupported = ts;
    // `GPUAdapterInfo`'s members are prototype getters, so a spread copies nothing. Read
    // them by name or the report says "adapter details unavailable" on a machine that
    // reported them perfectly well.
    const i = adapter.info || {};
    this.adapterInfo = { vendor: i.vendor, architecture: i.architecture, device: i.device, description: i.description };
    this.limits = { maxBufferSize: device.limits.maxBufferSize };
    this.#ctx = canvas.getContext('webgpu');
    this.#format = navigator.gpu.getPreferredCanvasFormat();
    this.#ctx.configure({ device, format: this.#format, alphaMode: 'opaque' });

    const module = device.createShaderModule({ code: SHADER });
    // Two uniform slots, not one. A render pass cannot have a buffer rewritten between its
    // draws, so the plate pass and the unit pass need distinct bytes; two bind groups over
    // one buffer at 256-byte alignment is the cheapest way to have both.
    this.#uniStride = Math.max(256, device.limits.minUniformBufferOffsetAlignment || 256);
    this.#uni = device.createBuffer({ size: this.#uniStride * 2, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
    const layout = device.createBindGroupLayout({
      entries: [{ binding: 0, visibility: GPUShaderStage.VERTEX | GPUShaderStage.FRAGMENT, buffer: { type: 'uniform' } }],
    });
    this.#bind = [0, 1].map((s) => device.createBindGroup({
      layout, entries: [{ binding: 0, resource: { buffer: this.#uni, offset: s * this.#uniStride, size: 64 } }],
    }));
    const pl = device.createPipelineLayout({ bindGroupLayouts: [layout] });

    // One vertex buffer per SoA column. `arrayStride: 4` with `stepMode: 'instance'` means
    // "advance one element of this column per instance" — the column IS the attribute.
    const col = (fmt) => ({ arrayStride: 4, stepMode: 'instance', attributes: [{ shaderLocation: 0, offset: 0, format: fmt }] });
    const bufs = [col('sint32'), col('sint32'), col('uint32')];
    bufs[1].attributes[0].shaderLocation = 1;
    bufs[2].attributes[0].shaderLocation = 2;

    this.#unitPipe = device.createRenderPipeline({
      layout: pl,
      vertex: { module, entryPoint: 'vs_units', buffers: bufs },
      fragment: { module, entryPoint: 'fs_units', targets: [{ format: this.#format }] },
      primitive: { topology: 'triangle-strip' },
    });
    this.#aggPipe = device.createRenderPipeline({
      layout: pl,
      vertex: {
        module, entryPoint: 'vs_agg',
        buffers: [{ arrayStride: 16, stepMode: 'instance', attributes: [{ shaderLocation: 0, offset: 0, format: 'uint32x4' }] }],
      },
      fragment: { module, entryPoint: 'fs_agg', targets: [{ format: this.#format }] },
      primitive: { topology: 'triangle-strip' },
    });

    if (this.#tsSupported) {
      this.#querySet = device.createQuerySet({ type: 'timestamp', count: 2 });
      this.#queryBuf = device.createBuffer({ size: 16, usage: GPUBufferUsage.QUERY_RESOLVE | GPUBufferUsage.COPY_SRC });
      this.#queryRead = device.createBuffer({ size: 16, usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ });
    }
  }

  get device() { return this.#device; }
  get timestampSupported() { return this.#tsSupported; }
  /** Nanoseconds the last resolved render pass occupied the GPU, or 0 if unavailable. */
  get lastGpuNs() { return this.#lastGpuNs; }

  /** (Re)allocate column buffers. Idempotent: a same-size call does nothing. */
  ensureCapacity(cells, worlds) {
    if (cells > this.#cells) {
      for (const b of [this.#bufX, this.#bufY, this.#bufTag]) b?.destroy();
      const usage = GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST;
      const bytes = cells * 4;
      this.#bufX = this.#device.createBuffer({ size: bytes, usage });
      this.#bufY = this.#device.createBuffer({ size: bytes, usage });
      this.#bufTag = this.#device.createBuffer({ size: bytes, usage });
      this.#cells = cells;
    }
    if (worlds > this.#worldsCap) {
      this.#bufStats?.destroy();
      this.#bufStats = this.#device.createBuffer({
        size: worlds * 16, usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST,
      });
      this.#worldsCap = worlds;
    }
  }

  /**
   * Upload one column slice. `src` is either a view over wasm linear memory (inline mode,
   * zero CPU copies before this point) or over the SharedArrayBuffer (worker mode, one).
   * WebGPU accepts `[AllowShared]` sources, so a SAB view needs no staging step.
   */
  writeColumn(which, dstElem, src, srcElem, elems) {
    const buf = which === 'x' ? this.#bufX : which === 'y' ? this.#bufY : which === 'tag' ? this.#bufTag : this.#bufStats;
    this.#device.queue.writeBuffer(buf, dstElem * 4, src, srcElem, elems);
  }

  setUniform({ gridCols, gridRows, stride, mapSpan, panX, panY, zoom, pointPx, vw, vh, worlds, flags,
               plate = 1, inset = 0.04 }, slot = 0) {
    const u32 = new Uint32Array(16);
    const f32 = new Float32Array(u32.buffer);
    u32[0] = gridCols; u32[1] = gridRows; u32[2] = stride; u32[3] = mapSpan;
    f32[4] = panX; f32[5] = panY; f32[6] = zoom; f32[7] = pointPx;
    f32[8] = vw; f32[9] = vh; u32[10] = worlds; u32[11] = flags;
    f32[12] = plate; f32[13] = inset;
    this.#device.queue.writeBuffer(this.#uni, slot * this.#uniStride, u32.buffer, 0, 64);
  }

  /** Encode and submit one frame. `mode` is 'units', 'aggregate', or 'cluster' (a dim
   *  per-world plate under the units, so a grid of thousands of worlds reads as a grid
   *  rather than as one field of noise). */
  draw(mode, instances, clear = [0.035, 0.04, 0.055, 1]) {
    const enc = this.#device.createCommandEncoder();
    const desc = {
      colorAttachments: [{
        view: this.#ctx.getCurrentTexture().createView(),
        clearValue: { r: clear[0], g: clear[1], b: clear[2], a: clear[3] },
        loadOp: 'clear', storeOp: 'store',
      }],
    };
    if (this.#tsSupported && !this.#queryBusy) {
      desc.timestampWrites = { querySet: this.#querySet, beginningOfPassWriteIndex: 0, endOfPassWriteIndex: 1 };
    }
    const pass = enc.beginRenderPass(desc);
    if (mode === 'aggregate') {
      pass.setBindGroup(0, this.#bind[0]);
      pass.setPipeline(this.#aggPipe);
      pass.setVertexBuffer(0, this.#bufStats);
      if (instances > 0) pass.draw(4, instances, 0, 0);
    } else {
      if (mode === 'cluster') {
        pass.setBindGroup(0, this.#bind[1]);
        pass.setPipeline(this.#aggPipe);
        pass.setVertexBuffer(0, this.#bufStats);
        pass.draw(4, this.#worldsCap, 0, 0);
      }
      pass.setBindGroup(0, this.#bind[0]);
      pass.setPipeline(this.#unitPipe);
      pass.setVertexBuffer(0, this.#bufX);
      pass.setVertexBuffer(1, this.#bufY);
      pass.setVertexBuffer(2, this.#bufTag);
      // One draw call. Every world, every unit.
      if (instances > 0) pass.draw(4, instances, 0, 0);
    }
    pass.end();
    if (desc.timestampWrites) {
      enc.resolveQuerySet(this.#querySet, 0, 2, this.#queryBuf, 0);
      enc.copyBufferToBuffer(this.#queryBuf, 0, this.#queryRead, 0, 16);
    }
    this.#device.queue.submit([enc.finish()]);
    if (desc.timestampWrites) this.#readTimestamps();
  }

  #readTimestamps() {
    this.#queryBusy = true;
    this.#queryRead.mapAsync(GPUMapMode.READ).then(() => {
      const t = new BigUint64Array(this.#queryRead.getMappedRange().slice(0));
      this.#queryRead.unmap();
      this.#lastGpuNs = Number(t[1] - t[0]);
      this.#queryBusy = false;
    }).catch(() => { this.#queryBusy = false; });
  }

  resize(w, h) {
    this.#canvas.width = w;
    this.#canvas.height = h;
  }

  /** Resolves when the GPU has finished everything submitted so far. The honest barrier
   *  for an uncapped throughput measurement — `requestAnimationFrame` only ever reports
   *  the display's refresh rate. */
  async waitIdle() { await this.#device.queue.onSubmittedWorkDone(); }
}
