// WebGL2 fallback, deliberately the same architecture as the WebGPU backend.
//
// The SoA-columns-as-instance-attributes trick predates WebGPU: `vertexAttribIPointer` +
// `vertexAttribDivisor(loc, 1)` binds an `i32` column as a per-instance integer attribute
// with no conversion, and `drawArraysInstanced` issues the whole cluster in one call. So
// the fallback is not a degraded design, it is the same design on an older API — which
// matters, because if the fallback had to interleave, its numbers would say nothing about
// the primary path.
//
// What is genuinely lost versus WebGPU: no timestamp queries (so GPU-side time is not
// measurable here), a driver-level uniform upload per frame instead of one buffer write,
// and no explicit control over when work is submitted.

const VS_UNITS = `#version 300 es
precision highp float;
precision highp int;
uniform uvec4 uGrid;    // gridCols, gridRows, stride, mapSpan
uniform vec4  uPanZoom; // pan.xy, zoom, pointPx
uniform vec4  uMisc;    // viewport.xy, worlds, flags
in int aX;
in int aY;
in uint aTag;
out vec2 vUv;
out vec3 vRgb;
const vec3 TEAM[8] = vec3[8](
  vec3(0.35,0.72,1.00), vec3(1.00,0.45,0.38), vec3(0.55,0.90,0.55), vec3(1.00,0.82,0.36),
  vec3(0.78,0.58,1.00), vec3(0.40,0.94,0.86), vec3(1.00,0.60,0.85), vec3(0.72,0.76,0.82));
void main() {
  vec2 c = vec2(float(gl_VertexID & 1), float((gl_VertexID >> 1) & 1));
  vUv = c;
  if ((aTag & 0x80000000u) == 0u) { gl_Position = vec4(1e6, 1e6, 0.5, 1.0); vRgb = vec3(0.0); return; }
  uint world = uint(gl_InstanceID) / uGrid.z;
  vec2 cell = vec2(float(world % uGrid.x), float(world / uGrid.x));
  vec2 inWorld = vec2(float(aX), float(aY)) / float(uGrid.w);
  vec2 g = (cell + inWorld) / vec2(float(uGrid.x), float(uGrid.y));
  vec2 p = vec2(g.x * 2.0 - 1.0, 1.0 - g.y * 2.0);
  p = (p - uPanZoom.xy) * uPanZoom.z;
  // Same tag unpacking as the WebGPU path: size 24..29, hp 16..23, hue 8..15, selected 30.
  float sizeClass = float((aTag >> 24u) & 0x3fu);
  float hp = float((aTag >> 16u) & 0xffu) / 255.0;
  float hue = float((aTag >> 8u) & 0xffu);
  uint sel = (aTag >> 30u) & 1u;
  float px2 = uPanZoom.w * (1.0 + (uPanZoom.w > 2.0 ? sizeClass * 0.22 : 0.0)) * (1.0 + float(sel) * 0.35);
  p += (c - vec2(0.5)) * (px2 * 2.0) / uMisc.xy;
  gl_Position = vec4(p, 0.5, 1.0);
  vec3 rgb = TEAM[aTag & 7u];
  float shear = (fract(hue * 0.0181) - 0.5) * 0.30;
  rgb = clamp(rgb + vec3(shear, shear * -0.5, shear * 0.35), vec3(0.0), vec3(1.0));
  rgb = mix(rgb * 0.22, rgb, 0.35 + 0.65 * hp);
  vRgb = sel == 1u ? mix(rgb, vec3(1.0), 0.55) : rgb;
}`;

const FS_UNITS = `#version 300 es
precision highp float;
uniform vec4 uPanZoom;
in vec2 vUv;
in vec3 vRgb;
out vec4 outColor;
void main() {
  if (uPanZoom.w > 2.5) {
    vec2 d = vUv - vec2(0.5);
    float r = dot(d, d);
    if (r > 0.25) discard;
    outColor = vec4(vRgb * (1.0 - r * 1.2), 1.0);
    return;
  }
  outColor = vec4(vRgb, 1.0);
}`;

