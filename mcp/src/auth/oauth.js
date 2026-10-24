// MCP OAuth(인가 코드 + PKCE S256) — 인가 서버와 리소스 서버를 이 MCP 서버 하나에 둔다.
//
// 공식 SDK 의 검증된 부품을 쓴다:
//   - mcpAuthRouter: /.well-known 메타데이터, /authorize(클라이언트·redirect_uri·S256 검사), /token(클라이언트 인증,
//     PKCE 검증), /register(DCR), /revoke
//   - requireBearerAuth: /mcp 의 Bearer 토큰 검사(만료·scope·resource=audience), 실패 시 WWW-Authenticate
// 여기서 채우는 것은 OAuthServerProvider(코드·토큰 발급과 저장)와 동의 화면(계정 연결 코드 입력)뿐이다.
//
// 보안 규칙:
//   - redirect_uri 는 허용 목록과 **정확히 같은 문자열**만 등록할 수 있다(DCR 에서 거절).
//   - resource 는 이 서버의 /mcp 와 같아야 한다(인가·토큰 요청 모두). 토큰에 resource 를 묶고 검사한다.
//   - 인가 코드: 5분, 한 번만. PKCE 검증 시도는 한 번 — 틀리면 코드 폐기. 이미 바꾼 코드가 다시 오면 그 코드로 나간
//     토큰을 모두 폐기한다(RFC 6749 4.1.2).
//   - 리프레시 토큰은 쓸 때마다 바뀐다(회전). 옛 리프레시가 다시 오면 가족 전체 폐기.
//   - 동의 화면: 일회용 요청 id, Origin 검사, 시도 5회 제한, 프레임·캐시 금지. 사용자 id 는 연결 코드에서만 나온다.
//   - 토큰·코드는 원문을 저장하지 않고(해시), 로그에 남기지 않는다.

import express from 'express';
import { mcpAuthRouter, getOAuthProtectedResourceMetadataUrl } from '@modelcontextprotocol/sdk/server/auth/router.js';
import { requireBearerAuth } from '@modelcontextprotocol/sdk/server/auth/middleware/bearerAuth.js';
import {
  InvalidClientMetadataError,
  InvalidGrantError,
  InvalidRequestError,
  InvalidScopeError,
  InvalidTargetError,
  InvalidTokenError,
} from '@modelcontextprotocol/sdk/server/auth/errors.js';

import { AuthStore, randomToken, sha256 } from './store.js';
import { findLinkCode } from './link-codes.js';

/** 내 프로필·내 옷장 읽기. /mcp 에 들어오려면 늘 필요하다. */
export const SCOPE_READ = 'wardrobe.read';
/** 데모 비교 만들기와 내 선택 저장·수정(데모 기록에만 쓴다). */
export const SCOPE_COMPARISONS_WRITE = 'comparisons.write';
export const SUPPORTED_SCOPES = [SCOPE_READ, SCOPE_COMPARISONS_WRITE];
export const SCOPE_LABELS = {
  [SCOPE_READ]: '내 프로필(이름)과 내 옷장 목록 읽기',
  [SCOPE_COMPARISONS_WRITE]: '코디 비교 만들기와 내 선택(A·B·둘 다 아쉬움) 저장·수정',
};
/** ChatGPT 의 고정 콜백(공식 문서). 콜백별 주소는 관리 화면의 정확한 값을 OAUTH_REDIRECT_URIS 로 더한다. */
export const DEFAULT_REDIRECT_URIS = ['https://chatgpt.com/connector_platform_oauth_redirect'];

const CODE_TTL_MS = 5 * 60 * 1000;
const PENDING_TTL_MS = 10 * 60 * 1000;
const ACCESS_TTL_MS = 60 * 60 * 1000;
const REFRESH_TTL_MS = 30 * 24 * 60 * 60 * 1000;
const MAX_CONSENT_ATTEMPTS = 5;
const MAX_CLIENTS = 200;

