// The renderer: terrain and objects, both drawn straight out of wasm memory.
//
// # The zero-copy claim, precisely
//
// The object pass binds the simulation's own `pos_x` / `pos_y` / `tag` allocations as three
// per-instance vertex buffers. There is no interleaved instance struct, no repack, no
// `f32` conversion and no per-entity JavaScript loop anywhere in the frame — the only copy
// is the one `writeBuffer` performs on upload, which no graphics API lets us avoid.
// Terrain is a `r16uint` texture holding `TData::mask` verbatim, uploaded only when
// `game_terrain_version` changes, and every colour on screen is decided from the engine's
// own tile bits in the fragment shader.
//
// # Failure channel first
//
// WebGPU reports validation failures as *events*, not exceptions: a bad pipeline still
// returns an object and every draw with it is silently dropped. The spectator lane lost a
// day to exactly that, so every pipeline creation here is wrapped in an error scope, the
// uncaptured-error handler is installed before the first draw, and `readback()` exists so
// "the canvas is black" can be turned into a pixel count.

const WGSL = `
struct Cam {
  org  : vec2f,   // camera top-left, in subtiles
  px   : f32,     // device pixels per subtile
  span : f32,     // map extent in subtiles
  vp   : vec2f,   // viewport in device pixels
  tiles: f32,     // map extent in tiles
  sub  : f32,     // subtiles per tile
};
@group(0) @binding(0) var<uniform> cam : Cam;
@group(0) @binding(1) var terrain : texture_2d<u32>;

fn toNdc(world : vec2f) -> vec2f {
  let p = (world - cam.org) * cam.px;
  return vec2f(p.x / cam.vp.x * 2.0 - 1.0, 1.0 - p.y / cam.vp.y * 2.0);
}

// ---- terrain ---------------------------------------------------------------------------

struct TerrOut { @builtin(position) pos : vec4f, @location(0) tile : vec2f };

@vertex
fn vs_terrain(@builtin(vertex_index) vi : u32) -> TerrOut {
  var corner = array<vec2f, 6>(
    vec2f(0.0, 0.0), vec2f(1.0, 0.0), vec2f(0.0, 1.0),
    vec2f(1.0, 0.0), vec2f(1.0, 1.0), vec2f(0.0, 1.0));
  let c = corner[vi];
  var o : TerrOut;
  o.pos = vec4f(toNdc(c * cam.span), 0.0, 1.0);
  o.tile = c * cam.tiles;
  return o;
}

fn hash2(p : vec2f) -> f32 {
  let h = fract(sin(dot(p, vec2f(127.1, 311.7))) * 43758.5453);
  return h;
}

@fragment
fn fs_terrain(i : TerrOut) -> @location(0) vec4f {
  let t = vec2i(clamp(i.tile, vec2f(0.0), vec2f(cam.tiles - 1.0)));
  let m = textureLoad(terrain, t, 0).r;
  let blocker = m & 3u;
  let surface = m & 0x30u;
  let n = hash2(floor(i.tile)) * 0.10 - 0.05;

  var c = vec3f(0.30, 0.38, 0.22) + n;              // grass
  if (surface == 0x20u) { c = vec3f(0.09, 0.20, 0.36) + n * 0.4; }   // water
  else if (surface == 0x30u) { c = vec3f(0.11, 0.28, 0.14) + n * 0.7; } // trees
  else if (surface == 0x10u) { c = vec3f(0.42, 0.35, 0.24) + n * 0.3; } // road
  if (blocker == 2u) { c = vec3f(0.40, 0.38, 0.36) + n; }            // mountain
  else if (blocker == 1u) { c = vec3f(0.30, 0.28, 0.27) + n; }       // cliff
  else if (blocker == 3u) { c = vec3f(0.20, 0.19, 0.22); }           // building

  // Tile grid, so the placement grid a player is judging distance against is visible.
  let f = fract(i.tile);
  let g = min(min(f.x, f.y), min(1.0 - f.x, 1.0 - f.y));
  if (cam.px * cam.sub > 14.0 && g < 0.02) { c = c * 0.88; }
  return vec4f(c, 1.0);
}

// ---- objects ---------------------------------------------------------------------------

struct ObjOut {
  @builtin(position) pos : vec4f,
  @location(0) uv   : vec2f,
  @location(1) rgba : vec4f,
  @location(2) st : vec4f,   // hp, selected, building, construction
};

const OWNER_COLOR = array<vec3f, 8>(
  vec3f(0.36, 0.62, 1.00),   // 0 blue
  vec3f(1.00, 0.36, 0.30),   // 1 red
  vec3f(0.42, 0.85, 0.48),   // 2 green
  vec3f(1.00, 0.75, 0.28),   // 3 amber
  vec3f(0.80, 0.50, 0.95),   // 4 violet
  vec3f(0.35, 0.85, 0.88),   // 5 cyan
  vec3f(0.95, 0.55, 0.80),   // 6 pink
  vec3f(0.75, 0.75, 0.75));  // 7 grey

@vertex
fn vs_obj(@builtin(vertex_index) vi : u32,
          @location(0) px : i32, @location(1) py : i32, @location(2) tag : u32) -> ObjOut {
  var o : ObjOut;
  if ((tag & 0x80000000u) == 0u) { o.pos = vec4f(2.0, 2.0, 2.0, 1.0); return o; }
  var corner = array<vec2f, 6>(
    vec2f(-1.0, -1.0), vec2f(1.0, -1.0), vec2f(-1.0, 1.0),
    vec2f(1.0, -1.0), vec2f(1.0, 1.0), vec2f(-1.0, 1.0));
  let c = corner[vi];

  let isBuilding = (tag >> 29u) & 1u;
  let size = f32((tag >> 24u) & 0xfu);
  let hp = f32((tag >> 16u) & 0xffu) / 255.0;
  let hue = f32((tag >> 8u) & 0xffu);
  let carry = (tag >> 4u) & 0xfu;
  let owner = tag & 0xfu;

  // Half-extent in subtiles. A building's is its real footprint; a unit's is a bulk class
  // derived from HITS, and is a readability choice, not a modelled radius.
  var half = cam.sub * (0.20 + size * 0.035);
  if (isBuilding == 1u) { half = cam.sub * size * 0.5; }
  // Never let a unit shrink below a couple of pixels, or an army becomes invisible.
  half = max(half, 2.4 / max(cam.px, 0.0001));

  let world = vec2f(f32(px), f32(py)) + c * half;
  o.pos = vec4f(toNdc(world), 0.0, 1.0);
  o.uv = c;

  var base = OWNER_COLOR[owner & 7u];
  // Type hue is the low byte of the real type_id: two different units of one player are
  // visibly different without inventing a sprite for each of 364 types.
  let tint = fract(hue / 37.0) * 0.30 - 0.15;
  base = clamp(base + vec3f(tint, tint * 0.5, -tint), vec3f(0.0), vec3f(1.0));
  if (carry > 0u) { base = mix(base, vec3f(0.98, 0.86, 0.35), 0.35); }
  o.rgba = vec4f(base, 1.0);
  o.st = vec4f(hp, f32((tag >> 30u) & 1u), f32(isBuilding), f32((tag >> 28u) & 1u));
  return o;
}

@fragment
fn fs_obj(i : ObjOut) -> @location(0) vec4f {
  let hp = i.st.x;
  let sel = i.st.y;
  let building = i.st.z;
  let underCon = i.st.w;

  var a = 1.0;
  var c = i.rgba.rgb;
  if (building < 0.5) {
    let r = length(i.uv);
    if (r > 1.0) { discard; }
    c = c * (0.55 + 0.45 * (1.0 - r * r));
    if (sel > 0.5 && r > 0.68) { c = vec3f(1.0, 1.0, 1.0); }
  } else {
    let e = max(abs(i.uv.x), abs(i.uv.y));
    if (e > 0.94) { c = c * 0.55; }
    if (sel > 0.5 && e > 0.86) { c = vec3f(1.0, 1.0, 1.0); }
    if (underCon > 0.5) {
      // Diagonal hatch, and only the built fraction is opaque.
      let s = fract((i.uv.x + i.uv.y) * 3.0);
      c = mix(c * 0.4, c, step(0.5, s));
      if (i.uv.y < 1.0 - 2.0 * hp) { a = 0.35; }
    }
  }
  // Damage reads as darkening plus a red cast, so a hurt army is legible at a glance.
  if (hp < 0.999) { c = mix(vec3f(0.55, 0.10, 0.08), c, 0.35 + 0.65 * hp); }
  return vec4f(c, a);
}
`;

