// ui/app.js(+ ext-apps App bridge)를 한 덩어리로 묶어 CSS 와 함께 dist/ui.html 하나에 넣는다.
// MCP Apps 화면은 외부 스크립트를 불러오지 않는 단일 HTML 이어야 한다(CSP 의 resourceDomains 가 비어 있다).
// 테스트용 호스트 흉내(test/harness)도 같이 묶는다.

import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

import { build } from 'esbuild';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const at = (p) => resolve(root, p);

const bundle = await build({
  entryPoints: [at('ui/app.js')],
  bundle: true,
  format: 'iife',
  platform: 'browser',
  target: ['es2020', 'safari15'],
  minify: true,
  write: false,
  legalComments: 'none',
});
const js = bundle.outputFiles[0].text.replaceAll('</script', '<\\/script');
const css = await readFile(at('ui/style.css'), 'utf8');
const template = await readFile(at('ui/index.html'), 'utf8');
// 치환 문자열의 `$` 패턴이 해석되지 않도록 함수로 넘긴다.
const html = template.replace('/*__STYLE__*/', () => css).replace('/*__SCRIPT__*/', () => js);

await mkdir(at('dist'), { recursive: true });
await writeFile(at('dist/ui.html'), html);

const harness = await build({
  entryPoints: [at('test/harness/host.js')],
  bundle: true,
  format: 'iife',
  platform: 'browser',
  target: ['es2020'],
  write: false,
});
await writeFile(at('dist/test-host.js'), harness.outputFiles[0].text);

console.error(`dist/ui.html ${(html.length / 1024).toFixed(1)}KB, dist/test-host.js 생성`);
