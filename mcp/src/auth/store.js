// OAuth 상태 저장소(로컬 JSON 파일 하나, 기본 mcp/.data/auth-store.json — gitignore).
//
// 비밀값(인가 코드·액세스/리프레시 토큰·연결 코드)은 원문을 저장하지 않는다 — sha256 해시만 키로 둔다.
// 파일이 새어 나가도 토큰을 쓸 수 없다. 클라이언트 등록(DCR)은 공개 정보라 그대로 둔다.
//
// 형식(version 1):
//   clients   { client_id: OAuthClientInformationFull + registered_at }
//   pending   { id: { client_id, redirect_uri, code_challenge, state, scopes, resource, expires_at, attempts } }
//             동의 화면 한 번. 원문 id 는 화면의 숨은 값이고(일회용), 여기도 해시로 둔다.
//   codes     { sha(code): { client_id, redirect_uri, code_challenge, resource, scopes, user_id, family, expires_at, challenged } }
//   used_codes{ sha(code): { family, at } }      이미 바꾼 코드 — 다시 오면 그 토큰 가족을 폐기한다.
//   access    { sha(token): { client_id, user_id, scopes, resource, family, expires_at } }
//   refresh   { sha(token): { ...같음 } }
//   used_refresh { sha(token): { family, at } }  회전된 리프레시 — 다시 오면 가족 폐기.
//   revoked_families { family: at }
//   consumed_link_codes { sha(code): { user_id, at } }
//
// 한 프로세스만 쓴다고 가정한다. 프로세스 안의 동시 요청은 `#queue` 로 한 줄로 세운다.

import { createHash, randomBytes } from 'node:crypto';
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';

export const sha256 = (value) => createHash('sha256').update(String(value)).digest('hex');
export const randomToken = (bytes = 32) => randomBytes(bytes).toString('base64url');

const EMPTY = () => ({
  version: 1,
  clients: {},
  pending: {},
  codes: {},
  used_codes: {},
  access: {},
  refresh: {},
  used_refresh: {},
  revoked_families: {},
  consumed_link_codes: {},
});

const DAY = 24 * 60 * 60 * 1000;

export class AuthStore {
  #file;
  #now;
  #data = EMPTY();
  #queue = Promise.resolve();

  constructor({ file, now = () => Date.now() }) {
    this.#file = file;
    this.#now = now;
  }

  now() {
    return this.#now();
  }

  async load() {
    if (!this.#file) return this;
    let raw;
    try {
      raw = await readFile(this.#file, 'utf8');
    } catch (e) {
      if (e.code === 'ENOENT') return this;
      throw e;
    }
    const parsed = JSON.parse(raw);
    if (parsed?.version !== 1) throw new Error(`인증 저장 파일 형식을 알 수 없어요: ${this.#file}`);
    this.#data = { ...EMPTY(), ...parsed };
    return this;
  }

  /** 읽기 전용 조회(동기). */
  get data() {
    return this.#data;
  }

  /** 쓰기를 한 줄로 세운다. 실패하면 메모리 변경을 되돌린다. */
  mutate(fn) {
    const run = this.#queue.then(async () => {
      const before = structuredClone(this.#data);
      try {
        const result = fn(this.#data);
        this.#prune();
        await this.#persist();
        return result;
      } catch (e) {
        this.#data = before;
        throw e;
      }
    });
    this.#queue = run.catch(() => {});
    return run;
  }

  async #persist() {
    if (!this.#file) return;
    await mkdir(dirname(this.#file), { recursive: true });
    const tmp = `${this.#file}.${process.pid}.tmp`;
    await writeFile(tmp, JSON.stringify(this.#data, null, 2), { mode: 0o600 });
    await rename(tmp, this.#file);
  }

  /** 만료된 항목 정리. 재사용 탐지용 기록은 30일 남긴다. */
  #prune() {
    const now = this.#now();
    for (const table of ['pending', 'codes', 'access', 'refresh']) {
      for (const [k, v] of Object.entries(this.#data[table])) {
        if (v.expires_at <= now) delete this.#data[table][k];
      }
    }
    for (const table of ['used_codes', 'used_refresh', 'revoked_families', 'consumed_link_codes']) {
      for (const [k, v] of Object.entries(this.#data[table])) {
        const at = typeof v === 'number' ? v : v.at;
        if (at + 30 * DAY <= now) delete this.#data[table][k];
      }
    }
  }

  /** 토큰 가족 하나(같은 인가에서 나온 액세스·리프레시)를 모두 무효화한다. mutate 안에서 부른다. */
  static revokeFamily(data, family, now) {
    data.revoked_families[family] = now;
    for (const table of ['access', 'refresh']) {
      for (const [k, v] of Object.entries(data[table])) {
        if (v.family === family) delete data[table][k];
      }
    }
  }
}
