import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { test } from 'node:test';

import { DemoStore } from '../src/demo-store.js';
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

test('store: 첫 저장은 먼저 쓴 것이 남는다 — 같은 응답 재제출, 다른 응답은 conflict(덮어쓰지 않음)', async () => {
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
  assert.equal(first.comparison.vote.revision, 1);
  assert.equal(first.comparison.vote.updated_at, first.comparison.vote.submitted_at);

  // 예상 가능한 충돌은 예외가 아니라 outcome 이다. 저장된 쪽을 같이 돌려준다.
  const conflict = await store.submitVote(c.id, { choice: 'a' });
  assert.equal(conflict.outcome, 'conflict');
  assert.equal(conflict.comparison.vote.choice, 'b');
  // 이유만 달라도 충돌이다(submit 은 바꾸지 않는다).
  assert.equal((await store.submitVote(c.id, { choice: 'b', reasons: ['weather_fit'] })).outcome, 'conflict');
  assert.equal(store.getComparison(c.id).vote.choice, 'b');
  assert.equal(store.getComparison(c.id).vote.revision, 1);
});

test('store: 명시적 수정 — revision 이 맞을 때만 바꾸고, 같은 요청 재시도는 멱등, 오래된 revision 은 stale', async () => {
  let t = Date.parse('2026-10-24T00:00:00Z');
  const store = new DemoStore({ file: null, now: () => new Date((t += 1000)) });
  const c = await store.createComparison({ situation: 'daily', mood: 'minimal' });
  assert.equal((await store.updateVote(c.id, 1, { choice: 'a' })).outcome, 'no_vote', '저장 전에는 수정할 것이 없다');
  assert.equal(store.getComparison(c.id).vote, null);

  const saved = (await store.submitVote(c.id, { choice: 'b', reasons: ['silhouette'], comment: '핏' })).comparison.vote;

  // 선택 바꾸기
  const toA = await store.updateVote(c.id, 1, { choice: 'a', reasons: ['silhouette'], comment: '핏' });
  assert.equal(toA.outcome, 'updated');
  assert.equal(toA.comparison.vote.choice, 'a');
  assert.equal(toA.comparison.vote.revision, 2);
  assert.equal(toA.comparison.vote.submitted_at, saved.submitted_at, '처음 저장 시각은 그대로');
  assert.notEqual(toA.comparison.vote.updated_at, saved.updated_at);

  // 같은 요청 재시도(응답을 잃은 경우): 바꾸지 않고 성공
  const retry = await store.updateVote(c.id, 1, { choice: 'a', reasons: ['silhouette'], comment: '핏' });
  assert.equal(retry.outcome, 'already_submitted');
  assert.equal(retry.comparison.vote.revision, 2);

  // 오래된 화면(revision 1)에서 다른 내용 → stale, 바뀌지 않음
  const stale = await store.updateVote(c.id, 1, { choice: 'neither' });
  assert.equal(stale.outcome, 'stale');
  assert.equal(stale.comparison.vote.choice, 'a');

  // 이유만, 메모만 바꾸기
  const reasonsOnly = await store.updateVote(c.id, 2, { choice: 'a', reasons: ['weather_fit', 'color_combo'], comment: '핏' });
  assert.equal(reasonsOnly.outcome, 'updated');
  assert.deepEqual(reasonsOnly.comparison.vote.reasons, ['color_combo', 'weather_fit']);
  const commentOnly = await store.updateVote(c.id, 3, { choice: 'a', reasons: ['color_combo', 'weather_fit'] });
  assert.equal(commentOnly.outcome, 'updated');
  assert.equal(commentOnly.comparison.vote.comment, null);
  assert.equal(commentOnly.comparison.vote.revision, 4);

  // neither 로
  const neither = await store.updateVote(c.id, 4, { choice: 'neither', reasons: ['weather_fit'] });
  assert.equal(neither.comparison.vote.choice, 'neither');

  // 잘못된 revision 값·잘못된 선택
  await assert.rejects(store.updateVote(c.id, 0, { choice: 'a' }), { code: 'invalid_vote' });
  await assert.rejects(store.updateVote(c.id, 1.5, { choice: 'a' }), { code: 'invalid_vote' });
  await assert.rejects(store.updateVote(c.id, 5, { choice: 'skip' }), { code: 'invalid_vote' });
  await assert.rejects(store.updateVote('demo_cmp_' + '0'.repeat(32), 1, { choice: 'a' }), { code: 'not_found' });
});

test('store: 같은 revision 으로 다른 수정 10개가 동시에 와도 하나만 반영된다', async () => {
  const store = new DemoStore({ file: null });
  const c = await store.createComparison({ situation: 'office', mood: 'amekaji' });
  await store.submitVote(c.id, { choice: 'a' });
  const drafts = Array.from({ length: 10 }, (_, i) => ({ choice: ['b', 'neither'][i % 2], comment: `화면 ${i}` }));
  const results = await Promise.all(drafts.map((d) => store.updateVote(c.id, 1, d)));
  const updated = results.filter((r) => r.outcome === 'updated');
  assert.equal(updated.length, 1);
  assert.equal(results.filter((r) => r.outcome === 'stale').length, 9);
  assert.equal(store.getComparison(c.id).vote.revision, 2);
  assert.equal(store.getComparison(c.id).vote.comment, updated[0].comparison.vote.comment);
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
    assert.equal(r.status, 'fulfilled');
    if (choices[i] !== winner) assert.equal(r.value.outcome, 'conflict');
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

test('store: revision 이 없는 옛(v1) 파일을 읽고, 수정하면 revision 2 가 된다', async () => {
  const { dir, cleanup } = await tempDir();
  try {
    const file = join(dir, 'demo-store.json');
    const s1 = await new DemoStore({ file }).load();
    const c = await s1.createComparison({ situation: 'weekend', mood: 'amekaji' });
    // 0.1.0 이 쓰던 모양으로 되돌린다(revision·updated_at 없음).
    const raw = JSON.parse(await readFile(file, 'utf8'));
    raw.comparisons[c.id].vote = {
      choice: 'b',
      reasons: ['color_combo'],
      comment: null,
      reason_polarity: 'chosen',
      chosen_item_key: raw.comparisons[c.id].b.item_key,
      submitted_at: '2026-10-24T01:00:00.000Z',
    };
    await writeFile(file, JSON.stringify(raw));

    const s2 = await new DemoStore({ file }).load();
    const view = s2.getComparison(c.id);
    assert.equal(view.vote.revision, 1);
    assert.equal(view.vote.updated_at, '2026-10-24T01:00:00.000Z');
    assert.equal((await s2.submitVote(c.id, { choice: 'b', reasons: ['color_combo'] })).outcome, 'already_submitted');
    const updated = await s2.updateVote(c.id, 1, { choice: 'a' });
    assert.equal(updated.outcome, 'updated');
    assert.equal(updated.comparison.vote.revision, 2);
    const onDisk = JSON.parse(await readFile(file, 'utf8')).comparisons[c.id].vote;
    assert.equal(onDisk.revision, 2);
    assert.equal(onDisk.submitted_at, '2026-10-24T01:00:00.000Z');
    assert.equal(onDisk.chosen_item_key, raw.comparisons[c.id].a.item_key);
  } finally {
    await cleanup();
  }
});