/** Uniform block, 8 floats. */
const UNIFORM_FLOATS = 8;

export class WebGPURenderer {
  constructor(canvas) { this.canvas = canvas; this.kind = 'webgpu'; this.errors = []; }

  async init() {
    if (!navigator.gpu) throw new Error('navigator.gpu is absent');
    const adapter = await navigator.gpu.requestAdapter();
    if (!adapter) throw new Error('no WebGPU adapter');
    const device = await adapter.requestDevice();
    this.device = device;
    device.addEventListener('uncapturederror', (e) => {
      this.errors.push(String(e.error && e.error.message ? e.error.message : e.error));
    });
    device.lost.then((info) => this.errors.push('device lost: ' + info.message));

    const ctx = this.canvas.getContext('webgpu');
    this.ctx = ctx;
    this.format = navigator.gpu.getPreferredCanvasFormat();
    ctx.configure({ device, format: this.format, alphaMode: 'opaque' });

    const module = device.createShaderModule({ code: WGSL });
    const info = await module.getCompilationInfo();
    for (const m of info.messages) {
      if (m.type === 'error') this.errors.push(`shader ${m.lineNum}: ${m.message}`);
    }
    if (this.errors.length) throw new Error(this.errors.join('; '));

    this.uniform = device.createBuffer({
      size: UNIFORM_FLOATS * 4,
      usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
    });
    this.layout = device.createBindGroupLayout({
      entries: [
        { binding: 0, visibility: GPUShaderStage.VERTEX | GPUShaderStage.FRAGMENT,
          buffer: { type: 'uniform' } },
        { binding: 1, visibility: GPUShaderStage.FRAGMENT,
          texture: { sampleType: 'uint' } },
      ],
    });
    const pl = device.createPipelineLayout({ bindGroupLayouts: [this.layout] });

    this.terrainPipe = await this._pipeline('terrain', {
      layout: pl,
      vertex: { module, entryPoint: 'vs_terrain' },
      fragment: { module, entryPoint: 'fs_terrain', targets: [{ format: this.format }] },
      primitive: { topology: 'triangle-list' },
    });
    this.objPipe = await this._pipeline('objects', {
      layout: pl,
      vertex: {
        module, entryPoint: 'vs_obj',
        buffers: [
          { arrayStride: 4, stepMode: 'instance',
            attributes: [{ shaderLocation: 0, offset: 0, format: 'sint32' }] },
          { arrayStride: 4, stepMode: 'instance',
            attributes: [{ shaderLocation: 1, offset: 0, format: 'sint32' }] },
          { arrayStride: 4, stepMode: 'instance',
            attributes: [{ shaderLocation: 2, offset: 0, format: 'uint32' }] },
        ],
      },
      fragment: {
        module, entryPoint: 'fs_obj',
        targets: [{
          format: this.format,
          blend: {
            color: { srcFactor: 'src-alpha', dstFactor: 'one-minus-src-alpha' },
            alpha: { srcFactor: 'one', dstFactor: 'one-minus-src-alpha' },
          },
        }],
      },
      primitive: { topology: 'triangle-list' },
    });
    return this;
  }

