// Static server for the spectator. No dependencies.
//
// # Why this file exists instead of `python3 -m http.server`
//
// `SharedArrayBuffer` is gated behind **cross-origin isolation**, which the browser grants
// only when the top-level document arrives with both:
//
//     Cross-Origin-Opener-Policy:   same-origin
//     Cross-Origin-Embedder-Policy: require-corp
//
// Without them `crossOriginIsolated` is false, the `SharedArrayBuffer` constructor is not
// even defined, and the multi-worker path cannot exist. It is not a performance tax — it is
// a hard gate, and it is the single most common reason a design like this one gets quietly
// downgraded to "postMessage a copy every frame".
//
// The price of turning it on: every cross-origin subresource must opt in with CORP or CORS.
// This app has zero cross-origin subresources — no CDN, no font service, no analytics —
// which is itself an argument for the no-framework build. `Cross-Origin-Resource-Policy:
// same-origin` is set on everything we serve so this page can be embedded by another
// isolated page of ours.
//
//     node web/serve.mjs [port]     # default 8787

import { createServer } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { join, extname, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(fileURLToPath(new URL('.', import.meta.url)), 'public');
const PORT = Number(process.argv[2] || 8787);

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  // Required for `WebAssembly.instantiateStreaming`; with the wrong type it throws and the
  // page silently falls back to nothing.
  '.wasm': 'application/wasm',
  '.wgsl': 'text/plain; charset=utf-8',
};

const server = createServer(async (req, res) => {
  const url = new URL(req.url, 'http://localhost');
  let p = normalize(decodeURIComponent(url.pathname));
  if (p === '/' || p.endsWith('/')) p += 'index.html';
  const file = join(ROOT, p);
  if (!file.startsWith(ROOT)) { res.writeHead(403).end('no'); return; }

  const headers = {
    'Cross-Origin-Opener-Policy': 'same-origin',
    'Cross-Origin-Embedder-Policy': 'require-corp',
    'Cross-Origin-Resource-Policy': 'same-origin',
    'Cache-Control': 'no-store',
  };

  try {
    const s = await stat(file);
    if (!s.isFile()) throw new Error('not a file');
    const body = await readFile(file);
    headers['Content-Type'] = MIME[extname(file)] || 'application/octet-stream';
    headers['Content-Length'] = body.length;
    res.writeHead(200, headers).end(body);
  } catch {
    res.writeHead(404, { ...headers, 'Content-Type': 'text/plain' }).end('404 ' + p);
  }
});

server.listen(PORT, '127.0.0.1', () => {
  console.log(`spectator on http://127.0.0.1:${PORT}/  (cross-origin isolated)`);
});