/** 같은 resource 인가(끝의 / 하나와 #조각은 무시) — SDK bearerAuth 와 같은 비교. */
function sameResource(a, b) {
  const norm = (v) => String(v).split('#')[0].replace(/\/$/, '');
  return Boolean(a) && norm(a) === norm(b);
}

const escapeHtml = (s) =>
  String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

/**
 * @param {{ store: AuthStore, issuer: URL, resource: URL, redirectAllowlist: string[], linkCodesFile: string,
 *           users: { exists(userId: string): Promise<boolean> } }} deps
 */
export function createOAuthProvider({ store, issuer, resource, redirectAllowlist, linkCodesFile, users }) {
  const allowlist = new Set(redirectAllowlist);

  const clientsStore = {
    getClient(clientId) {
      return store.data.clients[clientId];
    },
    async registerClient(client) {
      const uris = client.redirect_uris ?? [];
      if (uris.length === 0 || uris.some((u) => !allowlist.has(String(u)))) {
        throw new InvalidClientMetadataError('redirect_uri 가 허용 목록에 없어요.');
      }
      const grants = client.grant_types ?? ['authorization_code', 'refresh_token'];
      if (grants.some((g) => !['authorization_code', 'refresh_token'].includes(g))) {
        throw new InvalidClientMetadataError('authorization_code·refresh_token 만 지원해요.');
      }
      const method = client.token_endpoint_auth_method ?? 'client_secret_basic';
      if (!['none', 'client_secret_post', 'client_secret_basic'].includes(method)) {
        throw new InvalidClientMetadataError('지원하지 않는 token_endpoint_auth_method 예요.');
      }
      if (Object.keys(store.data.clients).length >= MAX_CLIENTS) {
        throw new InvalidClientMetadataError('등록할 수 있는 클라이언트 수를 넘었어요.');
      }
      const full = {
        ...client,
        client_id: `wec_${randomToken(18)}`,
        client_id_issued_at: Math.floor(store.now() / 1000),
        grant_types: grants,
        response_types: ['code'],
      };
      await store.mutate((d) => {
        d.clients[full.client_id] = full;
      });
      return full;
    },
  };

  const issueTokens = (d, { clientId, userId, scopes, family }) => {
    const access = `wea_${randomToken()}`;
    const refresh = `wer_${randomToken()}`;
    const now = store.now();
    const base = { client_id: clientId, user_id: userId, scopes, resource: resource.href, family };
    d.access[sha256(access)] = { ...base, expires_at: now + ACCESS_TTL_MS };
    d.refresh[sha256(refresh)] = { ...base, expires_at: now + REFRESH_TTL_MS };
    return {
      access_token: access,
      token_type: 'bearer',
      expires_in: Math.floor(ACCESS_TTL_MS / 1000),
      refresh_token: refresh,
      scope: scopes.join(' '),
    };
  };

  return {
    clientsStore,

    async authorize(client, params, res) {
      // resource 는 반드시 이 서버(/mcp). 없거나 다르면 클라이언트로 오류를 돌려보낸다.
      if (!params.resource || !sameResource(params.resource, resource)) {
        throw new InvalidTargetError('resource must be this MCP server');
      }
      // scope 를 안 보내면 지원하는 전부를 요청한 것으로 본다(동의 화면에 하나하나 보여 준다).
      // 읽기 없이는 /mcp 에 들어올 수 없으므로 읽기는 늘 포함한다.
      const requested = params.scopes?.length ? params.scopes : SUPPORTED_SCOPES;
      if (requested.some((s) => !SUPPORTED_SCOPES.includes(s))) throw new InvalidScopeError('Unsupported scope');
      const scopes = SUPPORTED_SCOPES.filter((s) => s === SCOPE_READ || requested.includes(s));
      const pendingId = randomToken(32);
      await store.mutate((d) => {
        d.pending[sha256(pendingId)] = {
          client_id: client.client_id,
          redirect_uri: params.redirectUri,
          code_challenge: params.codeChallenge,
          state: params.state ?? null,
          scopes,
          resource: resource.href,
          attempts: 0,
          expires_at: store.now() + PENDING_TTL_MS,
        };
      });
      sendConsentPage(res, { pendingId, client, scopes, redirectUri: params.redirectUri });
    },

    async challengeForAuthorizationCode(client, code) {
      const key = sha256(code);
      // 폐기·소모 같은 상태 변화는 오류를 던지기 전에 저장해야 한다(저장소는 예외가 나면 되돌린다).
      return orThrow(
        await store.mutate((d) => {
          const entry = d.codes[key];
          if (!entry) {
            replayed(d, key);
            return fail(new InvalidGrantError('Authorization code is invalid or already used'));
          }
          if (entry.client_id !== client.client_id || entry.expires_at <= store.now()) {
            return fail(new InvalidGrantError('Authorization code is not valid for this client'));
          }
          if (entry.challenged) {
            // PKCE 검증은 한 번만 시도할 수 있다. 두 번째면 코드를 버린다.
            delete d.codes[key];
            return fail(new InvalidGrantError('Authorization code cannot be retried'));
          }
          entry.challenged = true;
          return { ok: entry.code_challenge };
        }),
      );
    },

    async exchangeAuthorizationCode(client, code, _verifier, redirectUri, requestResource) {
      const key = sha256(code);
      return orThrow(
        await store.mutate((d) => {
          const entry = d.codes[key];
          if (!entry || !entry.challenged) {
            replayed(d, key);
            return fail(new InvalidGrantError('Authorization code is invalid or already used'));
          }
          // 이 시점부터 코드는 끝났다(성공이든 실패든).
          delete d.codes[key];
          d.used_codes[key] = { family: entry.family, at: store.now() };
          if (entry.client_id !== client.client_id || entry.expires_at <= store.now()) {
            return fail(new InvalidGrantError('Authorization code is not valid for this client'));
          }
          if (!redirectUri || redirectUri !== entry.redirect_uri) {
            return fail(new InvalidGrantError('redirect_uri does not match the authorization request'));
          }
          if (requestResource && !sameResource(requestResource, entry.resource)) {
            return fail(new InvalidTargetError('resource does not match the authorization request'));
          }
          return { ok: issueTokens(d, { clientId: client.client_id, userId: entry.user_id, scopes: entry.scopes, family: entry.family }) };
        }),
      );
    },

    async exchangeRefreshToken(client, refreshToken, scopes, requestResource) {
      const key = sha256(refreshToken);
      return orThrow(
        await store.mutate((d) => {
          const entry = d.refresh[key];
          if (!entry) {
            const used = d.used_refresh[key];
            if (used) AuthStore.revokeFamily(d, used.family, store.now());
            return fail(new InvalidGrantError('Refresh token is invalid or already used'));
          }
          if (entry.client_id !== client.client_id || entry.expires_at <= store.now() || d.revoked_families[entry.family]) {
            return fail(new InvalidGrantError('Refresh token is not valid'));
          }
          if (requestResource && !sameResource(requestResource, entry.resource)) {
            return fail(new InvalidTargetError('resource does not match'));
          }
          const next = scopes?.length ? scopes : entry.scopes;
          if (next.some((s) => !entry.scopes.includes(s))) return fail(new InvalidScopeError('Cannot widen scope'));
          delete d.refresh[key];
          d.used_refresh[key] = { family: entry.family, at: store.now() };
          return { ok: issueTokens(d, { clientId: client.client_id, userId: entry.user_id, scopes: next, family: entry.family }) };
        }),
      );
    },

    async verifyAccessToken(token) {
      const entry = store.data.access[sha256(token)];
      if (!entry || entry.expires_at <= store.now() || store.data.revoked_families[entry.family]) {
        // 이 메시지는 WWW-Authenticate 헤더에 들어간다 — ASCII 만.
        throw new InvalidTokenError('The access token is invalid, expired, or revoked');
      }
      return {
        token,
        clientId: entry.client_id,
        scopes: entry.scopes,
        expiresAt: Math.floor(entry.expires_at / 1000),
        resource: new URL(entry.resource),
        extra: { userId: entry.user_id },
      };
    },

    async revokeToken(client, { token }) {
      const key = sha256(token);
      await store.mutate((d) => {
        const entry = d.access[key] ?? d.refresh[key];
        // RFC 7009: 모르는 토큰도 200. 다른 클라이언트의 토큰은 건드리지 않는다.
        if (entry && entry.client_id === client.client_id) AuthStore.revokeFamily(d, entry.family, store.now());
      });
    },

    /** 동의 화면 제출. 성공하면 redirect URL, 아니면 다시 그릴 화면 정보. */
    async completeConsent({ pendingId, linkCode, action }) {
      const pkey = sha256(String(pendingId ?? ''));
      const pending = store.data.pending[pkey];
      if (!pending || pending.expires_at <= store.now()) return { gone: true };
      const client = store.data.clients[pending.client_id];
      if (!client) return { gone: true };

      if (action === 'deny') {
        await store.mutate((d) => {
          delete d.pending[pkey];
        });
        return { redirect: errorRedirect(pending, 'access_denied', '사용자가 연결을 거절했어요.') };
      }

      const found = await findLinkCode({ file: linkCodesFile, input: linkCode, now: store.now() });
      const valid =
        found && !store.data.consumed_link_codes[found.hash] && (await users.exists(found.userId));
      if (!valid) {
        const left = await store.mutate((d) => {
          const p = d.pending[pkey];
          if (!p) return 0;
          p.attempts += 1;
          if (p.attempts >= MAX_CONSENT_ATTEMPTS) {
            delete d.pending[pkey];
            return 0;
          }
          return MAX_CONSENT_ATTEMPTS - p.attempts;
        });
        if (left === 0) return { gone: true, tooMany: true };
        return { retry: { pendingId, client, scopes: pending.scopes, redirectUri: pending.redirect_uri, left } };
      }

      const code = randomToken(32);
      await store.mutate((d) => {
        if (!d.pending[pkey]) throw new InvalidRequestError('이미 처리된 요청이에요.');
        if (d.consumed_link_codes[found.hash]) throw new InvalidRequestError('이미 쓴 연결 코드예요.');
        delete d.pending[pkey];
        d.consumed_link_codes[found.hash] = { user_id: found.userId, at: store.now() };
        d.codes[sha256(code)] = {
          client_id: pending.client_id,
          redirect_uri: pending.redirect_uri,
          code_challenge: pending.code_challenge,
          resource: pending.resource,
          scopes: pending.scopes,
          user_id: found.userId,
          family: randomToken(12),
          challenged: false,
          expires_at: store.now() + CODE_TTL_MS,
        };
      });
      const url = new URL(pending.redirect_uri);
      url.searchParams.set('code', code);
      if (pending.state !== null) url.searchParams.set('state', pending.state);
      // 메타데이터의 issuer(SDK: issuerUrl.href)와 정확히 같은 문자열.
      url.searchParams.set('iss', issuer.href);
      return { redirect: url.href };
    },
  };

  /** mutate 안에서 오류를 "돌려준다" — 그 사이의 상태 변화(폐기 등)는 저장된다. */
  function fail(error) {
    return { error };
  }
  function orThrow(result) {
    if (result.error) throw result.error;
    return result.ok;
  }

  function replayed(d, key) {
    const used = d.used_codes[key];
    if (used) AuthStore.revokeFamily(d, used.family, store.now());
  }

  function errorRedirect(pending, error, description) {
    const url = new URL(pending.redirect_uri);
    url.searchParams.set('error', error);
    url.searchParams.set('error_description', description);
    if (pending.state !== null) url.searchParams.set('state', pending.state);
    return url.href;
  }
}

