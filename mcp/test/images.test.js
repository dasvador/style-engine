// 생성 룩북 사진: asset 허용 목록 검사, 이미지 HTTP 경로, 사진 비교 도구·CSP·선택 저장.
// 실제 생성 사진은 쓰지 않는다 — 테스트가 만든 작은 PNG 로 돈다.

import assert from 'node:assert/strict';
import { request as httpRequest } from 'node:http';
import { readFile, rm, symlink, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { after, before, describe, test } from 'node:test';

import { RESOURCE_MIME_TYPE } from '@modelcontextprotocol/ext-apps/server';

import { DemoStore } from '../src/demo-store.js';
import { imageResolver, loadImageAssets } from '../src/image-assets.js';
import { LEGACY_UI_URIS, TOOL_NAMES, UI_URI } from '../src/mcp-server.js';
import { PHOTO_ASSET_KEYS, PHOTO_SETS } from '../src/photo-sets.js';
import { connectClient, makePng, makeTestAssets, startServer, tempDir } from './helpers.js';

const PUBLIC = 'demo-abc.trycloudflare.com';

/** 경로를 정규화하지 않고 그대로 보내는 GET/HEAD(fetch 는 `..` 를 먼저 접는다). */
function rawGet(port, path, { method = 'GET', headers = {} } = {}) {
  return new Promise((resolve, reject) => {
    const req = httpRequest({ host: '127.0.0.1', port, path, method, headers }, (res) => {
      const chunks = [];
      res.on('data', (c) => chunks.push(c));
      res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) }));
    });
    req.on('error', reject);
    req.end();
  });
}

describe('asset 허용 목록 검사', () => {
  test('정상 manifest 는 사진 4장을 싣는다', async () => {
    const a = await makeTestAssets();
    try {
      const r = await loadImageAssets(a.dir);
      assert.deepEqual([...r.byKey.keys()].sort(), [...PHOTO_ASSET_KEYS].sort());
      assert.deepEqual(r.problems, []);
      for (const asset of r.byKey.values()) assert.equal(asset.mime, 'image/png');
    } finally {
      await a.cleanup();
    }
  });

  test('없는 디렉터리·manifest 는 사진 없음(오류 아님)', async () => {
    const r = await loadImageAssets('/nonexistent/wardrobe-assets');
    assert.equal(r.byKey.size, 0);
    const none = await loadImageAssets(null);
    assert.equal(none.byKey.size, 0);
  });

  test('손상·서명 불일치·심볼릭 링크·이상한 이름·없는 파일은 그 asset 만 뺀다', async () => {
    const a = await makeTestAssets();
    const outside = await tempDir();
    try {
      const manifest = JSON.parse(await readFile(join(a.dir, 'manifest.json'), 'utf8'));
      const [k1, k2, k3, k4] = PHOTO_ASSET_KEYS;
      // k1: 내용 손상(sha256 불일치)
      await writeFile(join(a.dir, 'images', a.files[k1]), makePng(20, 30, [1, 2, 3]));
      // k2: 심볼릭 링크로 바꿈(디렉터리 밖 .env 흉내)
      await writeFile(join(outside.dir, '.env'), 'SECRET=1');
      await rm(join(a.dir, 'images', a.files[k2]));
      await symlink(join(outside.dir, '.env'), join(a.dir, 'images', a.files[k2]));
      // k3: 경로를 품은 이름
      manifest.assets[k3].file = '../../.env';
      // k4: 파일 없음
      await rm(join(a.dir, 'images', a.files[k4]));
      await writeFile(join(a.dir, 'manifest.json'), JSON.stringify(manifest));

      const r = await loadImageAssets(a.dir);
      assert.equal(r.byKey.size, 0);
      assert.equal(r.problems.length, 4);
      assert.match(r.problems.join('\n'), /sha256 불일치/);
      assert.match(r.problems.join('\n'), /심볼릭 링크/);
      assert.match(r.problems.join('\n'), /이름 모양/);
      assert.match(r.problems.join('\n'), /파일 없음/);
    } finally {
      await a.cleanup();
      await outside.cleanup();
    }
  });

  test('확장자와 실제 형식이 다르면 뺀다(.png 이름의 텍스트 파일)', async () => {
    const a = await makeTestAssets([PHOTO_ASSET_KEYS[0]]);
    try {
      const manifest = JSON.parse(await readFile(join(a.dir, 'manifest.json'), 'utf8'));
      const text = Buffer.from('<svg onload=alert(1)>');
      const { createHash } = await import('node:crypto');
      const sha = createHash('sha256').update(text).digest('hex');
      const name = `${sha.slice(0, 32)}.png`;
      await writeFile(join(a.dir, 'images', name), text);
      manifest.assets[PHOTO_ASSET_KEYS[0]] = { ...manifest.assets[PHOTO_ASSET_KEYS[0]], file: name, sha256: sha };
      await writeFile(join(a.dir, 'manifest.json'), JSON.stringify(manifest));
      const r = await loadImageAssets(a.dir);
      assert.equal(r.byKey.size, 0);
      assert.match(r.problems[0], /서명/);
    } finally {
      await a.cleanup();
    }
  });
});

