import type { IncomingMessage, ServerResponse } from 'node:http';
import { defineConfig, type Plugin } from 'vite';
import react from '@vitejs/plugin-react';

// 개발 환경에서만 /api 를 Axum 개발 서버로 프록시한다.
// 운영에서는 Axum 이 dist 와 /api 를 같은 origin 에서 서빙하므로 프록시가 필요 없고,
// 그래서 프런트 코드는 항상 상대 경로 '/api/...' 로만 호출한다.
const BACKEND = process.env.VITE_BACKEND_ORIGIN ?? 'http://localhost:3003';

/**
 * 이 컴퓨터에서 직접 연 브라우저만 백엔드 프록시를 쓰게 한다.
 *
 * 백엔드의 개발 모드(WARDROBE_AUTH_MODE=dev-default-loopback)는 "루프백 연결 + localhost Host" 인 토큰 없는
 * 요청을 default 사용자로 본다. 프록시는 요청을 루프백에서 다시 보내고 Host 도 localhost 로 바꾸므로
 * (changeOrigin), 이 서버가 LAN(`--host`)이나 터널로 열려 있으면 바깥 요청이 그대로 "로컬" 이 된다.
 * 그래서 프록시 앞에서 원래 연결을 본다: 연결 상대가 루프백이고, Host 가 로컬이고, 전달 헤더가 없을 때만 넘긴다.
 */
const LOOPBACK = new Set(['127.0.0.1', '::1', '::ffff:127.0.0.1']);
const FORWARDING = ['forwarded', 'x-forwarded-for', 'x-forwarded-host', 'x-real-ip', 'cf-connecting-ip', 'true-client-ip'];

function isLocalHost(host: string | undefined): boolean {
  if (!host) return false;
  const name = host.startsWith('[') ? host.slice(1, host.indexOf(']')) : host.replace(/:\d+$/, '');
  return ['localhost', '127.0.0.1', '::1'].includes(name.toLowerCase());
}

function localBackendOnly(): Plugin {
  const guard = (req: IncomingMessage, res: ServerResponse, next: () => void) => {
    const path = req.url ?? '';
    if (!path.startsWith('/api') && !path.startsWith('/static')) return next();
    const local =
      LOOPBACK.has(req.socket.remoteAddress ?? '') &&
      isLocalHost(req.headers.host) &&
      !FORWARDING.some((h) => h in req.headers);
    if (local) return next();
    res.statusCode = 403;
    res.setHeader('content-type', 'text/plain; charset=utf-8');
    res.end('개발 서버의 백엔드 프록시는 이 컴퓨터에서 직접 연 브라우저만 쓸 수 있어요.');
  };
  return {
    name: 'wardrobe-local-backend-only',
    // configureServer 에서 바로 붙인 미들웨어는 Vite 내장(프록시 포함) 미들웨어보다 먼저 돈다.
    configureServer(server) {
      server.middlewares.use(guard);
    },
    configurePreviewServer(server) {
      server.middlewares.use(guard);
    },
  };
}

const proxy = {
  '/api': { target: BACKEND, changeOrigin: true, xfwd: false },
  // 생성 코디 사진은 Axum 이 /static 으로 제공한다(백엔드 개발 모드의 로컬 요청에만 열림).
  '/static': { target: BACKEND, changeOrigin: true, xfwd: false },
};

export default defineConfig({
  plugins: [react(), localBackendOnly()],
  server: {
    // 루프백에만 연다. `--host` 로 바꿔도 위 미들웨어가 바깥 요청의 프록시를 막는다.
    host: '127.0.0.1',
    port: 5173,
    proxy,
  },
  preview: {
    host: '127.0.0.1',
    proxy,
  },
  build: {
    outDir: 'dist',
    sourcemap: false,
  },
});