const VS_AGG = `#version 300 es
precision highp float;
precision highp int;
uniform uvec4 uGrid;
uniform vec4  uPanZoom;
uniform vec4  uMisc;
uniform vec4  uExtra;   // x = plate brightness, y = plate inset, z = hp scale, w = kill scale
in uvec4 aStats;
in uvec4 aStats2;
out vec3 vRgb;
vec3 ramp(float t) {
  float x = clamp(t, 0.0, 1.0);
  vec3 a = vec3(0.05,0.06,0.12), b = vec3(0.13,0.35,0.62), c = vec3(0.36,0.74,0.78), d = vec3(0.86,0.80,0.55);
  if (x < 0.34) return mix(a, b, x / 0.34);
  if (x < 0.67) return mix(b, c, (x - 0.34) / 0.33);
  return mix(c, d, (x - 0.67) / 0.33);
}
void main() {
  if (float(gl_InstanceID) >= uMisc.z) { gl_Position = vec4(1e6,1e6,0.5,1.0); vRgb = vec3(0.0); return; }
  uint id = uint(gl_InstanceID);
  vec2 cell = vec2(float(id % uGrid.x), float(id / uGrid.x));
  vec2 c = vec2(float(gl_VertexID & 1), float((gl_VertexID >> 1) & 1)) * (1.0 - 2.0 * uExtra.y) + vec2(uExtra.y);
  vec2 g = (cell + c) / vec2(float(uGrid.x), float(uGrid.y));
  vec2 p = vec2(g.x * 2.0 - 1.0, 1.0 - g.y * 2.0);
  p = (p - uPanZoom.xy) * uPanZoom.z;
  gl_Position = vec4(p, 0.5, 1.0);
  uint m = uint(uMisc.w) & 7u;
  float metric = float(aStats.x) / max(float(uGrid.z), 1.0);
  if (m == 1u) metric = float(aStats.z & 0xffffu) / 65535.0;
  else if (m == 2u) metric = float(aStats.y) / max(uExtra.z, 1.0);
  else if (m == 3u) metric = float(aStats2.x) / max(uExtra.w, 1.0);
  else if (m == 4u) metric = float(aStats2.w - 1u) / 3.0;
  metric = clamp(metric, 0.0, 1.0);
  vRgb = mix(vec3(0.062, 0.070, 0.094), ramp(metric), uExtra.x);
}`;

const FS_AGG = `#version 300 es
precision highp float;
in vec3 vRgb;
out vec4 outColor;
void main() { outColor = vec4(vRgb, 1.0); }`;

function compile(gl, vsSrc, fsSrc) {
  const mk = (type, src) => {
    const s = gl.createShader(type);
    gl.shaderSource(s, src); gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s));
    return s;
  };
  const p = gl.createProgram();
  gl.attachShader(p, mk(gl.VERTEX_SHADER, vsSrc));
  gl.attachShader(p, mk(gl.FRAGMENT_SHADER, fsSrc));
  gl.linkProgram(p);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p));
  return p;
}

export class WebGl2Backend {
  static get name() { return 'webgl2'; }

  #gl; #canvas; #unit; #agg; #vaoUnit; #vaoAgg;
  #bufX; #bufY; #bufTag; #bufStats;
  #cells = 0; #worldsCap = 0; #u = {};

  static async create(canvas) {
    const gl = canvas.getContext('webgl2', {
      alpha: false, antialias: false, depth: false, stencil: false,
      powerPreference: 'high-performance', desynchronized: true,
      preserveDrawingBuffer: false,
    });
    if (!gl) throw new Error('webgl2 unavailable');
    return new WebGl2Backend(gl, canvas);
  }

  constructor(gl, canvas) {
    this.#gl = gl; this.#canvas = canvas;
    this.adapterInfo = { vendor: gl.getParameter(gl.VENDOR), architecture: gl.getParameter(gl.RENDERER) };
    this.limits = {};
    this.#unit = compile(gl, VS_UNITS, FS_UNITS);
    this.#agg = compile(gl, VS_AGG, FS_AGG);
    this.#u.unit = ['uGrid', 'uPanZoom', 'uMisc'].reduce((a, n) => (a[n] = gl.getUniformLocation(this.#unit, n), a), {});
    this.#u.agg = ['uGrid', 'uPanZoom', 'uMisc', 'uExtra'].reduce((a, n) => (a[n] = gl.getUniformLocation(this.#agg, n), a), {});
    this.#vaoUnit = gl.createVertexArray();
    this.#vaoAgg = gl.createVertexArray();
    gl.disable(gl.DEPTH_TEST);
    gl.disable(gl.BLEND);
  }

  get timestampSupported() { return false; }
  get lastGpuNs() { return 0; }