/**
 * 동의 화면·오류 화면 헤더.
 *
 * - Referrer-Policy 는 `same-origin` 이다. `no-referrer` 이면 브라우저(Fetch 표준)가 같은 출처 폼 POST 의
 *   Origin 을 `null` 로 보내 /oauth/consent 의 Origin 검사(자기 origin 만 허용, null 거절)에 걸린다 — 실제
 *   ChatGPT 연결에서 403 이 난 원인. same-origin 은 같은 출처에만 Referer·Origin 을 싣고, 다른 출처
 *   (콜백 등)로는 Referer 를 보내지 않는다(동의 화면 주소의 쿼리가 새지 않는다).
 * - CSP form-action 은 POST 뒤의 302 리다이렉트에도 적용된다. 그래서 'self' 와 이 요청에 등록·검증된
 *   redirect_uri 의 origin 하나만 허용한다(와일드카드 없음). 폼이 없는 화면은 'none'.
 */
function pageHeaders(formRedirectOrigin = null) {
  const formAction = formRedirectOrigin ? `'self' ${formRedirectOrigin}` : "'none'";
  return {
    'Content-Type': 'text/html; charset=utf-8',
    'Cache-Control': 'no-store',
    'Content-Security-Policy': `default-src 'none'; style-src 'unsafe-inline'; form-action ${formAction}; frame-ancestors 'none'; base-uri 'none'`,
    'X-Frame-Options': 'DENY',
    'Referrer-Policy': 'same-origin',
    'X-Content-Type-Options': 'nosniff',
  };
}
const PAGE_HEADERS = pageHeaders();