  /** Create a pipeline inside an error scope, so a validation failure is not silent. */
  async _pipeline(name, desc) {
    this.device.pushErrorScope('validation');
    const p = this.device.createRenderPipeline(desc);
    const err = await this.device.popErrorScope();
    if (err) {
      this.errors.push(`${name} pipeline: ${err.message}`);
      throw new Error(`${name} pipeline: ${err.message}`);
    }
    return p;
  }

  /** (Re)allocate the terrain texture and the three instance buffers. */
  provision(tiles, capacity) {
    const d = this.device;
    if (this.terrainTex) this.terrainTex.destroy();
    this.terrainTex = d.createTexture({
      size: [tiles, tiles], format: 'r16uint',
      usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST,
    });
    this.bind = d.createBindGroup({
      layout: this.layout,
      entries: [
        { binding: 0, resource: { buffer: this.uniform } },
        { binding: 1, resource: this.terrainTex.createView() },
      ],
    });
    const mk = () => d.createBuffer({
      size: capacity * 4, usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST,
    });
    for (const b of [this.bx, this.by, this.bt]) if (b) b.destroy();
    this.bx = mk(); this.by = mk(); this.bt = mk();
    this.capacity = capacity;
    this.tiles = tiles;
  }

  uploadTerrain(tileView) {
    this.device.queue.writeTexture(
      { texture: this.terrainTex }, tileView,
      { bytesPerRow: this.tiles * 2, rowsPerImage: this.tiles },
      [this.tiles, this.tiles]);
  }

