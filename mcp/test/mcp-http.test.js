// 공식 SDK Client 로 진짜 HTTP 서버에 붙어 initialize → list → read → call 을 확인한다.
// 데모 fixture 로만 돈다 — 실제 추천·DB·유료 API 는 부르지 않는다.

import assert from 'node:assert/strict';
import { join } from 'node:path';
import { after, before, describe, test } from 'node:test';

import { RESOURCE_MIME_TYPE } from '@modelcontextprotocol/ext-apps/server';

import { ERROR_META_KEY, LEGACY_UI_URIS, TOOL_NAMES, UI_URI } from '../src/mcp-server.js';
import { connectClient, rawHttp, rawPost, startServer, tempDir } from './helpers.js';

const INIT = {
  jsonrpc: '2.0',
  id: 1,
  method: 'initialize',
  params: { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'raw', version: '0' } },
};

describe('MCP over Streamable HTTP', () => {
  let srv;
  let client;
  let tmp;

  before(async () => {
    tmp = await tempDir();
    srv = await startServer({ file: join(tmp.dir, 'store.json') });
    client = await connectClient(srv.url);
  });
  after(async () => {
    await client?.close();
    await srv?.close();
    await tmp?.cleanup();
  });

  test('initialize: 서버 이름과 데모 안내', () => {
    assert.equal(client.getServerVersion().name, 'wardrobe-edit-demo');
    const text = client.getInstructions() ?? '';
    assert.match(text, /데모 전용/);
    assert.match(text, /데모·샘플이라는 점을 밝힌다/);
    assert.match(text, /대신 고르지 않는다/);
    // 데모 모드에는 실제 계정 도구가 없다 — 지시문도 그것을 말하지 않는다.
    assert.doesNotMatch(text, /get_my_profile|list_my_wardrobe|연결된|실제 프로필/);
  });

  test('tools/list: 도구 다섯(사진 asset 없음), annotation·UI 연결·입력 스키마', async () => {
    const { tools } = await client.listTools();
    const byName = Object.fromEntries(tools.map((t) => [t.name, t]));
    // 이 서버에는 사진 asset 이 없다 — 사진 비교 도구는 나오지 않는다(test/images.test.js 에서 따로 본다).
    assert.deepEqual(
      Object.keys(byName).sort(),
      Object.values(TOOL_NAMES)
        .filter((n) => ![TOOL_NAMES.photo, TOOL_NAMES.myProfile, TOOL_NAMES.myWardrobe].includes(n))
        .sort(),
    );
    // demo 모드: 계정 연결 없음 — 모든 도구가 noauth 를 알린다(OpenAI: 도구 정의 최상위 securitySchemes).
    // SDK 클라이언트는 모르는 최상위 필드를 버리므로 _meta 사본으로 보고, 최상위는 원시 응답으로 본다.
    for (const t of tools) assert.deepEqual(t._meta.securitySchemes, [{ type: 'noauth' }], t.name);
    const raw = await rawPost(srv.url, { jsonrpc: '2.0', id: 9, method: 'tools/list', params: {} });
    for (const t of raw.json.result.tools) assert.deepEqual(t.securitySchemes, [{ type: 'noauth' }], t.name);

    assert.deepEqual(byName[TOOL_NAMES.browse].annotations, {
      readOnlyHint: true,
      destructiveHint: false,
      idempotentHint: true,
      openWorldHint: false,
    });
    assert.equal(byName[TOOL_NAMES.create].annotations.readOnlyHint, false);
    assert.equal(byName[TOOL_NAMES.create].annotations.idempotentHint, false);
    assert.equal(byName[TOOL_NAMES.get].annotations.readOnlyHint, true);
    assert.equal(byName[TOOL_NAMES.vote].annotations.destructiveHint, false);
    assert.equal(byName[TOOL_NAMES.vote].annotations.idempotentHint, true);
    assert.deepEqual(byName[TOOL_NAMES.update].annotations, {
      readOnlyHint: false,
      destructiveHint: true,
      idempotentHint: true,
      openWorldHint: false,
    });
    assert.deepEqual(byName[TOOL_NAMES.update].inputSchema.required.sort(), ['choice', 'comparison_id', 'expected_revision']);
    for (const t of tools) {
      assert.equal(t.annotations.openWorldHint, false, t.name);
      assert.match(t.description, /데모/, t.name);
      assert.equal(t.inputSchema.additionalProperties, false, `${t.name} 는 모르는 인자를 받지 않는다`);
    }
    assert.equal(UI_URI, 'ui://wardrobe-edit/compare-v3.html');
    for (const name of [TOOL_NAMES.create, TOOL_NAMES.get, TOOL_NAMES.vote, TOOL_NAMES.update]) {
      assert.equal(byName[name]._meta.ui.resourceUri, UI_URI, name);
    }
    assert.equal(byName[TOOL_NAMES.browse]._meta?.ui, undefined);
    assert.deepEqual(byName[TOOL_NAMES.vote].inputSchema.properties.choice.enum, ['a', 'b', 'neither']);
  });

  test('resources/list·read: UI 는 v3 + v1·v2 별칭(같은 HTML), 사진 없으면 외부 도메인 없음', async () => {
    const { resources } = await client.listResources();
    assert.deepEqual(
      resources.map((r) => r.uri),
      [UI_URI, ...LEGACY_UI_URIS],
    );
    const legacy = await client.readResource({ uri: LEGACY_UI_URIS[0] });
    assert.equal(legacy.contents[0].uri, LEGACY_UI_URIS[0]);
    assert.equal(legacy.contents[0].text, srv.uiHtml, '옛 카드도 새 화면을 받는다');
    const { contents } = await client.readResource({ uri: UI_URI });
    assert.equal(contents.length, 1);
    assert.equal(contents[0].mimeType, RESOURCE_MIME_TYPE);
    assert.deepEqual(contents[0]._meta.ui.csp, { connectDomains: [], resourceDomains: [] });
    const html = contents[0].text;
    assert.match(html, /<html lang="ko">/);
    assert.match(html, /ui\/initialize/, 'App bridge 가 묶여 있어야 한다');
    assert.doesNotMatch(html, /<script[^>]+src=/, '외부 스크립트를 불러오지 않는다');
    assert.doesNotMatch(html, /localhost:3003|127\.0\.0\.1:3003/);
  });

  test('browse_sample_wardrobe: 샘플 옷장, 카테고리 필터', async () => {
    const all = await client.callTool({ name: TOOL_NAMES.browse, arguments: {} });
    assert.equal(all.isError, undefined);
    assert.equal(all.structuredContent.demo, true);
    assert.equal(all.structuredContent.data_source, 'sample_fixture');
    assert.ok(all.structuredContent.items.length >= 10);
    assert.match(all.content[0].text, /샘플 옷장/);
    const shoes = await client.callTool({ name: TOOL_NAMES.browse, arguments: { category: '신발' } });
    assert.ok(shoes.structuredContent.items.every((i) => i.category === '신발'));
    const bad = await client.callTool({ name: TOOL_NAMES.browse, arguments: { category: '모자' } });
    assert.equal(bad.isError, true);
  });

  test('happy path: 비교 열기 → B 선택 저장 → 다시 읽기 → 같은 내용 재제출 → 다른 내용 충돌', async () => {
    const created = await client.callTool({ name: TOOL_NAMES.create, arguments: { situation: 'daily', mood: 'amekaji' } });
    assert.equal(created.isError, undefined);
    const c = created.structuredContent;
    assert.equal(c.demo, true);
    assert.equal(c.context.situation_label, '일상');
    assert.equal(c.context.mood_label, '아메카지');
    assert.match(c.context.weather_description, /샘플/);
    assert.equal(c.vote, null);
    assert.match(created.content[0].text, /데모 데이터예요/);
    assert.notDeepEqual(c.options[0].items, c.options[1].items);
    // 화면에 순위·점수·원래 순서를 내리지 않는다.
    assert.equal(JSON.stringify(c).match(/score|outfit_index|item_key|rank/), null);

    const vote = await client.callTool({
      name: TOOL_NAMES.vote,
      arguments: { comparison_id: c.id, choice: 'b', reasons: ['silhouette', 'color_combo'], comment: ' 바지 핏이 좋아요 ' },
    });
    assert.equal(vote.isError, undefined, JSON.stringify(vote));
    assert.equal(vote.structuredContent.ok, true);
    assert.equal(vote.structuredContent.already_submitted, false);
    assert.deepEqual(vote.structuredContent.comparison.vote.reasons, ['color_combo', 'silhouette']);
    assert.equal(vote.structuredContent.comparison.vote.comment, '바지 핏이 좋아요');

    const read = await client.callTool({ name: TOOL_NAMES.get, arguments: { comparison_id: c.id } });
    assert.equal(read.structuredContent.vote.choice, 'b');
    assert.deepEqual(read.structuredContent.options, c.options, '좌우와 아이템이 처음 그대로');

    const again = await client.callTool({
      name: TOOL_NAMES.vote,
      arguments: { comparison_id: c.id, choice: 'b', reasons: ['color_combo', 'silhouette'], comment: '바지 핏이 좋아요' },
    });
    assert.equal(again.structuredContent.already_submitted, true);

    // 다른 선택으로 다시 submit: 오류가 아니라 정상 결과(ok=false, outcome=conflict), 덮어쓰지 않음.
    const conflict = await client.callTool({ name: TOOL_NAMES.vote, arguments: { comparison_id: c.id, choice: 'a' } });
    assert.equal(conflict.isError, undefined, '호스트가 오류로 감싸지 않게 정상 결과로 돌려준다');
    assert.equal(conflict.structuredContent.ok, false);
    assert.equal(conflict.structuredContent.outcome, 'conflict');
    assert.equal(conflict.structuredContent.comparison.vote.choice, 'b', '저장된 쪽을 같이 돌려준다');
    assert.match(conflict.content[0].text, /^\[conflict\] 저장하지 않았어요/);
    const still = await client.callTool({ name: TOOL_NAMES.get, arguments: { comparison_id: c.id } });
    assert.equal(still.structuredContent.vote.choice, 'b');
  });

  test('neither: 둘 다 아쉬움은 저장되는 선택이다(이유 없이도)', async () => {
    const c = (await client.callTool({ name: TOOL_NAMES.create, arguments: { situation: 'office', mood: 'amekaji' } }))
      .structuredContent;
    const res = await client.callTool({ name: TOOL_NAMES.vote, arguments: { comparison_id: c.id, choice: 'neither' } });
    assert.equal(res.structuredContent.comparison.vote.choice, 'neither');
    assert.deepEqual(res.structuredContent.comparison.vote.reasons, []);
    assert.equal(res.structuredContent.comparison.vote.comment, null);
  });

  test('잘못된 입력: enum·id 모양·모르는 필드·긴 메모·없는 비교·다른 비교 id', async () => {
    const c = (await client.callTool({ name: TOOL_NAMES.create, arguments: { situation: 'weekend', mood: 'amekaji' } }))
      .structuredContent;
    const other = (await client.callTool({ name: TOOL_NAMES.create, arguments: { situation: 'weekend', mood: 'amekaji' } }))
      .structuredContent;
    const cases = [
      [TOOL_NAMES.vote, { comparison_id: c.id, choice: 'skip' }],
      [TOOL_NAMES.vote, { comparison_id: c.id, choice: 'c' }],
      [TOOL_NAMES.vote, { comparison_id: c.id }],
      [TOOL_NAMES.vote, { comparison_id: c.id, choice: 'a', reasons: ['price'] }],
      [TOOL_NAMES.vote, { comparison_id: c.id, choice: 'a', comment: '가'.repeat(201) }],
      [TOOL_NAMES.vote, { comparison_id: c.id, choice: 'a', item_ids: ['sample-top-01'] }],
      [TOOL_NAMES.vote, { comparison_id: c.id, choice: 'a', user_id: 'someone' }],
      [TOOL_NAMES.vote, { comparison_id: 'not-an-id', choice: 'a' }],
      [TOOL_NAMES.vote, { comparison_id: 'demo_cmp_' + 'f'.repeat(32), choice: 'a' }],
      [TOOL_NAMES.get, { comparison_id: '../../.env' }],
      [TOOL_NAMES.get, { comparison_id: 'demo_cmp_' + '0'.repeat(32) }],
      [TOOL_NAMES.create, { situation: 'party', mood: 'minimal' }],
      [TOOL_NAMES.create, { situation: 'daily' }],
      [TOOL_NAMES.create, { situation: 'daily', mood: 'minimal', batch_id: 'x' }],
    ];
    for (const [name, args] of cases) {
      const res = await client.callTool({ name, arguments: args });
      assert.equal(res.isError, true, `${name} ${JSON.stringify(args)} → ${JSON.stringify(res)}`);
      assert.ok(res.content[0].text.length > 0);
    }
    const notFound = await client.callTool({ name: TOOL_NAMES.get, arguments: { comparison_id: 'demo_cmp_' + '0'.repeat(32) } });
    assert.equal(notFound._meta[ERROR_META_KEY].code, 'not_found');

    // 실패한 시도 뒤에도 두 비교는 비어 있고, 한쪽에 저장해도 다른 쪽은 그대로다.
    for (const id of [c.id, other.id]) {
      assert.equal((await client.callTool({ name: TOOL_NAMES.get, arguments: { comparison_id: id } })).structuredContent.vote, null);
    }
    await client.callTool({ name: TOOL_NAMES.vote, arguments: { comparison_id: other.id, choice: 'a' } });
    assert.equal((await client.callTool({ name: TOOL_NAMES.get, arguments: { comparison_id: c.id } })).structuredContent.vote, null);
  });

  test('update_demo_preference: 저장 후 명시 수정, 재시도 멱등, 오래된 revision stale, 저장 전 no_vote', async () => {
    const c = (await client.callTool({ name: TOOL_NAMES.create, arguments: { situation: 'weekend', mood: 'minimal' } }))
      .structuredContent;
    const update = (args) => client.callTool({ name: TOOL_NAMES.update, arguments: { comparison_id: c.id, ...args } });

    const early = await update({ expected_revision: 1, choice: 'a' });
    assert.equal(early.isError, undefined);
    assert.equal(early.structuredContent.ok, false);
    assert.equal(early.structuredContent.outcome, 'no_vote');
    assert.match(early.content[0].text, /^\[no_vote\]/);

    await client.callTool({ name: TOOL_NAMES.vote, arguments: { comparison_id: c.id, choice: 'b', reasons: ['silhouette'] } });
    const changed = await update({ expected_revision: 1, choice: 'neither', reasons: ['weather_fit'], comment: '둘 다 추워 보여요' });
    assert.equal(changed.structuredContent.ok, true);
    assert.equal(changed.structuredContent.outcome, 'updated');
    assert.equal(changed.structuredContent.comparison.vote.revision, 2);
    assert.equal(changed.structuredContent.comparison.vote.choice, 'neither');

    const retry = await update({ expected_revision: 1, choice: 'neither', reasons: ['weather_fit'], comment: '둘 다 추워 보여요' });
    assert.equal(retry.structuredContent.outcome, 'already_submitted');
    assert.equal(retry.structuredContent.ok, true);
    assert.equal(retry.structuredContent.comparison.vote.revision, 2);

    const stale = await update({ expected_revision: 1, choice: 'a' });
    assert.equal(stale.isError, undefined);
    assert.equal(stale.structuredContent.ok, false);
    assert.equal(stale.structuredContent.outcome, 'stale');
    assert.equal(stale.structuredContent.comparison.vote.choice, 'neither');

    // 이유만 바꾸기
    const reasonsOnly = await update({ expected_revision: 2, choice: 'neither', reasons: ['color_combo'], comment: '둘 다 추워 보여요' });
    assert.equal(reasonsOnly.structuredContent.outcome, 'updated');
    const read = await client.callTool({ name: TOOL_NAMES.get, arguments: { comparison_id: c.id } });
    assert.deepEqual(read.structuredContent.vote.reasons, ['color_combo']);
    assert.equal(read.structuredContent.vote.revision, 3);

    // 잘못된 입력은 SDK 입력 검증 오류
    for (const bad of [
      { choice: 'a' },
      { expected_revision: 0, choice: 'a' },
      { expected_revision: '3', choice: 'a' },
      { expected_revision: 3, choice: 'skip' },
      { expected_revision: 3, choice: 'a', force: true },
    ]) {
      const r = await update(bad);
      assert.equal(r.isError, true, JSON.stringify(bad));
    }
    const missing = await client.callTool({
      name: TOOL_NAMES.update,
      arguments: { comparison_id: 'demo_cmp_' + '0'.repeat(32), expected_revision: 1, choice: 'a' },
    });
    assert.equal(missing._meta[ERROR_META_KEY].code, 'not_found');
  });

  test('없는 도구는 오류', async () => {
    const res = await client.callTool({ name: 'read_file', arguments: { path: '/etc/passwd' } }).catch((e) => ({ thrown: e }));
    assert.ok(res.thrown || res.isError, JSON.stringify(res));
  });

  test('동시 요청: 같은 비교에 다른 선택 12개를 HTTP 로 동시에 보내도 저장은 하나', async () => {
    const c = (await client.callTool({ name: TOOL_NAMES.create, arguments: { situation: 'daily', mood: 'minimal' } }))
      .structuredContent;
    const clients = await Promise.all(Array.from({ length: 12 }, () => connectClient(srv.url)));
    try {
      const results = await Promise.all(
        clients.map((cl, i) =>
          cl.callTool({ name: TOOL_NAMES.vote, arguments: { comparison_id: c.id, choice: ['a', 'b', 'neither'][i % 3] } }),
        ),
      );
      const saved = results.filter((r) => r.structuredContent.outcome === 'saved');
      assert.equal(saved.length, 1, JSON.stringify(results.map((r) => r.structuredContent)));
      const winner = saved[0].structuredContent.comparison.vote.choice;
      for (const r of results) {
        assert.equal(r.isError, undefined);
        assert.equal(r.structuredContent.comparison.vote.choice, winner, '모두 같은 최종 선택을 본다');
        assert.ok(['saved', 'already_submitted', 'conflict'].includes(r.structuredContent.outcome));
      }
    } finally {
      await Promise.all(clients.map((cl) => cl.close()));
    }
  });

  test('재시작: 같은 파일로 새 서버를 띄우면 비교와 선택이 남아 있다', async () => {
    const c = (await client.callTool({ name: TOOL_NAMES.create, arguments: { situation: 'office', mood: 'minimal' } }))
      .structuredContent;
    await client.callTool({ name: TOOL_NAMES.vote, arguments: { comparison_id: c.id, choice: 'a', reasons: ['practicality'] } });

    const second = await startServer({ file: join(tmp.dir, 'store.json') });
    const c2 = await connectClient(second.url);
    try {
      const res = await c2.callTool({ name: TOOL_NAMES.get, arguments: { comparison_id: c.id } });
      assert.equal(res.structuredContent.vote.choice, 'a');
      assert.deepEqual(res.structuredContent.vote.reasons, ['practicality']);
      assert.deepEqual(res.structuredContent.options, c.options);
    } finally {
      await c2.close();
      await second.close();
    }
  });

  describe('HTTP 경계', () => {
    test('깨진 JSON → 400 JSON-RPC parse error', async () => {
      const r = await rawPost(srv.url, '{"jsonrpc":"2.0",');
      assert.equal(r.status, 400);
      assert.equal(r.json.error.code, -32700);
      assert.equal(r.json.id, null);
    });

    test('JSON 이 아닌 Content-Type → 415', async () => {
      const r = await rawPost(srv.url, JSON.stringify(INIT), { 'content-type': 'text/plain' });
      assert.equal(r.status, 415);
    });

    test('64KB 넘는 본문 → 413', async () => {
      const big = { ...INIT, params: { ...INIT.params, pad: 'x'.repeat(70 * 1024) } };
      const r = await rawPost(srv.url, big);
      assert.equal(r.status, 413);
    });

    test('Content-Length 없이(chunked) 64KB 넘게 보내도 413 응답을 받는다', async () => {
      const { request } = await import('node:http');
      const status = await new Promise((resolve, reject) => {
        const req = request(srv.url, {
          method: 'POST',
          headers: { 'content-type': 'application/json', accept: 'application/json, text/event-stream' },
        });
        req.on('response', (res) => {
          res.resume();
          resolve(res.statusCode);
        });
        req.on('error', reject);
        for (let i = 0; i < 20; i++) req.write('x'.repeat(8 * 1024));
        req.end();
      });
      assert.equal(status, 413);
    });

    test('빈 배치·너무 큰 배치·JSON 원시값 → 400', async () => {
      assert.equal((await rawPost(srv.url, [])).status, 400);
      assert.equal((await rawPost(srv.url, Array.from({ length: 11 }, (_, i) => ({ ...INIT, id: i })))).status, 400);
      assert.equal((await rawPost(srv.url, '42')).status, 400);
      assert.equal((await rawPost(srv.url, 'null')).status, 400);
    });

    test('JSON-RPC 가 아닌 객체 → SDK 의 오류 응답(4xx)', async () => {
      const r = await rawPost(srv.url, { hello: 'world' });
      assert.ok(r.status >= 400 && r.status < 500, String(r.status));
      assert.ok(r.json?.error, r.text);
    });

    test('GET·DELETE·OPTIONS /mcp → 405, CORS 헤더 없음', async () => {
      for (const method of ['GET', 'DELETE', 'OPTIONS']) {
        const res = await fetch(srv.url, { method });
        assert.equal(res.status, 405, method);
        assert.equal(res.headers.get('access-control-allow-origin'), null);
      }
    });

    test('다른 경로 → 404, /healthz → 200', async () => {
      assert.equal((await fetch(srv.url.replace('/mcp', '/api/clothes'))).status, 404);
      assert.equal((await fetch(srv.url.replace('/mcp', '/'))).status, 404);
      const health = await fetch(srv.url.replace('/mcp', '/healthz'));
      assert.equal(health.status, 200);
      assert.deepEqual(await health.json(), { ok: true, demo: true, name: 'wardrobe-edit-demo' });
    });

    test('허용되지 않은 Host(DNS rebinding) → 403', async () => {
      const r = await rawHttp(srv.url, INIT, { host: 'evil.example.com' });
      assert.equal(r.status, 403);
      assert.equal((await rawHttp(srv.url, INIT, { host: `localhost:${srv.port}` })).status, 200);
      assert.equal((await rawHttp(srv.url, INIT, { host: `localhost:${srv.port + 1}` })).status, 403);
    });

    test('Origin 헤더가 붙은 브라우저 요청 → 403', async () => {
      const r = await rawPost(srv.url, INIT, { origin: 'https://evil.example.com' });
      assert.equal(r.status, 403);
      assert.equal(r.headers.get('access-control-allow-origin'), null);
    });

    test('demo 서버는 Authorization 이 붙은 요청을 익명으로 처리하지 않는다(400)', async () => {
      for (const auth of ['Bearer whatever', 'Bearer ', 'Basic dXNlcjpwYXNz']) {
        const r = await rawPost(srv.url, INIT, { authorization: auth });
        assert.equal(r.status, 400, auth);
        assert.match(r.json.error.message, /does not accept Authorization/);
      }
    });

    test('정상 initialize 원시 요청은 200', async () => {
      const r = await rawPost(srv.url, INIT);
      assert.equal(r.status, 200, r.text);
      assert.equal(r.json.result.serverInfo.name, 'wardrobe-edit-demo');
    });
  });
});