function page(title, body) {
  return `<!doctype html><html lang="ko"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>${escapeHtml(title)}</title><style>
body{font-family:-apple-system,"Apple SD Gothic Neo","Noto Sans KR",sans-serif;background:#f4f1eb;color:#24221f;margin:0;padding:24px}
main{max-width:440px;margin:0 auto;background:#fbfaf7;border:1px solid #d9d2c8;border-radius:10px;padding:20px}
h1{font-size:1.1rem;margin:0 0 12px}p,li{font-size:.9rem;line-height:1.6}.fine{font-size:.78rem;color:#6e675d}
input{width:100%;box-sizing:border-box;font:inherit;font-size:1.05rem;letter-spacing:.08em;padding:10px;border:1px solid #d9d2c8;border-radius:6px}
.row{display:flex;gap:8px;margin-top:14px}button{font:inherit;padding:10px 16px;border-radius:4px;border:1px solid #d9d2c8;background:#fbfaf7;cursor:pointer}
.primary{background:#24221f;color:#f4f1eb;border-color:#24221f}.err{color:#a3473f;background:#f0dedb;padding:8px 10px;border-radius:6px}
</style></head><body><main>${body}</main></body></html>`;
}

function sendConsentPage(res, { pendingId, client, scopes, redirectUri, error = null }) {
  const host = new URL(redirectUri).host;
  const name = client.client_name ? escapeHtml(client.client_name) : '외부 앱';
  const body = `<h1>Wardrobe Edit 계정 연결</h1>
<p><strong>${name}</strong>(${escapeHtml(host)})이 내 Wardrobe Edit 계정에서 다음을 하려고 해요.</p>
<ul>${scopes.map((s) => `<li>${escapeHtml(SCOPE_LABELS[s] ?? s)}</li>`).join('')}</ul>
<p>옷장의 옷 등록·수정·삭제, 사진 생성, 유료 추천은 하지 않아요.</p>
${error ? `<p class="err" role="alert">${escapeHtml(error)}</p>` : ''}
<form method="post" action="/oauth/consent" autocomplete="off">
<input type="hidden" name="request" value="${escapeHtml(pendingId)}">
<label for="code">계정 연결 코드</label>
<input id="code" name="link_code" inputmode="text" autocapitalize="characters" spellcheck="false" maxlength="40" placeholder="XXXX-XXXX-XXXX-XXXX-XXXX" required>
<p class="fine">서버를 운영하는 컴퓨터에서 <code>npm run link-code</code>로 받은 10분짜리 일회용 코드예요.</p>
<div class="row"><button class="primary" type="submit" name="action" value="allow">연결</button>
<button type="submit" name="action" value="deny" formnovalidate>거절</button></div>
</form>`;
  res
    .status(error ? 400 : 200)
    .set(pageHeaders(new URL(redirectUri).origin))
    .send(page('Wardrobe Edit 계정 연결', body));
}

