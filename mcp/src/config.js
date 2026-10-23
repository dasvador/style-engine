// 환경 변수 → 설정. 실제 서비스의 .env 는 읽지 않는다(dotenv 를 쓰지 않는다).

import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

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

export function loadConfig(env = process.env) {
  const mode = env.WARDROBE_MCP_MODE ?? 'demo';
  if (mode !== 'demo') {
    // 실서비스 연결은 이번 범위가 아니다(src/adapters/live.js 참고). 조용히 데모로 바꾸지 않고 멈춘다.
    throw new Error(`WARDROBE_MCP_MODE=${mode} 는 지원하지 않아요. 이 서버는 demo 모드만 있어요.`);
  }
  const port = int('MCP_PORT', env.MCP_PORT, 8787, 1, 65535);
  if (port === 3003) throw new Error('3003 은 Rust 본 서버 포트예요. 데모 MCP 는 다른 포트를 쓰세요.');
  const host = env.MCP_HOST ?? '127.0.0.1';
  // PUBLIC_HOST: 터널이 붙여 주는 공개 호스트 이름(예: abcd.trycloudflare.com). 포트는 붙이지 않는다.
  const publicHost = env.PUBLIC_HOST?.trim() || null;
  if (publicHost && !/^[a-z0-9.-]+(:\d+)?$/i.test(publicHost)) throw new Error(`PUBLIC_HOST 형식이 이상해요: ${publicHost}`);

  return {
    mode,
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
    },
    uiFile: resolve(PACKAGE_DIR, 'dist/ui.html'),
  };
}
