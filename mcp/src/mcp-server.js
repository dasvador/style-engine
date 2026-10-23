// MCP 도구·UI 리소스 정의. 요청마다 새 McpServer 를 만들어(stateless) 같은 저장소에 붙인다.
//
// 도구는 넷뿐이다. 파일·URL·셸·임의 HTTP 를 다루는 도구는 없고, 실제 서비스(3003)나 DB 에
// 닿는 경로도 없다. 모두 데모 fixture 와 로컬 데모 저장소만 쓴다.

import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { RESOURCE_MIME_TYPE, registerAppResource, registerAppTool } from '@modelcontextprotocol/ext-apps/server';
import * as z from 'zod';

import { CATEGORIES, MOODS, SITUATIONS } from './fixtures.js';
import { COMPARISON_ID_PATTERN, DEMO_NOTICE, StoreError } from './demo-store.js';
import { CHOICES, COMMENT_RAW_MAX_CHARS, REASONS } from './vote.js';

export const SERVER_INFO = { name: 'wardrobe-edit-demo', version: '0.1.0' };
export const UI_URI = 'ui://wardrobe-edit/compare-v1.html';

export const TOOL_NAMES = {
  browse: 'browse_sample_wardrobe',
  create: 'create_demo_comparison',
  get: 'get_demo_comparison',
  vote: 'submit_demo_preference',
};

const situationEnum = z.enum(Object.keys(SITUATIONS));
const moodEnum = z.enum(Object.keys(MOODS));
const comparisonId = z
  .string()
  .max(64)
  .regex(COMPARISON_ID_PATTERN, 'create_demo_comparison 이 돌려준 id(demo_cmp_…)만 받아요.');

const itemSchema = z.object({
  id: z.string(),
  name: z.string(),
  category: z.string(),
  color: z.string().nullable(),
  sub_category: z.string().nullable(),
  material: z.string().nullable(),
  has_image: z.boolean(),
});

const comparisonSchema = z.object({
  demo: z.literal(true),
  data_source: z.literal('sample_fixture'),
  notice: z.string(),
  id: z.string(),
  created_at: z.string(),
  expires_at: z.string(),
  context: z.object({
    situation: z.string(),
    situation_label: z.string(),
    mood_key: z.string(),
    mood_label: z.string(),
    temperature: z.number(),
    weather_description: z.string(),
  }),
  options: z.array(z.object({ slot: z.enum(['a', 'b']), items: z.array(itemSchema) })).length(2),
  vote: z
    .object({
      choice: z.enum(CHOICES),
      reasons: z.array(z.enum(REASONS)),
      comment: z.string().nullable(),
      submitted_at: z.string(),
    })
    .nullable(),
});

const DEMO_BOUNDARY =
  '데모 전용: 고정된 샘플 옷장과 미리 짜 둔 샘플 코디만 다루며, 실제 사용자 옷장·실제 추천 엔진·AI 생성 결과가 아니다.';

/** 오류 결과의 기계용 세부(code·message·저장된 비교)를 싣는 `_meta` 키. */
export const ERROR_META_KEY = 'wardrobe-edit/error';

/**
 * 오류 결과. structuredContent 는 넣지 않는다 — 공식 SDK 클라이언트는 isError 결과도 출력 스키마로
 * 검사하므로 오류 모양을 넣으면 호스트에서 깨진다. 모델은 content 글자를, 화면은 `_meta` 를 읽는다.
 */
function toolError(code, message, comparison = null) {
  return {
    isError: true,
    content: [{ type: 'text', text: `[${code}] ${message}` }],
    _meta: { [ERROR_META_KEY]: { code, message, ...(comparison ? { comparison } : {}) } },
  };
}

function describeComparison(view) {
  const lines = view.options.map(
    (o) => `${o.slot.toUpperCase()}: ${o.items.map((i) => `${i.name}(${i.color ?? '색 미기재'})`).join(', ')}`,
  );
  const voted = view.vote ? `\n저장된 선택: ${view.vote.choice}` : '\n아직 선택 없음.';
  return `${DEMO_NOTICE}\n상황 ${view.context.situation_label} · 무드 ${view.context.mood_label} · ${view.context.weather_description} ${view.context.temperature}°C\n비교 id: ${view.id}\n${lines.join('\n')}${voted}`;
}

/**
 * @param {{ store: import('./demo-store.js').DemoStore, uiHtml: string }} deps
 */
