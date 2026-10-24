// 동의 화면(/authorize → POST /oauth/consent)을 실제 Chromium 으로 제출한다.
//
// 실제 ChatGPT 연결에서 같은 출처 폼 POST 가 `Origin: null` 로 와 403 이 났다 — 페이지의
// `Referrer-Policy: no-referrer` 때문(Fetch 표준: no-referrer 면 요청의 Origin 을 null 로 직렬화).
// fetch 로 만든 요청은 Origin 을 마음대로 넣을 수 있어 이 결함을 잡지 못한다. 그래서 브라우저가 붙이는
// 실제 Origin·CSP(form-action 은 POST 뒤 리다이렉트에도 적용) 그대로 끝까지 간다.
// 콜백은 다른 origin 의 로컬 서버(http://localhost:<다른 포트>)로 등록한다 — 외부로 나가지 않는다.
// (Playwright 의 route 는 서버 302 로 이어진 요청을 가로채지 못해, chatgpt.com 을 콜백으로 쓰면 실제로 접속한다.)
// 브라우저는 내려받지 않고 설치된 것을 쓴다(없으면 이 묶음만 skip).

import assert from 'node:assert/strict';
import { createHash, randomBytes } from 'node:crypto';
import { createServer } from 'node:http';
import { existsSync, readdirSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { after, before, describe, test } from 'node:test';

import { chromium } from 'playwright-core';

import { issueLinkCode } from '../src/auth/link-codes.js';
import { startServer, tempDir } from './helpers.js';

function findChromium() {
  const candidates = [process.env.CHROME_PATH, chromium.executablePath()];
  const cache = join(homedir(), 'Library/Caches/ms-playwright');
  if (existsSync(cache)) {
    for (const d of readdirSync(cache).filter((n) => /^chromium-\d+$/.test(n)).sort().reverse()) {
      candidates.push(join(cache, d, 'chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'));
      candidates.push(join(cache, d, 'chrome-linux/chrome'));
    }
  }
  candidates.push('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome');
  return candidates.find((p) => p && existsSync(p)) ?? null;
}
const executablePath = findChromium();

const adapter = {
  async exists(id) {
    return id === 'user-a';
  },
  async getProfile() {
    return { username: 'alice', display_name: null, wardrobe_count: 0, wardrobe_by_category: {} };
  },
  async listWardrobe() {
    return [];
  },
};

describe('동의 화면 — 실제 브라우저 제출', { skip: executablePath ? false : '설치된 Chromium 을 찾지 못함' }, () => {
  let tmp;
  let srv;
  let browser;
  let base;
  let client;
  let linkFile;
  let cbServer;
  let REDIRECT;
  let CB_ORIGIN;
  /** 콜백 서버가 받은 요청 { url, referer, origin } */
  const callbacks = [];

  before(async () => {
    tmp = await tempDir();
    linkFile = join(tmp.dir, 'link-codes.json');
    cbServer = createServer((req, res) => {
      callbacks.push({ url: `${CB_ORIGIN}${req.url}`, referer: req.headers.referer ?? null, origin: req.headers.origin ?? null });
      res.end('callback received');
    });
    await new Promise((r) => cbServer.listen(0, '127.0.0.1', r));
    // 같은 루프백이지만 호스트 이름이 달라 다른 origin(localhost vs 127.0.0.1)이다.
    CB_ORIGIN = `http://localhost:${cbServer.address().port}`;
    REDIRECT = `${CB_ORIGIN}/connector_platform_oauth_redirect`;
    srv = await startServer({
      live: { adapter, linkCodesFile: linkFile, authStoreFile: join(tmp.dir, 'auth.json'), redirectAllowlist: [REDIRECT] },
    });
    base = `http://127.0.0.1:${srv.port}`;
    const reg = await fetch(`${base}/register`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ client_name: 'ChatGPT', redirect_uris: [REDIRECT], token_endpoint_auth_method: 'none' }),
    });
    client = await reg.json();
    browser = await chromium.launch({ executablePath, headless: true });
  });
  after(async () => {
    await browser?.close();
    await srv?.close();
    await new Promise((r) => cbServer?.close(r));
    await tmp?.cleanup();
  });

  function authorizeUrl(challenge, state = 'st-browser') {
    const u = new URL(`${base}/authorize`);
    for (const [k, v] of Object.entries({
      response_type: 'code',
      client_id: client.client_id,
      redirect_uri: REDIRECT,
      code_challenge: challenge,
      code_challenge_method: 'S256',
      state,
      scope: 'wardrobe.read comparisons.write',
      resource: `${base}/mcp`,
    })) {
      u.searchParams.set(k, v);
    }
    return u.href;
  }

  async function openConsent() {
    const verifier = randomBytes(32).toString('base64url');
    const challenge = createHash('sha256').update(verifier).digest('base64url');
    const page = await browser.newPage();
    const consentPosts = [];
    const cspErrors = [];
    const seen = callbacks.length;
    page.on('request', (r) => {
      if (r.method() === 'POST' && r.url() === `${base}/oauth/consent`) consentPosts.push(r.headers());
    });
    page.on('console', (m) => {
      if (/Content Security Policy|form-action/i.test(m.text())) cspErrors.push(m.text());
    });
    // 혹시라도 외부로 나가는 요청은 막는다(로컬 두 origin 만 허용).
    await page.route(/^https?:\/\/(?!127\.0\.0\.1|localhost)/, (route) => route.abort());
    const res = await page.goto(authorizeUrl(challenge));
    return { page, res, verifier, consentPosts, cspErrors, newCallbacks: () => callbacks.slice(seen) };
  }

  test('동의 화면 헤더: Referrer-Policy same-origin, form-action 은 자기 자신 + 등록된 콜백 origin 만', async () => {
    const { page, res } = await openConsent();
    const h = res.headers();
    assert.equal(res.status(), 200);
    assert.equal(h['referrer-policy'], 'same-origin', 'no-referrer 면 같은 출처 POST 의 Origin 이 null 이 된다');
    const csp = h['content-security-policy'];
    assert.ok(csp.includes(`form-action 'self' ${CB_ORIGIN};`), csp);
    assert.doesNotMatch(csp, /form-action[^;]*\*/, '와일드카드 없음');
    assert.match(csp, /frame-ancestors 'none'/);
    assert.equal(h['x-frame-options'], 'DENY');
    assert.equal(h['cache-control'], 'no-store');
    await page.close();
  });

  test('브라우저 제출: 같은 출처 Origin 이 실리고, 콜백으로 코드·state 가 돌아오며, 토큰으로 바꿀 수 있다', async () => {
    const { page, verifier, consentPosts, cspErrors, newCallbacks } = await openConsent();
    const { code } = await issueLinkCode({ file: linkFile, userId: 'user-a' });
    await page.fill('#code', code);
    await Promise.all([page.waitForURL((u) => u.href.startsWith(REDIRECT)), page.getByRole('button', { name: '연결' }).click()]);
    const callbacks = newCallbacks();
    assert.equal(consentPosts.length, 1);
    assert.equal(consentPosts[0].origin, base, '브라우저가 보낸 Origin 이 자기 origin 이어야 한다(null 이 아니라)');
    assert.deepEqual(cspErrors, [], 'POST 뒤 콜백 리다이렉트가 CSP form-action 에 막히지 않는다');
    assert.equal(callbacks.length, 1);
    const cb = new URL(callbacks[0].url);
    assert.equal(`${cb.origin}${cb.pathname}`, REDIRECT);
    assert.equal(cb.searchParams.get('state'), 'st-browser');
    assert.ok(cb.searchParams.get('code'));
    const tok = await fetch(`${base}/token`, {
      method: 'POST',
      headers: { 'content-type': 'application/x-www-form-urlencoded' },
      body: new URLSearchParams({
        grant_type: 'authorization_code',
        client_id: client.client_id,
        code: cb.searchParams.get('code'),
        code_verifier: verifier,
        redirect_uri: REDIRECT,
        resource: `${base}/mcp`,
      }),
    });
    assert.equal(tok.status, 200);
    await page.close();
  });

  test('콜백으로 가는 요청에 동의 화면 주소를 Referer 로 싣지 않는다', async () => {
    const { page, newCallbacks } = await openConsent();
    const { code } = await issueLinkCode({ file: linkFile, userId: 'user-a' });
    await page.fill('#code', code);
    await Promise.all([page.waitForURL((u) => u.href.startsWith(REDIRECT)), page.getByRole('button', { name: '연결' }).click()]);
    const [cb] = newCallbacks();
    assert.equal(cb.referer, null, '다른 출처 콜백으로 동의 화면 주소(쿼리 포함)를 보내지 않는다');
    await page.close();
  });

  test('틀린 코드 → 같은 화면에서 다시 시도 → 맞는 코드로 연결(재제출도 Origin 유지)', async () => {
    const { page, consentPosts, newCallbacks } = await openConsent();
    await page.fill('#code', 'AAAA-AAAA-AAAA-AAAA-AAAA');
    await page.getByRole('button', { name: '연결' }).click();
    await page.getByText(/코드가 맞지 않거나/).waitFor();
    const { code } = await issueLinkCode({ file: linkFile, userId: 'user-a' });
    await page.fill('#code', code);
    await Promise.all([page.waitForURL((u) => u.href.startsWith(REDIRECT)), page.getByRole('button', { name: '연결' }).click()]);
    assert.equal(consentPosts.length, 2);
    assert.ok(consentPosts.every((h) => h.origin === base));
    assert.equal(newCallbacks().length, 1);
    await page.close();
  });

  test('다른 출처(여기서는 opaque origin → Origin: null)에서 보낸 폼은 403 — null 을 허용하지 않는다', async () => {
    const { page: consent } = await openConsent();
    const request = await consent.locator('input[name="request"]').inputValue();
    await consent.close();
    const { code } = await issueLinkCode({ file: linkFile, userId: 'user-a' });
    const page = await browser.newPage();
    let sentOrigin = 'unset';
    page.on('request', (r) => {
      if (r.method() === 'POST') sentOrigin = r.headers().origin ?? null;
    });
    await page.setContent(
      `<form method="post" action="${base}/oauth/consent"><input name="request" value="${request}">` +
        `<input name="link_code" value="${code}"><button name="action" value="allow">go</button></form>`,
    );
    const [res] = await Promise.all([page.waitForResponse(`${base}/oauth/consent`), page.click('button')]);
    assert.equal(sentOrigin, 'null');
    assert.equal(res.status(), 403);
    await page.close();
  });
});
