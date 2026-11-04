// 코디 흐름 도구(바꾸기·계획·착용·수정·내역): 권한·입력 경계·결과 모양을 가짜 Rust 서버(루프백)로 잰다.
// DB 동작은 여기서 주장하지 않는다 — Rust 테스트(routes/outfit_flow_tests)와 scripts/e2e-outfit-flow.mjs 가 실제 DB 로 잰다.

import assert from 'node:assert/strict';
import { createHmac, randomBytes } from 'node:crypto';
import { createServer } from 'node:http';
import { after, before, describe, test } from 'node:test';
import { join } from 'node:path';

import { issueLinkCode } from '../src/auth/link-codes.js';
import { OUTFIT_PATHS, RecommendClient } from '../src/recommend.js';
import { TOOL_NAMES } from '../src/mcp-server.js';
import { connectClient, startServer, tempDir } from './helpers.js';
import { fakeAdapter, linkAccount, oauth } from './oauth-helpers.js';

const KEY = randomBytes(32);
const REF = '11111111-1111-4111-8111-111111111111';
const REF2 = '22222222-2222-4222-8222-222222222222';
const REC = '33333333-3333-4333-8333-333333333333';
const EVIL = 'OPENAI_API_KEY=sk-FAKE mysql://root:hunter2@127.0.0.1/db data:image/png;base64,EVILPNG';
const ALL = 'wardrobe.read comparisons.write recommendations.write outfits.write';
const OLD = 'wardrobe.read comparisons.write recommendations.write';

const item = (id, category) => ({ id, name: `옷 ${id}`, category, color: '블랙', sub_category: null, material: null, has_image: true });
const record = (over = {}) => ({
  record_id: REC,
  outfit_ref: REF,
  status: 'planned',
  date: '2026-11-04',
  items: [item('a-1', '상의'), item('a-2', '하의')],
  occasion: '일상',
  mood_key: null,
  temperature: 15,
  reaction: null,
  comment: null,
  revision: 1,
  planned_at: '2026-11-04T09:00:00',
  worn_at: null,
  cancelled_at: null,
  updated_at: '2026-11-04T09:00:00',
  ...over,
});

function verify(header, method, path) {
  const [v, uid, exp, sig] = String(header ?? '').split('.');
  if (v !== 'v1') return null;
  const userId = Buffer.from(uid, 'base64url').toString('utf8');
  const want = createHmac('sha256', KEY).update(`wardrobe-delegation/v1\n${method}\n${path}\n${userId}\n${exp}`).digest('hex');
  return want === sig ? userId : null;
}

