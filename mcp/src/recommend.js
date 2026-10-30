// live 모드의 실제 코디 추천 — Rust 서버의 `/api/recommendation/multi` 를 연결된 계정 대신 부른다.
//
// 추천 엔진을 여기서 다시 만들지 않는다. 날씨·무드·이력·점수는 모두 Rust 가 한다. 이 모듈은:
//   - 고정된 루프백 주소(http://127.0.0.1:<포트>)만 부른다. 주소는 설정에서만 만들고 요청·도구 인자로 바꿀 수 없다.
//     리다이렉트는 따라가지 않는다(redirect: 'error').
//   - 사용자 신원은 OAuth 로 검증된 주체(ownerOf)를 공유 키로 서명한 위임 헤더로만 넘긴다
//     (Rust `middleware::delegation`, 같은 형식). 서명은 요청 직전에 만들고 60초 뒤 만료된다.
//     Rust 의 api_token 을 읽거나 보내지 않는다.
//   - 한 계정은 한 번에 하나, 전체는 maxConcurrent 개까지. 넘으면 바로 'busy' 로 돌려준다(줄 세우지 않는다).
//   - 시간 제한(timeoutMs)을 넘기면 끊고 'timeout'. 자동으로 다시 부르지 않는다 — 한 번 부를 때마다 모델 비용과
//     추천 이력이 생긴다.
// 위임 키·서명 헤더는 로그·결과에 남기지 않는다.

import { createHmac } from 'node:crypto';

export const RECOMMEND_PATH = '/api/recommendation/multi';
const SIGN_TTL_SECONDS = 60;

/** Rust `middleware::delegation::sign` 과 같은 형식. */
export function signDelegation(key, method, path, userId, exp) {
  const msg = `wardrobe-delegation/v1\n${method}\n${path}\n${userId}\n${exp}`;
  const sig = createHmac('sha256', key).update(msg).digest('hex');
  return `v1.${Buffer.from(userId, 'utf8').toString('base64url')}.${exp}.${sig}`;
}

/**
 * Rust 응답 결과 분류.
 * @typedef {{ ok: true, body: any } | { ok: false, outcome: string, status?: number, message?: string }} UpstreamResult
 */
export class RecommendClient {
  #key;
  #inFlight = new Set();

  /**
   * @param {{ port: number, key: Buffer, timeoutMs?: number, maxConcurrent?: number, now?: () => number, fetchImpl?: typeof fetch }} opts
   */
  constructor({ port, key, timeoutMs = 90_000, maxConcurrent = 2, now = Date.now, fetchImpl = fetch }) {
    if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error('추천 포트가 이상해요');
    if (!Buffer.isBuffer(key) || key.length < 32) throw new Error('위임 키는 32바이트 이상이어야 해요');
    this.url = `http://127.0.0.1:${port}${RECOMMEND_PATH}`;
    this.#key = key;
    this.timeoutMs = timeoutMs;
    this.maxConcurrent = maxConcurrent;
    this.now = now;
    this.fetchImpl = fetchImpl;
  }

  /**
   * @param {string} userId OAuth 로 검증된 주체
   * @param {{ occasion: string, gender?: string, style_mood?: string }} body
   * @returns {Promise<UpstreamResult>}
   */
  async recommend(userId, body) {
    if (this.#inFlight.has(userId)) {
      return { ok: false, outcome: 'busy', message: '이 계정의 추천이 이미 만들어지는 중이에요. 끝난 뒤에 다시 요청해 주세요.' };
    }
    if (this.#inFlight.size >= this.maxConcurrent) {
      return { ok: false, outcome: 'busy', message: '지금 추천 요청이 몰려 있어요. 잠시 뒤에 다시 요청해 주세요.' };
    }
    this.#inFlight.add(userId);
    try {
      const exp = Math.floor(this.now() / 1000) + SIGN_TTL_SECONDS;
      let res;
      try {
        res = await this.fetchImpl(this.url, {
          method: 'POST',
          redirect: 'error',
          signal: AbortSignal.timeout(this.timeoutMs),
          headers: {
            'content-type': 'application/json',
            accept: 'application/json',
            'x-wardrobe-delegation': signDelegation(this.#key, 'POST', RECOMMEND_PATH, userId, exp),
          },
          body: JSON.stringify(body),
        });
      } catch (e) {
        if (e?.name === 'TimeoutError' || e?.name === 'AbortError') return { ok: false, outcome: 'timeout' };
        // redirect: 'error' 로 막힌 리다이렉트는 서버가 꺼진 것이 아니다.
        if (/redirect/i.test(String(e?.cause?.message ?? e?.message ?? ''))) return { ok: false, outcome: 'upstream_error' };
        return { ok: false, outcome: 'unavailable' };
      }
      let json = null;
      try {
        json = await res.json();
      } catch (e) {
        if (e?.name === 'TimeoutError' || e?.name === 'AbortError') return { ok: false, outcome: 'timeout' };
        /* 본문이 JSON 이 아니면 아래에서 upstream_error */
      }
      if (res.ok) {
        return json && typeof json === 'object' && Array.isArray(json.modes)
          ? { ok: true, body: json }
          : { ok: false, outcome: 'upstream_error', status: res.status };
      }
      // 오류 본문은 분류에만 쓰고 바깥으로 넘기지 않는다(외부 API 오류에 접속·설정 정보가 섞일 수 있다).
      const errorText = typeof json?.error === 'string' ? json.error : '';
      if (res.status === 404 && /region/i.test(errorText)) return { ok: false, outcome: 'no_region', status: 404 };
      if (res.status === 401 || res.status === 403) return { ok: false, outcome: 'delegation_rejected', status: res.status };
      if (res.status === 400) return { ok: false, outcome: 'invalid_request', status: 400 };
      return { ok: false, outcome: 'upstream_error', status: res.status };
    } finally {
      this.#inFlight.delete(userId);
    }
  }
}
