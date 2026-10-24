// live 모드: MCP OAuth(인가 코드 + PKCE S256)와 계정별 격리.
//
// 1부(언제나 돈다): 가짜 어댑터(메모리 속 A·B 두 계정)로 OAuth 전 과정과 거절 경우, 계정 격리를 잰다.
// 2부(TEST_DATABASE_URL 이 있을 때만): 실제 LiveAdapter 로 임시 테스트 DB 의 A·B 옷장 격리를 잰다.
//     없으면 skip 으로 표시된다(통과로 세지 않는다).
// 실제 ChatGPT·외부 IdP 를 부르지 않는다. 발급자는 로컬 http://127.0.0.1:포트다.

import assert from 'node:assert/strict';
import { createHash, randomBytes } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { after, before, describe, test } from 'node:test';

import { issueLinkCode } from '../src/auth/link-codes.js';
import { DEFAULT_REDIRECT_URIS } from '../src/auth/oauth.js';
import { TOOL_NAMES } from '../src/mcp-server.js';
import { connectClient, startServer, tempDir } from './helpers.js';

const REDIRECT = DEFAULT_REDIRECT_URIS[0];

/** 메모리 속 계정 두 개. 실제 어댑터와 같은 메서드. */
function fakeAdapter() {
  const users = {
    'user-a': { username: 'alice', display_name: '앨리스', items: [item('a-1', 'A 네이비 니트', '상의'), item('a-2', 'A 데님', '하의')] },
    'user-b': { username: 'bob', display_name: null, items: [item('b-1', 'B 블랙 코트', '아우터')] },
  };
  function item(id, name, category) {
    return { id, name, category, color: null, sub_category: null, material: null, has_image: false };
  }
  return {
    calls: [],
    async exists(id) {
      return Object.hasOwn(users, id);
    },
    async getProfile(id) {
      this.calls.push(['profile', id]);
      const u = users[id];
      if (!u) return null;
      const byCat = {};
      for (const i of u.items) byCat[i.category] = (byCat[i.category] ?? 0) + 1;
      return { username: u.username, display_name: u.display_name, wardrobe_count: u.items.length, wardrobe_by_category: byCat };
    },
    async listWardrobe(id, { category = null } = {}) {
      this.calls.push(['wardrobe', id]);
      return (users[id]?.items ?? []).filter((i) => !category || i.category === category);
    },
  };
}

const b64url = (buf) => Buffer.from(buf).toString('base64url');
function pkce() {
  const verifier = b64url(randomBytes(32));
  const challenge = b64url(createHash('sha256').update(verifier).digest());
  return { verifier, challenge };
}

/** OAuth 한 바퀴를 손으로 돈다(브라우저 대신 fetch). 단계별로 끊어 쓸 수 있게 함수로 나눈다. */
function oauth(srv) {
  const base = `http://127.0.0.1:${srv.port}`;
  const resource = `${base}/mcp`;
  return {
    base,
    resource,
    async register(body = {}) {
      const res = await fetch(`${base}/register`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          client_name: 'test-client',
          redirect_uris: [REDIRECT],
          token_endpoint_auth_method: 'none',
          grant_types: ['authorization_code', 'refresh_token'],
          response_types: ['code'],
          ...body,
        }),
      });
      return { status: res.status, json: await res.json() };
    },
    authorizeUrl(client, { challenge, state = 'st-1', redirect = REDIRECT, res = resource, method = 'S256', scope = 'wardrobe.read comparisons.write' } = {}) {
      const u = new URL(`${base}/authorize`);
      u.searchParams.set('response_type', 'code');
      u.searchParams.set('client_id', client.client_id);
      u.searchParams.set('redirect_uri', redirect);
      u.searchParams.set('code_challenge', challenge);
      u.searchParams.set('code_challenge_method', method);
      u.searchParams.set('state', state);
      if (scope) u.searchParams.set('scope', scope);
      if (res !== null) u.searchParams.set('resource', res);
      return u.href;
    },
    async openConsent(client, opts) {
      const res = await fetch(this.authorizeUrl(client, opts), { redirect: 'manual' });
      const html = await res.text();
      const request = html.match(/name="request" value="([^"]+)"/)?.[1] ?? null;
      return { status: res.status, html, request, location: res.headers.get('location'), headers: res.headers };
    },
    async consent(request, linkCode, { action = 'allow', origin = base } = {}) {
      const headers = { 'content-type': 'application/x-www-form-urlencoded' };
      if (origin) headers.origin = origin;
      const res = await fetch(`${base}/oauth/consent`, {
        method: 'POST',
        redirect: 'manual',
        headers,
        body: new URLSearchParams({ request, link_code: linkCode, action }).toString(),
      });
      return { status: res.status, location: res.headers.get('location'), html: await res.text() };
    },
    async token(params) {
      const res = await fetch(`${base}/token`, {
        method: 'POST',
        headers: { 'content-type': 'application/x-www-form-urlencoded' },
        body: new URLSearchParams(params).toString(),
      });
      return { status: res.status, json: await res.json() };
    },
    async revoke(clientId, token) {
      const res = await fetch(`${base}/revoke`, {
        method: 'POST',
        headers: { 'content-type': 'application/x-www-form-urlencoded' },
        body: new URLSearchParams({ client_id: clientId, token }).toString(),
      });
      return res.status;
    },
    async mcpStatus(token) {
      const res = await fetch(resource, {
        method: 'POST',
        headers: {
          'content-type': 'application/json',
          accept: 'application/json, text/event-stream',
          ...(token ? { authorization: `Bearer ${token}` } : {}),
        },
        body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'tools/list', params: {} }),
      });
      return { status: res.status, www: res.headers.get('www-authenticate'), body: await res.text() };
    },
  };
}

