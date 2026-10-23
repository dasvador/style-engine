// 데모 비교·선택 저장소. 실제 DB 가 아니라 로컬 JSON 파일 하나(기본 mcp/.data/demo-store.json)다.
//
// 형식(version 1):
//   { "version": 1, "comparisons": { "<id>": StoredComparison } }
//   StoredComparison = { id, created_at, expires_at, situation, mood, weather,
//                        a: { outfit_index, item_key, items }, b: {...}, vote: StoredVote | null }
//   StoredVote = { choice, reasons, comment, reason_polarity, chosen_item_key, submitted_at }
// 수명: 만든 뒤 ttlMs(기본 7일)가 지나면 읽을 때·쓸 때 지운다. 개수는 maxComparisons(기본 500)
// 를 넘으면 오래된 것부터 지운다. 파일을 지우면 데모 기록이 모두 사라진다.
//
// 한 프로세스만 이 파일을 쓴다고 가정한다. 프로세스 안의 동시 요청은 아래 `#queue` 가 한 줄로
// 세워 처리하므로 같은 비교에 응답이 둘 남지 않는다(먼저 쓴 것이 남는다).

import { randomBytes } from 'node:crypto';
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';

import { CATEGORIES, MOODS, SAMPLE_PAIRS, SAMPLE_WARDROBE, SITUATIONS, itemKey, snapshotItems } from './fixtures.js';
import { reasonPolarity, sameVote, validateVote } from './vote.js';

/** 비교 id 모양. 서버가 만든 값만 받는다 — 다른 모양은 저장소를 뒤지지 않고 없는 것으로 친다. */
export const COMPARISON_ID_PATTERN = /^demo_cmp_[0-9a-f]{32}$/;

/** 기존 Rust 비교의 REPRESENTATION 과 같은 자리. 데모 fixture 로 만든 비교임을 남긴다. */
export const REPRESENTATION = 'demo_fixture_garment_text_and_color';

export const DEMO_NOTICE =
  '데모 데이터예요. 샘플 옷장으로 미리 짜 둔 고정 코디이며, 실제 옷장이나 AI 추천 결과가 아니에요.';

const DAY_MS = 24 * 60 * 60 * 1000;

export class DemoStore {
  #file;
  #ttlMs;
  #max;
  #now;
  #random;
  #data = { version: 1, comparisons: {} };
  #queue = Promise.resolve();

  /**
   * @param {{ file: string | null, ttlMs?: number, maxComparisons?: number,
   *           now?: () => Date, randomBool?: () => boolean }} opts
   *   file 이 null 이면 메모리에만 둔다(테스트용).
   */
  constructor({ file, ttlMs = 7 * DAY_MS, maxComparisons = 500, now = () => new Date(), randomBool } = {}) {
    this.#file = file;
    this.#ttlMs = ttlMs;
    this.#max = maxComparisons;
    this.#now = now;
    this.#random = randomBool ?? (() => (randomBytes(1)[0] & 1) === 0);
  }