  /**
   * Draw one frame. `v` holds the simulation's live views; `n` is the live object count.
   * Returns the milliseconds spent uploading, which is the only CPU cost that scales with
   * the object count.
   */
  draw(v, n, cam, vp) {
    const d = this.device;
    const t0 = performance.now();
    if (n > 0) {
      d.queue.writeBuffer(this.bx, 0, v.x.buffer, v.x.byteOffset, n * 4);
      d.queue.writeBuffer(this.by, 0, v.y.buffer, v.y.byteOffset, n * 4);
      d.queue.writeBuffer(this.bt, 0, v.tag.buffer, v.tag.byteOffset, n * 4);
    }
    const u = new Float32Array(UNIFORM_FLOATS);
    u[0] = cam.x; u[1] = cam.y; u[2] = cam.px; u[3] = cam.span;
    u[4] = vp[0]; u[5] = vp[1]; u[6] = cam.tiles; u[7] = cam.sub;
    d.queue.writeBuffer(this.uniform, 0, u);
    const upload = performance.now() - t0;

    const enc = d.createCommandEncoder();
    const pass = enc.beginRenderPass({
      colorAttachments: [{
        view: this.ctx.getCurrentTexture().createView(),
        clearValue: { r: 0.03, g: 0.04, b: 0.06, a: 1 },
        loadOp: 'clear', storeOp: 'store',
      }],
    });
    pass.setBindGroup(0, this.bind);
    pass.setPipeline(this.terrainPipe);
    pass.draw(6);
    if (n > 0) {
      pass.setPipeline(this.objPipe);
      pass.setVertexBuffer(0, this.bx);
      pass.setVertexBuffer(1, this.by);
      pass.setVertexBuffer(2, this.bt);
      pass.draw(6, n);
    }
    pass.end();
    d.queue.submit([enc.finish()]);
    return upload;
  }

