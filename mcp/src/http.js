// Streamable HTTP 입구. 노출하는 경로는 `POST /mcp` 와 `GET /healthz` 뿐이다.
//
//   - Host 헤더가 허용 목록(루프백 + PUBLIC_HOST)에 없으면 403 — DNS rebinding 막기.
//   - Origin 헤더가 오면 허용 목록에 있어야 한다. CORS 헤더는 내지 않는다(브라우저에서 직접 부를
//     일이 없다 — ChatGPT·Claude 는 서버에서 부르고, 화면은 호스트 bridge 로 부른다).
//   - 본문은 JSON 만, 기본 64KB 까지. 요청 수는 프로세스 전체 1분 창으로 제한한다.
//   - 세션 없는(stateless) 모드: 요청마다 새 transport·McpServer 를 만든다. GET/DELETE 는 405.
//   - `GET|HEAD /assets/img/<해시 이름>`: 시작할 때 검사한 허용 목록(image-assets.js)의 사진만 내보낸다.
//     요청 경로로 파일을 찾지 않는다 — 이름으로 Map 을 조회할 뿐이고, 없으면 404.

import { createServer } from 'node:http';

import { StreamableHTTPServerTransport } from '@modelcontextprotocol/sdk/server/streamableHttp.js';

import { ASSET_NAME_PATTERN, ASSET_ROUTE_PREFIX } from './image-assets.js';
import { isAuthPath } from './auth/oauth.js';
import { buildMcpServer } from './mcp-server.js';

/**
 * @typedef {object} HttpConfig
 * @property {string[]} allowedHosts  Host 헤더 값(포트 포함) 허용 목록
 * @property {string[]} allowedOrigins Origin 헤더 허용 목록(없으면 Origin 이 붙은 요청은 모두 거절)
 * @property {number} maxBodyBytes
 * @property {number} maxBatch        JSON-RPC 배치 최대 길이
 * @property {number} rateLimitPerMinute
 */

export function defaultAllowedHosts(port, publicHost) {
  const hosts = [`127.0.0.1:${port}`, `localhost:${port}`, `[::1]:${port}`];
  if (publicHost) hosts.push(publicHost.toLowerCase());
  return hosts;
}

function sendJson(res, status, body, headers = {}) {
  if (res.headersSent) return;
  res.writeHead(status, {
    'content-type': 'application/json; charset=utf-8',
    'cache-control': 'no-store',
    'x-content-type-options': 'nosniff',
    ...headers,
  });
  res.end(JSON.stringify(body));
}

function rpcError(res, status, code, message, headers) {
  sendJson(res, status, { jsonrpc: '2.0', error: { code, message }, id: null }, headers);
}

function readBody(req, limit) {
  return new Promise((resolve, reject) => {
    const declared = Number(req.headers['content-length']);
    if (Number.isFinite(declared) && declared > limit) {
      reject(Object.assign(new Error('too large'), { status: 413 }));
      req.resume();
      return;
    }
    const chunks = [];
    let size = 0;
    req.on('data', (chunk) => {
      if (size > limit) return;
      size += chunk.length;
      if (size > limit) {
        // 소켓을 바로 끊지 않는다 — 413 을 보낸 뒤 `connection: close` 로 닫는다. 나머지는 버린다.
        chunks.length = 0;
        reject(Object.assign(new Error('too large'), { status: 413 }));
        return;
      }
      chunks.push(chunk);
    });
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf8')));
    req.on('error', reject);
  });
}

/** 1분 창 요청 수 제한(프로세스 전체). 공개 터널 뒤에서 데모가 마구 불리는 것을 막는 정도다. */
function makeRateLimiter(perMinute, now = () => Date.now()) {
  let windowStart = now();
  let count = 0;
  return () => {
    const t = now();
    if (t - windowStart >= 60_000) {
      windowStart = t;
      count = 0;
    }
    count += 1;
    return count <= perMinute;
  };
}

