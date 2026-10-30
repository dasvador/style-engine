// live 모드 실제 추천(recommend_my_outfit): 가짜 Rust 추천 서버(루프백)로 잰다. 모델·실제 DB·ChatGPT 를 부르지 않는다.
//
// 가짜 서버는 Rust middleware::delegation 과 같은 방식으로 위임 헤더를 검증하고, 받은 요청을 모두 기록한다.

import assert from 'node:assert/strict';
import { createHmac, randomBytes } from 'node:crypto';
import { createServer } from 'node:http';
import { after, before, describe, test } from 'node:test';
import { join } from 'node:path';

import { issueLinkCode } from '../src/auth/link-codes.js';
import { RECOMMEND_PATH, RecommendClient } from '../src/recommend.js';
import { TOOL_NAMES } from '../src/mcp-server.js';
import { connectClient, startServer, tempDir } from './helpers.js';
import { fakeAdapter, linkAccount, oauth } from './oauth-helpers.js';

const KEY = randomBytes(32);
/** 악의적인(또는 설정이 새는) 400 본문. 어느 조각도 도구 결과에 나오면 안 된다. */
const EVIL_ERROR =
  'OPENAI_API_KEY=sk-FAKEFAKEFAKEFAKE1234 is invalid; db mysql://root:hunter2@127.0.0.1:3306/wardrobe ' +
  'see http://127.0.0.1:3003/internal/debug data:image/png;base64,EVILPNG';
const ALL_SCOPES = 'wardrobe.read comparisons.write recommendations.write';

/** Rust 쪽 검증과 같은 규칙. 맞으면 user id, 아니면 null. */
function verifyDelegation(header, method, path) {
  const [v, uid, exp, sig, rest] = String(header ?? '').split('.');
  if (v !== 'v1' || !uid || !exp || !sig || rest !== undefined) return null;
  const userId = Buffer.from(uid, 'base64url').toString('utf8');
  const now = Math.floor(Date.now() / 1000);
  if (Number(exp) < now || Number(exp) > now + 120) return null;
  const want = createHmac('sha256', KEY).update(`wardrobe-delegation/v1\n${method}\n${path}\n${userId}\n${exp}`).digest('hex');
  return want === sig ? userId : null;
}

/** 사용자 옷장으로 만든 Rust 응답 모양. 내보내면 안 되는 값(사진·wear·batch_id)도 일부러 싣는다. */
function rustResponse(userId, items) {
  const outfit = items.map((i) => ({
    id: i.id,
    category: i.category,
    name: i.name,
    reason: `${i.name} 이유`,
    image_url: 'data:image/png;base64,SECRETPHOTO',
    wear: 'worn closed, internal prompt text',
  }));
  return {
    modes: [
      {
        mode: 'todays_pick',
        mode_label: '오늘의 추천',
        mode_description: '오늘 날씨와 상황에 가장 잘 맞는 코디',
        outfit,
        recommendation: '차분한 데일리 룩',
        weather_summary: '맑고 선선해요',
        tips: ['가볍게 겹쳐 입기'],
        score: 82,
        verdict: '좋음',
        reason: '날씨와 조합 점수가 가장 높아요',
        revival_items: [],
        scoring_detail: { style_score: 80, final_score: 82 },
        shadow_score: 70,
      },
    ],
    weather_summary: '맑고 선선해요',
    notice: null,
    batch_id: `batch-${userId}`,
  };
}

