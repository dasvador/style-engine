import { request as httpRequest } from 'node:http';
import { createServer as createNetServer } from 'node:net';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';

import { DemoStore } from '../src/demo-store.js';
import { createHttpServer, defaultAllowedHosts } from '../src/http.js';

export const UI_FILE = fileURLToPath(new URL('../dist/ui.html', import.meta.url));

export async function tempDir() {
  const dir = await mkdtemp(join(tmpdir(), 'wardrobe-mcp-test-'));
  return { dir, cleanup: () => rm(dir, { recursive: true, force: true }) };
}

async function freePort() {
  const probe = createNetServer();
  await new Promise((resolve) => probe.listen(0, '127.0.0.1', resolve));
  const { port } = probe.address();
  await new Promise((resolve) => probe.close(resolve));
  return port;
}

/** 빈 포트에 데모 서버를 띄운다. 실제 서버 main.js 와 같은 createHttpServer 를 쓴다. */
export async function startServer({ file = null, http = {}, store: givenStore, publicHost = null } = {}) {
  const uiHtml = await readFile(UI_FILE, 'utf8');
  const store = givenStore ?? (await new DemoStore({ file }).load());
  const port = await freePort();
  const config = {
    allowedHosts: defaultAllowedHosts(port, publicHost),
    allowedOrigins: [],
    maxBodyBytes: 64 * 1024,
    maxBatch: 10,
    rateLimitPerMinute: 10_000,
    ...http,
  };
  const server = createHttpServer({ store, uiHtml, config });
  await new Promise((resolve) => server.listen(port, '127.0.0.1', resolve));
  return {
    port,
    url: `http://127.0.0.1:${port}/mcp`,
    store,
    uiHtml,
    close: () =>
      new Promise((resolve) => {
        server.closeAllConnections();
        server.close(() => resolve());
      }),
  };
}

export async function connectClient(url) {
  const client = new Client({ name: 'wardrobe-mcp-test', version: '0.0.0' });
  await client.connect(new StreamableHTTPClientTransport(new URL(url)));
  return client;
}

/** SDK 를 거치지 않는 원시 HTTP 요청. */
export async function rawPost(url, body, headers = {}) {
  const res = await fetch(url, {
    method: 'POST',
    headers: { 'content-type': 'application/json', accept: 'application/json, text/event-stream', ...headers },
    body: typeof body === 'string' ? body : JSON.stringify(body),
  });
  const text = await res.text();
  let json = null;
  try {
    json = JSON.parse(text);
  } catch {
    /* 본문이 JSON 이 아닐 수 있다 */
  }
  return { status: res.status, json, text, headers: res.headers };
}

/** Host 헤더를 직접 정해 보내는 요청(fetch 는 Host 를 바꾸지 못한다). */
export function rawHttp(url, body, headers = {}) {
  const payload = JSON.stringify(body);
  return new Promise((resolve, reject) => {
    const req = httpRequest(
      url,
      {
        method: 'POST',
        headers: {
          'content-type': 'application/json',
          accept: 'application/json, text/event-stream',
          'content-length': Buffer.byteLength(payload),
          ...headers,
        },
      },
      (res) => {
        let text = '';
        res.setEncoding('utf8');
        res.on('data', (c) => (text += c));
        res.on('end', () => resolve({ status: res.statusCode, text }));
      },
    );
    req.on('error', reject);
    req.end(payload);
  });
}
