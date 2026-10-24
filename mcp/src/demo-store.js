// 데모 비교·선택 저장소. 실제 DB 가 아니라 로컬 JSON 파일 하나(기본 mcp/.data/demo-store.json)다.
//
// 형식(version 1):
//   { "version": 1, "comparisons": { "<id>": StoredComparison } }
//   StoredComparison = { id, created_at, expires_at, situation, mood, weather,
//                        a: { outfit_index, item_key, items }, b: {...}, vote: StoredVote | null }
//   owner(선택): live 모드에서 비교를 만든 인증 사용자 id. 있으면 그 사용자만, 없으면 익명(데모)만 읽고 쓴다.
//   화면·도구 결과로 내보내지 않는다. 없는 옛 파일은 모두 익명 비교로 읽힌다.
//   StoredVote = { choice, reasons, comment, reason_polarity, chosen_item_key, submitted_at,
//                  revision, updated_at }
//   사진 비교(kind: "photo")는 photo_set 과 각 쪽의 image_key 를 더 가진다(없으면 글자 비교). 사진 URL 은
//   저장하지 않고 읽을 때 지금 asset 허용 목록으로 만든다 — 공개 주소가 바뀌거나 파일이 빠져도 옛 URL 이 남지 않는다.
//   revision·updated_at 은 선택 수정이 생기며 더했다. 없는 옛 파일의 선택은 revision 1,
//   updated_at = submitted_at 으로 읽는다(파일을 다시 쓰지 않아도 된다). 선택은 비교당 늘 하나다 —
//   수정하면 그 자리에서 바뀌고 이전 값은 남기지 않는다.
// 수명: 만든 뒤 ttlMs(기본 7일)가 지나면 읽을 때·쓸 때 지운다. 개수는 maxComparisons(기본 500)
// 를 넘으면 오래된 것부터 지운다. 파일을 지우면 데모 기록이 모두 사라진다.
//
// 한 프로세스만 이 파일을 쓴다고 가정한다. 프로세스 안의 동시 요청은 아래 `#queue` 가 한 줄로
// 세워 처리하므로 같은 비교에 응답이 둘 남지 않는다. 첫 저장(submitVote)은 먼저 쓴 것이 남고,
// 바꾸기는 updateVote 로만 한다 — 호출자가 본 revision 이 지금과 같을 때만 바꾼다(낙관적 동시성).

import { randomBytes } from 'node:crypto';
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';

import { CATEGORIES, MOODS, SAMPLE_PAIRS, SAMPLE_WARDROBE, SITUATIONS, itemKey, snapshotItems } from './fixtures.js';
import { PHOTO_MOODS, PHOTO_SETS } from './photo-sets.js';
import { reasonPolarity, sameVote, validateVote } from './vote.js';

/** 비교 id 모양. 서버가 만든 값만 받는다 — 다른 모양은 저장소를 뒤지지 않고 없는 것으로 친다. */
export const COMPARISON_ID_PATTERN = /^demo_cmp_[0-9a-f]{32}$/;

/** 기존 Rust 비교의 REPRESENTATION 과 같은 자리. 데모 fixture 로 만든 비교임을 남긴다. */
export const REPRESENTATION = 'demo_fixture_garment_text_and_color';

export const DEMO_NOTICE =
  '데모 데이터예요. 샘플 옷장으로 미리 짜 둔 고정 코디이며, 실제 옷장이나 AI 추천 결과가 아니에요.';

export const PHOTO_NOTICE =
  '데모 데이터예요. 이 앱에서 이전에 만들어 둔 생성 룩북 사진 두 장을 골라 보여 줘요. 지금 새로 추천하거나 생성한 것이 아니고, 실제 옷장도 아니에요.';

/** 사진 비교의 표현 방식(Rust REPRESENTATION 자리). */
export const PHOTO_REPRESENTATION = 'demo_curated_generated_lookbook_photo_with_item_text';

const DAY_MS = 24 * 60 * 60 * 1000;

export class DemoStore {
  #file;
  #ttlMs;
  #max;
  #now;
  #random;
  #data = { version: 1, comparisons: {} };
  #queue = Promise.resolve();
  #images;