/**
 * 인증이 붙은 express 앱. /mcp 는 Bearer 검사를 통과해야 onMcp 로 간다(req.auth 에 토큰 정보).
 * @param {{ provider: ReturnType<typeof createOAuthProvider>, issuer: URL, resource: URL, ownOrigin: string,
 *           onMcp: (req, res) => void }} deps
 */
export function createAuthApp({ provider, issuer, resource, ownOrigin, onMcp }) {
  const app = express();
  app.disable('x-powered-by');
  app.set('trust proxy', false);

  app.use(
    mcpAuthRouter({
      provider,
      issuerUrl: issuer,
      resourceServerUrl: resource,
      scopesSupported: SUPPORTED_SCOPES,
      resourceName: 'Wardrobe Edit',
    }),
  );
  // SDK 는 /.well-known/oauth-protected-resource/mcp 에 둔다. OpenAI 문서의 접미사 없는 경로도 같은 내용으로.
  app.get('/.well-known/oauth-protected-resource', (req, res) => {
    res.set('Cache-Control', 'no-store').json({
      resource: resource.href,
      authorization_servers: [issuer.href],
      scopes_supported: SUPPORTED_SCOPES,
      resource_name: 'Wardrobe Edit',
    });
  });

  app.post('/oauth/consent', express.urlencoded({ extended: false, limit: '4kb' }), async (req, res) => {
    const origin = req.headers.origin;
    if (origin !== undefined && origin !== ownOrigin) {
      return res.status(403).set(PAGE_HEADERS).send(page('거절됨', '<p class="err">다른 사이트에서 보낸 요청이에요.</p>'));
    }
    try {
      const out = await provider.completeConsent({
        pendingId: req.body?.request,
        linkCode: req.body?.link_code,
        action: req.body?.action,
      });
      if (out.redirect) return res.redirect(302, out.redirect);
      if (out.retry) {
        return sendConsentPage(res, {
          ...out.retry,
          error: `코드가 맞지 않거나 만료·사용됐어요. ${out.retry.left}번 더 시도할 수 있어요.`,
        });
      }
      return res
        .status(400)
        .set(PAGE_HEADERS)
        .send(
          page(
            '다시 시작해 주세요',
            `<p class="err">${out.tooMany ? '시도 횟수를 넘었어요.' : '연결 요청이 만료됐거나 이미 처리됐어요.'} ChatGPT 에서 연결을 처음부터 다시 시작해 주세요.</p>`,
          ),
        );
    } catch {
      return res.status(400).set(PAGE_HEADERS).send(page('오류', '<p class="err">요청을 처리하지 못했어요. 처음부터 다시 시작해 주세요.</p>'));
    }
  });

  const bearer = requireBearerAuth({
    verifier: provider,
    requiredScopes: [SCOPE_READ],
    resourceMetadataUrl: getOAuthProtectedResourceMetadataUrl(resource),
    expectedResource: resource,
  });
  app.all('/mcp', bearer, (req, res) => onMcp(req, res));
  app.use((req, res) => res.status(404).json({ error: 'not_found' }));
  return app;
}

/** 인증 경로인가(이 경로들은 express 앱이 처리한다). */
export function isAuthPath(pathname) {
  return (
    pathname.startsWith('/.well-known/oauth-') ||
    pathname.startsWith('/.well-known/openid-configuration') ||
    ['/authorize', '/token', '/register', '/revoke', '/oauth/consent'].includes(pathname)
  );
}