describe('사진 비교 — 서버(테스트 asset, PUBLIC_HOST 설정)', () => {
  let a;
  let srv;
  let client;

  before(async () => {
    a = await makeTestAssets();
    srv = await startServer({ assetsDir: a.dir, publicHost: PUBLIC });
    client = await connectClient(srv.url);
  });
  after(async () => {
    await client?.close();
    await srv?.close();
    await a?.cleanup();
  });

  test('허용된 사진만 GET/HEAD, 정확한 MIME·길이·ETag·캐시, 304', async () => {
    const name = a.files[PHOTO_ASSET_KEYS[0]];
    const get = await rawGet(srv.port, `/assets/img/${name}`);
    assert.equal(get.status, 200);
    assert.equal(get.headers['content-type'], 'image/png');
    assert.equal(Number(get.headers['content-length']), get.body.length);
    assert.equal(get.headers['x-content-type-options'], 'nosniff');
    assert.equal(get.headers['cross-origin-resource-policy'], 'cross-origin');
    assert.match(get.headers['cache-control'], /immutable/);
    assert.equal(get.headers['access-control-allow-origin'], undefined);
    assert.deepEqual(get.body, await readFile(join(a.dir, 'images', name)));

    const head = await rawGet(srv.port, `/assets/img/${name}`, { method: 'HEAD' });
    assert.equal(head.status, 200);
    assert.equal(head.body.length, 0);
    assert.equal(head.headers['content-length'], get.headers['content-length']);

    const cached = await rawGet(srv.port, `/assets/img/${name}`, { headers: { 'if-none-match': get.headers.etag } });
    assert.equal(cached.status, 304);
    assert.equal(cached.body.length, 0);

    const post = await rawGet(srv.port, `/assets/img/${name}`, { method: 'POST' });
    assert.equal(post.status, 405);
  });

  test('목록 밖·경로 조작·원본 static·manifest·.env 는 404, 다른 Host 는 403', async () => {
    const paths = [
      '/assets/img/' + '0'.repeat(32) + '.png',
      '/assets/img/' + a.files[PHOTO_ASSET_KEYS[0]].replace('.png', '.jpg'),
      '/assets/img/',
      '/assets/img/../../.env',
      '/assets/img/..%2f..%2f.env',
      '/assets/img/%2e%2e/%2e%2e/.env',
      '/assets/img/....//....//.env',
      '/assets/img/manifest.json',
      '/assets/manifest.json',
      '/assets/sources.json',
      '/assets/img/sources.json',
      '/.assets/manifest.json',
      '/static/images/11111111-2222-4333-8444-555555555555.png',
      '/assets/img/' + a.files[PHOTO_ASSET_KEYS[0]] + '/..',
      '/.env',
    ];
    for (const p of paths) {
      const r = await rawGet(srv.port, p);
      assert.equal(r.status, 404, p);
      assert.doesNotMatch(r.body.toString(), /SECRET|DATABASE_URL|static_file/, p);
    }
    const evil = await rawGet(srv.port, `/assets/img/${a.files[PHOTO_ASSET_KEYS[0]]}`, { headers: { host: 'evil.example.com' } });
    assert.equal(evil.status, 403);
  });

  test('도구 목록에 사진 비교 도구(준비된 세트만), CSP 는 자기 https origin 하나', async () => {
    const { tools } = await client.listTools();
    const photo = tools.find((t) => t.name === TOOL_NAMES.photo);
    assert.ok(photo);
    assert.deepEqual(photo.inputSchema.properties.set.enum.sort(), Object.keys(PHOTO_SETS).sort());
    assert.equal(photo.inputSchema.additionalProperties, false);
    assert.deepEqual(photo.annotations, { readOnlyHint: false, destructiveHint: false, idempotentHint: false, openWorldHint: false });
    assert.match(photo.description, /이전에 이미 만들어 둔/);
    assert.match(photo.description, /새로 추천하거나 이미지를 생성하지 않/);
    assert.equal(photo._meta.ui.resourceUri, UI_URI);
    assert.equal(UI_URI, 'ui://wardrobe-edit/compare-v3.html');

    for (const uri of [UI_URI, ...LEGACY_UI_URIS]) {
      const { contents } = await client.readResource({ uri });
      assert.equal(contents[0].mimeType, RESOURCE_MIME_TYPE);
      assert.deepEqual(contents[0]._meta.ui.csp, { connectDomains: [], resourceDomains: [`https://${PUBLIC}`] }, uri);
    }
  });

  test('사진 비교: 각 쪽 사진은 그 쪽 아이템으로 만든 사진, https 절대 URL, 출처·표시 정보', async () => {
    for (const set of Object.keys(PHOTO_SETS)) {
      const res = await client.callTool({ name: TOOL_NAMES.photo, arguments: { set } });
      assert.equal(res.isError, undefined, JSON.stringify(res));
      const c = res.structuredContent;
      assert.equal(c.data_source, 'curated_generated_lookbook');
      assert.match(c.notice, /이전에 만들어 둔 생성 룩북 사진/);
      assert.match(c.notice, /새로 추천하거나 생성한 것이 아니/);
      assert.equal(c.context.photo_set, set);
      assert.equal(c.context.mood_key, PHOTO_SETS[set].mood);
      for (const o of c.options) {
        // 아이템 묶음으로 어느 쪽 정의인지 찾고, 사진이 바로 그 정의의 asset 인지 본다.
        const side = PHOTO_SETS[set].sides.find((s) => s.items[0].id === o.items[0].id);
        assert.ok(side, '정의된 아이템 묶음 그대로');
        assert.deepEqual(o.items.map((i) => i.name), side.items.map((i) => i.name));
        assert.equal(o.image.url, `https://${PUBLIC}/assets/img/${a.files[side.key]}`);
        assert.equal(o.image.alt, side.alt);
        assert.equal(o.image.presentation, 'generated_lookbook_photo');
        assert.equal(o.image.source, 'existing_generated_image');
        assert.equal(o.image.matches, 'items_of_this_option');
        assert.equal(o.image.width, 20);
      }
      assert.notEqual(c.options[0].image.url, c.options[1].image.url);
      assert.match(res.content[0].text, /이전에 생성해 둔 룩북 사진 있음/);
      assert.doesNotMatch(JSON.stringify(res), /static\/images|static_file|[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}|sources\.json/);
    }
  });

  test('사진 비교도 선택 저장·수정·다시 읽기가 같다', async () => {
    const c = (await client.callTool({ name: TOOL_NAMES.photo, arguments: { set: 'pop_color_pattern' } })).structuredContent;
    const saved = await client.callTool({ name: TOOL_NAMES.vote, arguments: { comparison_id: c.id, choice: 'a', reasons: ['color_combo'] } });
    assert.equal(saved.structuredContent.outcome, 'saved');
    assert.ok(saved.structuredContent.comparison.options[0].image.url);
    const updated = await client.callTool({
      name: TOOL_NAMES.update,
      arguments: { comparison_id: c.id, expected_revision: 1, choice: 'b', reasons: ['silhouette'] },
    });
    assert.equal(updated.structuredContent.outcome, 'updated');
    const read = await client.callTool({ name: TOOL_NAMES.get, arguments: { comparison_id: c.id } });
    assert.equal(read.structuredContent.vote.choice, 'b');
    assert.deepEqual(read.structuredContent.options, c.options, '좌우·사진이 처음 그대로');
  });

  test('글자 비교에는 사진이 없다(image null) — 무드가 없는 조합에 사진을 끼우지 않는다', async () => {
    const c = (await client.callTool({ name: TOOL_NAMES.create, arguments: { situation: 'daily', mood: 'amekaji' } })).structuredContent;
    assert.equal(c.data_source, 'sample_fixture');
    assert.deepEqual(c.options.map((o) => o.image), [null, null]);
    assert.equal(c.context.photo_set, null);
  });

  test('잘못된 세트·임의 URL·경로 인자는 입력 검증 오류', async () => {
    for (const args of [
      { set: 'glam' },
      { set: '../../static/images/11111111-2222-4333-8444-555555555555.png' },
      { set: 'pop_color_pattern', image_url: 'https://evil.example.com/x.png' },
      { set: 'pop_color_pattern', path: '/etc/passwd' },
      {},
    ]) {
      const r = await client.callTool({ name: TOOL_NAMES.photo, arguments: args });
      assert.equal(r.isError, true, JSON.stringify(args));
    }
  });
});