describe('공개 호스트·요청 수 제한 설정', () => {
  test('PUBLIC_HOST 로 준 이름은 Host 로 받고, 그 밖은 거절한다', async () => {
    const srv = await startServer({ publicHost: 'demo-abc.trycloudflare.com' });
    try {
      assert.equal((await rawHttp(srv.url, INIT, { host: 'demo-abc.trycloudflare.com' })).status, 200);
      assert.equal((await rawHttp(srv.url, INIT, { host: 'DEMO-ABC.trycloudflare.com' })).status, 200);
      assert.equal((await rawHttp(srv.url, INIT, { host: 'other.trycloudflare.com' })).status, 403);
    } finally {
      await srv.close();
    }
  });

  test('ALLOWED_ORIGINS 에 넣은 Origin 만 받는다', async () => {
    const srv = await startServer({ http: { allowedOrigins: ['https://inspector.local'] } });
    try {
      assert.equal((await rawPost(srv.url, INIT, { origin: 'https://inspector.local' })).status, 200);
      assert.equal((await rawPost(srv.url, INIT, { origin: 'https://inspector.local.evil' })).status, 403);
    } finally {
      await srv.close();
    }
  });

  test('1분 요청 수를 넘으면 429', async () => {
    const srv = await startServer({ http: { rateLimitPerMinute: 3 } });
    try {
      const statuses = [];
      for (let i = 0; i < 5; i++) statuses.push((await rawPost(srv.url, INIT)).status);
      assert.deepEqual(statuses, [200, 200, 200, 429, 429]);
    } finally {
      await srv.close();
    }
  });
});
