import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// 개발 환경에서만 /api 를 Axum 개발 서버로 프록시한다.
// 운영에서는 Axum 이 dist 와 /api 를 같은 origin 에서 서빙하므로 프록시가 필요 없고,
// 그래서 프런트 코드는 항상 상대 경로 '/api/...' 로만 호출한다.
const BACKEND = process.env.VITE_BACKEND_ORIGIN ?? 'http://localhost:3003';

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      '/api': { target: BACKEND, changeOrigin: true },
      // 업로드된 의류/룩북 이미지는 Axum 이 /static 으로 제공한다.
      '/static': { target: BACKEND, changeOrigin: true },
    },
  },
  build: {
    outDir: 'dist',
    sourcemap: false,
  },
});
