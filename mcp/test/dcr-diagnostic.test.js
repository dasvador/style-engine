// POST /register 진단 로그: MCP_ACCESS_LOG=1 일 때만, 이유와 안전한 redirect_uri 만 남고 판정은 바뀌지 않는다.

import assert from 'node:assert/strict';
import { after, before, describe, test } from 'node:test';
import { join } from 'node:path';

import { describeRedirectUri, validationIssues } from '../src/auth/dcr-diagnostic.js';
import { startServer, tempDir } from './helpers.js';
import { REDIRECT, fakeAdapter, oauth } from './oauth-helpers.js';

const SECRETS = ['s3cret-pass', 'q-token-value', 'frag-value', 'statement-secret', 'name-secret'];

describe('DCR 진단 로그', () => {
  let tmp, on, off;
  const lines = [];
  const offLines = [];
  const dcr = () => lines.filter((l) => l.startsWith('dcr '));
  const start = (accessLog, log, name) =>
    startServer({
      http: { accessLog },
      log,
      live: { adapter: fakeAdapter(), linkCodesFile: join(tmp.dir, `${name}.json`) },
    });

  before(async () => {
    tmp = await tempDir();
    on = await start(true, (l) => lines.push(l), 'on');
    off = await start(false, (l) => offLines.push(l), 'off');
  });
  after(async () => {
    await on?.close();
    await off?.close();
    await tmp?.cleanup();
  });

  const post = (srv, body, headers = { 'content-type': 'application/json' }) =>
    fetch(`http://127.0.0.1:${srv.port}/register`, { method: 'POST', headers, body });
  const logged = async (fn) => {
    lines.length = 0;
    await fn();
    for (let i = 0; i < 50 && dcr().length === 0; i += 1) await new Promise((r) => setTimeout(r, 5));
    assert.equal(dcr().length, 1, JSON.stringify(lines));
    return dcr()[0];
  };

  test('허용 목록 밖: 이유와 origin·path 만, 쿼리·조각·사용자 정보·본문은 없다', async () => {
    const line = await logged(async () => {
      const r = await oauth(on).register({
        client_name: 'name-secret',
        software_statement: 'statement-secret',
        redirect_uris: [`https://user:s3cret-pass@chatgpt.com/connector/oauth/abc?token=q-token-value#frag-value`, REDIRECT],
      });
      assert.equal(r.status, 400);
    });
    assert.match(line, /^dcr status=400 error=invalid_client_metadata reason=redirect_uri_not_allowed /);
    assert.match(line, /https:\/\/chatgpt\.com\/connector\/oauth\/abc\(not_allowed,has_query,has_fragment,has_credentials\)/);
    assert.ok(line.includes(`${REDIRECT}(allowed)`), line);
    for (const s of SECRETS) assert.ok(!lines.join('\n').includes(s), s);
  });

  test('registerClient 의 나머지 거절도 이유가 나온다', async () => {
    const line = await logged(() => oauth(on).register({ grant_types: ['client_credentials'] }));
    assert.match(line, /reason=grant_type_not_supported/);
    assert.match(line, /grant_types=client_credentials/);
    const method = await logged(() => oauth(on).register({ token_endpoint_auth_method: 'private_key_jwt' }));
    assert.match(method, /reason=auth_method_not_supported .*auth_method=private_key_jwt/);
  });

  test('SDK 스키마 거절: 필드 경로와 코드만 남고 값은 남지 않는다', async () => {
    const line = await logged(async () => {
      const r = await oauth(on).register({ client_uri: 'not-a-url-name-secret', redirect_uris: [REDIRECT] });
      assert.equal(r.status, 400);
    });
    assert.match(line, /reason=sdk_validation issues=client_uri:/);
    assert.ok(line.includes(`${REDIRECT}(allowed)`), line);
    assert.ok(!line.includes('name-secret'), line);
  });

  test('깨진 JSON·본문 없음·JSON 아닌 형식', async () => {
    const broken = await logged(async () => {
      const r = await post(on, '{"client_name":"name-secret",');
      assert.equal(r.status, 400);
      assert.equal((await r.json()).error, 'invalid_client_metadata');
    });
    assert.match(broken, /status=400 .*reason=body_not_json/);
    assert.ok(!broken.includes('name-secret'));
    const form = await logged(() => post(on, 'a=1', { 'content-type': 'application/x-www-form-urlencoded; charset=utf-8' }));
    assert.match(form, /reason=sdk_validation issues=\(body\):/);
    assert.match(form, /content_type=application\/x-www-form-urlencoded /);
  });

  test('성공: 실제 callback 이 보이고 client_id·client_secret 은 없다', async () => {
    let reg;
    const line = await logged(async () => {
      reg = await oauth(on).register({ token_endpoint_auth_method: 'client_secret_post' });
      assert.equal(reg.status, 201);
    });
    assert.match(line, /^dcr status=201 error=- reason=registered /);
    assert.ok(line.includes(`${REDIRECT}(allowed)`), line);
    assert.ok(reg.json.client_secret);
    const all = lines.join('\n');
    assert.ok(!all.includes(reg.json.client_secret) && !all.includes(reg.json.client_id));
  });

  test('꺼져 있으면 로그가 없고 판정은 같다', async () => {
    const o = oauth(off);
    assert.equal((await o.register({ redirect_uris: ['https://evil.example/cb'] })).status, 400);
    assert.equal((await o.register()).status, 201);
    assert.equal(offLines.filter((l) => l.startsWith('dcr ')).length, 0);
  });

  test('진단이 켜져도 허용 판정은 넓어지지 않는다', async () => {
    const o = oauth(on);
    for (const uri of [`${REDIRECT}/`, `${REDIRECT}?x=1`, 'https://chatgpt.com/connector/oauth/other']) {
      assert.equal((await o.register({ redirect_uris: [uri] })).status, 400, uri);
    }
  });

  test('값 다듬기', () => {
    const list = new Set(['https://a.example/cb']);
    assert.equal(describeRedirectUri('https://a.example/cb', list), 'https://a.example/cb(allowed)');
    assert.equal(describeRedirectUri('not a url', list), '(invalid-url)(not_allowed)');
    assert.equal(describeRedirectUri('https://a.example/cb\n\rinjected', list).includes('\n'), false);
    assert.equal(validationIssues('그냥 문장'), null);
    assert.equal(validationIssues('[{"code":"invalid_format","path":["redirect_uris",0],"message":"secret"}]'), 'redirect_uris.0:invalid_format');
    assert.equal(validationIssues('[{"code":"x y secret","path":["a b"],"message":"m"}]'), '(other):(other)');
  });
});