async function startUpstream() {
  const state = { requests: [], respond: null, gate: null };
  const server = createServer(async (req, res) => {
    let raw = '';
    for await (const c of req) raw += c;
    const userId = verify(req.headers['x-wardrobe-delegation'], req.method, req.url);
    state.requests.push({ userId, url: req.url, method: req.method, body: JSON.parse(raw || 'null') });
    if (state.gate) await state.gate;
    const [status, body] = state.respond?.(req.url) ?? [200, { outcome: 'planned', record: record() }];
    if (status === 'reset') {
      // 처리는 끝났는데 응답 전에 연결이 끊긴 경우.
      req.socket.destroy();
      return;
    }
    if (status === 302) {
      res.writeHead(302, { location: `http://127.0.0.1:${server.address().port}/elsewhere` }).end();
      return;
    }
    res.writeHead(status, { 'content-type': 'application/json' }).end(JSON.stringify(body));
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  return { state, port: server.address().port, close: () => new Promise((r) => server.close(r)) };
}

describe('코디 흐름 도구', () => {
  let tmp, srv, up, o, client;
  const files = {};
  const adapter = fakeAdapter();
  const code = (userId) => issueLinkCode({ file: files.link, userId, ttlMs: 600_000 }).then((r) => r.code);
  const open = [];
  const connect = async (userId, scope = ALL) => {
    const t = await linkAccount(o, client, await code(userId), { scope });
    const mcp = await connectClient(o.resource, { token: t.access_token });
    open.push(mcp);
    return { mcp, token: t };
  };
  let a;

  before(async () => {
    tmp = await tempDir();
    files.link = join(tmp.dir, 'link-codes.json');
    up = await startUpstream();
    const recommender = new RecommendClient({ port: up.port, key: KEY, timeoutMs: 2_000 });
    srv = await startServer({ live: { adapter, linkCodesFile: files.link, recommender } });
    o = oauth(srv);
    client = (await o.register()).json;
    a = (await connect('user-a')).mcp;
  });
  after(async () => {
    for (const m of open) await m.close();
    await srv?.close();
    await up?.close();
    await tmp?.cleanup();
  });
  const reset = (respond = null) => {
    up.state.requests = [];
    up.state.respond = respond;
    up.state.gate = null;
  };

  test('도구 목록: scope·annotation·설명', async () => {
    const { tools } = await a.listTools();
    const by = Object.fromEntries(tools.map((t) => [t.name, t]));
    const scopes = (n) => by[n]._meta.securitySchemes[0].scopes;
    assert.deepEqual(scopes(TOOL_NAMES.replaceItem), ['wardrobe.read', 'recommendations.write']);
    for (const n of [TOOL_NAMES.planOutfit, TOOL_NAMES.recordWorn, TOOL_NAMES.updateRecord]) {
      assert.deepEqual(scopes(n), ['wardrobe.read', 'outfits.write'], n);
      assert.equal(by[n].annotations.readOnlyHint, false);
    }
    assert.deepEqual(scopes(TOOL_NAMES.history), ['wardrobe.read']);
    assert.equal(by[TOOL_NAMES.history].annotations.readOnlyHint, true);
    assert.equal(by[TOOL_NAMES.updateRecord].annotations.destructiveHint, true);
    assert.equal(by[TOOL_NAMES.recordWorn].annotations.destructiveHint, false);
    assert.equal(by[TOOL_NAMES.recordWorn].annotations.idempotentHint, true);
    // 착용은 사용자가 말했을 때만, 계획은 착용이 아니다, 조건을 버리지 않는다.
    assert.match(by[TOOL_NAMES.recordWorn].description, /직접 말했을 때만/);
    assert.match(by[TOOL_NAMES.planOutfit].description, /실제 착용 기록이 아니다/);
    assert.match(by[TOOL_NAMES.replaceItem].description, /버리고 부르지 않는다/);
    // 코디를 모델이 조립해 보낼 칸이 없다.
    for (const n of [TOOL_NAMES.planOutfit, TOOL_NAMES.recordWorn]) {
      assert.ok(!Object.keys(by[n].inputSchema.properties).some((k) => ['items', 'outfit', 'user_id'].includes(k)), n);
    }
    assert.match(a.getInstructions(), /record_worn_outfit 은 사용자가 '입었어'라고 직접 말했을 때만/);
  });

  test('새 권한이 없는 옛 토큰: 쓰기는 재동의 요구, Rust 를 부르지 않고, 리프레시로 넓어지지 않는다', async () => {
    reset();
    const { mcp, token } = await connect('user-a', OLD);
    for (const [name, args] of [
      [TOOL_NAMES.planOutfit, { outfit_ref: REF }],
      [TOOL_NAMES.recordWorn, { outfit_ref: REF }],
      [TOOL_NAMES.updateRecord, { record_id: REC, expected_revision: 1, action: 'undo' }],
    ]) {
      const r = await mcp.callTool({ name, arguments: args });
      assert.equal(r.isError, true, name);
      const www = r._meta['mcp/www_authenticate'][0];
      assert.match(www, /error="insufficient_scope"/);
      // 다시 연결해도 추천 권한을 잃지 않게 같이 요청한다.
      assert.match(www, /scope="wardrobe.read outfits.write recommendations.write"/);
    }
    assert.equal(up.state.requests.length, 0);

    // 추천 권한으로 되는 일은 그대로다.
    reset(() => [200, { outcome: 'no_candidates', message: '없음', alternatives: [] }]);
    const r = await mcp.callTool({ name: TOOL_NAMES.replaceItem, arguments: { outfit_ref: REF, category: '신발' } });
    assert.equal(r.structuredContent.outcome, 'no_candidates');
    reset(() => [200, { today: '2026-11-04', records: [], has_more: false }]);
    const h = await mcp.callTool({ name: TOOL_NAMES.history, arguments: {} });
    assert.equal(h.structuredContent.ok, true);

    // 리프레시: 새 scope 를 요청하면 거절, 그냥 돌리면 옛 scope 그대로.
    const widen = await o.token({ grant_type: 'refresh_token', client_id: client.client_id, refresh_token: token.refresh_token, scope: ALL, resource: o.resource });
    assert.equal(widen.status, 400);
    assert.equal(widen.json.error, 'invalid_scope');
  });

  test('wardrobe.read 만: 내역은 읽고, 바꾸기·기록은 못 한다', async () => {
    const { mcp } = await connect('user-a', 'wardrobe.read');
    reset();
    const r = await mcp.callTool({ name: TOOL_NAMES.replaceItem, arguments: { outfit_ref: REF, category: '신발' } });
    assert.match(r._meta['mcp/www_authenticate'][0], /insufficient_scope/);
    assert.equal(up.state.requests.length, 0);
  });

  test('재동의로 새 권한을 받으면 쓸 수 있고, 위임 주체는 토큰의 계정이다', async () => {
    reset();
    const { mcp } = await connect('user-b');
    const r = await mcp.callTool({ name: TOOL_NAMES.planOutfit, arguments: { outfit_ref: REF, date: '2026-11-05', comment: '약속' } });
    assert.equal(r.structuredContent.outcome, 'planned');
    assert.match(r.content[0].text, /아직 입은 기록은 아니에요/);
    assert.deepEqual(up.state.requests.map((q) => [q.method, q.url, q.userId]), [['POST', OUTFIT_PATHS.plan, 'user-b']]);
    assert.deepEqual(up.state.requests[0].body, { outfit_ref: REF, date: '2026-11-05', comment: '약속' });
  });

  test('토큰 없이는 /mcp 에 못 들어온다(익명 대체 없음)', async () => {
    const res = await fetch(o.resource, {
      method: 'POST',
      headers: { 'content-type': 'application/json', accept: 'application/json, text/event-stream' },
      body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'tools/call', params: { name: TOOL_NAMES.planOutfit, arguments: { outfit_ref: REF } } }),
    });
    assert.equal(res.status, 401);
  });

  test('입력 경계: 지어낸 코디·사용자 id·잘못된 참조·날짜는 Rust 까지 가지 않는다', async () => {
    reset();
    for (const [name, args] of [
      [TOOL_NAMES.planOutfit, { outfit_ref: REF, items: [{ id: 'b-1', name: '지어낸 옷' }] }],
      [TOOL_NAMES.planOutfit, { outfit_ref: REF, user_id: 'user-b' }],
      [TOOL_NAMES.planOutfit, { outfit_ref: '2번 코디' }],
      [TOOL_NAMES.planOutfit, { outfit_ref: REF, date: '내일' }],
      [TOOL_NAMES.recordWorn, { outfit_ref: REF, reaction: '좋았음' }],
      [TOOL_NAMES.replaceItem, { outfit_ref: REF, category: '모자' }],
      [TOOL_NAMES.updateRecord, { record_id: REC, expected_revision: 0, action: 'cancel' }],
      [TOOL_NAMES.updateRecord, { record_id: REC, expected_revision: 1, action: 'delete' }],
    ]) {
      const r = await a.callTool({ name, arguments: args });
      assert.equal(r.isError, true, JSON.stringify(args));
    }
    // 참조와 기록을 같이, 또는 revision 없이 보내면 어느 쪽인지 고르지 않는다.
    for (const args of [{}, { record_id: REC }, { outfit_ref: REF, record_id: REC, expected_revision: 1 }]) {
      const r = await a.callTool({ name: TOOL_NAMES.recordWorn, arguments: args });
      assert.equal(r.structuredContent.outcome, 'invalid_request');
    }
    assert.equal(up.state.requests.length, 0);
  });

  test('실패는 고정 안내로만 나가고 Rust 오류 본문이 새지 않는다', async () => {
    for (const [status, body, outcome] of [
      [404, { code: 'ref_not_found', error: EVIL }, 'ref_not_found'],
      [409, { code: 'revision_conflict', error: EVIL, current_revision: 7 }, 'revision_conflict'],
      [500, { error: EVIL }, 'upstream_error'],
      [500, { code: 'x'.repeat(100), error: EVIL }, 'upstream_error'],
      [400, { code: 'never_heard_of_it', error: EVIL }, 'upstream_error'],
      [401, { error: EVIL }, 'delegation_rejected'],
      [422, 'Failed to deserialize', 'invalid_request'],
      [302, null, 'upstream_error'],
    ]) {
      reset(() => [status, body]);
      const r = await a.callTool({ name: TOOL_NAMES.recordWorn, arguments: { outfit_ref: REF } });
      assert.equal(r.structuredContent.ok, false);
      assert.equal(r.structuredContent.outcome, outcome);
      const all = JSON.stringify(r);
      for (const leak of ['sk-FAKE', 'hunter2', 'EVILPNG']) assert.ok(!all.includes(leak));
      if (outcome === 'revision_conflict') assert.equal(r.structuredContent.current_revision, 7);
    }
  });

  test('응답 전에 연결이 끊기면 저장 여부를 단정하지 않고, 다시 보내지 않는다', async () => {
    for (const [name, args] of [
      [TOOL_NAMES.recordWorn, { outfit_ref: REF }],
      [TOOL_NAMES.planOutfit, { outfit_ref: REF }],
      [TOOL_NAMES.updateRecord, { record_id: REC, expected_revision: 1, action: 'undo' }],
    ]) {
      reset(() => ['reset']);
      const r = await a.callTool({ name, arguments: args });
      assert.equal(r.structuredContent.ok, false, name);
      assert.equal(r.structuredContent.outcome, 'unavailable', name);
      assert.match(r.structuredContent.message, /저장됐는지 알 수 없/);
      assert.match(r.structuredContent.message, /list_my_outfit_history/);
      assert.doesNotMatch(JSON.stringify(r), /저장되지 않았어요/);
      assert.equal(up.state.requests.length, 1, '서버는 한 번만 받았고 자동 재시도가 없다');
    }
  });

  test('성공처럼 보이는 어긋난 응답은 성공으로 내보내지 않는다', async () => {
    for (const body of [
      { outcome: 'planned' },
      { outcome: 'planned', record: { record_id: REC } },
      { outcome: 'made_up', record: record() },
      { outcome: 'planned', record: record({ status: 'done' }) },
    ]) {
      reset(() => [200, body]);
      const r = await a.callTool({ name: TOOL_NAMES.planOutfit, arguments: { outfit_ref: REF } });
      assert.equal(r.structuredContent.outcome, 'upstream_error');
    }
  });

  test('바꾸기: 대안·되묻기·남의 옷이 든 응답', async () => {
    const proposed = (ids) => ({
      outcome: 'proposed',
      message: '신발 대안 1개',
      outfit_ref: REF,
      category: '하의',
      replaced_item: item('a-2', '하의'),
      alternatives: [
        {
          outfit_ref: REF2,
          new_item: item(ids[1], '하의'),
          items: ids.map((id) => item(id, '상의')),
          rationale: ['두께가 보통 → 두꺼움 로 더 두꺼워요.'],
          weather_fit: 'ok',
        },
      ],
      weather: { temperature: 15, source: 'recorded', at: 'x' },
      limitations: ['제안일 뿐이에요.'],
    });
    reset(() => [200, proposed(['a-1', 'a-2'])]);
    const r = await a.callTool({ name: TOOL_NAMES.replaceItem, arguments: { outfit_ref: REF, category: '하의', preference: 'warmer' } });
    assert.equal(r.structuredContent.ok, true);
    assert.equal(r.structuredContent.alternatives[0].outfit_ref, REF2);
    assert.match(r.content[0].text, /아무것도 저장·기록하지 않았어요/);
    assert.equal(up.state.requests[0].url, OUTFIT_PATHS.replacement);

    reset(() => [200, proposed(['a-1', 'b-1'])]);
    const foreign = await a.callTool({ name: TOOL_NAMES.replaceItem, arguments: { outfit_ref: REF, category: '하의' } });
    assert.equal(foreign.structuredContent.outcome, 'foreign_item');
    assert.equal(foreign.structuredContent.alternatives, undefined);

    reset(() => [200, { outcome: 'needs_clarification', reason: 'unsupported_constraint', message: '방수는 확인할 수 없어요', supported_preferences: ['comfortable'], alternatives: [] }]);
    const ask = await a.callTool({ name: TOOL_NAMES.replaceItem, arguments: { outfit_ref: REF, category: '신발', request_text: '방수 신발' } });
    assert.equal(ask.structuredContent.ok, false);
    assert.equal(ask.structuredContent.outcome, 'needs_clarification');
    assert.equal(ask.structuredContent.reason, 'unsupported_constraint');
    assert.equal(up.state.requests[0].body.request_text, '방수 신발', '조건 원문을 서버에 그대로 넘긴다');
  });

  test('호출 수 제한: 계정당 동시 2개를 넘으면 바로 busy', async () => {
    reset();
    let release;
    up.state.gate = new Promise((r) => (release = r));
    const calls = [0, 1].map(() => a.callTool({ name: TOOL_NAMES.planOutfit, arguments: { outfit_ref: REF } }));
    while (up.state.requests.length < 2) await new Promise((r) => setTimeout(r, 5));
    const third = await a.callTool({ name: TOOL_NAMES.planOutfit, arguments: { outfit_ref: REF } });
    assert.equal(third.structuredContent.outcome, 'busy');
    assert.equal(up.state.requests.length, 2, '자동 재시도 없음');
    release();
    await Promise.all(calls);
  });

  test('내역: worn 거르기와 기록 모양', async () => {
    reset(() => [200, { today: '2026-11-04', records: [record({ status: 'worn', reaction: 'comfortable', revision: 2 })], has_more: true }]);
    const r = await a.callTool({ name: TOOL_NAMES.history, arguments: { status: 'worn', limit: 5 } });
    assert.equal(r.structuredContent.records[0].status, 'worn');
    assert.equal(r.structuredContent.has_more, true);
    assert.match(r.content[0].text, /입음/);
    assert.deepEqual(up.state.requests[0].body, { status: 'worn', limit: 5 });
    assert.ok(!JSON.stringify(r).includes('has_image'));
  });

  test('클라이언트: 고정 경로만 부른다', async () => {
    const c = new RecommendClient({ port: up.port, key: KEY });
    await assert.rejects(() => c.outfit('/api/clothes', 'user-a', {}));
    await assert.rejects(() => c.outfit('constructor', 'user-a', {}));
  });
});
