// POST /register(DCR) 진단 로그. MCP_ACCESS_LOG=1 일 때만 붙는다. 허용 판정은 바꾸지 않고 결과만 적는다.
//
// 거절은 세 곳에서 나온다: 본문 해석(express.json), SDK 스키마 검증(registerClient 앞), registerClient 규칙.
// 앞의 둘은 registerClient 까지 오지 않아 응답을 보고 가른다.
//
// 남기는 것: 상태, 거절 이유, 스키마가 거절한 필드 경로와 코드, redirect_uri 의 scheme·host·path 와 허용 여부.
// 남기지 않는 것: 쿼리·조각·사용자 정보, client_secret·client_id·토큰, 본문, 스키마 오류 문장, 응답 본문.

import express from 'express';

/** registerClient 가 돌려주는 문장. 진단 로그가 이유를 가르는 데도 쓴다. */
export const DCR_REJECTIONS = {
  redirect_uri_not_allowed: 'redirect_uri 가 허용 목록에 없어요.',
  grant_type_not_supported: 'authorization_code·refresh_token 만 지원해요.',
  auth_method_not_supported: '지원하지 않는 token_endpoint_auth_method 예요.',
  client_limit_reached: '등록할 수 있는 클라이언트 수를 넘었어요.',
  body_not_json: 'Request body is not valid JSON',
};

const MAX_URIS = 5;
const MAX_URI_CHARS = 200;
const TOKEN = /^[A-Za-z0-9_.:/+-]{1,80}$/;

const token = (v) => (typeof v === 'string' && TOKEN.test(v) ? v : '(other)');

/** redirect_uri 를 로그에 적을 모양으로. 정확히 일치해야 하므로 지운 부분이 있으면 표시만 남긴다. */
export function describeRedirectUri(raw, allowlist) {
  const allowed = allowlist.has(String(raw)) ? 'allowed' : 'not_allowed';
  let url;
  try {
    url = new URL(String(raw));
  } catch {
    return `(invalid-url)(${allowed})`;
  }
  const flags = [
    allowed,
    url.search ? 'has_query' : null,
    url.hash ? 'has_fragment' : null,
    url.username || url.password ? 'has_credentials' : null,
  ].filter(Boolean);
  return `${url.protocol}//${url.host}${url.pathname}`.slice(0, MAX_URI_CHARS) + `(${flags.join(',')})`;
}

/** SDK 스키마 오류 문장(zod 오류 JSON)에서 필드 경로와 코드만 꺼낸다. 문장은 버린다. */
export function validationIssues(description) {
  let issues;
  try {
    issues = JSON.parse(description);
  } catch {
    return null;
  }
  if (!Array.isArray(issues)) return null;
  return issues
    .slice(0, 10)
    .map((i) => {
      const path = (Array.isArray(i?.path) ? i.path : []).map((p) => token(String(p))).join('.') || '(body)';
      return `${path}:${token(i?.code)}`;
    })
    .join(',');
}

function reasonOf(status, reply) {
  if (status < 400) return { reason: 'registered' };
  const description = reply?.error_description;
  const known = Object.entries(DCR_REJECTIONS).find(([, text]) => text === description);
  if (known) return { reason: known[0] };
  const issues = typeof description === 'string' ? validationIssues(description) : null;
  if (issues !== null) return { reason: 'sdk_validation', issues };
  return { reason: 'other' };
}

/** 로그 한 줄. req.body 는 express.json 이 읽은 값이며 여기 적힌 필드만 쓴다. */
export function dcrLine({ status, reply, body, contentType, allowlist }) {
  const { reason, issues } = reasonOf(status, reply);
  const uris = Array.isArray(body?.redirect_uris) ? body.redirect_uris : [];
  const media = String(contentType ?? '').split(';')[0].trim();
  return [
    `dcr status=${status}`,
    `error=${reply?.error ? token(reply.error) : '-'}`,
    `reason=${reason}`,
    issues ? `issues=${issues}` : null,
    `content_type=${media ? token(media) : '-'}`,
    `auth_method=${body?.token_endpoint_auth_method === undefined ? '-' : token(body.token_endpoint_auth_method)}`,
    `grant_types=${Array.isArray(body?.grant_types) ? body.grant_types.slice(0, 5).map(token).join(',') : '-'}`,
    `redirect_uris=${uris.length ? uris.slice(0, MAX_URIS).map((u) => describeRedirectUri(u, allowlist)).join(',') : '-'}`,
    uris.length > MAX_URIS ? `redirect_uri_count=${uris.length}` : null,
  ]
    .filter(Boolean)
    .join(' ');
}

/** `/register` 앞에 두는 미들웨어. 깨진 본문은 스택을 내보내지 않고 400 으로 끝낸다. */
export function dcrDiagnostic({ allowlist, log }) {
  const parse = express.json();
  return (req, res, next) => {
    if (req.method !== 'POST') return next();
    let reply = null;
    const json = res.json.bind(res);
    res.json = (value) => {
      reply = value;
      return json(value);
    };
    res.on('finish', () => {
      log(dcrLine({ status: res.statusCode, reply, body: req.body, contentType: req.headers['content-type'], allowlist }));
    });
    parse(req, res, (err) => {
      if (!err) return next();
      res.status(err.status ?? 400).json({ error: 'invalid_client_metadata', error_description: DCR_REJECTIONS.body_not_json });
    });
  };
}