/** 등록 → 동의(연결 코드) → 코드 → 토큰까지. */
async function linkAccount(o, client, linkCode, { scope } = {}) {
  const { verifier, challenge } = pkce();
  const page = await o.openConsent(client, scope === undefined ? { challenge } : { challenge, scope });
  assert.equal(page.status, 200, page.html);
  const done = await o.consent(page.request, linkCode);
  assert.equal(done.status, 302, done.html);
  const cb = new URL(done.location);
  const code = cb.searchParams.get('code');
  const tok = await o.token({
    grant_type: 'authorization_code',
    client_id: client.client_id,
    code,
    code_verifier: verifier,
    redirect_uri: REDIRECT,
    resource: o.resource,
  });
  assert.equal(tok.status, 200, JSON.stringify(tok.json));
  return { ...tok.json, code, callback: cb };
}

describe('live 모드 — OAuth 와 계정 격리(가짜 어댑터)', () => {
  let tmp;
  let srv;
  let o;
  let client;
  let clock = Date.now();
  const now = () => clock;
  const files = {};
  const adapter = fakeAdapter();
  const code = (userId, ttlMs) => issueLinkCode({ file: files.link, userId, ttlMs, now: clock }).then((r) => r.code);

  before(async () => {
    tmp = await tempDir();
    files.link = join(tmp.dir, 'link-codes.json');
    files.auth = join(tmp.dir, 'auth-store.json');
    srv = await startServer({ live: { adapter, linkCodesFile: files.link, authStoreFile: files.auth, now } });
    o = oauth(srv);
    const reg = await o.register();
    assert.equal(reg.status, 201, JSON.stringify(reg.json));
    client = reg.json;
  });
  after(async () => {
    await srv?.close();
    await tmp?.cleanup();
  });

  test('discovery: 보호 리소스·인가 서버 메타데이터, S256, 같은 issuer 문자열', async () => {
    const prm = await (await fetch(`${o.base}/.well-known/oauth-protected-resource/mcp`)).json();
    assert.equal(prm.resource, o.resource);
    const root = await (await fetch(`${o.base}/.well-known/oauth-protected-resource`)).json();
    assert.deepEqual(root.authorization_servers, prm.authorization_servers);
    const as = await (await fetch(`${o.base}/.well-known/oauth-authorization-server`)).json();
    assert.equal(as.issuer, prm.authorization_servers[0], 'issuer 가 정확히 같은 문자열');
    assert.ok(as.code_challenge_methods_supported.includes('S256'));
    assert.ok(as.registration_endpoint);
    assert.deepEqual(as.scopes_supported, ['wardrobe.read', 'comparisons.write']);
  });

  test('토큰 없음·틀린 토큰 → 401 + WWW-Authenticate(resource_metadata)', async () => {
    for (const token of [null, 'nope', 'wea_' + 'x'.repeat(40)]) {
      const r = await o.mcpStatus(token);
      assert.equal(r.status, 401, String(token));
      assert.match(r.www, /^Bearer /);
      assert.match(r.www, /resource_metadata="http:\/\/127\.0\.0\.1:\d+\/\.well-known\/oauth-protected-resource\/mcp"/);
    }
  });

  test('DCR: 허용 목록 밖 redirect_uri·모르는 grant 는 등록 거절', async () => {
    for (const body of [
      { redirect_uris: ['https://evil.example/cb'] },
      { redirect_uris: [REDIRECT, 'https://evil.example/cb'] },
      { redirect_uris: [`${REDIRECT}/`] },
      { redirect_uris: [REDIRECT], grant_types: ['client_credentials'] },
    ]) {
      const r = await o.register(body);
      assert.equal(r.status, 400, JSON.stringify(body));
    }
  });

  test('A 연결 → 내 프로필·내 옷장만, 도구 목록은 oauth2, 사용자 id 인자는 거절', async () => {
    const a = await linkAccount(o, client, await code('user-a'));
    assert.equal(a.callback.searchParams.get('state'), 'st-1');
    assert.equal(a.callback.searchParams.get('iss'), `${o.base}/`);
    assert.equal(a.token_type, 'bearer');
    assert.equal(a.scope, 'wardrobe.read comparisons.write');
    const mcp = await connectClient(o.resource, { token: a.access_token });
    try {
      const { tools } = await mcp.listTools();
      const names = tools.map((t) => t.name);
      assert.ok(names.includes(TOOL_NAMES.myProfile) && names.includes(TOOL_NAMES.myWardrobe));

      // 지시문: 실제 계정 읽기와 데모를 구분하고, 연결 안 된 기능은 없다고 말한다.
      const text = mcp.getInstructions() ?? '';
      assert.doesNotMatch(text, /^데모 전용/, 'live 서버 전체를 데모 전용이라고 하지 않는다');
      assert.match(text, /get_my_profile·list_my_wardrobe 는 OAuth 로 연결된 사용자 본인 계정의 실제 프로필과 옷장을 읽는다/);
      assert.match(text, /샘플 옷장과 이미 생성해 둔 룩북 사진으로 하는 데모/);
      assert.match(text, /연결된 계정에 귀속/);
      assert.match(text, /실제 추천 엔진과 옷 사진 등록은 아직 연결되지 않았다/);
      assert.match(text, /대신 고르지 않는다/);
      for (const mentioned of text.match(/\b[a-z]+(?:_[a-z]+)+\b/g)) {
        assert.ok(names.includes(mentioned) || mentioned === TOOL_NAMES.photo, `지시문이 말하는 도구 ${mentioned} 가 목록에 있다`);
      }
      const writeTools = [TOOL_NAMES.create, TOOL_NAMES.photo, TOOL_NAMES.vote, TOOL_NAMES.update];
      for (const t of tools) {
        const scopes = writeTools.includes(t.name) ? ['wardrobe.read', 'comparisons.write'] : ['wardrobe.read'];
        assert.deepEqual(t._meta.securitySchemes, [{ type: 'oauth2', scopes }], t.name);
      }

      const profile = await mcp.callTool({ name: TOOL_NAMES.myProfile, arguments: {} });
      assert.equal(profile.structuredContent.username, 'alice');
      const wardrobe = await mcp.callTool({ name: TOOL_NAMES.myWardrobe, arguments: {} });
      assert.deepEqual(wardrobe.structuredContent.items.map((i) => i.id), ['a-1', 'a-2']);
      for (const args of [{ user_id: 'user-b' }, { userId: 'user-b' }, { owner: 'user-b' }]) {
        const r = await mcp.callTool({ name: TOOL_NAMES.myWardrobe, arguments: args });
        assert.equal(r.isError, true, JSON.stringify(args));
      }
      assert.ok(adapter.calls.every(([, id]) => id === 'user-a'), '어댑터는 토큰의 사용자로만 불렸다');
    } finally {
      await mcp.close();
    }
  });

  test('A·B 두 계정: 옷장·데모 비교·선택이 서로 보이지 않는다', async () => {
    const a = await linkAccount(o, client, await code('user-a'));
    const b = await linkAccount(o, client, await code('user-b'));
    const ma = await connectClient(o.resource, { token: a.access_token });
    const mb = await connectClient(o.resource, { token: b.access_token });
    try {
      const wb = await mb.callTool({ name: TOOL_NAMES.myWardrobe, arguments: {} });
      assert.deepEqual(wb.structuredContent.items.map((i) => i.id), ['b-1']);
      const pb = await mb.callTool({ name: TOOL_NAMES.myProfile, arguments: {} });
      assert.equal(pb.structuredContent.username, 'bob');

      // A 가 만든 데모 비교는 B 에게 없다(읽기·저장·수정 모두)
      const c = (await ma.callTool({ name: TOOL_NAMES.create, arguments: { situation: 'daily', mood: 'minimal' } })).structuredContent;
      const getB = await mb.callTool({ name: TOOL_NAMES.get, arguments: { comparison_id: c.id } });
      assert.equal(getB.isError, true);
      const voteB = await mb.callTool({ name: TOOL_NAMES.vote, arguments: { comparison_id: c.id, choice: 'a' } });
      assert.equal(voteB.isError, true);
      const updB = await mb.callTool({ name: TOOL_NAMES.update, arguments: { comparison_id: c.id, expected_revision: 1, choice: 'b' } });
      assert.equal(updB.isError, true);
      const voteA = await ma.callTool({ name: TOOL_NAMES.vote, arguments: { comparison_id: c.id, choice: 'b' } });
      assert.equal(voteA.structuredContent.outcome, 'saved');
      const getA = await ma.callTool({ name: TOOL_NAMES.get, arguments: { comparison_id: c.id } });
      assert.equal(getA.structuredContent.vote.choice, 'b');
      // 저장소에는 소유자가 남지만 응답에는 싣지 않는다
      assert.equal(srv.store.getComparison(c.id, 'user-a').vote.choice, 'b');
      assert.equal(srv.store.getComparison(c.id, null), null, '익명으로도 안 보인다');
      assert.doesNotMatch(JSON.stringify(getA), /user-a|owner/);
    } finally {
      await ma.close();
      await mb.close();
    }
  });

  test('scope: 읽기만 받은 연결은 쓰기 도구가 insufficient_scope 로 막히고 아무것도 저장되지 않는다', async () => {
    const before = srv.store.size;
    const t = await linkAccount(o, client, await code('user-a'), { scope: 'wardrobe.read' });
    assert.equal(t.scope, 'wardrobe.read');
    const mcp = await connectClient(o.resource, { token: t.access_token });
    try {
      const ok = await mcp.callTool({ name: TOOL_NAMES.myWardrobe, arguments: {} });
      assert.equal(ok.isError, undefined, '읽기는 된다');
      for (const [name, args] of [
        [TOOL_NAMES.create, { situation: 'daily', mood: 'minimal' }],
        [TOOL_NAMES.vote, { comparison_id: 'demo_cmp_' + '0'.repeat(32), choice: 'a' }],
        [TOOL_NAMES.update, { comparison_id: 'demo_cmp_' + '0'.repeat(32), expected_revision: 1, choice: 'a' }],
      ]) {
        const r = await mcp.callTool({ name, arguments: args });
        assert.equal(r.isError, true, name);
        const challenge = r._meta['mcp/www_authenticate'][0];
        assert.match(challenge, /error="insufficient_scope"/, name);
        assert.match(challenge, /scope="wardrobe\.read comparisons\.write"/, name);
        assert.match(challenge, /resource_metadata="http:\/\/127\.0\.0\.1:\d+\/\.well-known\/oauth-protected-resource\/mcp"/);
      }
      assert.equal(srv.store.size, before, '거절된 쓰기는 데모 저장소에 아무것도 남기지 않는다');
    } finally {
      await mcp.close();
    }
    // scope 를 넓혀 리프레시할 수는 없다
    const widen = await o.token({
      grant_type: 'refresh_token',
      client_id: client.client_id,
      refresh_token: t.refresh_token,
      scope: 'wardrobe.read comparisons.write',
    });
    assert.equal(widen.status, 400);
    assert.equal(widen.json.error, 'invalid_scope');
  });

  test('동의 화면: 틀린 코드는 다시 시도(5회 한도), 다른 사이트 Origin 은 403, 거절은 access_denied', async () => {
    const { challenge } = pkce();
    const page = await o.openConsent(client, { challenge });
    assert.equal(page.headers.get('x-frame-options'), 'DENY');
    assert.equal(page.headers.get('cache-control'), 'no-store');
    assert.match(page.headers.get('content-security-policy'), /frame-ancestors 'none'/);

    const foreign = await o.consent(page.request, await code('user-a'), { origin: 'https://evil.example' });
    assert.equal(foreign.status, 403);
    for (let i = 4; i >= 1; i--) {
      const r = await o.consent(page.request, 'AAAA-AAAA-AAAA-AAAA-AAAA');
      assert.equal(r.status, 400);
      assert.match(r.html, new RegExp(`${i}번 더`));
    }
    const last = await o.consent(page.request, 'AAAA-AAAA-AAAA-AAAA-AAAA');
    assert.match(last.html, /시도 횟수를 넘었어요/);
    // 이 요청은 이제 끝났다 — 맞는 코드여도 안 된다
    const late = await o.consent(page.request, await code('user-a'));
    assert.equal(late.status, 400);
    assert.match(late.html, /만료됐거나 이미 처리/);

    const page2 = await o.openConsent(client, { challenge, state: 'st-deny' });
    const deny = await o.consent(page2.request, '', { action: 'deny' });
    assert.equal(deny.status, 302);
    const cb = new URL(deny.location);
    assert.equal(cb.searchParams.get('error'), 'access_denied');
    assert.equal(cb.searchParams.get('state'), 'st-deny');
  });

  test('연결 코드: 한 번만, 만료되면 못 씀, 없는 계정 코드는 거절', async () => {
    const once = await code('user-a');
    await linkAccount(o, client, once);
    const { challenge } = pkce();
    const page = await o.openConsent(client, { challenge });
    const again = await o.consent(page.request, once);
    assert.equal(again.status, 400, '이미 쓴 코드');

    const short = await code('user-a', 60_000);
    clock += 61_000;
    const page2 = await o.openConsent(client, { challenge });
    const expired = await o.consent(page2.request, short);
    assert.equal(expired.status, 400, '만료된 코드');

    const ghost = await code('user-ghost');
    const page3 = await o.openConsent(client, { challenge });
    assert.equal((await o.consent(page3.request, ghost)).status, 400, '없는 계정');
  });

  test('인가 요청 거절: 등록 안 된 redirect·resource 다름/없음·S256 아님', async () => {
    const { challenge } = pkce();
    const wrongRedirect = await o.openConsent(client, { challenge, redirect: 'https://evil.example/cb' });
    assert.equal(wrongRedirect.status, 400, '등록 안 된 redirect 는 그쪽으로 보내지 않고 바로 400');
    assert.equal(wrongRedirect.location, null);

    for (const res of ['https://other.example/mcp', `${o.base}/other`, null]) {
      const r = await o.openConsent(client, { challenge, res });
      assert.equal(r.status, 302, String(res));
      const cb = new URL(r.location);
      assert.equal(cb.origin + cb.pathname, REDIRECT);
      assert.equal(cb.searchParams.get('error'), 'invalid_target', String(res));
    }
    const plain = await o.openConsent(client, { challenge, method: 'plain' });
    assert.ok(plain.status === 400 || new URL(plain.location).searchParams.get('error'), 'plain 거절');
    const badScope = await o.openConsent(client, { challenge, scope: 'wardrobe.write' });
    assert.equal(new URL(badScope.location).searchParams.get('error'), 'invalid_scope');
  });

  test('토큰 교환 거절: PKCE 불일치(한 번 틀리면 코드 폐기)·redirect 다름·resource 다름·코드 재사용(발급 토큰 폐기)', async () => {
    async function codeFor(userId) {
      const p = pkce();
      const page = await o.openConsent(client, { challenge: p.challenge });
      const done = await o.consent(page.request, await code(userId));
      return { ...p, code: new URL(done.location).searchParams.get('code') };
    }
    const base = (c, extra = {}) => ({
      grant_type: 'authorization_code',
      client_id: client.client_id,
      code: c.code,
      code_verifier: c.verifier,
      redirect_uri: REDIRECT,
      resource: o.resource,
      ...extra,
    });

    const c1 = await codeFor('user-a');
    const bad = await o.token(base(c1, { code_verifier: b64url(randomBytes(32)) }));
    assert.equal(bad.status, 400);
    assert.equal(bad.json.error, 'invalid_grant');
    const retry = await o.token(base(c1));
    assert.equal(retry.status, 400, 'PKCE 실패 뒤 같은 코드는 못 쓴다');

    const c2 = await codeFor('user-a');
    const wrongRedirect = await o.token(base(c2, { redirect_uri: 'https://chatgpt.com/other' }));
    assert.equal(wrongRedirect.json.error, 'invalid_grant');

    const c3 = await codeFor('user-a');
    const wrongResource = await o.token(base(c3, { resource: 'https://other.example/mcp' }));
    assert.equal(wrongResource.status, 400);
    assert.ok(['invalid_target', 'invalid_grant'].includes(wrongResource.json.error), JSON.stringify(wrongResource.json));

    const c4 = await codeFor('user-a');
    const ok = await o.token(base(c4));
    assert.equal(ok.status, 200);
    assert.equal((await o.mcpStatus(ok.json.access_token)).status, 200);
    const replay = await o.token(base(c4));
    assert.equal(replay.json.error, 'invalid_grant');
    assert.equal((await o.mcpStatus(ok.json.access_token)).status, 401, '재사용이 오면 그 코드로 나간 토큰도 폐기');
  });

  test('리프레시 회전·재사용 탐지·revoke·만료', async () => {
    const t = await linkAccount(o, client, await code('user-b'));
    const r1 = await o.token({ grant_type: 'refresh_token', client_id: client.client_id, refresh_token: t.refresh_token, resource: o.resource });
    assert.equal(r1.status, 200, JSON.stringify(r1.json));
    assert.notEqual(r1.json.refresh_token, t.refresh_token);
    assert.equal((await o.mcpStatus(r1.json.access_token)).status, 200);
    // 옛 리프레시 재사용 → 거절 + 가족 폐기
    const reuse = await o.token({ grant_type: 'refresh_token', client_id: client.client_id, refresh_token: t.refresh_token });
    assert.equal(reuse.json.error, 'invalid_grant');
    assert.equal((await o.mcpStatus(r1.json.access_token)).status, 401);

    // revoke
    const t2 = await linkAccount(o, client, await code('user-b'));
    assert.equal(await o.revoke(client.client_id, t2.access_token), 200);
    assert.equal((await o.mcpStatus(t2.access_token)).status, 401);
    const afterRevoke = await o.token({ grant_type: 'refresh_token', client_id: client.client_id, refresh_token: t2.refresh_token });
    assert.equal(afterRevoke.status, 400, '같은 가족의 리프레시도 폐기');

    // 만료(액세스 1시간)
    const t3 = await linkAccount(o, client, await code('user-b'));
    assert.equal((await o.mcpStatus(t3.access_token)).status, 200);
    clock += 61 * 60 * 1000;
    const expired = await o.mcpStatus(t3.access_token);
    assert.equal(expired.status, 401);
    assert.match(expired.www, /invalid_token/);
  });

  test('저장 파일에는 토큰·코드 원문이 없고, 다른 resource(포트)의 서버는 그 토큰을 받지 않는다', async () => {
    const t = await linkAccount(o, client, await code('user-a'));
    const raw = await readFile(files.auth, 'utf8');
    for (const secret of [t.access_token, t.refresh_token, t.code]) assert.ok(!raw.includes(secret));
    const links = await readFile(files.link, 'utf8');
    assert.doesNotMatch(links, /[0-9A-Z]{4}-[0-9A-Z]{4}-/);

    const second = await startServer({ live: { adapter, linkCodesFile: files.link, authStoreFile: files.auth, now } });
    try {
      // 다른 포트라 resource 가 다르다 — 원래 서버용 토큰은 audience 가 맞지 않아 거절돼야 한다.
      const o2 = oauth(second);
      assert.equal((await o2.mcpStatus(t.access_token)).status, 401, '다른 resource 의 토큰');
    } finally {
      await second.close();
    }
    // 원래 서버에서는 그대로 유효하다(같은 포트로 다시 띄우는 경우는 아래 묶음).
    assert.equal((await o.mcpStatus(t.access_token)).status, 200);
  });
});

