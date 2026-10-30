// 환경 변수 → 설정. 실제 서비스의 .env 는 읽지 않는다(dotenv 를 쓰지 않는다).

import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

import { DEFAULT_REDIRECT_URIS } from './auth/oauth.js';
import { defaultAllowedHosts } from './http.js';

const PACKAGE_DIR = resolve(fileURLToPath(new URL('..', import.meta.url)));

function int(name, value, fallback, min, max) {
  if (value === undefined || value === '') return fallback;
  const n = Number(value);
  if (!Number.isInteger(n) || n < min || n > max) throw new Error(`${name} 는 ${min}..${max} 사이 정수여야 해요: ${value}`);
  return n;
}

function list(value) {
  return (value ?? '')
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean);
}

/**
 * 실제 추천(Rust 추천 엔진) 연결. 위임 키(WARDROBE_DELEGATION_KEY, Rust 와 같은 값)가 있을 때만 켠다.
 * 주소는 언제나 http://127.0.0.1:<LIVE_RECOMMEND_PORT> — 호스트는 바꿀 수 없다.
 */
function recommendConfig(env) {
  const raw = env.WARDROBE_DELEGATION_KEY?.trim();
  if (!raw) return null;
  if (!/^[0-9a-f]{64,}$/i.test(raw) || raw.length % 2 !== 0) {
    // 값은 출력하지 않는다.
    throw new Error('WARDROBE_DELEGATION_KEY 는 32바이트 이상 hex 여야 해요.');
  }
  return {
    key: Buffer.from(raw, 'hex'),
    port: int('LIVE_RECOMMEND_PORT', env.LIVE_RECOMMEND_PORT, 3003, 1, 65535),
    timeoutMs: int('RECOMMEND_TIMEOUT_SECONDS', env.RECOMMEND_TIMEOUT_SECONDS, 90, 5, 300) * 1000,
    maxConcurrent: int('RECOMMEND_MAX_CONCURRENT', env.RECOMMEND_MAX_CONCURRENT, 2, 1, 8),
  };
}

export function loadConfig(env = process.env) {
  const mode = env.WARDROBE_MCP_MODE ?? 'demo';
  if (mode !== 'demo' && mode !== 'live') {
    // 모르는 값을 조용히 데모로 바꾸지 않고 멈춘다.
    throw new Error(`WARDROBE_MCP_MODE=${mode} 는 지원하지 않아요(demo 또는 live).`);
  }
  const port = int('MCP_PORT', env.MCP_PORT, 8787, 1, 65535);
  if (port === 3003) throw new Error('3003 은 Rust 본 서버 포트예요. 데모 MCP 는 다른 포트를 쓰세요.');
  const host = env.MCP_HOST ?? '127.0.0.1';
  // PUBLIC_HOST: 터널이 붙여 주는 공개 호스트 이름(예: abcd.trycloudflare.com). 포트는 붙이지 않는다.
  const publicHost = env.PUBLIC_HOST?.trim() || null;
  if (publicHost && !/^[a-z0-9.-]+(:\d+)?$/i.test(publicHost)) throw new Error(`PUBLIC_HOST 형식이 이상해요: ${publicHost}`);

  // live: 계정 연결(OAuth) + 연결된 사용자의 실제 옷장 읽기. 공개 HTTPS 주소와 DB 가 있어야 켠다.
  let live = null;
  if (mode === 'live') {
    const localIssuer = env.LIVE_LOCAL_ISSUER === '1';
    if (!publicHost && !localIssuer) {
      throw new Error('live 모드에는 PUBLIC_HOST(HTTPS 공개 호스트)가 필요해요. 로컬 시험만 하려면 LIVE_LOCAL_ISSUER=1.');
    }
    if (!env.LIVE_DATABASE_URL) throw new Error('live 모드에는 LIVE_DATABASE_URL(읽기 전용 DB 접속)이 필요해요.');
    live = {
      // 발급자·리소스 origin. 요청 헤더가 아니라 설정에서만 만든다.
      origin: publicHost && !localIssuer ? `https://${publicHost.toLowerCase()}` : `http://127.0.0.1:${port}`,
      databaseUrl: env.LIVE_DATABASE_URL,
      redirectAllowlist: [...new Set([...DEFAULT_REDIRECT_URIS, ...list(env.OAUTH_REDIRECT_URIS)])],
      authStoreFile: env.AUTH_STORE_FILE ? resolve(env.AUTH_STORE_FILE) : resolve(PACKAGE_DIR, '.data/auth-store.json'),
      linkCodesFile: env.LINK_CODES_FILE ? resolve(env.LINK_CODES_FILE) : resolve(PACKAGE_DIR, '.data/link-codes.json'),
      recommend: recommendConfig(env),
    };
  }

  return {
    mode,
    live,
    host,
    port,
    publicHost,
    dataFile: env.DEMO_DATA_FILE ? resolve(env.DEMO_DATA_FILE) : resolve(PACKAGE_DIR, '.data/demo-store.json'),
    ttlHours: int('DEMO_TTL_HOURS', env.DEMO_TTL_HOURS, 168, 1, 24 * 90),
    maxComparisons: int('DEMO_MAX_COMPARISONS', env.DEMO_MAX_COMPARISONS, 500, 10, 10_000),
    http: {
      allowedHosts: [...defaultAllowedHosts(port, publicHost), ...list(env.ALLOWED_HOSTS)],
      allowedOrigins: list(env.ALLOWED_ORIGINS),
      maxBodyBytes: int('MAX_BODY_BYTES', env.MAX_BODY_BYTES, 64 * 1024, 1024, 1024 * 1024),
      maxBatch: 10,
      rateLimitPerMinute: int('RATE_LIMIT_PER_MINUTE', env.RATE_LIMIT_PER_MINUTE, 300, 10, 100_000),
      // 진단용 접근 로그(메타데이터만, 기본 꺼짐). MCP_ACCESS_LOG=1
      accessLog: env.MCP_ACCESS_LOG === '1',
    },
    uiFile: resolve(PACKAGE_DIR, 'dist/ui.html'),
    // 이미 생성해 둔 룩북 사진의 로컬 사본·manifest(gitignore). 없으면 사진 비교 도구가 나오지 않는다.
    assetsDir: env.DEMO_ASSETS_DIR ? resolve(env.DEMO_ASSETS_DIR) : resolve(PACKAGE_DIR, '.assets'),
  };
}