function serveAsset(req, res, name, assets) {
  if (req.method !== 'GET' && req.method !== 'HEAD') {
    return sendJson(res, 405, { error: 'method_not_allowed' }, { allow: 'GET, HEAD' });
  }
  const asset = ASSET_NAME_PATTERN.test(name) ? assets.get(name) : undefined;
  if (!asset) return sendJson(res, 404, { error: 'not_found' });
  const headers = {
    'content-type': asset.mime,
    'cache-control': 'public, max-age=86400, immutable',
    etag: asset.etag,
    'x-content-type-options': 'nosniff',
    // 호스트의 샌드박스 iframe(다른 origin)이 <img> 로 읽을 수 있게.
    'cross-origin-resource-policy': 'cross-origin',
    'content-security-policy': "default-src 'none'; sandbox",
  };
  if (req.headers['if-none-match'] === asset.etag) {
    res.writeHead(304, headers);
    return res.end();
  }
  res.writeHead(200, { ...headers, 'content-length': asset.bytes });
  return req.method === 'HEAD' ? res.end() : res.end(asset.data);
}

/**
 * @param {{ store: import('./demo-store.js').DemoStore, uiHtml: string, config: HttpConfig,
 *           log?: (msg: string) => void, assets?: Map<string, import('./image-assets.js').LoadedAsset>,
 *           imageOrigin?: string | null,
 *           auth?: { makeApp: (onMcp: (req, res) => void) => import('express').Express, ownOrigin: string,
 *                    live: object } | null }} deps
 *   assets: 해시 이름 → 검사한 사진. imageOrigin: 도구 결과의 사진 URL·CSP 에 쓰는 이 서버의 origin.
 *   auth: live 모드(계정 연결). 주어지면 OAuth 경로를 열고 /mcp 는 Bearer 토큰이 있어야 한다.
 */
/**
 * 접근 로그 한 줄(선택형, `config.accessLog`). 진단용 메타데이터만 남긴다 — 토큰·쿼리·본문·도구 인자·결과 값은
 * 싣지 않는다. Authorization·Origin 은 있는지 여부(Origin 은 호스트)만, User-Agent 는 앞 토큰만.
 */
function accessLine(req, res, url, started) {
  const ua = String(req.headers['user-agent'] ?? '-').split(/[\s/]/)[0].slice(0, 40);
  const origin = req.headers.origin ? (() => { try { return new URL(req.headers.origin).host || req.headers.origin; } catch { return 'invalid'; } })() : '-';
  const accept = String(req.headers.accept ?? '');
  const rpc = res.__rpcSummary ?? '-';
  return [
    `access ${req.method} ${url.pathname} ${res.statusCode} ${Math.round(performance.now() - started)}ms`,
    `rpc=${rpc}`,
    `auth=${req.headers.authorization ? 'yes' : 'no'}`,
    `origin=${origin}`,
    `accept=${accept.includes('text/event-stream') ? 'json+sse' : accept ? 'other' : 'none'}`,
    `proto=${req.headers['mcp-protocol-version'] ?? '-'}`,
    `ua=${ua}`,
  ].join(' ');
}

/** JSON-RPC 본문 요약: 메서드(도구 이름). 배치는 쉼표로. 인자는 싣지 않는다. */
function rpcSummary(body) {
  const one = (m) => (m?.method === 'tools/call' && typeof m.params?.name === 'string' ? `tools/call(${m.params.name.slice(0, 60)})` : String(m?.method ?? 'response').slice(0, 60));
  return Array.isArray(body) ? body.slice(0, 10).map(one).join(',') : one(body);
}