describe('live 모드 — 재시작 지속성(같은 포트·같은 저장 파일)', () => {
  test('서버를 내렸다 같은 포트로 다시 띄우면 발급한 토큰이 그대로 유효', async () => {
    const tmp = await tempDir();
    try {
      const files = { link: join(tmp.dir, 'l.json'), auth: join(tmp.dir, 'a.json') };
      const adapter = fakeAdapter();
      const first = await startServer({ live: { adapter, linkCodesFile: files.link, authStoreFile: files.auth } });
      const o1 = oauth(first);
      const client = (await o1.register()).json;
      const { code } = await issueLinkCode({ file: files.link, userId: 'user-a' });
      const t = await linkAccount(o1, client, code);
      const port = first.port;
      await first.close();

      const second = await startServer({ live: { adapter, linkCodesFile: files.link, authStoreFile: files.auth }, port });
      try {
        const o2 = oauth(second);
        assert.equal(second.port, port);
        assert.equal((await o2.mcpStatus(t.access_token)).status, 200);
        const refreshed = await o2.token({ grant_type: 'refresh_token', client_id: client.client_id, refresh_token: t.refresh_token });
        assert.equal(refreshed.status, 200, '클라이언트 등록·리프레시도 남아 있다');
      } finally {
        await second.close();
      }
    } finally {
      await tmp.cleanup();
    }
  });
});