  /**
   * @param {{ file: string | null, ttlMs?: number, maxComparisons?: number,
   *           now?: () => Date, randomBool?: () => boolean,
   *           images?: (assetKey: string) => ({ url: string, width: number, height: number } | null) }} opts
   *   file 이 null 이면 메모리에만 둔다(테스트용). images 는 asset 키 → 지금 내보낼 수 있는 사진(없으면 null).
   */
  constructor({ file, ttlMs = 7 * DAY_MS, maxComparisons = 500, now = () => new Date(), randomBool, images } = {}) {
    this.#images = images ?? (() => null);
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

  /**
   * 살아 있는 비교. owner 는 인증된 사용자 id(live 모드) 또는 null(익명 데모).
   * 비교를 만든 주체와 같을 때만 보인다 — 다른 사용자의 비교, 익명↔인증 사이도 없는 것과 같다.
   */
  #live(id, owner = null) {
    if (typeof id !== 'string' || !COMPARISON_ID_PATTERN.test(id)) return null;
    const c = this.#data.comparisons[id];
    if (!c || Date.parse(c.expires_at) <= this.#now().getTime()) return null;
    if ((c.owner ?? null) !== (owner ?? null)) return null;
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
  createComparison({ situation, mood, owner = null }) {
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
        ...(owner ? { owner } : {}),
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
      return { result: toView(stored, this.#images), dirty: true };
    });
  }

  /** 두 사진이 모두 있는 사진 세트 키. 사진이 하나라도 없는 세트는 내보내지 않는다. */
  availablePhotoSets() {
    return Object.keys(PHOTO_SETS).filter((k) => PHOTO_SETS[k].sides.every((side) => this.#images(side.key)));
  }

  /** 이미 만들어 둔 룩북 사진 두 장으로 비교를 만든다. 좌우는 한 번 무작위. */
  createPhotoComparison({ set, owner = null }) {
    if (!Object.hasOwn(PHOTO_SETS, set)) {
      return Promise.reject(new StoreError('unknown_scenario', '지원하지 않는 사진 세트예요.'));
    }
    if (!this.availablePhotoSets().includes(set)) {
      return Promise.reject(new StoreError('unknown_scenario', '이 사진 세트의 사진이 지금 준비돼 있지 않아요.'));
    }
    const def = PHOTO_SETS[set];
    return this.#exclusive(() => {
      this.#prune();
      const now = this.#now();
      const id = `demo_cmp_${randomBytes(16).toString('hex')}`;
      const order = this.#random() ? [0, 1] : [1, 0];
      const side = (idx) => ({
        outfit_index: idx,
        item_key: itemKey(def.sides[idx].items.map((i) => i.id)),
        items: def.sides[idx].items.map((i) => ({ ...i })),
        image_key: def.sides[idx].key,
        image_alt: def.sides[idx].alt,
      });
      const stored = {
        id,
        ...(owner ? { owner } : {}),
        kind: 'photo',
        photo_set: set,
        created_at: now.toISOString(),
        expires_at: new Date(now.getTime() + this.#ttlMs).toISOString(),
        situation: def.situation,
        mood: def.mood,
        weather: { temperature: def.temperature, weather_description: def.weather_description },
        representation: PHOTO_REPRESENTATION,
        a: side(order[0]),
        b: side(order[1]),
        vote: null,
      };
      this.#data.comparisons[id] = stored;
      return { result: toView(stored, this.#images), dirty: true };
    });
  }

  /** 비교 하나. 없거나 만료됐거나 id 모양이 다르면 null. */
  getComparison(id, owner = null) {
    const c = this.#live(id, owner);
    return c ? toView(c, this.#images) : null;
  }

  /**
   * 첫 선택을 남긴다. 이미 선택이 있으면 바꾸지 않는다(먼저 쓴 것이 남는다).
   * 예상 가능한 결과(이미 같은 선택·이미 다른 선택)는 예외가 아니라 outcome 으로 돌려준다.
   * @returns {Promise<{ outcome: 'saved' | 'already_submitted' | 'conflict', comparison: object }>}
   * @throws {StoreError} not_found · invalid_vote
   */
  submitVote(id, rawVote, owner = null) {
    let vote;
    try {
      vote = validateVote(rawVote);
    } catch (e) {
      return Promise.reject(new StoreError('invalid_vote', e.message));
    }
    return this.#exclusive(() => {
      const c = this.#mustLive(id, owner);
      if (c.vote) {
        const outcome = sameVote(c.vote, vote) ? 'already_submitted' : 'conflict';
        return { result: { outcome, comparison: toView(c, this.#images) }, dirty: false };
      }
      const now = this.#now().toISOString();
      c.vote = { ...this.#voteRecord(c, vote), submitted_at: now, updated_at: now, revision: 1 };
      return { result: { outcome: 'saved', comparison: toView(c, this.#images) }, dirty: true };
    });
  }

  /**
   * 저장된 선택을 바꾼다. 호출자가 마지막으로 본 revision(expectedRevision)이 지금과 같을 때만
   * 바꾸고 revision 을 1 올린다. 오래된 화면 둘이 서로 덮어쓰지 않게 하는 장치다.
   *   - 지금 선택과 내용이 같으면 바꾸지 않고 already_submitted(같은 요청 재시도 포함).
   *   - revision 이 다르고 내용도 다르면 stale — 바꾸지 않고 지금 상태를 돌려준다.
   *   - 아직 선택이 없으면 no_vote — submitVote 로 먼저 저장해야 한다.
   * @returns {Promise<{ outcome: 'updated' | 'already_submitted' | 'stale' | 'no_vote', comparison: object }>}
   * @throws {StoreError} not_found · invalid_vote
   */
  updateVote(id, expectedRevision, rawVote, owner = null) {
    let vote;
    try {
      vote = validateVote(rawVote);
      if (!Number.isInteger(expectedRevision) || expectedRevision < 1) {
        throw new Error('expected_revision 은 1 이상의 정수여야 해요.');
      }
    } catch (e) {
      return Promise.reject(new StoreError('invalid_vote', e.message));
    }
    return this.#exclusive(() => {
      const c = this.#mustLive(id, owner);
      if (!c.vote) return { result: { outcome: 'no_vote', comparison: toView(c, this.#images) }, dirty: false };
      if (sameVote(c.vote, vote)) return { result: { outcome: 'already_submitted', comparison: toView(c, this.#images) }, dirty: false };
      const current = revisionOf(c.vote);
      if (current !== expectedRevision) return { result: { outcome: 'stale', comparison: toView(c, this.#images) }, dirty: false };
      c.vote = {
        ...this.#voteRecord(c, vote),
        submitted_at: c.vote.submitted_at,
        updated_at: this.#now().toISOString(),
        revision: current + 1,
      };
      return { result: { outcome: 'updated', comparison: toView(c, this.#images) }, dirty: true };
    });
  }

  #mustLive(id, owner) {
    const c = this.#live(id, owner);
    if (!c) throw new StoreError('not_found', '이 비교를 찾을 수 없어요. 만료됐거나 다른 비교 id 예요.');
    return c;
  }

  /** 선택 본문. 고른 코디의 정체는 저장된 노출에서 읽는다 — 호출자가 보낸 아이템 이름·id 는 받지 않는다. */
  #voteRecord(c, vote) {
    const chosen = vote.choice === 'a' ? c.a : vote.choice === 'b' ? c.b : null;
    return {
      ...vote,
      reason_polarity: reasonPolarity(vote.choice),
      chosen_item_key: chosen ? chosen.item_key : null,
    };
  }

  /** 테스트·점검용: 지금 남아 있는 비교 수. */
  get size() {
    return Object.keys(this.#data.comparisons).length;
  }
}

export class StoreError extends Error {
  /** @param {'not_found'|'invalid_vote'|'unknown_scenario'} code */
  constructor(code, message) {
    super(message);
    this.code = code;
  }
}

/** 옛 파일의 선택에는 revision 이 없다 — 한 번 저장된 것으로 친다. */
function revisionOf(vote) {
  return Number.isInteger(vote.revision) && vote.revision >= 1 ? vote.revision : 1;
}

/**
 * 화면·모델에 내려 주는 비교. 기존 Rust `ComparisonView`(id·created_at·context·options·vote)와
 * 같은 모양에 데모 표시(demo·data_source·notice)와 상황 이름을 더했다. 원래 코디 순서·내부 키는
 * 넣지 않는다 — 고르는 데 영향을 주거나 쓸모없는 정보다.
 *
 * 사진 비교면 각 option 에 image 를 싣는다. 그 사진은 바로 그 option 의 items 로 만든 것이다
 * (photo-sets.js). 사진 파일이 지금 없으면 image 는 null 이고 화면은 글자·색 점으로 보여 준다.
 * 다른 사진으로 바꿔 끼우지 않는다.
 */
export function toView(c, images = () => null) {
  const photo = c.kind === 'photo';
  const option = (slot, side) => {
    let image = null;
    if (photo && side.image_key) {
      const found = images(side.image_key);
      if (found) {
        image = {
          url: found.url,
          width: found.width,
          height: found.height,
          alt: side.image_alt,
          presentation: 'generated_lookbook_photo',
          source: 'existing_generated_image',
          matches: 'items_of_this_option',
        };
      }
    }
    return { slot, items: side.items.map((i) => ({ ...i })), image };
  };
  return {
    demo: true,
    data_source: photo ? 'curated_generated_lookbook' : 'sample_fixture',
    notice: photo ? PHOTO_NOTICE : DEMO_NOTICE,
    id: c.id,
    created_at: c.created_at,
    expires_at: c.expires_at,
    context: {
      situation: c.situation,
      situation_label: SITUATIONS[c.situation],
      mood_key: c.mood,
      mood_label: MOODS[c.mood] ?? PHOTO_MOODS[c.mood] ?? c.mood,
      temperature: c.weather.temperature,
      weather_description: c.weather.weather_description,
      photo_set: photo ? c.photo_set : null,
    },
    options: [option('a', c.a), option('b', c.b)],
    vote: c.vote
      ? {
          choice: c.vote.choice,
          reasons: [...c.vote.reasons],
          comment: c.vote.comment,
          submitted_at: c.vote.submitted_at,
          updated_at: c.vote.updated_at ?? c.vote.submitted_at,
          revision: revisionOf(c.vote),
        }
      : null,
  };
}