  /** 파일을 읽는다. 없으면 빈 저장소로 시작한다. 깨진 파일은 덮어쓰지 않고 오류로 멈춘다. */
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
    if (parsed?.version !== 1 || typeof parsed.comparisons !== 'object' || parsed.comparisons === null) {
      throw new Error(`데모 저장 파일 형식을 알 수 없어요: ${this.#file}`);
    }
    this.#data = parsed;
    return this;
  }

  /** 쓰기를 한 줄로 세운다. 파일 저장이 실패하면 메모리 변경도 되돌린다. */
  #exclusive(fn) {
    const run = this.#queue.then(async () => {
      const before = structuredClone(this.#data);
      try {
        const { result, dirty } = fn();
        if (dirty) await this.#persist();
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

  /** 만료·초과분 정리. 지운 것이 있으면 true. */
  #prune() {
    const now = this.#now().getTime();
    const entries = Object.values(this.#data.comparisons);
    let removed = false;
    for (const c of entries) {
      if (Date.parse(c.expires_at) <= now) {
        delete this.#data.comparisons[c.id];
        removed = true;
      }
    }
    const alive = Object.values(this.#data.comparisons).sort((x, y) => x.created_at.localeCompare(y.created_at));
    while (alive.length >= this.#max) {
      delete this.#data.comparisons[alive.shift().id];
      removed = true;
    }
    return removed;
  }

  #live(id) {
    if (typeof id !== 'string' || !COMPARISON_ID_PATTERN.test(id)) return null;
    const c = this.#data.comparisons[id];
    if (!c || Date.parse(c.expires_at) <= this.#now().getTime()) return null;
    return c;
  }

  // ─── 옷장 ───

  listWardrobe(category) {
    const items = category ? SAMPLE_WARDROBE.filter((i) => i.category === category) : SAMPLE_WARDROBE;
    return {
      demo: true,
      data_source: 'sample_fixture',
      notice: '데모용 샘플 옷장이에요. 실제 사용자 옷장이 아니에요.',
      categories: CATEGORIES,
      items: items.map((i) => ({ ...i })),
    };
  }

  // ─── 비교 ───

  /** 상황·무드의 샘플 두 코디로 새 비교를 만든다. 좌우는 여기서 한 번 무작위로 정한다. */
  createComparison({ situation, mood }) {
    const pair = SAMPLE_PAIRS[`${situation}:${mood}`];
    if (!pair) return Promise.reject(new StoreError('unknown_scenario', '지원하지 않는 상황·무드 조합이에요.'));
    return this.#exclusive(() => {
      this.#prune();
      const now = this.#now();
      const id = `demo_cmp_${randomBytes(16).toString('hex')}`;
      const order = this.#random() ? [0, 1] : [1, 0];
      const side = (idx) => ({
        outfit_index: idx,
        item_key: itemKey(pair.outfits[idx]),
        items: snapshotItems(pair.outfits[idx]),
      });
      const stored = {
        id,
        created_at: now.toISOString(),
        expires_at: new Date(now.getTime() + this.#ttlMs).toISOString(),
        situation,
        mood,
        weather: { ...pair.weather },
        representation: REPRESENTATION,
        a: side(order[0]),
        b: side(order[1]),
        vote: null,
      };
      this.#data.comparisons[id] = stored;
      return { result: toView(stored), dirty: true };
    });
  }

  /** 비교 하나. 없거나 만료됐거나 id 모양이 다르면 null. */
  getComparison(id) {
    const c = this.#live(id);
    return c ? toView(c) : null;
  }

  /**
   * 선택을 남긴다. 비교 하나에 한 번이고 먼저 쓴 것이 남는다.
   * @returns {Promise<{ outcome: 'saved' | 'already_submitted', comparison: object }>}
   * @throws {StoreError} not_found · conflict · invalid_vote
   */
  submitVote(id, rawVote) {
    let vote;
    try {
      vote = validateVote(rawVote);
    } catch (e) {
      return Promise.reject(new StoreError('invalid_vote', e.message));
    }
    return this.#exclusive(() => {
      const c = this.#live(id);
      if (!c) throw new StoreError('not_found', '이 비교를 찾을 수 없어요. 만료됐거나 다른 비교 id 예요.');
      if (c.vote) {
        if (sameVote(c.vote, vote)) return { result: { outcome: 'already_submitted', comparison: toView(c) }, dirty: false };
        throw new StoreError('conflict', '이 비교에는 이미 다른 선택이 저장돼 있어요. 처음 고른 것이 남아요.', toView(c));
      }
      // 고른 코디의 정체는 저장된 노출에서 읽는다. 호출자가 보낸 아이템 이름·id 는 받지 않는다.
      const chosen = vote.choice === 'a' ? c.a : vote.choice === 'b' ? c.b : null;
      c.vote = {
        ...vote,
        reason_polarity: reasonPolarity(vote.choice),
        chosen_item_key: chosen ? chosen.item_key : null,
        submitted_at: this.#now().toISOString(),
      };
      return { result: { outcome: 'saved', comparison: toView(c) }, dirty: true };
    });
  }

  /** 테스트·점검용: 지금 남아 있는 비교 수. */
  get size() {
    return Object.keys(this.#data.comparisons).length;
  }
}

export class StoreError extends Error {
  /** @param {'not_found'|'conflict'|'invalid_vote'|'unknown_scenario'} code */
  constructor(code, message, comparison) {
    super(message);
    this.code = code;
    this.comparison = comparison;
  }
}

/**
 * 화면·모델에 내려 주는 비교. 기존 Rust `ComparisonView`(id·created_at·context·options·vote)와
 * 같은 모양에 데모 표시(demo·data_source·notice)와 상황 이름을 더했다. 원래 코디 순서·내부 키는
 * 넣지 않는다 — 고르는 데 영향을 주거나 쓸모없는 정보다.
 */
export function toView(c) {
  return {
    demo: true,
    data_source: 'sample_fixture',
    notice: DEMO_NOTICE,
    id: c.id,
    created_at: c.created_at,
    expires_at: c.expires_at,
    context: {
      situation: c.situation,
      situation_label: SITUATIONS[c.situation],
      mood_key: c.mood,
      mood_label: MOODS[c.mood],
      temperature: c.weather.temperature,
      weather_description: c.weather.weather_description,
    },
    options: [
      { slot: 'a', items: c.a.items.map((i) => ({ ...i })) },
      { slot: 'b', items: c.b.items.map((i) => ({ ...i })) },
    ],
    vote: c.vote
      ? { choice: c.vote.choice, reasons: [...c.vote.reasons], comment: c.vote.comment, submitted_at: c.vote.submitted_at }
      : null,
  };
}