describe('live 모드 — 실제 DB 어댑터(임시 테스트 DB)', { skip: process.env.TEST_DATABASE_URL ? false : 'TEST_DATABASE_URL 없음 — 실DB 격리 미검증' }, () => {
  test('A·B 실제 옷장 행이 토큰 주인 기준으로만 읽힌다', async () => {
    const url = process.env.TEST_DATABASE_URL;
    assert.ok(new URL(url).pathname.includes('test'), 'test 가 든 DB 이름만');
    const mysql = (await import('mysql2/promise')).default;
    const { LiveAdapter } = await import('../src/adapters/live.js');
    const conn = await mysql.createConnection({ uri: url });
    const run = randomBytes(5).toString('hex');
    const ua = `mcp-live-${run}-a`;
    const ub = `mcp-live-${run}-b`;
    const tmp = await tempDir();
    let adapter;
    let srv;
    try {
      for (const [id, name] of [[ua, 'A'], [ub, 'B']]) {
        await conn.execute('INSERT INTO app_user (id, username, api_token, display_name) VALUES (?, ?, ?, ?)', [id, id, `tok-${id}`, name]);
      }
      await conn.execute("INSERT INTO clothing (id, user_id, name, category, color) VALUES (?, ?, 'A 니트', '상의', '네이비'), (?, ?, 'B 코트', '아우터', '블랙')", [
        `${ua}-1`, ua, `${ub}-1`, ub,
      ]);
      adapter = await LiveAdapter.connect(url);
      assert.equal(await adapter.exists(ua), true);
      assert.equal(await adapter.exists('nobody-' + run), false);
      const files = { link: join(tmp.dir, 'l.json'), auth: join(tmp.dir, 'a.json') };
      srv = await startServer({ live: { adapter, linkCodesFile: files.link, authStoreFile: files.auth } });
      const o = oauth(srv);
      const client = (await o.register()).json;
      const ta = await linkAccount(o, client, (await issueLinkCode({ file: files.link, userId: ua })).code);
      const tb = await linkAccount(o, client, (await issueLinkCode({ file: files.link, userId: ub })).code);
      for (const [t, mine, theirs, name] of [
        [ta, `${ua}-1`, `${ub}-1`, 'A'],
        [tb, `${ub}-1`, `${ua}-1`, 'B'],
      ]) {
        const mcp = await connectClient(o.resource, { token: t.access_token });
        try {
          const w = await mcp.callTool({ name: TOOL_NAMES.myWardrobe, arguments: {} });
          const ids = w.structuredContent.items.map((i) => i.id);
          assert.deepEqual(ids, [mine], name);
          assert.ok(!ids.includes(theirs));
          assert.ok(!ids.some((id) => !id.startsWith(`mcp-live-${run}`)), 'default·다른 사용자 옷이 섞이지 않는다');
          const p = await mcp.callTool({ name: TOOL_NAMES.myProfile, arguments: {} });
          assert.equal(p.structuredContent.display_name, name);
          assert.equal(p.structuredContent.wardrobe_count, 1);
        } finally {
          await mcp.close();
        }
      }
    } finally {
      await srv?.close();
      await adapter?.close();
      await conn.execute('DELETE FROM clothing WHERE user_id IN (?, ?)', [ua, ub]);
      await conn.execute('DELETE FROM app_user WHERE id IN (?, ?)', [ua, ub]);
      await conn.end();
      await tmp.cleanup();
    }
  });
});

