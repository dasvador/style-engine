#!/usr/bin/env node
// 실제 Rust 서버 + 일회용 MySQL + 실제 MCP(OAuth·위임·LiveAdapter)로 코디 흐름을 끝까지 돈다. 모델·날씨는 Rust 의
// `e2e-stub` 빌드가 고정 응답으로 바꾼다 — 비용이 들지 않는다. ChatGPT 는 거치지 않는다.
//
//   cargo build --features e2e-stub
//   E2E_DATABASE_URL=mysql://root@127.0.0.1:33307/wear_test node mcp/scripts/e2e-outfit-flow.mjs
//
// DB 이름에 test 가 없으면 시작하지 않는다. 사용자·옷은 실행마다 새 id 로 넣는다.

import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { randomBytes, randomUUID } from 'node:crypto';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import mysql from 'mysql2/promise';

import { LiveAdapter } from '../src/adapters/live.js';
import { issueLinkCode } from '../src/auth/link-codes.js';
import { TOOL_NAMES } from '../src/mcp-server.js';
import { RecommendClient } from '../src/recommend.js';
import { connectClient, startServer } from '../test/helpers.js';
import { linkAccount, oauth } from '../test/oauth-helpers.js';

const DB = process.env.E2E_DATABASE_URL ?? '';
if (!/\/[^/?]*test[^/?]*(\?|$)/.test(DB)) {
  console.error('E2E_DATABASE_URL 은 이름에 test 가 든 일회용 DB 여야 해요.');
  process.exit(2);
}
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const PORT = Number(process.env.E2E_RUST_PORT ?? 13003);
const KEY = randomBytes(32);
const run = randomUUID().slice(0, 8);
const steps = [];
const step = (name, detail = '') => {
  steps.push(name);
  console.log(`ok ${steps.length} - ${name}${detail ? ` (${detail})` : ''}`);
};

const db = await mysql.createConnection({ uri: DB, dateStrings: true });
const work = await mkdtemp(join(tmpdir(), 'wear-e2e-'));
// .env 를 읽지 않게 저장소 밖에서 띄운다.
const rust = spawn(join(ROOT, 'target/debug/style-engine'), [], {
  cwd: work,
  env: {
    PATH: process.env.PATH,
    DATABASE_URL: DB,
    WARDROBE_E2E_STUB: '1',
    WARDROBE_E2E_PORT: String(PORT),
    WARDROBE_BIND_HOST: '127.0.0.1',
    WARDROBE_STUB_TEMPERATURE: '15',
    WARDROBE_DELEGATION_KEY: KEY.toString('hex'),
    IMAGE_MONTHLY_BUDGET_USD: '0',
    RUST_LOG: 'style_engine=warn',
  },
  stdio: ['ignore', 'ignore', 'inherit'],
});
let srv;
const clients = [];
const cleanup = async () => {
  for (const c of clients) await c.close().catch(() => {});
  await srv?.close().catch(() => {});
  rust.kill('SIGTERM');
  await db.end().catch(() => {});
  await rm(work, { recursive: true, force: true });
};