  /**
   * Render one frame into a texture **this backend owns** and read the pixels back.
   *
   * This is the only capture that tells the truth about a WebGPU canvas: the swapchain
   * texture has already gone to the compositor by the time anything can copy it, and every
   * screenshot path returns a plausible-looking black rectangle whether the renderer drew
   * or not. Returns `{ w, h, nonBlack, sample }`.
   */
  async readback(v, n, cam, size = 256) {
    const d = this.device;
    // The readback target MUST carry the same format the pipelines were built against.
    // A `rgba8unorm` attachment under a `bgra8unorm` pipeline is a validation failure, and
    // WebGPU signals that by silently dropping every draw — i.e. by handing back exactly
    // the uniformly black image a broken renderer would produce.
    const tex = d.createTexture({
      size: [size, size], format: this.format,
      usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC,
    });
    d.pushErrorScope('validation');
    const bpr = size * 4;                        // 256*4 = 1024, already 256-aligned
    const buf = d.createBuffer({
      size: bpr * size, usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ,
    });
    const u = new Float32Array(UNIFORM_FLOATS);
    u[0] = cam.x; u[1] = cam.y; u[2] = cam.px; u[3] = cam.span;
    u[4] = size; u[5] = size; u[6] = cam.tiles; u[7] = cam.sub;
    d.queue.writeBuffer(this.uniform, 0, u);
    if (n > 0) {
      d.queue.writeBuffer(this.bx, 0, v.x.buffer, v.x.byteOffset, n * 4);
      d.queue.writeBuffer(this.by, 0, v.y.buffer, v.y.byteOffset, n * 4);
      d.queue.writeBuffer(this.bt, 0, v.tag.buffer, v.tag.byteOffset, n * 4);
    }
    const enc = d.createCommandEncoder();
    const pass = enc.beginRenderPass({
      colorAttachments: [{
        view: tex.createView(), clearValue: { r: 0, g: 0, b: 0, a: 1 },
        loadOp: 'clear', storeOp: 'store',
      }],
    });
    pass.setBindGroup(0, this.bind);
    pass.setPipeline(this.terrainPipe);
    pass.draw(6);
    if (n > 0) {
      pass.setPipeline(this.objPipe);
      pass.setVertexBuffer(0, this.bx);
      pass.setVertexBuffer(1, this.by);
      pass.setVertexBuffer(2, this.bt);
      pass.draw(6, n);
    }
    pass.end();
    enc.copyTextureToBuffer({ texture: tex }, { buffer: buf, bytesPerRow: bpr }, [size, size]);
    d.queue.submit([enc.finish()]);
    const err = await d.popErrorScope();
    if (err) this.errors.push('readback: ' + err.message);
    await buf.mapAsync(GPUMapMode.READ);
    const px = new Uint8Array(buf.getMappedRange().slice(0));
    buf.unmap(); buf.destroy(); tex.destroy();
    let nonBlack = 0;
    const hist = new Map();
    for (let i = 0; i < px.length; i += 4) {
      if (px[i] + px[i + 1] + px[i + 2] > 12) nonBlack++;
      const k = (px[i] >> 4) << 8 | (px[i + 1] >> 4) << 4 | (px[i + 2] >> 4);
      hist.set(k, (hist.get(k) ?? 0) + 1);
    }
    return {
      w: size, h: size, nonBlack, distinctColours: hist.size, format: this.format,
      errors: this.errors.slice(),
    };
  }
}

/**
 * Canvas-2D fallback. Same data, no GPU: it exists so a browser without WebGPU still gets
 * a playable client, and it says which one is running rather than pretending.
 */
