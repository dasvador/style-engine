// Streamable HTTP 입구. 노출하는 경로는 `POST /mcp` 와 `GET /healthz` 뿐이다.
//
//   - Host 헤더가 허용 목록(루프백 + PUBLIC_HOST)에 없으면 403 — DNS rebinding 막기.
//   - Origin 헤더가 오면 허용 목록에 있어야 한다. CORS 헤더는 내지 않는다(브라우저에서 직접 부를
//     일이 없다 — ChatGPT·Claude 는 서버에서 부르고, 화면은 호스트 bridge 로 부른다).
//   - 본문은 JSON 만, 기본 64KB 까지. 요청 수는 프로세스 전체 1분 창으로 제한한다.
//   - 세션 없는(stateless) 모드: 요청마다 새 transport·McpServer 를 만든다. GET/DELETE 는 405.

import { createServer } from 'node:http';

import { StreamableHTTPServerTransport } from '@modelcontextprotocol/sdk/server/streamableHttp.js';

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

/**
 * @param {{ store: import('./demo-store.js').DemoStore, uiHtml: string, config: HttpConfig,
 *           log?: (msg: string) => void }} deps
 */
export function createHttpServer({ store, uiHtml, config, log = () => {} }) {
  const allowedHosts = new Set(config.allowedHosts.map((h) => h.toLowerCase()));
  const allowedOrigins = new Set(config.allowedOrigins);
  const allow = makeRateLimiter(config.rateLimitPerMinute);

  const server = createServer(async (req, res) => {
    const url = new URL(req.url ?? '/', 'http://placeholder');
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
    if (url.pathname !== '/mcp') return sendJson(res, 404, { error: 'not_found' });

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
    if (Array.isArray(body) && (body.length === 0 || body.length > config.maxBatch)) {
      return rpcError(res, 400, -32600, `Invalid Request: batch must have 1..${config.maxBatch} messages`);
    }
    if (body === null || typeof body !== 'object') {
      return rpcError(res, 400, -32600, 'Invalid Request: expected a JSON-RPC object');
    }

    const mcp = buildMcpServer({ store, uiHtml });
    const transport = new StreamableHTTPServerTransport({ sessionIdGenerator: undefined, enableJsonResponse: true });
    res.on('close', () => {
      void transport.close();
      void mcp.close();
    });
    try {
      await mcp.connect(transport);
      await transport.handleRequest(req, res, body);
    } catch (e) {
      log(`MCP 처리 오류: ${e?.stack ?? e}`);
      rpcError(res, 500, -32603, 'Internal error');
    }
  });

  server.requestTimeout = 15_000;
  server.headersTimeout = 10_000;
  server.keepAliveTimeout = 5_000;
  return server;
}
