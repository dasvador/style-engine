import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { test } from 'node:test';

import { DemoStore, StoreError } from '../src/demo-store.js';
import { SAMPLE_PAIRS, SAMPLE_WARDROBE, itemKey } from '../src/fixtures.js';
import { InvalidVote, validateVote } from '../src/vote.js';
import { tempDir } from './helpers.js';

test('fixture: 모든 샘플 코디가 옷장 아이템만 쓰고, 한 쌍의 두 코디는 서로 다르다', () => {
  const ids = new Set(SAMPLE_WARDROBE.map((i) => i.id));
  for (const [key, pair] of Object.entries(SAMPLE_PAIRS)) {
    assert.equal(pair.outfits.length, 2, key);
    for (const outfit of pair.outfits) for (const id of outfit) assert.ok(ids.has(id), `${key}: ${id}`);
    assert.notEqual(itemKey(pair.outfits[0]), itemKey(pair.outfits[1]), key);
  }
  assert.ok(SAMPLE_WARDROBE.every((i) => i.id.startsWith('sample-')));
});

test('vote: Rust validate_vote 와 같은 정규화(이유 정렬·중복 제거, 메모 정리)', () => {
  assert.deepEqual(validateVote({ choice: 'b', reasons: ['silhouette', 'color_combo', 'silhouette'], comment: ' 바지 핏이 좋아요 ' }), {
    choice: 'b',
    reasons: ['color_combo', 'silhouette'],
    comment: '바지 핏이 좋아요',
  });
  assert.equal(validateVote({ choice: 'a', comment: '   ' }).comment, null);
  assert.equal(validateVote({ choice: 'a', comment: '줄\n바꿈\u0007\r' }).comment, '줄\n바꿈');
  assert.equal(validateVote({ choice: 'neither' }).reasons.length, 0);
  // 200자는 되고 201자는 안 된다(글자 수 기준, 바이트 아님).
  assert.equal(validateVote({ choice: 'a', comment: '가'.repeat(200) }).comment.length, 200);
  assert.throws(() => validateVote({ choice: 'a', comment: '가'.repeat(201) }), InvalidVote);
  // 앞뒤 공백을 떼고 200자면 된다.
  assert.equal(validateVote({ choice: 'a', comment: `  ${'가'.repeat(200)}  ` }).comment.length, 200);
  for (const bad of [{ choice: 'skip' }, { choice: 'c' }, {}, { choice: 'a', reasons: ['price'] }, { choice: 'a', comment: 3 }]) {
    assert.throws(() => validateVote(bad), InvalidVote, JSON.stringify(bad));
  }
});

test('store: 비교→선택→다시 읽기, 같은 응답 재제출, 다른 응답 충돌', async () => {
  const store = new DemoStore({ file: null, randomBool: () => true });
  const c = await store.createComparison({ situation: 'office', mood: 'minimal' });
  assert.match(c.id, /^demo_cmp_[0-9a-f]{32}$/);
  assert.equal(c.demo, true);
  assert.equal(c.vote, null);
  assert.deepEqual(
    c.options.map((o) => o.slot),
    ['a', 'b'],
  );
  // randomBool=true → 첫 코디가 A
  assert.deepEqual(
    c.options[0].items.map((i) => i.id),
    SAMPLE_PAIRS['office:minimal'].outfits[0],
  );

  const first = await store.submitVote(c.id, { choice: 'b', reasons: ['silhouette', 'color_combo'], comment: '핏' });
  assert.equal(first.outcome, 'saved');
  assert.deepEqual(first.comparison.vote.reasons, ['color_combo', 'silhouette']);

  const again = await store.submitVote(c.id, { choice: 'b', reasons: ['color_combo', 'silhouette'], comment: ' 핏 ' });
  assert.equal(again.outcome, 'already_submitted');
  assert.equal(again.comparison.vote.submitted_at, first.comparison.vote.submitted_at);

  await assert.rejects(store.submitVote(c.id, { choice: 'a' }), (e) => e instanceof StoreError && e.code === 'conflict' && e.comparison.vote.choice === 'b');
  // 이유만 달라도 충돌이다(먼저 쓴 것이 남는다).
  await assert.rejects(store.submitVote(c.id, { choice: 'b', reasons: ['weather_fit'] }), { code: 'conflict' });
  assert.equal(store.getComparison(c.id).vote.choice, 'b');
});

test('store: 비교 id 를 섞지 않는다 — 한 비교의 선택이 다른 비교에 남지 않는다', async () => {
  const store = new DemoStore({ file: null });
  const one = await store.createComparison({ situation: 'daily', mood: 'amekaji' });
  const two = await store.createComparison({ situation: 'daily', mood: 'amekaji' });
  assert.notEqual(one.id, two.id);
  await store.submitVote(one.id, { choice: 'a' });
  assert.equal(store.getComparison(two.id).vote, null);
  const second = await store.submitVote(two.id, { choice: 'neither', reasons: ['weather_fit'] });
  assert.equal(second.outcome, 'saved');
  assert.equal(store.getComparison(one.id).vote.choice, 'a');
  assert.equal(store.getComparison(two.id).vote.choice, 'neither');
});