try {
  for (let i = 0; ; i += 1) {
    const up = await fetch(`http://127.0.0.1:${PORT}/api/health`).then((r) => r.ok).catch(() => false);
    if (up) break;
    if (i > 100) throw new Error('Rust 서버가 뜨지 않았어요');
    await new Promise((r) => setTimeout(r, 200));
  }

  // 두 계정과 옷장
  const users = { a: `e2e-${run}-a`, b: `e2e-${run}-b` };
  const id = (u, k) => `${run}-${u}-${k}`;
  const wardrobe = [
    ['top1', '화이트 옥스퍼드 셔츠', '상의', null, 'medium', '밝음', '베이스', '베이직', 3],
    ['top2', '네이비 니트', '상의', null, 'medium', '어두움', '연결템', '베이직', 3],
    ['bottom1', '차콜 슬랙스', '하의', null, 'medium', '어두움', '구조템', '포멀', 4],
    ['bottom2', '인디고 데님', '하의', null, 'medium', '중간', '구조템', '베이직', 2],
    ['outer1', '네이비 블레이저', '아우터', null, 'medium', '어두움', '구조템', '포멀', 4],
    ['shoe1', '블랙 힐', '신발', '힐', 'medium', '어두움', '구조템', '포멀', 4],
    ['shoe2', '화이트 스니커즈', '신발', '스니커즈', 'medium', '밝음', '연결템', '베이직', 2],
    ['shoe3', '브라운 로퍼', '신발', '로퍼', 'medium', '중간', '연결템', '베이직', 3],
    ['bag1', '블랙 토트백', '가방', null, 'medium', '어두움', '베이스', '베이직', 3],
  ];
  for (const u of ['a', 'b']) {
    await db.execute('INSERT INTO app_user (id, username, api_token) VALUES (?, ?, ?)', [users[u], users[u], `tok-${run}-${u}`]);
    await db.execute(
      'INSERT INTO region_setting (id, user_id, name, latitude, longitude) VALUES (?, ?, ?, 37.57, 126.98)',
      [randomUUID(), users[u], '서울'],
    );
    for (const [k, name, category, sub, thickness, tone, role, style, formality] of wardrobe) {
      await db.execute(
        'INSERT INTO clothing (id, user_id, name, category, sub_category, color, thickness, tone, role, style, formality_level) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)',
        [id(u, k), users[u], name, category, sub, '기본', thickness, tone, role, style, formality],
      );
    }
  }
  const adapter = await LiveAdapter.connect(DB);
  const recommender = new RecommendClient({ port: PORT, key: KEY, timeoutMs: 20_000 });
  const linkFile = join(work, 'link-codes.json');
  srv = await startServer({ live: { adapter, linkCodesFile: linkFile, recommender } });
  const o = oauth(srv);
  const client = (await o.register()).json;
  const connect = async (user, scope) => {
    const { code } = await issueLinkCode({ file: linkFile, userId: user, ttlMs: 600_000 });
    const t = await linkAccount(o, client, code, { scope });
    const mcp = await connectClient(o.resource, { token: t.access_token });
    clients.push(mcp);
    return mcp;
  };
  const ALL = 'wardrobe.read recommendations.write outfits.write';
  const A = await connect(users.a, ALL);
  const B = await connect(users.b, ALL);
  const call = async (mcp, name, args = {}) => (await mcp.callTool({ name, arguments: args })).structuredContent;

  // 1. 추천 → 참조
  const rec = await call(A, TOOL_NAMES.recommend);
  assert.equal(rec.ok, true, JSON.stringify(rec));
  const first = rec.modes[0];
  assert.ok(first.outfit_ref, '코디 참조');
  const allIds = rec.modes.flatMap((m) => m.items.map((i) => i.id));
  assert.ok(allIds.every((x) => x.startsWith(`${run}-a-`)), '내 옷만');
  step('실제 추천이 코디 참조를 돌려준다', `${rec.modes.length}개, ${first.items.map((i) => i.name).join(' + ')}`);

  // 2. 판단 못 하는 조건
  const rain = await call(A, TOOL_NAMES.replaceItem, { outfit_ref: first.outfit_ref, category: '신발', request_text: '비 오는 날 신을 방수 신발' });
  assert.equal(rain.outcome, 'needs_clarification');
  step('방수 조건은 무시하지 않고 되묻는다', rain.reason);

  // 3. 신발만 편한 쪽으로
  const shoesBefore = first.items.find((i) => i.category === '신발');
  const swap = await call(A, TOOL_NAMES.replaceItem, { outfit_ref: first.outfit_ref, category: '신발', preference: 'comfortable' });
  // 스텁이 이미 스니커즈를 골랐을 수 있어 편한 대안이 로퍼 하나일 수 있다.
  assert.equal(swap.outcome, 'proposed', JSON.stringify(swap));
  const alt = swap.alternatives[0];
  for (const before of first.items.filter((i) => i.category !== '신발')) {
    assert.ok(alt.items.some((i) => i.id === before.id && i.name === before.name), '나머지 옷 그대로');
  }
  assert.notEqual(alt.new_item.id, shoesBefore.id);
  assert.equal(alt.items.length, first.items.length);
  step('신발만 바뀐 대안', `${shoesBefore.name} → ${alt.new_item.name}: ${alt.rationale[0]}`);
  const [[{ n: noWear }]] = await db.execute('SELECT COUNT(*) AS n FROM outfit_wear WHERE user_id = ?', [users.a]);
  assert.equal(Number(noWear), 0);
  step('바꾸기 제안은 기록을 남기지 않는다');

  // 4. 다른 계정
  for (const [name, args] of [
    [TOOL_NAMES.planOutfit, { outfit_ref: alt.outfit_ref }],
    [TOOL_NAMES.recordWorn, { outfit_ref: first.outfit_ref }],
    [TOOL_NAMES.replaceItem, { outfit_ref: first.outfit_ref, category: '신발' }],
  ]) {
    const denied = await call(B, name, args);
    assert.equal(denied.outcome, 'ref_not_found', name);
  }
  const forged = await call(A, TOOL_NAMES.replaceItem, { outfit_ref: first.outfit_ref, category: '신발', replacement_item_id: id('b', 'shoe2') });
  assert.equal(forged.outcome, 'item_not_owned');
  step('다른 계정의 참조·옷은 없는 것과 같다');

  // 5. 입을 계획
  const plan = await call(A, TOOL_NAMES.planOutfit, { outfit_ref: alt.outfit_ref, comment: '저녁 약속' });
  assert.equal(plan.outcome, 'planned');
  const again = await call(A, TOOL_NAMES.planOutfit, { outfit_ref: alt.outfit_ref });
  assert.equal(again.outcome, 'already_planned');
  const wornNone = await call(A, TOOL_NAMES.history, { status: 'worn' });
  assert.equal(wornNone.records.length, 0);
  const noPenalty = await call(A, TOOL_NAMES.recommend);
  assert.ok(noPenalty.modes.every((m) => m.wear_note === null), '계획은 착용 감점이 아니다');
  step('계획은 착용 내역·감점에 들어가지 않는다', `record ${plan.record.record_id.slice(0, 8)}`);

  // 6. 낡은 revision → 실제 착용 → 재시도
  const edited = await call(A, TOOL_NAMES.updateRecord, { record_id: plan.record.record_id, expected_revision: 1, action: 'edit', comment: '점심 약속' });
  assert.equal(edited.outcome, 'updated');
  const stale = await call(A, TOOL_NAMES.recordWorn, { record_id: plan.record.record_id, expected_revision: 1 });
  assert.equal(stale.outcome, 'revision_conflict');
  assert.equal(stale.current_revision, 2);
  const worn = await call(A, TOOL_NAMES.recordWorn, { record_id: plan.record.record_id, expected_revision: 2, reaction: 'comfortable' });
  assert.equal(worn.outcome, 'worn');
  const retry = await call(A, TOOL_NAMES.recordWorn, { record_id: plan.record.record_id, expected_revision: 2 });
  assert.equal(retry.outcome, 'already_worn');
  const [[{ n: oneRow }]] = await db.execute('SELECT COUNT(*) AS n FROM outfit_wear WHERE user_id = ?', [users.a]);
  assert.equal(Number(oneRow), 1);
  step('입었어: 충돌 감지, 재시도 중복 없음, 행 하나');

  // 7. 내역과 다음 추천
  const hist = await call(A, TOOL_NAMES.history, {});
  assert.equal(hist.records[0].status, 'worn');
  assert.equal(hist.records[0].reaction, 'comfortable');
  const historyB = await call(B, TOOL_NAMES.history, {});
  assert.equal(historyB.records.length, 0);
  const after = await call(A, TOOL_NAMES.recommend);
  const noted = after.modes.filter((m) => m.wear_note);
  assert.ok(noted.length > 0, '입은 옷이 든 코디에 사실이 붙는다');
  step('다음 추천에 착용 사실이 보인다', noted[0].wear_note);
  const [[{ n: exposure }]] = await db.execute('SELECT COUNT(*) AS n FROM outfit_recommendation_history WHERE user_id = ?', [users.a]);
  assert.ok(Number(exposure) >= 3, '추천 이력(노출)은 착용 기록과 별개');

  // 8. 되돌리기
  const undo = await call(A, TOOL_NAMES.updateRecord, { record_id: plan.record.record_id, expected_revision: 3, action: 'undo' });
  assert.equal(undo.outcome, 'undone_to_planned');
  const clean = await call(A, TOOL_NAMES.recommend);
  assert.ok(clean.modes.every((m) => m.wear_note === null), '되돌린 착용은 사라진다');
  step('되돌리면 착용 사실도 추천에서 사라진다');

  // 9. 권한
  const old = await connect(users.a, 'wardrobe.read recommendations.write');
  const res = await old.callTool({ name: TOOL_NAMES.recordWorn, arguments: { outfit_ref: first.outfit_ref } });
  assert.equal(res.isError, true);
  assert.match(res._meta['mcp/www_authenticate'][0], /insufficient_scope/);
  step('outfits.write 없는 옛 연결은 재동의를 요구한다');

  await adapter.close();
  console.log(`\n전부 통과: ${steps.length}단계. Rust 실서버 프로세스·실제 MySQL·MCP OAuth 경유, 모델·날씨는 고정 응답.`);
  await cleanup();
  process.exit(0);
} catch (e) {
  console.error('실패:', e);
  await cleanup();
  process.exit(1);
}