export class Canvas2DRenderer {
  constructor(canvas) { this.canvas = canvas; this.kind = 'canvas2d'; this.errors = []; }
  async init() {
    this.g = this.canvas.getContext('2d');
    if (!this.g) throw new Error('no 2d context');
    return this;
  }
  provision(tiles, capacity) {
    this.tiles = tiles; this.capacity = capacity;
    if (typeof OffscreenCanvas !== 'undefined') {
      this.terrainCanvas = new OffscreenCanvas(tiles, tiles);
    } else {
      this.terrainCanvas = document.createElement('canvas');
      this.terrainCanvas.width = tiles;
      this.terrainCanvas.height = tiles;
    }
    this.tg = this.terrainCanvas.getContext('2d');
    this.img = this.tg.createImageData(tiles, tiles);
  }
  uploadTerrain(tileView) {
    const d = this.img.data;
    for (let i = 0; i < this.tiles * this.tiles; i++) {
      const m = tileView[i];
      const blocker = m & 3, surface = m & 0x30;
      let r = 77, g = 97, b = 56;
      if (surface === 0x20) { r = 23; g = 51; b = 92; }
      else if (surface === 0x30) { r = 28; g = 71; b = 36; }
      else if (surface === 0x10) { r = 107; g = 89; b = 61; }
      if (blocker === 2) { r = 102; g = 97; b = 92; }
      else if (blocker === 1) { r = 77; g = 71; b = 69; }
      else if (blocker === 3) { r = 51; g = 48; b = 56; }
      d[i * 4] = r; d[i * 4 + 1] = g; d[i * 4 + 2] = b; d[i * 4 + 3] = 255;
    }
    this.tg.putImageData(this.img, 0, 0);
  }
  draw(v, n, cam, vp) {
    const g = this.g;
    const t0 = performance.now();
    g.imageSmoothingEnabled = false;
    g.fillStyle = '#080a0e';
    g.fillRect(0, 0, vp[0], vp[1]);
    const s = cam.px * cam.sub; // pixels per tile
    g.drawImage(this.terrainCanvas, -cam.x * cam.px, -cam.y * cam.px, cam.tiles * s,
      cam.tiles * s);
    const OWNER = ['#5c9eff', '#ff5c4d', '#6bd97a', '#ffbf47', '#cc80f2', '#59d9e0'];
    for (let i = 0; i < n; i++) {
      const tag = v.tag[i];
      if ((tag & 0x80000000) === 0) continue;
      const sx = (v.x[i] - cam.x) * cam.px, sy = (v.y[i] - cam.y) * cam.px;
      if (sx < -40 || sy < -40 || sx > vp[0] + 40 || sy > vp[1] + 40) continue;
      const isB = (tag >>> 29) & 1, size = (tag >>> 24) & 0xf;
      const hp = ((tag >>> 16) & 0xff) / 255;
      g.fillStyle = OWNER[(tag & 0xf) % OWNER.length];
      g.globalAlpha = 0.35 + 0.65 * hp;
      if (isB) {
        const h = size * 0.5 * cam.sub * cam.px;
        g.fillRect(sx - h, sy - h, h * 2, h * 2);
      } else {
        const r = Math.max(2, (0.2 + size * 0.035) * cam.sub * cam.px);
        g.beginPath(); g.arc(sx, sy, r, 0, 6.2832); g.fill();
      }
      if (tag & 0x40000000) {
        g.globalAlpha = 1; g.strokeStyle = '#fff'; g.lineWidth = 1.5;
        g.strokeRect(sx - 7, sy - 7, 14, 14);
      }
    }
    g.globalAlpha = 1;
    return performance.now() - t0;
  }
  async readback() {
    const g = this.g;
    const d = g.getImageData(0, 0, this.canvas.width, this.canvas.height).data;
    let nonBlack = 0;
    const hist = new Map();
    for (let i = 0; i < d.length; i += 4) {
      if (d[i] + d[i + 1] + d[i + 2] > 12) nonBlack++;
      const k = (d[i] >> 4) << 8 | (d[i + 1] >> 4) << 4 | (d[i + 2] >> 4);
      hist.set(k, (hist.get(k) ?? 0) + 1);
    }
    return { w: this.canvas.width, h: this.canvas.height, nonBlack, distinctColours: hist.size };
  }
}

/**
 * Try WebGPU, fall back to canvas-2D, and report which one won *and why*.
 *
 * A canvas can only ever hand out one kind of context, and `getContext('webgpu')` claims it
 * even when the device is later found unusable — so the fallback gets a **fresh element**
 * cloned in place. Without that, one WebGPU failure takes the 2D path down with it and the
 * page dies with the useless message "no 2d context".
 */
export async function makeRenderer(canvas, prefer) {
  let why = null;
  if (prefer !== 'canvas2d') {
    try { return await new WebGPURenderer(canvas).init(); }
    catch (e) { why = e.message; console.warn('WebGPU unavailable:', e.message); }
  }
  let target = canvas;
  if (why !== null) {
    const fresh = canvas.cloneNode(false);
    canvas.replaceWith(fresh);
    target = fresh;
  }
  const r = await new Canvas2DRenderer(target).init();
  if (why) r.errors.push('webgpu declined: ' + why);
  return r;
}