describe('사진이 없거나 일부만 있을 때', () => {
  test('asset 이 없으면 사진 도구가 없고 CSP 도 비어 있다', async () => {
    const srv = await startServer({ publicHost: PUBLIC });
    const client = await connectClient(srv.url);
    try {
      const { tools } = await client.listTools();
      assert.equal(tools.some((t) => t.name === TOOL_NAMES.photo), false);
      const { contents } = await client.readResource({ uri: UI_URI });
      assert.deepEqual(contents[0]._meta.ui.csp, { connectDomains: [], resourceDomains: [] });
    } finally {
      await client.close();
      await srv.close();
    }
  });

  test('한 세트의 사진만 있으면 그 세트만 고를 수 있다', async () => {
    const popKeys = PHOTO_SETS.pop_color_pattern.sides.map((s) => s.key);
    const a = await makeTestAssets(popKeys);
    const srv = await startServer({ assetsDir: a.dir, publicHost: PUBLIC });
    const client = await connectClient(srv.url);
    try {
      const photo = (await client.listTools()).tools.find((t) => t.name === TOOL_NAMES.photo);
      assert.deepEqual(photo.inputSchema.properties.set.enum, ['pop_color_pattern']);
      const r = await client.callTool({ name: TOOL_NAMES.photo, arguments: { set: 'edgy_leather' } });
      assert.equal(r.isError, true);
    } finally {
      await client.close();
      await srv.close();
      await a.cleanup();
    }
  });

  test('저장된 사진 비교를 사진 없이 다시 읽으면 image 는 null(다른 사진으로 바꾸지 않음), 투표 기록은 그대로', async () => {
    const a = await makeTestAssets();
    const tmp = await tempDir();
    try {
      const file = join(tmp.dir, 'store.json');
      const assets = await loadImageAssets(a.dir);
      const s1 = await new DemoStore({ file, images: imageResolver(assets.byKey, 'https://x.example') }).load();
      const c = await s1.createPhotoComparison({ set: 'edgy_leather' });
      await s1.submitVote(c.id, { choice: 'b' });
      const s2 = await new DemoStore({ file }).load(); // 사진 없음
      const view = s2.getComparison(c.id);
      assert.deepEqual(view.options.map((o) => o.image), [null, null]);
      assert.deepEqual(view.options.map((o) => o.items), c.options.map((o) => o.items));
      assert.equal(view.vote.choice, 'b');
      assert.equal(view.data_source, 'curated_generated_lookbook');
      const raw = JSON.parse(await readFile(file, 'utf8')).comparisons[c.id];
      assert.equal(raw.kind, 'photo');
      assert.ok(!JSON.stringify(raw).includes('https://'), '저장 파일에는 URL 을 남기지 않는다');
    } finally {
      await a.cleanup();
      await tmp.cleanup();
    }
  });

  test('PUBLIC_HOST 가 없으면 로컬 http origin(로컬 검사용)', async () => {
    const a = await makeTestAssets();
    const srv = await startServer({ assetsDir: a.dir });
    const client = await connectClient(srv.url);
    try {
      const c = (await client.callTool({ name: TOOL_NAMES.photo, arguments: { set: 'pop_color_pattern' } })).structuredContent;
      assert.match(c.options[0].image.url, new RegExp(`^http://127\\.0\\.0\\.1:${srv.port}/assets/img/[0-9a-f]{32}\\.png$`));
      const { contents } = await client.readResource({ uri: UI_URI });
      assert.deepEqual(contents[0]._meta.ui.csp.resourceDomains, [`http://127.0.0.1:${srv.port}`]);
    } finally {
      await client.close();
      await srv.close();
      await a.cleanup();
    }
  });
});
