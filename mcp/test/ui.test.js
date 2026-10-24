// 화면 테스트: 진짜 브라우저 + 공식 AppBridge 호스트 흉내 + 진짜 데모 MCP 서버.
// ChatGPT·Claude 호스트 E2E 가 아니다 — 같은 MCP Apps 규약(ui/initialize · tool-input · tool-result ·
// tools/call)을 공식 SDK 로 흉내 낸 것이다. 브라우저는 내려받지 않고 이미 설치된 것을 찾는다.

import assert from 'node:assert/strict';
import { existsSync, readdirSync } from 'node:fs';
import { mkdir, readFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { after, before, describe, test } from 'node:test';

import { chromium } from 'playwright-core';

import { ERROR_META_KEY, TOOL_NAMES, UI_URI } from '../src/mcp-server.js';
import { loadImageAssets } from '../src/image-assets.js';
import { PHOTO_ASSET_KEYS, PHOTO_SETS } from '../src/photo-sets.js';
import { connectClient, makeTestAssets, startServer } from './helpers.js';

const SHOTS = fileURLToPath(new URL('../.artifacts/screenshots/', import.meta.url));
const HOST_JS = fileURLToPath(new URL('../dist/test-host.js', import.meta.url));

function findChromium() {
  const candidates = [process.env.CHROME_PATH, chromium.executablePath()];
  const cache = join(homedir(), 'Library/Caches/ms-playwright');
  if (existsSync(cache)) {
    for (const d of readdirSync(cache).filter((n) => /^chromium-\d+$/.test(n)).sort().reverse()) {
      candidates.push(join(cache, d, 'chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'));
      candidates.push(join(cache, d, 'chrome-mac/Chromium.app/Contents/MacOS/Chromium'));
      candidates.push(join(cache, d, 'chrome-linux/chrome'));
    }
  }
  candidates.push('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome');
  return candidates.find((p) => p && existsSync(p)) ?? null;
}

const executablePath = findChromium();

/**
 * 스크린샷. 크기 알림이 멎을 때까지 기다린 뒤 viewport 를 문서 높이까지 늘려 찍는다 — 샌드박스
 * iframe 은 다른 프로세스에서 그려져 fullPage 캡처가 처음 viewport 밖을 비워 둔다.
 */
async function shot(page, name) {
  await page.evaluate(() => window.settleSize());
  const size = page.viewportSize();
  const height = await page.evaluate(() => document.documentElement.scrollHeight);
  await page.setViewportSize({ width: size.width, height: Math.max(height, size.height) });
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  await page.screenshot({ path: join(SHOTS, name) });
  await page.setViewportSize(size);
}


describe('비교 화면(MCP App) — 호스트 흉내', { skip: executablePath ? false : '설치된 Chromium 을 찾지 못함(CHROME_PATH 로 지정)' }, () => {
  let srv;
  let client;
  let browser;
  let html;
  let hostJs;
  /**
   * 호스트 쪽 장애 흉내. 도구 이름이 맞는 다음 호출 하나에 한 번씩 적용한다.
   *   network: 서버에 닿기 전에 끊김
   *   after:   서버는 처리했는데 응답 전에 시간 초과
   *   chatgpt: 서버는 처리했고, 결과가 저장 실패(ok=false·isError)면 호스트가 긴 -32000 오류로 감싸 던짐
   *            (실제 ChatGPT 에서 본 모양)
   * @type {{ mode: 'network'|'after'|'chatgpt', tool: string }[]}
   */
  let faults = [];
  const CHATGPT_WRAPPED =
    'MCP error -32000: {"detail":"Error code: INVALID_ARGUMENT; Error: RuntimeException: Error calling MCP tool: ' +
    "[TextContent(type='text', text='[conflict] 이 비교에는 이미 다른 선택이 저장돼 있어요.', annotations=None, meta=None)]\"}";
  /** 화면에 새면 안 되는 원문 오류 조각. */
  const RAW = /MCP error|TextContent|INVALID_ARGUMENT|RuntimeException|detail|\{|-320/;

  async function callWithFaults(params) {
    const i = faults.findIndex((f) => f.tool === params.name);
    const fault = i >= 0 ? faults.splice(i, 1)[0] : null;
    if (fault?.mode === 'network') throw new Error('fetch failed: ECONNRESET');
    const result = await client.callTool(params);
    if (fault?.mode === 'after') throw new Error('MCP error -32001: Request timed out');
    if (fault?.mode === 'chatgpt' && (result.isError || result.structuredContent?.ok === false)) {
      throw new Error(CHATGPT_WRAPPED);
    }
    return result;
  }

  async function visibleText(frame) {
    return frame.locator('#root').innerText();
  }

  async function saveVote(id, args) {
    return client.callTool({ name: TOOL_NAMES.vote, arguments: { comparison_id: id, ...args } });
  }

  async function serverVote(id) {
    return (await client.callTool({ name: TOOL_NAMES.get, arguments: { comparison_id: id } })).structuredContent.vote;
  }

  before(async () => {
    srv = await startServer();
    client = await connectClient(srv.url);
    // 화면 HTML 은 호스트처럼 resources/read 로 받는다.
    html = (await client.readResource({ uri: UI_URI })).contents[0].text;
    hostJs = await readFile(HOST_JS, 'utf8');
    browser = await chromium.launch({ executablePath, headless: true });
    await mkdir(SHOTS, { recursive: true });
  });
  after(async () => {
    await browser?.close();
    await client?.close();
    await srv?.close();
  });

  async function openHost({ viewport = { width: 760, height: 900 }, theme = 'light', toolInput = null } = {}) {
    const page = await browser.newPage({ viewport });
    const errors = [];
    page.on('pageerror', (e) => errors.push(e));
    await page.exposeFunction('__callTool', callWithFaults);
    await page.setContent(
      `<!doctype html><html><body style="margin:0;background:#ddd"><div id="frame-wrap" style="max-width:${viewport.width}px;margin:0 auto"></div></body></html>`,
    );
    await page.addScriptTag({ content: hostJs });
    await page.evaluate((args) => window.startHost(args), { html, theme, toolInput });
    const frame = page.frameLocator('iframe');
    return { page, frame, errors };
  }

  async function createComparison(situation = 'daily', mood = 'amekaji') {
    return client.callTool({ name: TOOL_NAMES.create, arguments: { situation, mood } });
  }

  test('로딩 → 렌더 → B·이유·메모 → 저장 → 서버에 남음 (데스크톱·모바일 스크린샷)', async () => {
    const { page, frame, errors } = await openHost({ toolInput: { situation: 'daily', mood: 'amekaji' } });
    await frame.getByText('샘플 코디를 불러오는 중...').waitFor();
    await shot(page, '01-loading-desktop.png');

    const result = await createComparison();
    await page.evaluate((r) => window.sendToolResult(r), result);
    await frame.getByText('어느 쪽이 더 끌리나요?').waitFor();
    assert.ok(await frame.getByText('데모', { exact: true }).isVisible());
    assert.ok(await frame.getByText(/실제 옷장이나 AI 추천 결과가 아니에요/).isVisible());
    assert.equal(await frame.locator('label.option').count(), 2);
    // A·B 카드는 같은 모양(같은 아이템 수가 아니어도 같은 구조·같은 클래스)이다.
    const classes = await frame.locator('label.option').evaluateAll((els) => els.map((e) => e.className));
    assert.deepEqual(classes, ['option', 'option']);
    const saveBtn = frame.getByRole('button', { name: '선택 저장' });
    assert.equal(await saveBtn.isDisabled(), true, '고르기 전에는 저장할 수 없다');
    await shot(page, '02-ready-desktop.png');

    await frame.locator('label.option[data-slot="b"]').click();
    await frame.getByText('실루엣').click();
    await frame.getByText('색 조합').click();
    const memo = '  바지 핏이 좋아요  ';
    await frame.locator('textarea').fill(memo);
    await frame.getByText(`${[...memo].length}/200`, { exact: true }).waitFor();
    await shot(page, '03-selected-desktop.png');
    await saveBtn.click();
    await frame.getByText('선택을 저장했어요.').waitFor();
    await frame.getByText('B 코디를 골랐어요.').waitFor();
    await frame.getByText('고른 이유: 색 조합, 실루엣').waitFor();
    await shot(page, '04-saved-desktop.png');

    const saved = await client.callTool({ name: TOOL_NAMES.get, arguments: { comparison_id: result.structuredContent.id } });
    assert.deepEqual(saved.structuredContent.vote.reasons, ['color_combo', 'silhouette']);
    assert.equal(saved.structuredContent.vote.choice, 'b');
    assert.equal(saved.structuredContent.vote.comment, '바지 핏이 좋아요');

    const calls = await page.evaluate(() => window.__toolCalls);
    // 호스트 결과를 받으면 먼저 지금 상태를 읽고(get), 고른 뒤 저장한다(submit).
    assert.deepEqual(calls.map((c) => c.name), [TOOL_NAMES.get, TOOL_NAMES.vote]);
    assert.equal(calls[1].arguments.comparison_id, result.structuredContent.id);
    const ctx = await page.evaluate(() => window.__modelContext);
    assert.equal(ctx.length, 1);
    assert.match(ctx[0].content[0].text, /B 코디/);
    assert.deepEqual(errors, []);
    await page.close();

    // 같은 비교를 모바일 너비로 다시 열면 저장된 결과가 보인다(get 결과).
    const mobile = await openHost({ viewport: { width: 375, height: 812 } });
    const again = await client.callTool({ name: TOOL_NAMES.get, arguments: { comparison_id: result.structuredContent.id } });
    await mobile.page.evaluate((r) => window.sendToolResult(r), again);
    await mobile.frame.getByText('B 코디를 골랐어요.').waitFor();
    await shot(mobile.page, '05-saved-mobile.png');
    await mobile.page.close();
  });

  test('모바일: 카드가 한 줄씩 쌓이고, 둘 다 아쉬움이면 이유 제목이 "아쉬운 점"', async () => {
    const { page, frame } = await openHost({ viewport: { width: 375, height: 812 } });
    const result = await createComparison('office', 'minimal');
    await page.evaluate((r) => window.sendToolResult(r), result);
    await frame.getByText('어느 쪽이 더 끌리나요?').waitFor();
    const boxes = await frame.locator('label.option').evaluateAll((els) => els.map((e) => e.getBoundingClientRect().toJSON()));
    assert.ok(boxes[1].top >= boxes[0].bottom - 1, '좁은 화면에서는 위아래로 쌓인다');
    assert.ok(Math.abs(boxes[0].width - boxes[1].width) < 1, '두 카드 너비가 같다');
    await shot(page, '06-ready-mobile.png');

    await frame.getByText('둘 다 아쉬워요', { exact: true }).click();
    await frame.getByText('아쉬운 점 (선택)').waitFor();
    await frame.getByText('날씨 적합성').click();
    await frame.getByRole('button', { name: '선택 저장' }).click();
    await frame.getByText('둘 다 아쉽다고 남겼어요.').waitFor();
    await frame.getByText('아쉬운 점: 날씨 적합성').waitFor();
    await shot(page, '07-neither-saved-mobile.png');
    const saved = await client.callTool({ name: TOOL_NAMES.get, arguments: { comparison_id: result.structuredContent.id } });
    assert.equal(saved.structuredContent.vote.choice, 'neither');
    await page.close();
  });

  test('건너뛰기는 아무것도 저장하지 않고, 다시 고를 수 있다', async () => {
    const { page, frame } = await openHost();
    const result = await createComparison('weekend', 'minimal');
    await page.evaluate((r) => window.sendToolResult(r), result);
    await frame.locator('label.option[data-slot="a"]').click();
    await frame.getByRole('button', { name: '건너뛰기' }).click();
    await frame.getByText('건너뛰었어요. 아무것도 저장하지 않았어요.').waitFor();
    await shot(page, '08-skipped-desktop.png');
    const calls = (await page.evaluate(() => window.__toolCalls)).map((c) => c.name);
    assert.deepEqual(calls, [TOOL_NAMES.get], '건너뛰기는 저장 도구를 부르지 않는다');
    const read = await client.callTool({ name: TOOL_NAMES.get, arguments: { comparison_id: result.structuredContent.id } });
    assert.equal(read.structuredContent.vote, null);
    await frame.getByRole('button', { name: '다시 고르기' }).click();
    await frame.getByText('어느 쪽이 더 끌리나요?').waitFor();
    await page.close();
  });

  test('저장 실패 → 오류와 다시 시도 → 고른 내용 그대로 저장', async () => {
    const { page, frame } = await openHost();
    const result = await createComparison('daily', 'minimal');
    await page.evaluate((r) => window.sendToolResult(r), result);
    await frame.locator('label.option[data-slot="a"]').click();
    await frame.getByText('내 취향').click();
    faults = [{ mode: 'network', tool: TOOL_NAMES.vote }];
    await frame.getByRole('button', { name: '선택 저장' }).click();
    await frame.getByText(/저장하지 못했어요/).waitFor();
    assert.doesNotMatch(await visibleText(frame), RAW);
    await shot(page, '09-save-error-desktop.png');
    assert.equal(await frame.locator('label.option[data-slot="a"] input').isChecked(), true, '고른 내용이 남아 있다');
    await frame.getByRole('button', { name: '다시 시도' }).click();
    await frame.getByText('A 코디를 골랐어요.').waitFor();
    const read = await client.callTool({ name: TOOL_NAMES.get, arguments: { comparison_id: result.structuredContent.id } });
    assert.deepEqual(read.structuredContent.vote.reasons, ['personal_taste']);
    await page.close();
  });

  test('다른 선택이 먼저 저장돼 있으면 충돌 안내와 저장된 쪽을 보여 준다', async () => {
    const { page, frame } = await openHost();
    const result = await createComparison('office', 'amekaji');
    await page.evaluate((r) => window.sendToolResult(r), result);
    await frame.getByText('어느 쪽이 더 끌리나요?').waitFor();
    // 화면을 띄운 뒤 대화(모델)에서 먼저 A 를 저장했다.
    await client.callTool({ name: TOOL_NAMES.vote, arguments: { comparison_id: result.structuredContent.id, choice: 'a' } });
    await frame.locator('label.option[data-slot="b"]').click();
    await frame.getByRole('button', { name: '선택 저장' }).click();
    await frame.getByText(/이미 다른 선택이 저장돼 있어요/).waitFor();
    await frame.getByText('A 코디를 골랐어요.').waitFor();
    await shot(page, '10-conflict-desktop.png');
    await page.close();
  });

  test('모델이 직접 submit 을 부른 결과가 오면 저장된 결과를 그린다, 오류 결과면 오류를 그린다', async () => {
    const { page, frame } = await openHost({ theme: 'dark' });
    const created = await createComparison('weekend', 'amekaji');
    const vote = await client.callTool({
      name: TOOL_NAMES.vote,
      arguments: { comparison_id: created.structuredContent.id, choice: 'neither', comment: '둘 다 무거워요' },
    });
    await page.evaluate((r) => window.sendToolResult(r), vote);
    await frame.getByText('둘 다 아쉽다고 남겼어요.').waitFor();
    await frame.getByText('메모: 둘 다 무거워요').waitFor();
    assert.equal(await frame.locator('html').getAttribute('data-theme'), 'dark');
    await shot(page, '11-dark-saved-desktop.png');
    await page.close();

    const second = await openHost();
    const missing = await client.callTool({ name: TOOL_NAMES.get, arguments: { comparison_id: 'demo_cmp_' + '0'.repeat(32) } });
    assert.equal(missing._meta[ERROR_META_KEY].code, 'not_found');
    await second.page.evaluate((r) => window.sendToolResult(r), missing);
    await second.frame.getByText(/이 비교를 찾을 수 없어요/).waitFor();
    await shot(second.page, '12-not-found-desktop.png');
    await second.page.close();
  });
  test('오래된 create 카드를 다시 열면 서버에서 지금 상태를 읽어 처음부터 저장된 화면을 보여 준다', async () => {
    const created = await createComparison('daily', 'amekaji');
    const id = created.structuredContent.id;
    await saveVote(id, { choice: 'b', reasons: ['silhouette'] });

    const { page, frame } = await openHost();
    // 호스트는 대화에 남은 옛 결과(vote=null)를 그대로 다시 보낸다.
    assert.equal(created.structuredContent.vote, null);
    await page.evaluate((r) => window.sendToolResult(r), created);
    await frame.getByText('B 코디를 골랐어요.').waitFor();
    assert.equal(await frame.locator('label.option').count(), 0, '고르는 화면을 거치지 않는다');
    await frame.getByRole('button', { name: '선택 수정' }).waitFor();
    const calls = (await page.evaluate(() => window.__toolCalls)).map(({ name, arguments: args }) => ({ name, args }));
    assert.deepEqual(calls, [{ name: TOOL_NAMES.get, args: { comparison_id: id } }]);
    await shot(page, '13-reopened-old-card-saved.png');

    // 같은 옛 결과가 되풀이돼도 조회는 상한 안에서 끝난다(무한 반복 없음).
    for (let i = 0; i < 8; i++) await page.evaluate((r) => window.sendToolResult(r), created);
    await page.waitForTimeout(400);
    const gets = (await page.evaluate(() => window.__toolCalls)).filter((c) => c.name === TOOL_NAMES.get).length;
    assert.ok(gets <= 5, `get ${gets}번`);
    await frame.getByText('B 코디를 골랐어요.').waitFor();
    await page.close();
  });

  test('저장 상태 조회가 실패하면 짧은 안내와 다시 시도, 다시 시도하면 저장된 화면', async () => {
    const created = await createComparison('office', 'minimal');
    await saveVote(created.structuredContent.id, { choice: 'a' });
    const { page, frame } = await openHost();
    faults = [{ mode: 'network', tool: TOOL_NAMES.get }];
    await page.evaluate((r) => window.sendToolResult(r), created);
    await frame.getByText('저장 상태를 확인하지 못했어요. 잠시 후 다시 시도해 주세요.').waitFor();
    assert.doesNotMatch(await visibleText(frame), RAW);
    await shot(page, '14-sync-failed.png');
    await frame.getByRole('button', { name: '다시 시도' }).click();
    await frame.getByText('A 코디를 골랐어요.').waitFor();
    await page.close();
  });

  test('선택 수정: 저장된 값이 채워지고, B→둘 다 아쉬움·이유·메모 변경 저장, 바뀐 것 없으면 저장 불가, 취소하면 원래대로', async () => {
    const created = await createComparison('weekend', 'minimal');
    const id = created.structuredContent.id;
    await saveVote(id, { choice: 'b', reasons: ['color_combo'], comment: '색이 좋아요' });
    const { page, frame } = await openHost({ viewport: { width: 375, height: 812 } });
    await page.evaluate((r) => window.sendToolResult(r), created);
    await frame.getByText('B 코디를 골랐어요.').waitFor();

    // 취소
    await frame.getByRole('button', { name: '선택 수정' }).click();
    await frame.getByText('선택 수정 — 어느 쪽이 더 끌리나요?').waitFor();
    assert.equal(await frame.locator('label.option[data-slot="b"] input').isChecked(), true);
    assert.equal(await frame.locator('input[value="color_combo"]').isChecked(), true);
    assert.equal(await frame.locator('textarea').inputValue(), '색이 좋아요');
    assert.equal(await frame.getByRole('button', { name: '수정 저장' }).isDisabled(), true, '바뀐 것이 없으면 저장할 수 없다');
    assert.equal(await frame.getByRole('button', { name: '건너뛰기' }).count(), 0, '수정 중에는 건너뛰기 대신 취소');
    await frame.locator('label.option[data-slot="a"]').click();
    await frame.getByRole('button', { name: '취소' }).click();
    await frame.getByText('B 코디를 골랐어요.').waitFor();
    assert.equal((await serverVote(id)).choice, 'b');
    assert.equal((await page.evaluate(() => window.__toolCalls)).filter((c) => c.name === TOOL_NAMES.update).length, 0);

    // B → 둘 다 아쉬움, 이유 바꾸기
    await frame.getByRole('button', { name: '선택 수정' }).click();
    await frame.getByText('둘 다 아쉬워요', { exact: true }).click();
    await frame.getByText('아쉬운 점 (선택)').waitFor();
    await frame.locator('input[value="color_combo"]').uncheck();
    await frame.getByText('날씨 적합성').click();
    await shot(page, '15-edit-form-mobile.png');
    await frame.getByRole('button', { name: '수정 저장' }).click();
    await frame.getByText('선택을 수정했어요.').waitFor();
    await frame.getByText('둘 다 아쉽다고 남겼어요.').waitFor();
    await frame.getByText('아쉬운 점: 날씨 적합성').waitFor();
    await frame.getByText(/1번 수정함/).waitFor();
    await shot(page, '16-edited-saved-mobile.png');
    let v = await serverVote(id);
    assert.equal(v.choice, 'neither');
    assert.deepEqual(v.reasons, ['weather_fit']);
    assert.equal(v.revision, 2);

    // 메모만 바꾸기
    await frame.getByRole('button', { name: '선택 수정' }).click();
    await frame.locator('textarea').fill('색이 좋아요 그래도 추워요');
    assert.equal(await frame.getByRole('button', { name: '수정 저장' }).isDisabled(), false);
    await frame.getByRole('button', { name: '수정 저장' }).click();
    await frame.getByText('메모: 색이 좋아요 그래도 추워요').waitFor();
    v = await serverVote(id);
    assert.equal(v.choice, 'neither');
    assert.equal(v.revision, 3);

    // 다시 A 로
    await frame.getByRole('button', { name: '선택 수정' }).click();
    await frame.locator('label.option[data-slot="a"]').click();
    await frame.getByRole('button', { name: '수정 저장' }).click();
    await frame.getByText('A 코디를 골랐어요.').waitFor();
    assert.equal((await serverVote(id)).revision, 4);
    await page.close();
  });

  test('두 화면: 오래된 화면의 수정은 덮어쓰지 않고 최신 선택과 안내를 보여 준다', async () => {
    const created = await createComparison('daily', 'minimal');
    const id = created.structuredContent.id;
    await saveVote(id, { choice: 'a' });
    const one = await openHost();
    const two = await openHost();
    for (const h of [one, two]) {
      await h.page.evaluate((r) => window.sendToolResult(r), created);
      await h.frame.getByText('A 코디를 골랐어요.').waitFor();
      await h.frame.getByRole('button', { name: '선택 수정' }).click();
    }
    await one.frame.locator('label.option[data-slot="b"]').click();
    await one.frame.getByRole('button', { name: '수정 저장' }).click();
    await one.frame.getByText('B 코디를 골랐어요.').waitFor();

    await two.frame.getByText('둘 다 아쉬워요', { exact: true }).click();
    await two.frame.getByRole('button', { name: '수정 저장' }).click();
    await two.frame.getByText(/그사이 다른 화면에서 선택이 바뀌었어요/).waitFor();
    await two.frame.getByText('B 코디를 골랐어요.').waitFor();
    await shot(two.page, '17-stale-edit.png');
    const v = await serverVote(id);
    assert.equal(v.choice, 'b');
    assert.equal(v.revision, 2);

    // 같은 iframe 에 다른 비교의 결과가 오면 그 비교의 상태로 바뀐다(이전 비교의 선택을 섞지 않는다).
    const fresh = await createComparison('weekend', 'amekaji');
    await two.page.evaluate((r) => window.sendToolResult(r), fresh);
    await two.frame.getByText('어느 쪽이 더 끌리나요?').waitFor();
    assert.equal(await two.frame.locator('input:checked').count(), 0);
    assert.match(await visibleText(two.frame), /주말 나들이/);
    await two.frame.locator('label.option[data-slot="a"]').click();
    await two.frame.getByRole('button', { name: '선택 저장' }).click();
    await two.frame.getByText('선택을 저장했어요.').waitFor();
    assert.equal((await serverVote(fresh.structuredContent.id)).choice, 'a');
    assert.equal((await serverVote(id)).choice, 'b');
    await one.page.close();
    await two.page.close();
  });

  test('저장·수정은 됐는데 응답이 시간 초과: 다시 읽어 성공으로 보여 주고 한 번만 반영', async () => {
    const created = await createComparison('office', 'amekaji');
    const id = created.structuredContent.id;
    const { page, frame } = await openHost();
    await page.evaluate((r) => window.sendToolResult(r), created);
    await frame.locator('label.option[data-slot="b"]').click();
    faults = [{ mode: 'after', tool: TOOL_NAMES.vote }];
    await frame.getByRole('button', { name: '선택 저장' }).click();
    await frame.getByText('선택을 저장했어요.').waitFor();
    assert.doesNotMatch(await visibleText(frame), RAW);

    await frame.getByRole('button', { name: '선택 수정' }).click();
    await frame.getByText('실루엣').click();
    faults = [{ mode: 'after', tool: TOOL_NAMES.update }];
    await frame.getByRole('button', { name: '수정 저장' }).click();
    await frame.getByText('선택을 수정했어요.').waitFor();
    assert.doesNotMatch(await visibleText(frame), RAW);
    const v = await serverVote(id);
    assert.deepEqual(v.reasons, ['silhouette']);
    assert.equal(v.revision, 2, '재전송하지 않고 다시 읽어 판정했다');
    const names = (await page.evaluate(() => window.__toolCalls)).map((c) => c.name);
    assert.equal(names.filter((n) => n === TOOL_NAMES.update).length, 1);
    await page.close();
  });

  test('ChatGPT형 -32000 오류로 감싼 conflict: 원문을 숨기고 저장된 선택과 안내를 보여 준다', async () => {
    const created = await createComparison('weekend', 'amekaji');
    const id = created.structuredContent.id;
    const { page, frame } = await openHost();
    await page.evaluate((r) => window.sendToolResult(r), created);
    await frame.getByText('어느 쪽이 더 끌리나요?').waitFor();
    // 화면이 열린 뒤 대화에서 먼저 A 를 저장했다.
    await saveVote(id, { choice: 'a' });
    await frame.locator('label.option[data-slot="b"]').click();
    faults = [{ mode: 'chatgpt', tool: TOOL_NAMES.vote }];
    await frame.getByRole('button', { name: '선택 저장' }).click();
    await frame.getByText(/이미 다른 선택이 저장돼 있어요. 바꾸려면 ‘선택 수정’을 눌러 주세요./).waitFor();
    await frame.getByText('A 코디를 골랐어요.').waitFor();
    assert.doesNotMatch(await visibleText(frame), RAW);
    await shot(page, '18-wrapped-conflict.png');
    assert.equal((await serverVote(id)).choice, 'a');

    // 같은 카드에서 명시적으로 수정하면 B 로 바뀐다.
    await frame.getByRole('button', { name: '선택 수정' }).click();
    await frame.locator('label.option[data-slot="b"]').click();
    await frame.getByRole('button', { name: '수정 저장' }).click();
    await frame.getByText('B 코디를 골랐어요.').waitFor();
    assert.equal((await serverVote(id)).choice, 'b');
    await page.close();
  });

  test('네트워크 실패(저장·확인 모두): 입력을 지키고 짧은 한국어 안내, 다시 시도하면 저장', async () => {
    const created = await createComparison('daily', 'amekaji');
    const id = created.structuredContent.id;
    const { page, frame } = await openHost();
    await page.evaluate((r) => window.sendToolResult(r), created);
    await frame.locator('label.option[data-slot="b"]').click();
    await frame.getByText('실용성/착용 편의').click();
    await frame.locator('textarea').fill('편해 보여요');
    faults = [
      { mode: 'network', tool: TOOL_NAMES.vote },
      { mode: 'network', tool: TOOL_NAMES.get },
    ];
    await frame.getByRole('button', { name: '선택 저장' }).click();
    await frame.getByText('저장하지 못했어요. 연결을 확인하고 다시 시도해 주세요. 고른 내용은 그대로 있어요.').waitFor();
    const text = await visibleText(frame);
    assert.doesNotMatch(text, RAW);
    assert.doesNotMatch(text, /ECONNRESET|fetch failed/);
    assert.equal(await frame.locator('label.option[data-slot="b"] input').isChecked(), true);
    assert.equal(await frame.locator('input[value="practicality"]').isChecked(), true);
    assert.equal(await frame.locator('textarea').inputValue(), '편해 보여요');
    assert.equal(await serverVote(id), null);
    await shot(page, '19-network-failed-input-kept.png');
    await frame.getByRole('button', { name: '다시 시도' }).click();
    await frame.getByText('선택을 저장했어요.').waitFor();
    const v = await serverVote(id);
    assert.equal(v.comment, '편해 보여요');
    assert.deepEqual(v.reasons, ['practicality']);
    await page.close();
  });
});

/** 실제 로컬 asset(mcp/.assets, gitignore). 4장이 모두 검사를 통과할 때만 실제 사진 화면을 찍는다. */
const REAL_ASSETS = fileURLToPath(new URL('../.assets/', import.meta.url));
const realAssets = await loadImageAssets(REAL_ASSETS);
const hasRealAssets = PHOTO_ASSET_KEYS.every((k) => realAssets.byKey.has(k));

/** 사진 비교 화면 묶음. 서버·asset 을 받아 호스트 흉내를 연다. */
function photoSuite(name, { assetsDir, real }) {
  const skip = !executablePath ? '설치된 Chromium 을 찾지 못함' : !assetsDir ? '로컬 실제 asset 없음(mcp/.assets)' : false;
  describe(name, { skip }, () => {
    let srv;
    let client;
    let browser;
    let html;
    let hostJs;
    let testAssets;

    before(async () => {
      let dir = assetsDir;
      if (dir === 'generated') {
        testAssets = await makeTestAssets();
        dir = testAssets.dir;
      }
      srv = await startServer({ assetsDir: dir });
      client = await connectClient(srv.url);
      html = (await client.readResource({ uri: UI_URI })).contents[0].text;
      hostJs = await readFile(HOST_JS, 'utf8');
      browser = await chromium.launch({ executablePath, headless: true });
      await mkdir(SHOTS, { recursive: true });
    });
    after(async () => {
      await browser?.close();
      await client?.close();
      await srv?.close();
      await testAssets?.cleanup();
    });

    async function open(viewport = { width: 760, height: 900 }) {
      const page = await browser.newPage({ viewport });
      const errors = [];
      page.on('pageerror', (e) => errors.push(e));
      await page.exposeFunction('__callTool', (params) => client.callTool(params));
      await page.setContent(
        `<!doctype html><html><body style="margin:0;background:#ddd"><div id="frame-wrap" style="max-width:${viewport.width}px;margin:0 auto"></div></body></html>`,
      );
      await page.addScriptTag({ content: hostJs });
      await page.evaluate((args) => window.startHost(args), { html, theme: 'light' });
      return { page, frame: page.frameLocator('iframe'), errors };
    }

    async function photoComparison(set) {
      return client.callTool({ name: TOOL_NAMES.photo, arguments: { set } });
    }

    /** 사진이 다 뜰 때까지 기다리고 [naturalWidth, naturalHeight] 를 돌려준다. */
    async function loadedImages(frame) {
      const imgs = frame.locator('.photo-frame img');
      await imgs.first().waitFor();
      return imgs.evaluateAll(async (els) => {
        await Promise.all(els.map((el) => (el.complete ? null : new Promise((r) => el.addEventListener('load', r, { once: true })))));
        return els.map((el) => [el.naturalWidth, el.naturalHeight, el.getBoundingClientRect().toJSON()]);
      });
    }

    for (const set of Object.keys(PHOTO_SETS)) {
      test(`${set}: 두 사진이 같은 크기 칸에 전신 그대로 뜨고, 고르고 저장·수정된다 (데스크톱·모바일)`, async () => {
        const created = await photoComparison(set);
        assert.equal(created.isError, undefined);
        const id = created.structuredContent.id;
        for (const [label, viewport] of [
          ['desktop', { width: 760, height: 900 }],
          ['mobile', { width: 375, height: 812 }],
        ]) {
          const { page, frame, errors } = await open(viewport);
          await page.evaluate((r) => window.sendToolResult(r), created);
          await frame.getByText('어느 쪽이 더 끌리나요?').waitFor();
          await frame.getByText(/이전에 만들어 둔 생성 룩북 사진이에요/).waitFor();
          const imgs = await loadedImages(frame);
          assert.equal(imgs.length, 2);
          for (const [w, h] of imgs) {
            assert.ok(w > 0 && h > 0, `naturalWidth ${w}`);
            if (real) assert.deepEqual([w, h], [1024, 1536], '생성 원본 그대로(축소 없음)');
          }
          const [ra, rb] = imgs.map((x) => x[2]);
          assert.ok(Math.abs(ra.width - rb.width) < 1 && Math.abs(ra.height - rb.height) < 1, 'A·B 사진 칸 크기가 같다');
          assert.ok(Math.abs(ra.top - rb.top) < 1, '나란히(모바일 포함)');
          assert.equal(await frame.locator('.photo-frame img').first().evaluate((el) => getComputedStyle(el).objectFit), 'contain');
          // alt 는 그 쪽 아이템을 말한다.
          const alts = await frame.locator('.photo-frame img').evaluateAll((els) => els.map((e) => e.alt));
          for (const [i, alt] of alts.entries()) {
            const first = created.structuredContent.options[i].items[0].name;
            assert.ok(alt.includes(first), `${alt} ⊃ ${first}`);
          }
          await shot(page, `${real ? '2' : '3'}0-photo-${set}-${label}${real ? '' : '-testpng'}.png`);
          assert.deepEqual(errors, []);
          await page.close();
        }

        // 저장 → 저장된 화면에도 사진 → 수정 → 다른 쪽
        const { page, frame } = await open({ width: 760, height: 900 });
        await page.evaluate((r) => window.sendToolResult(r), created);
        await frame.locator('label.option[data-slot="b"]').click();
        await frame.getByText('실루엣').click();
        await frame.getByRole('button', { name: '선택 저장' }).click();
        await frame.getByText('B 코디를 골랐어요.').waitFor();
        assert.equal((await loadedImages(frame)).length, 2, '저장된 화면에도 두 사진');
        if (real) await shot(page, `2${set === 'pop_color_pattern' ? '1' : '2'}-photo-${set}-saved.png`);
        await frame.getByRole('button', { name: '선택 수정' }).click();
        await frame.locator('label.option[data-slot="a"]').click();
        await frame.getByRole('button', { name: '수정 저장' }).click();
        await frame.getByText('A 코디를 골랐어요.').waitFor();
        const v = (await client.callTool({ name: TOOL_NAMES.get, arguments: { comparison_id: id } })).structuredContent.vote;
        assert.equal(v.choice, 'a');
        assert.equal(v.revision, 2);
        await page.close();
      });
    }

    test('사진을 못 불러오면 같은 크기 칸에 대체 문구, 카드는 그대로 골라 저장된다', async () => {
      const created = await photoComparison('edgy_leather');
      const broken = created.structuredContent.options[0].image.url;
      const { page, frame } = await open({ width: 375, height: 812 });
      await page.route(broken, (route) => route.abort('failed'));
      await page.evaluate((r) => window.sendToolResult(r), created);
      await frame.getByText('사진을 불러오지 못했어요. 아래 아이템으로 골라 주세요.').waitFor();
      assert.equal(await frame.locator('.photo-frame img').count(), 1, '다른 쪽 사진은 그대로');
      const frames = await frame.locator('.photo-frame').evaluateAll((els) => els.map((e) => e.getBoundingClientRect().height));
      assert.ok(Math.abs(frames[0] - frames[1]) < 1, '칸 크기 유지');
      await shot(page, `${real ? '24' : '34'}-photo-broken-fallback-mobile.png`);
      await frame.locator('label.option[data-slot="a"]').click();
      await frame.getByRole('button', { name: '선택 저장' }).click();
      await frame.getByText('A 코디를 골랐어요.').waitFor();
      await page.close();
    });

    test('같은 iframe 에 글자 비교 결과가 오면 사진 없이 글자·색 점으로 보인다', async () => {
      const text = await client.callTool({ name: TOOL_NAMES.create, arguments: { situation: 'office', mood: 'minimal' } });
      const { page, frame } = await open();
      await page.evaluate((r) => window.sendToolResult(r), text);
      await frame.getByText('어느 쪽이 더 끌리나요?').waitFor();
      assert.equal(await frame.locator('.photo').count(), 0);
      await frame.getByText(/샘플 옷장으로 미리 짜 둔 고정 코디예요/).waitFor();
      await page.close();
    });
  });
}

photoSuite('사진 비교 화면 — 테스트 PNG', { assetsDir: 'generated', real: false });
photoSuite('사진 비교 화면 — 로컬 실제 생성 사진', { assetsDir: hasRealAssets ? REAL_ASSETS : null, real: true });
