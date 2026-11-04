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
// 코디 흐름(바꾸기·계획·착용·기록)도 같은 위임으로 부른다. 경로는 아래 OUTFIT_PATHS 의 고정 값뿐이고 호출자가 경로를 넘길 수
// 없다. id 는 본문에 실리며 소유자 확인은 Rust 가 서명된 사용자로 한다. 모델을 부르지 않으므로 짧게 끊고(15초), 계정당·전체 동시
// 호출 수를 제한한다. 이것도 자동으로 다시 부르지 않는다.
// 위임 키·서명 헤더는 로그·결과에 남기지 않는다.

import { createHmac } from 'node:crypto';

export const RECOMMEND_PATH = '/api/recommendation/multi';
/** 위임으로 부르는 코디 흐름 경로. Rust `middleware::delegation::ALLOWED` 와 같은 고정 값이다. */
export const OUTFIT_PATHS = Object.freeze({
  replacement: '/api/outfits/replacement',
  plan: '/api/outfits/plan',
  worn: '/api/outfits/worn',
  record: '/api/outfits/record',
  history: '/api/outfits/history',
});
const SIGN_TTL_SECONDS = 60;
const OUTFIT_TIMEOUT_MS = 15_000;
const OUTFIT_MAX_PER_USER = 2;
const OUTFIT_MAX_TOTAL = 8;

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
  #outfitInFlight = new Map();

  /**
   * @param {{ port: number, key: Buffer, timeoutMs?: number, maxConcurrent?: number, now?: () => number, fetchImpl?: typeof fetch }} opts
   */
  constructor({ port, key, timeoutMs = 90_000, maxConcurrent = 2, now = Date.now, fetchImpl = fetch }) {
    if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error('추천 포트가 이상해요');
    if (!Buffer.isBuffer(key) || key.length < 32) throw new Error('위임 키는 32바이트 이상이어야 해요');
    this.origin = `http://127.0.0.1:${port}`;
    this.url = `${this.origin}${RECOMMEND_PATH}`;
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
      const sent = await this.#post(this.url, RECOMMEND_PATH, userId, body, this.timeoutMs);
      if (!sent.res) return sent;
      const { res, json } = sent;
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

  /**
   * 코디 흐름 호출. action 은 OUTFIT_PATHS 의 키만 받는다.
   * Rust 의 오류는 code(짧은 영문 식별자)만 꺼낸다 — 문장은 호출부의 고정 안내를 쓴다.
   * @param {keyof typeof OUTFIT_PATHS} action
   * @param {string} userId OAuth 로 검증된 주체
   * @param {object} body
   * @returns {Promise<UpstreamResult>}
   */
  async outfit(action, userId, body) {
    if (!Object.hasOwn(OUTFIT_PATHS, action)) throw new Error('알 수 없는 코디 호출');
    const mine = this.#outfitInFlight.get(userId) ?? 0;
    const total = [...this.#outfitInFlight.values()].reduce((a, b) => a + b, 0);
    if (mine >= OUTFIT_MAX_PER_USER || total >= OUTFIT_MAX_TOTAL) return { ok: false, outcome: 'busy' };
    this.#outfitInFlight.set(userId, mine + 1);
    try {
      const path = OUTFIT_PATHS[action];
      const sent = await this.#post(`${this.origin}${path}`, path, userId, body, OUTFIT_TIMEOUT_MS);
      if (!sent.res) return sent;
      const { res, json } = sent;
      if (res.ok) {
        return json && typeof json === 'object' ? { ok: true, body: json } : { ok: false, outcome: 'upstream_error', status: res.status };
      }
      if (res.status === 401 || res.status === 403) return { ok: false, outcome: 'delegation_rejected', status: res.status };
      const code = typeof json?.code === 'string' && /^[a-z_]{1,40}$/.test(json.code) ? json.code : null;
      const currentRevision = Number.isInteger(json?.current_revision) ? json.current_revision : undefined;
      if (code) return { ok: false, outcome: code, status: res.status, current_revision: currentRevision };
      return { ok: false, outcome: res.status === 422 ? 'invalid_request' : 'upstream_error', status: res.status };
    } finally {
      const left = (this.#outfitInFlight.get(userId) ?? 1) - 1;
      if (left > 0) this.#outfitInFlight.set(userId, left);
      else this.#outfitInFlight.delete(userId);
    }
  }

  /** 고정 경로 하나를 서명해서 부른다. 경로는 이 파일의 상수에서만 온다. */
  async #post(url, path, userId, body, timeoutMs) {
    const exp = Math.floor(this.now() / 1000) + SIGN_TTL_SECONDS;
    let res;
    try {
      res = await this.fetchImpl(url, {
        method: 'POST',
        redirect: 'error',
        signal: AbortSignal.timeout(timeoutMs),
        headers: {
          'content-type': 'application/json',
          accept: 'application/json',
          'x-wardrobe-delegation': signDelegation(this.#key, 'POST', path, userId, exp),
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
      /* 본문이 JSON 이 아니면 호출부에서 upstream_error */
    }
    return { res, json };
  }
}