export function buildMcpServer({ store, uiHtml }) {
  const server = new McpServer(SERVER_INFO, {
    instructions:
      `${DEMO_BOUNDARY} 사용자에게 결과를 전할 때 데모·샘플이라는 점을 밝힌다. ` +
      'submit_demo_preference 는 사용자가 대화나 화면에서 직접 A·B·둘 다 아쉬움을 밝혔을 때만 부른다 — 대신 고르지 않는다.',
  });

  registerAppResource(
    server,
    'Wardrobe Edit 데모 코디 비교 화면',
    UI_URI,
    {
      description: '샘플 코디 두 개를 나란히 보고 A·B·둘 다 아쉬움을 고르는 데모 화면.',
      _meta: { ui: { prefersBorder: true } },
    },
    async () => ({
      contents: [
        {
          uri: UI_URI,
          mimeType: RESOURCE_MIME_TYPE,
          text: uiHtml,
          // 외부 스크립트·이미지·네트워크를 쓰지 않는다. 화면의 서버 호출은 호스트 bridge(tools/call)로만 간다.
          _meta: { ui: { prefersBorder: true, csp: { connectDomains: [], resourceDomains: [] } } },
        },
      ],
    }),
  );

  server.registerTool(
    TOOL_NAMES.browse,
    {
      title: '샘플 옷장 보기(데모)',
      description:
        `${DEMO_BOUNDARY} 데모용 샘플 옷장 아이템 목록을 돌려준다. 읽기만 하고 아무것도 저장하지 않는다. ` +
        'category 를 주면 그 카테고리만 돌려준다.',
      inputSchema: z.strictObject({
        category: z.enum(CATEGORIES).optional().describe('아우터·상의·하의·신발·가방 중 하나(선택).'),
      }),
      outputSchema: z.object({
        demo: z.literal(true),
        data_source: z.literal('sample_fixture'),
        notice: z.string(),
        categories: z.array(z.string()),
        items: z.array(itemSchema),
      }),
      annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false },
    },
    async ({ category }) => {
      const result = store.listWardrobe(category);
      const text = result.items.map((i) => `- [${i.category}] ${i.name} · ${i.color}`).join('\n');
      return { content: [{ type: 'text', text: `${result.notice}\n${text}` }], structuredContent: result };
    },
  );

  registerAppTool(
    server,
    TOOL_NAMES.create,
    {
      title: '샘플 코디 두 개 비교 열기(데모)',
      description:
        `${DEMO_BOUNDARY} 상황(situation)과 무드(mood)에 맞춰 미리 짜 둔 샘플 코디 두 개로 새 A/B 비교를 만들고 비교 화면을 띄운다. ` +
        '부를 때마다 새 비교 id 가 생기고, 보여 준 두 코디가 데모 저장소에 기록된다(선택은 기록하지 않는다). ' +
        `situation: ${Object.entries(SITUATIONS).map(([k, v]) => `${k}=${v}`).join(', ')}. ` +
        `mood: ${Object.entries(MOODS).map(([k, v]) => `${k}=${v}`).join(', ')}.`,
      inputSchema: z.strictObject({
        situation: situationEnum.describe('상황 키'),
        mood: moodEnum.describe('무드 키'),
      }),
      outputSchema: comparisonSchema,
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: false, openWorldHint: false },
      _meta: { ui: { resourceUri: UI_URI } },
    },
    async ({ situation, mood }) => {
      try {
        const view = await store.createComparison({ situation, mood });
        return { content: [{ type: 'text', text: describeComparison(view) }], structuredContent: view };
      } catch (e) {
        if (e instanceof StoreError) return toolError(e.code, e.message);
        throw e;
      }
    },
  );

  registerAppTool(
    server,
    TOOL_NAMES.get,
    {
      title: '데모 비교 다시 보기',
      description:
        `${DEMO_BOUNDARY} create_demo_comparison 이 만든 비교 하나와 저장된 선택(있으면)을 읽어 비교 화면에 다시 띄운다. 읽기만 한다.`,
      inputSchema: z.strictObject({ comparison_id: comparisonId }),
      outputSchema: comparisonSchema,
      annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false },
      _meta: { ui: { resourceUri: UI_URI } },
    },
    async ({ comparison_id }) => {
      const view = store.getComparison(comparison_id);
      if (!view) return toolError('not_found', '이 비교를 찾을 수 없어요. 만료됐거나 다른 비교 id 예요.');
      return { content: [{ type: 'text', text: describeComparison(view) }], structuredContent: view };
    },
  );

  registerAppTool(
    server,
    TOOL_NAMES.vote,
    {
      title: '데모 비교 선택 저장',
      description:
        `${DEMO_BOUNDARY} 사용자가 직접 밝힌 선택을 데모 비교 하나에 저장한다: choice 는 a, b, neither(둘 다 아쉬움). ` +
        '사용자가 고르지 않았으면 부르지 않는다(건너뛰기는 저장할 것이 없다). 이유(reasons)와 메모(comment, 200자)는 선택 사항. ' +
        '비교 하나에 한 번만 저장되고 먼저 저장한 것이 남는다 — 같은 내용을 다시 보내면 already_submitted, 다른 내용이면 conflict 오류. ' +
        '데모 로컬 파일에만 쓰며 실제 서비스 데이터는 바꾸지 않는다.',
      inputSchema: z.strictObject({
        comparison_id: comparisonId,
        choice: z.enum(CHOICES).describe('a, b, neither(둘 다 아쉬움)'),
        reasons: z
          .array(z.enum(REASONS))
          .max(REASONS.length)
          .optional()
          .describe('A·B 면 고른 이유, neither 면 아쉬운 점. color_combo·silhouette·personal_taste·practicality·weather_fit'),
        comment: z.string().max(COMMENT_RAW_MAX_CHARS).optional().describe('한 줄 메모(앞뒤 공백 제거 후 200자 이내)'),
      }),
      outputSchema: z.object({
        ok: z.literal(true),
        already_submitted: z.boolean(),
        comparison: comparisonSchema,
      }),
      // 같은 내용을 다시 보내면 결과가 같다(idempotent). 저장된 선택을 덮어쓰지 않는다(destructive 아님).
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: true, openWorldHint: false },
      _meta: { ui: { resourceUri: UI_URI } },
    },
    async ({ comparison_id, choice, reasons, comment }) => {
      try {
        const { outcome, comparison } = await store.submitVote(comparison_id, { choice, reasons, comment });
        const already = outcome === 'already_submitted';
        return {
          content: [
            {
              type: 'text',
              text: `${already ? '같은 선택이 이미 저장돼 있어요' : '선택을 저장했어요'}(데모 기록): ${choice}. 비교 id ${comparison_id}.`,
            },
          ],
          structuredContent: { ok: true, already_submitted: already, comparison },
        };
      } catch (e) {
        if (e instanceof StoreError) {
          return toolError(e.code, e.message, e.comparison);
        }
        throw e;
      }
    },
  );

  return server;
}
