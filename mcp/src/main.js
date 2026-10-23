// 데모 MCP 서버 실행: `npm start` (빌드 후 이 파일). 기본 http://127.0.0.1:8787/mcp
// 끝내기: 이 프로세스에 Ctrl+C(SIGINT) 또는 SIGTERM.

import { readFile } from 'node:fs/promises';

import { loadConfig } from './config.js';
import { DemoStore } from './demo-store.js';
import { createHttpServer } from './http.js';

const log = (msg) => console.error(`[wardrobe-edit-demo] ${msg}`);

const config = loadConfig();
const uiHtml = await readFile(config.uiFile, 'utf8').catch(() => {
  throw new Error(`UI 파일이 없어요: ${config.uiFile}. 먼저 \`npm run build\` 를 실행하세요.`);
});
const store = await new DemoStore({
  file: config.dataFile,
  ttlMs: config.ttlHours * 60 * 60 * 1000,
  maxComparisons: config.maxComparisons,
}).load();

const server = createHttpServer({ store, uiHtml, config: config.http, log });
server.listen(config.port, config.host, () => {
  log(`데모 모드 · pid ${process.pid} · http://${config.host}:${config.port}/mcp`);
  log(`허용 Host: ${config.http.allowedHosts.join(', ')}`);
  log(`데모 기록 파일: ${config.dataFile} (보존 ${config.ttlHours}시간)`);
  if (!['127.0.0.1', 'localhost', '::1'].includes(config.host)) {
    log(`주의: 루프백이 아닌 ${config.host} 에 바인딩했어요.`);
  }
});

for (const sig of ['SIGINT', 'SIGTERM']) {
  process.on(sig, () => {
    log(`${sig} 받음, 종료합니다.`);
    server.close(() => process.exit(0));
    setTimeout(() => process.exit(0), 2000).unref();
  });
}