describe('live 어댑터 안전장치', () => {
  test('clothing.user_id 가 없는 DB(마이그레이션 전)면 시작을 거부한다 — 전역 옷장을 읽지 않는다', async () => {
    const { LiveAdapter } = await import('../src/adapters/live.js');
    const stub = { query: async () => [[{ n: 0 }]] };
    await assert.rejects(new LiveAdapter(stub).checkSchema(), /마이그레이션/);
  });

  test('모르는 카테고리·지나친 limit 는 질의 전에 막거나 줄인다', async () => {
    const { LiveAdapter } = await import('../src/adapters/live.js');
    const seen = [];
    const stub = { execute: async (sql, params) => (seen.push([sql, params]), [[]]) };
    const a = new LiveAdapter(stub);
    await assert.rejects(a.listWardrobe('u', { category: "상의' OR 1=1 --" }), /카테고리/);
    await a.listWardrobe('u', { limit: 100000 });
    assert.match(seen.at(-1)[0], /WHERE user_id = \? ORDER BY .* LIMIT 200$/);
    assert.deepEqual(seen.at(-1)[1], ['u']);
  });
});

describe('진단용 접근 로그(MCP_ACCESS_LOG)', () => {
  test('경로·상태·JSON-RPC 메서드·도구 이름만 남고 토큰·코드·쿼리·옷 이름은 남지 않는다', async () => {
    const tmp = await tempDir();
    const lines = [];
    try {
      const files = { link: join(tmp.dir, 'l.json'), auth: join(tmp.dir, 'a.json') };
      const srv = await startServer({
        live: { adapter: fakeAdapter(), linkCodesFile: files.link, authStoreFile: files.auth },
        http: { accessLog: true },
        log: (m) => lines.push(m),
      });
      try {
        const o = oauth(srv);
        assert.equal((await o.mcpStatus(null)).status, 401);
        const client = (await o.register()).json;
        const { code } = await issueLinkCode({ file: files.link, userId: 'user-a' });
        const t = await linkAccount(o, client, code);
        const mcp = await connectClient(o.resource, { token: t.access_token });
        await mcp.callTool({ name: TOOL_NAMES.myWardrobe, arguments: { category: '상의' } });
        await mcp.close();
        const secrets = [t.access_token, t.refresh_token, t.code, code, 'st-1', 'A 네이비 니트', '상의'];
        const access = lines.filter((l) => l.startsWith('access '));
        assert.ok(access.some((l) => / POST \/mcp 401 /.test(l) && /rpc=- /.test(l) && /auth=no/.test(l)), access.join('\n'));
        assert.ok(access.some((l) => / GET \/authorize 200 /.test(l)));
        assert.ok(access.some((l) => / POST \/oauth\/consent 302 /.test(l)));
        assert.ok(access.some((l) => / POST \/token 200 /.test(l)));
        assert.ok(access.some((l) => /rpc=initialize /.test(l) && / 200 /.test(l) && /auth=yes/.test(l) && /accept=json\+sse/.test(l)));
        assert.ok(access.some((l) => /rpc=tools\/call\(list_my_wardrobe\) /.test(l) && / 200 /.test(l)));
        for (const l of lines) {
          for (const secret of secrets) assert.ok(!l.includes(secret), `로그에 값이 섞였다: ${l.slice(0, 80)}`);
          assert.ok(!l.includes('?'), `쿼리 문자열이 섞였다: ${l.slice(0, 80)}`);
        }
      } finally {
        await srv.close();
      }
    } finally {
      await tmp.cleanup();
    }
  });

  test('기본은 꺼져 있다(접근 줄을 남기지 않음)', async () => {
    const lines = [];
    const srv = await startServer({ log: (m) => lines.push(m) });
    try {
      await fetch(srv.url.replace('/mcp', '/healthz'));
      assert.equal(lines.filter((l) => l.startsWith('access ')).length, 0);
    } finally {
      await srv.close();
    }
  });
});