test('store: 없는 id·모양이 다른 id·지원하지 않는 조합', async () => {
  const store = new DemoStore({ file: null });
  for (const id of ['demo_cmp_' + '0'.repeat(32), 'x', '../../etc/passwd', '', null, 'demo_cmp_' + 'g'.repeat(32)]) {
    assert.equal(store.getComparison(id), null, String(id));
    await assert.rejects(store.submitVote(id, { choice: 'a' }), { code: 'not_found' });
  }
  await assert.rejects(store.createComparison({ situation: 'party', mood: 'minimal' }), { code: 'unknown_scenario' });
  await assert.rejects(store.createComparison({ situation: '__proto__', mood: 'x' }), { code: 'unknown_scenario' });
  await assert.rejects(store.submitVote('demo_cmp_' + '0'.repeat(32), { choice: 'skip' }), { code: 'invalid_vote' });
});

test('store: 동시에 다른 선택 20개가 와도 하나만 저장된다', async () => {
  const store = new DemoStore({ file: null });
  const c = await store.createComparison({ situation: 'weekend', mood: 'minimal' });
  const choices = Array.from({ length: 20 }, (_, i) => ['a', 'b', 'neither'][i % 3]);
  const results = await Promise.allSettled(choices.map((choice) => store.submitVote(c.id, { choice })));
  const saved = results.filter((r) => r.status === 'fulfilled' && r.value.outcome === 'saved');
  assert.equal(saved.length, 1);
  const winner = saved[0].value.comparison.vote.choice;
  for (const [i, r] of results.entries()) {
    if (choices[i] === winner) assert.equal(r.status, 'fulfilled');
    else assert.equal(r.reason.code, 'conflict');
  }
});

test('store: 파일에 남고 다시 읽으면 그대로다(재시작), 깨진 파일은 덮어쓰지 않는다', async () => {
  const { dir, cleanup } = await tempDir();
  try {
    const file = join(dir, 'nested', 'demo-store.json');
    const s1 = await new DemoStore({ file }).load();
    const c = await s1.createComparison({ situation: 'daily', mood: 'minimal' });
    await s1.submitVote(c.id, { choice: 'neither', reasons: ['color_combo'], comment: '둘 다 심심해요' });

    const s2 = await new DemoStore({ file }).load();
    assert.deepEqual(s2.getComparison(c.id), s1.getComparison(c.id));
    const saved = JSON.parse(await readFile(file, 'utf8'));
    assert.equal(saved.version, 1);
    assert.equal(saved.comparisons[c.id].vote.reason_polarity, 'both_lacking');
    assert.equal(saved.comparisons[c.id].vote.chosen_item_key, null);

    await writeFile(file, '{"not":"ours"}');
    await assert.rejects(new DemoStore({ file }).load(), /형식/);
    assert.equal(await readFile(file, 'utf8'), '{"not":"ours"}');
  } finally {
    await cleanup();
  }
});

test('store: 만료된 비교는 없는 것과 같고, 개수 상한을 넘으면 오래된 것부터 지운다', async () => {
  let now = new Date('2026-10-24T00:00:00Z');
  const store = new DemoStore({ file: null, ttlMs: 60_000, maxComparisons: 10, now: () => now });
  const old = await store.createComparison({ situation: 'daily', mood: 'minimal' });
  now = new Date(now.getTime() + 61_000);
  assert.equal(store.getComparison(old.id), null);
  await assert.rejects(store.submitVote(old.id, { choice: 'a' }), { code: 'not_found' });

  const ids = [];
  for (let i = 0; i < 12; i++) {
    now = new Date(now.getTime() + 1000);
    ids.push((await store.createComparison({ situation: 'daily', mood: 'minimal' })).id);
  }
  assert.ok(store.size <= 10);
  assert.equal(store.getComparison(ids[0]), null);
  assert.notEqual(store.getComparison(ids.at(-1)), null);
});

test('store: 파일 저장이 실패하면 메모리 변경도 되돌린다', async () => {
  const { dir, cleanup } = await tempDir();
  try {
    const blocker = join(dir, 'blocker');
    await writeFile(blocker, 'file, not a directory');
    // load() 없이 시작한다 — 읽기는 성공한 뒤 쓰기만 실패하는 상황.
    const store = new DemoStore({ file: join(blocker, 'demo-store.json') });
    await assert.rejects(store.createComparison({ situation: 'daily', mood: 'minimal' }));
    assert.equal(store.size, 0);
  } finally {
    await cleanup();
  }
});