export function createHttpServer({ store, uiHtml, config, log = () => {}, assets = new Map(), imageOrigin = null, auth = null }) {
  const allowedHosts = new Set(config.allowedHosts.map((h) => h.toLowerCase()));
  // live 모드의 동의 화면은 이 서버 자신의 페이지에서 POST 한다 — 자기 origin 은 받는다.
  const allowedOrigins = new Set([...config.allowedOrigins, ...(auth ? [auth.ownOrigin] : [])]);
  const allow = makeRateLimiter(config.rateLimitPerMinute);

  /** POST /mcp 본체. live 모드에서는 Bearer 검사를 통과한 뒤 여기로 온다(req.auth = 검증된 토큰 정보). */
  async function handleMcp(req, res) {
    if (req.method !== 'POST') {
      // 세션 없는 모드라 서버→클라이언트 SSE 스트림(GET)과 세션 종료(DELETE)를 쓰지 않는다.
      return rpcError(res, 405, -32000, 'Method not allowed. Use POST /mcp.', { allow: 'POST' });
    }
    const contentType = String(req.headers['content-type'] ?? '');
    if (!/^application\/json\b/i.test(contentType)) {
      return rpcError(res, 415, -32000, 'Content-Type must be application/json');
    }

    let body;
    try {
      const raw = await readBody(req, config.maxBodyBytes);
      body = JSON.parse(raw);
    } catch (e) {
      if (e.status === 413) {
        return rpcError(res, 413, -32000, `Request body exceeds ${config.maxBodyBytes} bytes`, { connection: 'close' });
      }
      return rpcError(res, 400, -32700, 'Parse error: body is not valid JSON');
    }
    res.__rpcSummary = rpcSummary(body);
    if (Array.isArray(body) && (body.length === 0 || body.length > config.maxBatch)) {
      return rpcError(res, 400, -32600, `Invalid Request: batch must have 1..${config.maxBatch} messages`);
    }
    if (body === null || typeof body !== 'object') {
      return rpcError(res, 400, -32600, 'Invalid Request: expected a JSON-RPC object');
    }

    const mcp = buildMcpServer({ store, uiHtml, imageOrigin, live: auth ? auth.live : null });
    const transport = new StreamableHTTPServerTransport({ sessionIdGenerator: undefined, enableJsonResponse: true });
    res.on('close', () => {
      void transport.close();
      void mcp.close();
    });
    try {
      await mcp.connect(transport);
      // SDK 가 req.auth 를 도구 처리기의 extra.authInfo 로 넘긴다.
      await transport.handleRequest(req, res, body);
    } catch (e) {
      log(`MCP 처리 오류: ${e?.stack ?? e}`);
      rpcError(res, 500, -32603, 'Internal error');
    }
  }

  const authApp = auth ? auth.makeApp((req, res) => void handleMcp(req, res)) : null;

  const server = createServer(async (req, res) => {
    const url = new URL(req.url ?? '/', 'http://placeholder');
    if (config.accessLog) {
      const started = performance.now();
      res.on('finish', () => log(accessLine(req, res, url, started)));
    }
    const host = (req.headers.host ?? '').toLowerCase();

    if (!allowedHosts.has(host)) {
      log(`거절(host): ${host || '(없음)'}`);
      return sendJson(res, 403, { error: 'host_not_allowed' });
    }
    const origin = req.headers.origin;
    if (origin !== undefined && !allowedOrigins.has(origin)) {
      log(`거절(origin): ${origin}`);
      return sendJson(res, 403, { error: 'origin_not_allowed' });
    }
    if (!allow()) return rpcError(res, 429, -32000, 'Too many requests (demo rate limit)', { 'retry-after': '60' });

    if (url.pathname === '/healthz') {
      if (req.method !== 'GET') return sendJson(res, 405, { error: 'method_not_allowed' }, { allow: 'GET' });
      return sendJson(res, 200, { ok: true, demo: true, name: 'wardrobe-edit-demo' });
    }
    if (url.pathname.startsWith(ASSET_ROUTE_PREFIX)) {
      return serveAsset(req, res, url.pathname.slice(ASSET_ROUTE_PREFIX.length), assets);
    }
    if (authApp && (url.pathname === '/mcp' || isAuthPath(url.pathname))) {
      // OAuth 경로와 Bearer 검사는 SDK 부품(express)이 처리한다.
      return authApp(req, res);
    }
    if (url.pathname !== '/mcp') return sendJson(res, 404, { error: 'not_found' });
    if (req.headers.authorization !== undefined) {
      // demo 모드는 토큰을 검증할 수 없다. 토큰이 붙은 요청을 익명으로 처리하지 않는다 — 계정 연결은 live 모드.
      return rpcError(res, 400, -32001, 'This demo server does not accept Authorization tokens. Use the live (OAuth) server.');
    }
    return handleMcp(req, res);
  });

  server.requestTimeout = 15_000;
  server.headersTimeout = 10_000;
  server.keepAliveTimeout = 5_000;
  return server;
}
