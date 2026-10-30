// live 모드 테스트 공용: 가짜 어댑터(A·B 두 계정)와 손으로 도는 OAuth.

import assert from 'node:assert/strict';
import { createHash, randomBytes } from 'node:crypto';

import { DEFAULT_REDIRECT_URIS } from '../src/auth/oauth.js';

export const REDIRECT = DEFAULT_REDIRECT_URIS[0];

/** 메모리 속 계정 두 개. 실제 어댑터와 같은 메서드. */
export function fakeAdapter() {
  const users = {
    'user-a': { username: 'alice', display_name: '앨리스', items: [item('a-1', 'A 네이비 니트', '상의'), item('a-2', 'A 데님', '하의')] },
    'user-b': { username: 'bob', display_name: null, items: [item('b-1', 'B 블랙 코트', '아우터')] },
  };
  // 추천 사전 확인용: 지역(없으면 no_region)·성별별 무드. A·B 는 지역이 있다.
  const regions = { 'user-a': '서울 종로구', 'user-b': '부산 해운대구' };
  const moodsByGender = { female: ['minimal', 'street', 'glam', 'edgy', 'pop'], male: ['minimal', 'street', 'amekaji'] };
  const extraUsers = {
    'user-c': { username: 'carol', display_name: null, items: [item('c-1', 'C 셔츠', '상의')] }, // 지역 없음
    'user-d': { username: 'dave', display_name: null, items: [] }, // 빈 옷장
  };
  Object.assign(users, extraUsers);
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
    async recommendContext(id, { gender = null, mood = null } = {}) {
      this.calls.push(['recommendContext', id]);
      return {
        wardrobe: users[id]?.items.length ?? 0,
        region: regions[id] ?? null,
        moodOk: !mood || (moodsByGender[gender] ?? []).includes(mood),
      };
    },
    async ownedClothingIds(id, ids) {
      this.calls.push(['owned', id]);
      const mine = new Set((users[id]?.items ?? []).map((i) => i.id));
      return new Set(ids.filter((x) => mine.has(x)));
    },
    async batchWeather(id, batchId) {
      this.calls.push(['weather', id]);
      return batchId === `batch-${id}` ? { temperature: 14.5, apparent_temperature: 13, description: '맑음' } : null;
    },
    async listWardrobe(id, { category = null } = {}) {
      this.calls.push(['wardrobe', id]);
      return (users[id]?.items ?? []).filter((i) => !category || i.category === category);
    },
  };
}

export const b64url = (buf) => Buffer.from(buf).toString('base64url');
export function pkce() {
  const verifier = b64url(randomBytes(32));
  const challenge = b64url(createHash('sha256').update(verifier).digest());
  return { verifier, challenge };
}

/** OAuth 한 바퀴를 손으로 돈다(브라우저 대신 fetch). 단계별로 끊어 쓸 수 있게 함수로 나눈다. */
export function oauth(srv) {
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
export async function linkAccount(o, client, linkCode, { scope } = {}) {
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