/** 가짜 Rust 추천 서버. behavior 로 응답을 바꾼다. */
async function startUpstream(adapter) {
  const state = { behavior: 'ok', requests: [], redirectHits: 0, delayMs: 0 };
  const server = createServer(async (req, res) => {
    if (req.url === '/elsewhere') {
      state.redirectHits += 1;
      res.writeHead(200, { 'content-type': 'application/json' }).end('{"modes":[]}');
      return;
    }
    let raw = '';
    for await (const c of req) raw += c;
    const userId = verifyDelegation(req.headers['x-wardrobe-delegation'], req.method, req.url);
    state.requests.push({ userId, url: req.url, headers: req.headers, body: JSON.parse(raw || 'null') });
    const json = (status, body) => res.writeHead(status, { 'content-type': 'application/json' }).end(JSON.stringify(body));
    if (req.url !== RECOMMEND_PATH || !userId || req.headers.authorization) return json(401, { error: '로그인이 필요합니다.' });
    if (state.delayMs) await new Promise((r) => setTimeout(r, state.delayMs));
    if (res.destroyed) return;
    const items = await adapter.listWardrobe(userId);
    switch (state.behavior) {
      case 'ok':
        return json(200, rustResponse(userId, items));
      case 'foreign':
        return json(200, rustResponse(userId, [...items, { id: 'b-1', name: 'B 블랙 코트', category: '아우터' }]));
      case 'empty':
        return json(200, { modes: [], weather_summary: '추워요', notice: '오늘 날씨에 맞는 코디 후보가 부족해요.', batch_id: null });
      case 'html500':
        res.writeHead(500, { 'content-type': 'text/html' }).end('<h1>boom</h1>');
        return;
      case 'reject':
        return json(401, { error: '로그인이 필요합니다.' });
      case 'redirect':
        res.writeHead(302, { location: `http://127.0.0.1:${server.address().port}/elsewhere` }).end();
        return;
      case 'region404':
        return json(404, { error: 'No region configured. Set a region first.' });
      case 'evil400':
        return json(400, { error: EVIL_ERROR });
      case 'nullMode':
        return json(200, { ...rustResponse(userId, items), modes: [null] });
      case 'nullItem': {
        const r = rustResponse(userId, items);
        r.modes[0].outfit.push(null);
        return json(200, r);
      }
      case 'emptyItems': {
        const r = rustResponse(userId, items);
        r.modes[0].outfit = [];
        return json(200, r);
      }
      case 'itemNoId': {
        const r = rustResponse(userId, items);
        delete r.modes[0].outfit[0].id;
        return json(200, r);
      }
      case 'outfitNotArray':
        return json(200, { ...rustResponse(userId, items), modes: [{ mode: 'todays_pick', outfit: 'x' }] });
      default:
        return json(500, {});
    }
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  return {
    state,
    port: server.address().port,
    close: () =>
      new Promise((r) => {
        server.closeAllConnections();
        server.close(() => r());
      }),
  };
}

describe('recommend_my_outfit — 실제 추천 연결(가짜 Rust 서버)', () => {
  let tmp;
  let srv;
  let up;
  let o;
  let client;
  let recommender;
  const adapter = fakeAdapter();
  const files = {};
  const code = (userId) => issueLinkCode({ file: files.link, userId, ttlMs: 600_000 }).then((r) => r.code);
  const tokens = {};
  const connect = async (userId, scope = ALL_SCOPES) => {
    const t = await linkAccount(o, client, await code(userId), { scope });
    return { mcp: await connectClient(o.resource, { token: t.access_token }), token: t };
  };
  const call = async (userId, args = {}) => {
    const { mcp } = tokens[userId] ?? (tokens[userId] = await connect(userId));
    return mcp.callTool({ name: TOOL_NAMES.recommend, arguments: args });
  };

  before(async () => {
    tmp = await tempDir();
    files.link = join(tmp.dir, 'link-codes.json');
    up = await startUpstream(adapter);
    recommender = new RecommendClient({ port: up.port, key: KEY, timeoutMs: 2_000, maxConcurrent: 2 });
    srv = await startServer({ live: { adapter, linkCodesFile: files.link, recommender } });
    o = oauth(srv);
    client = (await o.register()).json;
  });
  after(async () => {
    for (const t of Object.values(tokens)) await t.mcp.close();
    await srv?.close();
    await up?.close();
    await tmp?.cleanup();
  });
  const reset = (behavior = 'ok') => {
    up.state.behavior = behavior;
    up.state.requests = [];
    up.state.delayMs = 0;
    recommender.timeoutMs = 2_000;
  };

  test('도구 목록: oauth2(wardrobe.read recommendations.write), 부작용을 알리는 annotation', async () => {
    const { mcp } = (tokens['user-a'] = await connect('user-a'));
    const { tools } = await mcp.listTools();
    const t = tools.find((x) => x.name === TOOL_NAMES.recommend);
    assert.ok(t, '추천 도구가 있다');
    assert.deepEqual(t._meta.securitySchemes, [{ type: 'oauth2', scopes: ['wardrobe.read', 'recommendations.write'] }]);
    assert.equal(t.annotations.readOnlyHint, false);
    assert.equal(t.annotations.idempotentHint, false);
    assert.equal(t.annotations.openWorldHint, true);
    assert.match(t.description, /비용/);
    assert.match(t.description, /착용 기록 아님/);
    assert.match(t.description, /내부 점수다 — 확률이나 100점 만점이 아니고/);
    assert.doesNotMatch(t.description, /품질을 보장한다/);
    assert.match(mcp.getInstructions(), /recommend_my_outfit 는 실제 추천 엔진/);
    assert.deepEqual(Object.keys(t.inputSchema.properties).sort(), ['gender', 'mood', 'situation']);
  });

  test('추천 키가 없는 live 서버에는 도구가 없고, 지시문도 추천이 없다고 말한다', async () => {
    const t2 = await tempDir();
    const s2 = await startServer({ live: { adapter, linkCodesFile: join(t2.dir, 'l.json') } });
    try {
      const o2 = oauth(s2);
      const c2 = (await o2.register()).json;
      const code2 = (await issueLinkCode({ file: join(t2.dir, 'l.json'), userId: 'user-a', ttlMs: 600_000 })).code;
      const tok = await linkAccount(o2, c2, code2, { scope: ALL_SCOPES });
      const mcp = await connectClient(o2.resource, { token: tok.access_token });
      const { tools } = await mcp.listTools();
      assert.ok(!tools.some((x) => x.name === TOOL_NAMES.recommend));
      assert.match(mcp.getInstructions(), /실제 추천 엔진과 옷 사진 등록은 아직 연결되지 않았다/);
      await mcp.close();
    } finally {
      await s2.close();
      await t2.cleanup();
    }
  });

  test('예전 연결(읽기+비교 scope)은 insufficient_scope 로 재동의를 요청하고, 리프레시로 권한이 넓어지지 않는다', async () => {
    reset();
    const old = await connect('user-a', 'wardrobe.read comparisons.write');
    try {
      const r = await old.mcp.callTool({ name: TOOL_NAMES.recommend, arguments: {} });
      assert.equal(r.isError, true);
      const www = r._meta['mcp/www_authenticate'][0];
      assert.match(www, /error="insufficient_scope"/);
      assert.match(www, /scope="wardrobe.read recommendations.write"/);
      assert.equal(up.state.requests.length, 0, 'Rust 를 부르지 않았다');
      const widened = await o.token({
        grant_type: 'refresh_token',
        client_id: client.client_id,
        refresh_token: old.token.refresh_token,
        scope: ALL_SCOPES,
      });
      assert.equal(widened.status, 400);
      assert.equal(widened.json.error, 'invalid_scope');
    } finally {
      await old.mcp.close();
    }
    // 동의 화면은 새 권한을 하나하나 보여 준다
    const { challenge } = { challenge: 'x'.repeat(43) };
    const page = await o.openConsent(client, { challenge, scope: 'wardrobe.read recommendations.write' });
    assert.match(page.html, /오늘 코디 추천 받기\(AI 모델 사용, 추천 이력이 내 계정에 남음\)/);
  });

  test('A·B 두 계정: 각자 자기 옷으로만 추천되고, 위임 주체는 토큰의 계정이다', async () => {
    reset();
    const a = await call('user-a', { situation: '출근', gender: 'female', mood: 'minimal' });
    const b = await call('user-b');
    assert.equal(a.structuredContent.outcome, 'recommended', a.content[0].text);
    assert.deepEqual(a.structuredContent.modes[0].items.map((i) => i.id), ['a-1', 'a-2']);
    assert.deepEqual(b.structuredContent.modes[0].items.map((i) => i.id), ['b-1']);
    assert.deepEqual(up.state.requests.map((r) => r.userId), ['user-a', 'user-b']);
    assert.deepEqual(up.state.requests[0].body, { occasion: '출근', gender: 'female', style_mood: 'minimal' });
    assert.deepEqual(up.state.requests[1].body, { occasion: '일상' });
    for (const r of up.state.requests) assert.equal(r.headers.authorization, undefined, 'Rust 토큰을 보내지 않는다');
    // 결과 모양: 실제 옷 id·이름·카테고리·이유, 지역·실제 날씨(batch), 모드별
    const s = a.structuredContent;
    assert.equal(s.source, 'live_recommendation_engine');
    assert.equal(s.region, '서울 종로구');
    assert.deepEqual(s.weather, { temperature: 14.5, apparent_temperature: 13, description: '맑음' });
    assert.equal(s.modes[0].label, '오늘의 추천');
    assert.equal(s.modes[0].reason, '날씨와 조합 점수가 가장 높아요');
    assert.match(s.side_effects, /착용 기록 아님/);
    assert.match(a.content[0].text, /A 네이비 니트 \(id a-1\)/);
    assert.match(a.content[0].text, /내부 점수예요\(확률·100점 만점 아님\)/);
  });

  test('응답에서 사진·이미지 프롬프트 문장·내부 id·위임 값이 빠진다', async () => {
    reset();
    const r = await call('user-a');
    const all = JSON.stringify(r);
    for (const leak of ['data:image', 'SECRETPHOTO', 'image_url', 'wear', 'internal prompt', 'batch-user-a', 'batch_id', 'shadow_score', 'scoring_detail', 'x-wardrobe-delegation', KEY.toString('hex')]) {
      assert.ok(!all.includes(leak), `응답에 ${leak} 이 없다`);
    }
    const sent = up.state.requests[0].headers['x-wardrobe-delegation'];
    assert.ok(sent && !all.includes(sent));
  });

  test('인자 검증: 무드만·스트리트 출근·성별에 없는 무드·모르는 값·사용자 id 인자는 Rust 를 부르지 않고 거절', async () => {
    reset();
    const expect = async (args, outcome) => {
      const r = await call('user-a', args);
      assert.equal(r.isError, undefined, JSON.stringify(args));
      assert.equal(r.structuredContent.ok, false);
      assert.equal(r.structuredContent.outcome, outcome, JSON.stringify(args));
    };
    await expect({ mood: 'minimal' }, 'mood_needs_gender');
    await expect({ gender: 'female', mood: 'street', situation: '출근' }, 'street_not_for_work');
    await expect({ gender: 'male', mood: 'glam' }, 'mood_not_for_gender');
    for (const args of [{ situation: '파티' }, { gender: 'other' }, { mood: 'goth', gender: 'female' }, { user_id: 'user-b' }, { userId: 'user-b' }]) {
      const r = await call('user-a', args);
      assert.equal(r.isError, true, JSON.stringify(args));
    }
    assert.equal(up.state.requests.length, 0);
  });

  test('빈 옷장·지역 없음은 모델을 부르기 전에 알아듣게 돌려준다', async () => {
    reset();
    const d = await call('user-d');
    assert.equal(d.structuredContent.outcome, 'empty_wardrobe');
    const c = await call('user-c');
    assert.equal(c.structuredContent.outcome, 'no_region');
    assert.match(c.content[0].text, /지역/);
    assert.equal(up.state.requests.length, 0);
  });

  test('후보 부족·Rust 지역 404·위임 거절·HTML 500·리다이렉트·남의 옷 id → 각각의 결과, 데모로 바꾸지 않는다', async () => {
    const cases = [
      ['empty', 'no_candidates'],
      ['region404', 'no_region'],
      ['reject', 'delegation_rejected'],
      ['html500', 'upstream_error'],
      ['redirect', 'upstream_error'],
      ['foreign', 'foreign_item'],
      ['evil400', 'invalid_request'],
      ['nullMode', 'upstream_error'],
      ['nullItem', 'upstream_error'],
      ['emptyItems', 'upstream_error'],
      ['itemNoId', 'upstream_error'],
      ['outfitNotArray', 'upstream_error'],
    ];
    for (const [behavior, outcome] of cases) {
      reset(behavior);
      up.state.redirectHits = 0;
      const r = await call('user-a');
      assert.equal(r.structuredContent.outcome, outcome, behavior);
      assert.equal(r.structuredContent.ok, false, behavior);
      const all = JSON.stringify(r);
      assert.ok(!/demo|샘플|b-1/.test(all.replace(/데모 결과로 바꾸지 않았어요/g, '')), `${behavior}: 데모·남의 옷 없음`);
      assert.equal(up.state.requests.length, 1, `${behavior}: 한 번만 불렀다`);
      if (behavior === 'redirect') assert.equal(up.state.redirectHits, 0, '리다이렉트를 따라가지 않는다');
    }
  });

  test('시간 초과: 기다리기를 멈추고 다시 부르지 않는다. 서버가 꺼져 있으면 unavailable', async () => {
    reset();
    up.state.delayMs = 1_500;
    recommender.timeoutMs = 300;
    const r = await call('user-a');
    assert.equal(r.structuredContent.outcome, 'timeout');
    assert.match(r.content[0].text, /바로 다시 부르지 마세요/);
    await new Promise((res) => setTimeout(res, 1_600));
    assert.equal(up.state.requests.length, 1, '자동 재시도 없음');

    const off = new RecommendClient({ port: up.port, key: KEY, timeoutMs: 1_000 });
    await up.close();
    try {
      const res = await off.recommend('user-a', { occasion: '일상' });
      assert.deepEqual(res, { ok: false, outcome: 'unavailable' });
    } finally {
      up = await startUpstream(adapter);
      recommender.url = `http://127.0.0.1:${up.port}${RECOMMEND_PATH}`;
    }
  });

  test('같은 계정의 동시 요청은 하나만 보내고 나머지는 busy', async () => {
    reset();
    up.state.delayMs = 300;
    const [r1, r2] = await Promise.all([call('user-a'), call('user-a')]);
    const outcomes = [r1.structuredContent.outcome, r2.structuredContent.outcome].sort();
    assert.deepEqual(outcomes, ['busy', 'recommended']);
    assert.equal(up.state.requests.length, 1);
  });
});

describe('RecommendClient 주소 고정', () => {
  test('호스트는 언제나 127.0.0.1, 짧은 키·이상한 포트는 거절', () => {
    const c = new RecommendClient({ port: 3003, key: randomBytes(32) });
    assert.equal(c.url, 'http://127.0.0.1:3003/api/recommendation/multi');
    assert.throws(() => new RecommendClient({ port: 3003, key: randomBytes(8) }));
    assert.throws(() => new RecommendClient({ port: 0, key: randomBytes(32) }));
    assert.throws(() => new RecommendClient({ port: 'evil.example', key: randomBytes(32) }));
  });
});

describe('recommend_my_outfit — 추천 서버 오류 본문을 내보내지 않는다', () => {
  test('400 본문의 키·접속 문자열·내부 URL·data URI 가 결과 어디에도 없다', async () => {
    const tmp = await tempDir();
    const adapter = fakeAdapter();
    const up = await startUpstream(adapter);
    up.state.behavior = 'evil400';
    const recommender = new RecommendClient({ port: up.port, key: KEY, timeoutMs: 2_000 });
    const srv = await startServer({ live: { adapter, linkCodesFile: join(tmp.dir, 'l.json'), recommender } });
    try {
      const o = oauth(srv);
      const client = (await o.register()).json;
      const code = (await issueLinkCode({ file: join(tmp.dir, 'l.json'), userId: 'user-a', ttlMs: 600_000 })).code;
      const t = await linkAccount(o, client, code, { scope: ALL_SCOPES });
      const mcp = await connectClient(o.resource, { token: t.access_token });
      const r = await mcp.callTool({ name: TOOL_NAMES.recommend, arguments: {} });
      await mcp.close();
      assert.equal(r.structuredContent.outcome, 'invalid_request');
      const all = JSON.stringify(r);
      for (const piece of ['sk-FAKE', 'OPENAI_API_KEY', 'mysql://', 'hunter2', '3306', '/internal/debug', 'data:image', 'EVILPNG']) {
        assert.ok(!all.includes(piece), `결과에 ${piece} 이 없다`);
      }
      assert.match(r.content[0].text, /운영자에게 알려 주세요/);
    } finally {
      await srv.close();
      await up.close();
      await tmp.cleanup();
    }
  });
});
