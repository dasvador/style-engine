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
import { connectClient, startServer } from './helpers.js';

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
  /** 다음 tools/call 몇 번을 호스트 쪽에서 실패시킨다(네트워크·호스트 오류 흉내). */
  let failNextCalls = 0;

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
    await page.exposeFunction('__callTool', async (params) => {
      if (failNextCalls > 0) {
        failNextCalls -= 1;
        throw new Error('호스트 연결이 끊겼어요(테스트)');
      }
      return client.callTool(params);
    });
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
    assert.deepEqual(calls.map((c) => c.name), [TOOL_NAMES.vote]);
    assert.equal(calls[0].arguments.comparison_id, result.structuredContent.id);
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
    assert.deepEqual(await page.evaluate(() => window.__toolCalls), [], '건너뛰기는 서버를 부르지 않는다');
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
    failNextCalls = 1;
    await frame.getByRole('button', { name: '선택 저장' }).click();
    await frame.getByText(/저장하지 못했어요/).waitFor();
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
});
