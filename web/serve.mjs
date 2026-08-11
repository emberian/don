// Loopback server for the playable client. No third-party dependencies.
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
//
// A bounded same-origin JSON API also owns the browser side of the local MatchStart handoff.
// It invokes the configured native service-match-peer, and exposes seed/epoch/roster only after
// both of that program's independent processes agree on StartGame and MatchStart. The API never
// binds beyond 127.0.0.1. Its turn endpoint admits only the fixed empty-input barrier;
// gameplay commands remain paused until a later tranche gives their wire an owner.

import { createServer } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { join, extname, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  LOCAL_MATCH_PROTOCOL, LOCAL_MATCH_TURN_RELAY, LocalMatchGateway, MAX_LOCAL_MATCH_BODY_BYTES,
  validateEmptyTurnRequest,
} from './local-match.mjs';

const ROOT = join(fileURLToPath(new URL('.', import.meta.url)), 'public');
const PORT = Number(process.argv[2] || 8787);
const LOCAL_MATCH_PEER = process.env.DON_SERVICE_MATCH_PEER || join(
  ROOT, '..', '..', 'crates', 'don-crossplay', 'target', 'debug', 'service-match-peer');
const localMatches = new LocalMatchGateway(LOCAL_MATCH_PEER);

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

const BASE_HEADERS = {
  'Cross-Origin-Opener-Policy': 'same-origin',
  'Cross-Origin-Embedder-Policy': 'require-corp',
  'Cross-Origin-Resource-Policy': 'same-origin',
  'Cache-Control': 'no-store',
};

function sendJson(res, status, value) {
  const body = Buffer.from(JSON.stringify(value));
  res.writeHead(status, {
    ...BASE_HEADERS,
    'Content-Type': 'application/json; charset=utf-8',
    'Content-Length': body.length,
  }).end(body);
}

async function readJson(req) {
  const contentType = String(req.headers['content-type'] ?? '').split(';', 1)[0].trim();
  if (contentType !== 'application/json') throw new Error('request content type must be application/json');
  const chunks = [];
  let size = 0;
  for await (const chunk of req) {
    size += chunk.length;
    if (size > MAX_LOCAL_MATCH_BODY_BYTES) throw new Error('local match request body is too large');
    chunks.push(chunk);
  }
  let value;
  try { value = JSON.parse(Buffer.concat(chunks).toString('utf8')); }
  catch { throw new Error('local match request body is not valid JSON'); }
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('local match request body must be an object');
  }
  return value;
}

async function serveLocalMatch(req, res, url) {
  if (!url.pathname.startsWith('/api/local-match')) return false;
  try {
    if (url.pathname === '/api/local-match' && req.method === 'GET') {
      sendJson(res, 200, {
        protocol: LOCAL_MATCH_PROTOCOL,
        available: await localMatches.available(),
        players: 2,
        turnRelay: LOCAL_MATCH_TURN_RELAY,
      });
      return true;
    }
    if (url.pathname === '/api/local-match/lobbies' && req.method === 'POST') {
      if (!(await localMatches.available())) {
        sendJson(res, 503, {
          protocol: LOCAL_MATCH_PROTOCOL,
          error: 'configured service-match-peer binary is absent or not executable',
        });
        return true;
      }
      const body = await readJson(req);
      sendJson(res, 201, localMatches.create(body.seed, body.name));
      return true;
    }
    const route = url.pathname.match(
      /^\/api\/local-match\/lobbies\/([a-f0-9]{8})(?:\/(join|ready|turn|turn-ack))?$/);
    if (!route) {
      sendJson(res, 404, { protocol: LOCAL_MATCH_PROTOCOL, error: 'unknown local match endpoint' });
      return true;
    }
    const [, code, action] = route;
    if (!action && req.method === 'GET') {
      sendJson(res, 200, localMatches.snapshot(code, url.searchParams.get('token') ?? ''));
      return true;
    }
    if (!action && req.method === 'DELETE') {
      const body = await readJson(req);
      sendJson(res, 200, { protocol: LOCAL_MATCH_PROTOCOL, left: localMatches.leave(code, body.token) });
      return true;
    }
    if (action === 'join' && req.method === 'POST') {
      const body = await readJson(req);
      sendJson(res, 200, localMatches.join(code, body.name));
      return true;
    }
    if (action === 'ready' && req.method === 'POST') {
      const body = await readJson(req);
      sendJson(res, 200, localMatches.ready(code, body.token, body.ready ?? true));
      return true;
    }
    if (action === 'turn' && req.method === 'POST') {
      // No command/body field is accepted: this tranche is intentionally an empty-input gate.
      const body = validateEmptyTurnRequest(await readJson(req));
      sendJson(res, 200, localMatches.submitTurn(code, body.token, body.stamp));
      return true;
    }
    if (action === 'turn-ack' && req.method === 'POST') {
      const body = await readJson(req);
      if (Object.keys(body).some((key) =>
        !['token', 'stamp', 'frame', 'digest', 'rngState'].includes(key))) {
        throw new Error('turn acknowledgement contains unsupported fields');
      }
      sendJson(res, 200, localMatches.acknowledgeTurn(
        code, body.token, body.stamp, body.frame, body.digest, body.rngState));
      return true;
    }
    sendJson(res, 405, { protocol: LOCAL_MATCH_PROTOCOL, error: 'method not allowed' });
  } catch (error) {
    sendJson(res, 400, { protocol: LOCAL_MATCH_PROTOCOL, error: error.message });
  }
  return true;
}

const server = createServer(async (req, res) => {
  const url = new URL(req.url, 'http://localhost');
  if (await serveLocalMatch(req, res, url)) return;
  let p = normalize(decodeURIComponent(url.pathname));
  if (p === '/' || p.endsWith('/')) p += 'index.html';
  const file = join(ROOT, p);
  if (!file.startsWith(ROOT)) { res.writeHead(403).end('no'); return; }

  const headers = { ...BASE_HEADERS };

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
  console.log(`web client on http://127.0.0.1:${PORT}/  (cross-origin isolated, loopback only)`);
});
