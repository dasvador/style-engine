// 데모 MCP 서버 실행: `npm start` (빌드 후 이 파일). 기본 http://127.0.0.1:8787/mcp
// 끝내기: 이 프로세스에 Ctrl+C(SIGINT) 또는 SIGTERM.

import { readFile } from 'node:fs/promises';

import { loadConfig } from './config.js';
import { DemoStore } from './demo-store.js';
import { createHttpServer } from './http.js';
import { imageOrigin, imageResolver, loadImageAssets } from './image-assets.js';
import { LiveAdapter } from './adapters/live.js';
import { createLiveAuth } from './auth/setup.js';
import { RecommendClient } from './recommend.js';

const log = (msg) => console.error(`[wardrobe-edit-demo] ${msg}`);

const config = loadConfig();
const uiHtml = await readFile(config.uiFile, 'utf8').catch(() => {
  throw new Error(`UI 파일이 없어요: ${config.uiFile}. 먼저 \`npm run build\` 를 실행하세요.`);
});
const assets = await loadImageAssets(config.assetsDir);
const origin = imageOrigin(config);
const store = await new DemoStore({
  file: config.dataFile,
  ttlMs: config.ttlHours * 60 * 60 * 1000,
  maxComparisons: config.maxComparisons,
  images: imageResolver(assets.byKey, origin),
}).load();

// live 모드: 계정 연결(OAuth)과 연결된 사용자의 실제 옷장 읽기. 접속 문자열은 로그에 남기지 않는다.
let auth = null;
if (config.live) {
  const adapter = await LiveAdapter.connect(config.live.databaseUrl);
  auth = await createLiveAuth({
    origin: config.live.origin,
    authStoreFile: config.live.authStoreFile,
    linkCodesFile: config.live.linkCodesFile,
    redirectAllowlist: config.live.redirectAllowlist,
    adapter,
    recommender: config.live.recommend ? new RecommendClient(config.live.recommend) : null,
  });
}

const server = createHttpServer({
  store,
  uiHtml,
  config: config.http,
  log,
  assets: assets.byName,
  imageOrigin: origin,
  auth,
});
server.listen(config.port, config.host, () => {
  log(`${config.mode === 'live' ? 'live 모드(계정 연결)' : '데모 모드'} · pid ${process.pid} · http://${config.host}:${config.port}/mcp`);
  if (auth) {
    log(`OAuth 발급자 ${auth.issuer.href} · 리소스 ${auth.resource.href}`);
    log(`허용 redirect_uri: ${config.live.redirectAllowlist.join(', ')}`);
    log(
      config.live.recommend
        ? `실제 추천: 켜짐 → http://127.0.0.1:${config.live.recommend.port} (제한 ${config.live.recommend.timeoutMs / 1000}초, 동시 ${config.live.recommend.maxConcurrent})`
        : '실제 추천: 꺼짐(WARDROBE_DELEGATION_KEY 없음)',
    );
  }
  log(`허용 Host: ${config.http.allowedHosts.join(', ')}`);
  log(`데모 기록 파일: ${config.dataFile} (보존 ${config.ttlHours}시간)`);
  const sets = store.availablePhotoSets();
  log(`생성 룩북 사진: asset ${assets.byKey.size}개, 사진 세트 ${sets.length ? sets.join(', ') : '없음'} · 이미지 origin ${origin}`);
  for (const p of assets.problems) log(`사진 asset 제외: ${p}`);
  if (sets.length && !config.publicHost) {
    log('주의: PUBLIC_HOST 가 없어 사진 URL 이 로컬 http 예요. ChatGPT·Claude 에서는 사진이 보이지 않아요.');
  }
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