  ensureCapacity(cells, worlds) {
    const gl = this.#gl;
    if (cells > this.#cells) {
      for (const b of [this.#bufX, this.#bufY, this.#bufTag]) if (b) gl.deleteBuffer(b);
      const mk = () => { const b = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, b); gl.bufferData(gl.ARRAY_BUFFER, cells * 4, gl.DYNAMIC_DRAW); return b; };
      this.#bufX = mk(); this.#bufY = mk(); this.#bufTag = mk();
      this.#cells = cells;

      gl.bindVertexArray(this.#vaoUnit);
      const bind = (buf, loc, type) => {
        gl.bindBuffer(gl.ARRAY_BUFFER, buf);
        gl.enableVertexAttribArray(loc);
        // Integer attribute path: no normalisation, no float conversion. The `i32`
        // subtile positions arrive in the shader exactly as the sim stores them.
        gl.vertexAttribIPointer(loc, 1, type, 0, 0);
        gl.vertexAttribDivisor(loc, 1);
      };
      bind(this.#bufX, gl.getAttribLocation(this.#unit, 'aX'), gl.INT);
      bind(this.#bufY, gl.getAttribLocation(this.#unit, 'aY'), gl.INT);
      bind(this.#bufTag, gl.getAttribLocation(this.#unit, 'aTag'), gl.UNSIGNED_INT);
      gl.bindVertexArray(null);
    }
    if (worlds > this.#worldsCap) {
      if (this.#bufStats) gl.deleteBuffer(this.#bufStats);
      this.#bufStats = gl.createBuffer();
      gl.bindBuffer(gl.ARRAY_BUFFER, this.#bufStats);
      gl.bufferData(gl.ARRAY_BUFFER, worlds * 32, gl.DYNAMIC_DRAW);
      this.#worldsCap = worlds;
      gl.bindVertexArray(this.#vaoAgg);
      // Eight u32 per world, read as two vec4u out of one interleaved instance buffer.
      gl.bindBuffer(gl.ARRAY_BUFFER, this.#bufStats);
      for (const [name, off] of [['aStats', 0], ['aStats2', 16]]) {
        const loc = gl.getAttribLocation(this.#agg, name);
        if (loc < 0) continue;
        gl.enableVertexAttribArray(loc);
        gl.vertexAttribIPointer(loc, 4, gl.UNSIGNED_INT, 32, off);
        gl.vertexAttribDivisor(loc, 1);
      }
      gl.bindVertexArray(null);
    }
  }

  writeColumn(which, dstElem, src, srcElem, elems) {
    const gl = this.#gl;
    const buf = which === 'x' ? this.#bufX : which === 'y' ? this.#bufY : which === 'tag' ? this.#bufTag : this.#bufStats;
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferSubData(gl.ARRAY_BUFFER, dstElem * 4, src, srcElem, elems);
  }

  // WebGL2 sets uniforms per program at draw time, so the two-slot dance the WebGPU
  // backend needs for the plate pass is unnecessary here: slot 1 is simply the second
  // `setUniform` call, kept so both backends take the same calls from the caller.
  setUniform(p, slot = 0) { this.#p[slot] = p; }
  #p = [null, null];

  #applyUniforms(u, slot) {
    const gl = this.#gl, p = this.#p[slot] || this.#p[0];
    gl.uniform4ui(u.uGrid, p.gridCols, p.gridRows, p.stride, p.mapSpan);
    gl.uniform4f(u.uPanZoom, p.panX, p.panY, p.zoom, p.pointPx);
    gl.uniform4f(u.uMisc, p.vw, p.vh, p.worlds, p.flags);
    if (u.uExtra) gl.uniform4f(u.uExtra, p.plate ?? 1, p.inset ?? 0.04, p.hpScale ?? 1, p.killScale ?? 1);
  }

  draw(mode, instances, clear = [0.035, 0.04, 0.055, 1]) {
    const gl = this.#gl;
    gl.viewport(0, 0, this.#canvas.width, this.#canvas.height);
    gl.clearColor(clear[0], clear[1], clear[2], clear[3]);
    gl.clear(gl.COLOR_BUFFER_BIT);
    if (mode === 'aggregate') {
      if (instances <= 0) return;
      gl.useProgram(this.#agg); this.#applyUniforms(this.#u.agg, 0); gl.bindVertexArray(this.#vaoAgg);
      gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, instances);
      gl.bindVertexArray(null);
      return;
    }
    if (mode === 'cluster') {
      gl.useProgram(this.#agg); this.#applyUniforms(this.#u.agg, 1); gl.bindVertexArray(this.#vaoAgg);
      gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, this.#worldsCap);
      gl.bindVertexArray(null);
    }
    if (instances <= 0) return;
    gl.useProgram(this.#unit); this.#applyUniforms(this.#u.unit, 0); gl.bindVertexArray(this.#vaoUnit);
    gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, instances);
    gl.bindVertexArray(null);
  }

  /** Pixels of the current drawing buffer. `readPixels` is valid until the compositor
   *  swaps, so the caller redraws immediately before asking. Rows come back bottom-up. */
  async readback(drawAgain) {
    const gl = this.#gl, w = this.#canvas.width, h = this.#canvas.height;
    if (drawAgain) drawAgain();
    const flip = new Uint8Array(w * h * 4);
    gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, flip);
    const out = new Uint8ClampedArray(w * h * 4);
    for (let y = 0; y < h; y++) out.set(flip.subarray((h - 1 - y) * w * 4, (h - y) * w * 4), y * w * 4);
    for (let i = 3; i < out.length; i += 4) out[i] = 255;
    return { width: w, height: h, rgba: out };
  }

  resize(w, h) { this.#canvas.width = w; this.#canvas.height = h; }

  /** `gl.finish()` is the only barrier WebGL2 offers. It stalls the CPU until the GPU
   *  drains, which is exactly what an uncapped throughput measurement needs and exactly
   *  what a real frame loop must never do. */
  async waitIdle() { this.#gl.finish(); }
}
