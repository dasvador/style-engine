import { request as httpRequest } from 'node:http';
import { createServer as createNetServer } from 'node:net';
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { crc32, deflateSync } from 'node:zlib';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';

import { DemoStore } from '../src/demo-store.js';
import { createHttpServer, defaultAllowedHosts } from '../src/http.js';
import { imageOrigin, imageResolver, loadImageAssets } from '../src/image-assets.js';
import { PHOTO_ASSET_KEYS } from '../src/photo-sets.js';
import { createLiveAuth } from '../src/auth/setup.js';
import { DEFAULT_REDIRECT_URIS } from '../src/auth/oauth.js';

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
/**
 * live: { adapter, authStoreFile?, linkCodesFile, redirectAllowlist?, now? } — 계정 연결 모드로 띄운다.
 * 발급자는 로컬 http://127.0.0.1:포트(SDK 가 루프백만 http 를 허용한다).
 */
export async function startServer({ file = null, http = {}, store: givenStore, publicHost = null, assetsDir = null, live = null, port: fixedPort = null, log } = {}) {
  const uiHtml = await readFile(UI_FILE, 'utf8');
  const port = fixedPort ?? (await freePort());
  const assets = await loadImageAssets(assetsDir);
  const origin = imageOrigin({ publicHost, port });
  const store = givenStore ?? (await new DemoStore({ file, images: imageResolver(assets.byKey, origin) }).load());
  const config = {
    allowedHosts: defaultAllowedHosts(port, publicHost),
    allowedOrigins: [],
    maxBodyBytes: 64 * 1024,
    maxBatch: 10,
    rateLimitPerMinute: 10_000,
    ...http,
  };
  const auth = live
    ? await createLiveAuth({
        origin: `http://127.0.0.1:${port}`,
        authStoreFile: live.authStoreFile ?? null,
        linkCodesFile: live.linkCodesFile,
        redirectAllowlist: live.redirectAllowlist ?? DEFAULT_REDIRECT_URIS,
        adapter: live.adapter,
        recommender: live.recommender ?? null,
        now: live.now,
      })
    : null;
  const server = createHttpServer({ store, uiHtml, config, assets: assets.byName, imageOrigin: origin, auth, log });
  await new Promise((resolve) => server.listen(port, '127.0.0.1', resolve));
  return {
    port,
    url: `http://127.0.0.1:${port}/mcp`,
    store,
    uiHtml,
    assets,
    origin,
    auth,
    close: () =>
      new Promise((resolve) => {
        server.closeAllConnections();
        server.close(() => resolve());
      }),
  };
}

export async function connectClient(url, { token = null } = {}) {
  const client = new Client({ name: 'wardrobe-mcp-test', version: '0.0.0' });
  const requestInit = token ? { headers: { Authorization: `Bearer ${token}` } } : undefined;
  await client.connect(new StreamableHTTPClientTransport(new URL(url), { requestInit }));
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

/** 단색 PNG(가로 w, 세로 h). 테스트용 사진 asset — 실제 생성 사진을 테스트에 넣지 않는다. */
export function makePng(w, h, [r, g, b]) {
  const chunk = (type, data) => {
    const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(body));
    return Buffer.concat([len, body, crc]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 2; // RGB
  const row = Buffer.concat([Buffer.from([0]), Buffer.from(Array.from({ length: w }, () => [r, g, b]).flat())]);
  const raw = Buffer.concat(Array.from({ length: h }, () => row));
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

/**
 * 테스트용 asset 디렉터리(manifest + images/). keys 마다 서로 다른 색의 PNG 를 만든다.
 * @returns {Promise<{ dir: string, files: Record<string, string>, cleanup: () => Promise<void> }>}
 */
export async function makeTestAssets(keys = PHOTO_ASSET_KEYS) {
  const { dir, cleanup } = await tempDir();
  await mkdir(join(dir, 'images'));
  const assets = {};
  const files = {};
  for (const [i, key] of keys.entries()) {
    const png = makePng(20, 30, [40 + i * 50, 90, 200 - i * 40]);
    const sha256 = createHash('sha256').update(png).digest('hex');
    const name = `${sha256.slice(0, 32)}.png`;
    await writeFile(join(dir, 'images', name), png);
    assets[key] = { file: name, sha256, bytes: png.length, width: 20, height: 30, mime: 'image/png', source: { test: true } };
    files[key] = name;
  }
  await writeFile(join(dir, 'manifest.json'), JSON.stringify({ version: 1, assets }));
  return { dir, files, cleanup };
}
