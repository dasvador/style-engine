// MCP 도구·UI 리소스 정의. 요청마다 새 McpServer 를 만들어(stateless) 같은 저장소에 붙인다.
//
// 데모 도구 다섯(사진 asset 이 준비돼 있으면 여섯): 샘플 옷장·미리 짜 둔 샘플 코디·이미 생성해 둔 룩북 사진으로 하는 A/B 비교와
// 선택 저장. 데모 fixture 와 이 서버의 로컬 데모 저장소만 쓴다.
// live 모드(계정 연결 OAuth)에서는 연결된 계정 본인의 프로필·옷장을 읽는 도구 둘이 더해진다. 이 둘은 어댑터
// (adapters/live.js)를 통해 서비스 DB 를 SELECT 로만 읽는다 — Rust 서버(3003)를 거치지 않는다.
// 위임 키가 설정돼 있으면 실제 코디 추천 도구(recommend_my_outfit)도 나온다. 이것만 루프백의 Rust 추천 엔진을
// 부른다(recommend.js) — 모델 비용이 들고 추천 이력이 남는다.
// 파일·URL·셸·임의 HTTP 를 다루는 도구는 없다.

import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { ListToolsRequestSchema } from '@modelcontextprotocol/sdk/types.js';
import { RESOURCE_MIME_TYPE, registerAppResource, registerAppTool } from '@modelcontextprotocol/ext-apps/server';
import * as z from 'zod';

import { CATEGORIES, MOODS, SITUATIONS } from './fixtures.js';
import { PHOTO_SETS } from './photo-sets.js';
import { COMPARISON_ID_PATTERN, DEMO_NOTICE, StoreError } from './demo-store.js';
import { CHOICES, COMMENT_RAW_MAX_CHARS, REASONS } from './vote.js';

export const SERVER_INFO = { name: 'wardrobe-edit-demo', version: '0.3.0' };
/**
 * 화면 리소스. v2 는 선택 수정, v3 는 생성 룩북 사진(과 그 사진을 위한 CSP)이 생기며 올렸다 — 호스트가
 * 옛 HTML 을 캐시해 두었어도 새 도구 결과는 v3 를 가리켜 새 화면을 받는다. v1·v2 는 이미 대화에 남은
 * 옛 카드가 다시 열릴 때를 위한 별칭으로 같은 (새) HTML 과 CSP 를 돌려준다. 새 화면은 옛 결과 모양도 읽는다.
 */
export const UI_URI = 'ui://wardrobe-edit/compare-v3.html';
export const LEGACY_UI_URIS = ['ui://wardrobe-edit/compare-v1.html', 'ui://wardrobe-edit/compare-v2.html'];

export const TOOL_NAMES = {
  browse: 'browse_sample_wardrobe',
  create: 'create_demo_comparison',
  get: 'get_demo_comparison',
  vote: 'submit_demo_preference',
  update: 'update_demo_preference',
  photo: 'create_demo_photo_comparison',
  // live 모드(계정 연결)에서만
  myProfile: 'get_my_profile',
  myWardrobe: 'list_my_wardrobe',
  // live + 위임 키가 있을 때만
  recommend: 'recommend_my_outfit',
};

/**
 * 인증된 사용자 id. 언제나 SDK 가 검증한 토큰(extra.authInfo)에서만 꺼낸다 — 도구 인자로 받지 않는다.
 * demo 모드(인증 없음)에서는 null.
 */
export function ownerOf(extra) {
  const id = extra?.authInfo?.extra?.userId;
  return typeof id === 'string' && id.length > 0 ? id : null;
}

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

const imageSchema = z
  .object({
    url: z.string(),
    width: z.number(),
    height: z.number(),
    alt: z.string(),
    presentation: z.literal('generated_lookbook_photo'),
    source: z.literal('existing_generated_image'),
    matches: z.literal('items_of_this_option'),
  })
  .nullable();

const comparisonSchema = z.object({
  demo: z.literal(true),
  data_source: z.enum(['sample_fixture', 'curated_generated_lookbook']),
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
    photo_set: z.string().nullable(),
  }),
  options: z.array(z.object({ slot: z.enum(['a', 'b']), items: z.array(itemSchema), image: imageSchema })).length(2),
  vote: z
    .object({
      choice: z.enum(CHOICES),
      reasons: z.array(z.enum(REASONS)),
      comment: z.string().nullable(),
      submitted_at: z.string(),
      updated_at: z.string(),
      revision: z.number().int().min(1),
    })
    .nullable(),
});

/** 선택 저장·수정 결과. 예상 가능한 결과(이미 저장됨·충돌·오래된 화면)도 오류가 아니라 이 모양으로 온다. */
const voteResultSchema = z.object({
  ok: z.boolean(),
  outcome: z.enum(['saved', 'updated', 'already_submitted', 'conflict', 'stale', 'no_vote']),
  already_submitted: z.boolean(),
  comparison: comparisonSchema,
});

const voteInput = {
  choice: z.enum(CHOICES).describe('a, b, neither(둘 다 아쉬움)'),
  reasons: z
    .array(z.enum(REASONS))
    .max(REASONS.length)
    .optional()
    .describe('A·B 면 고른 이유, neither 면 아쉬운 점. color_combo·silhouette·personal_taste·practicality·weather_fit'),
  comment: z.string().max(COMMENT_RAW_MAX_CHARS).optional().describe('한 줄 메모(앞뒤 공백 제거 후 200자 이내)'),
};

const CHOICE_LABEL = { a: 'A 코디', b: 'B 코디', neither: '둘 다 아쉬움' };

/** 모델이 읽는 결과 문장. 저장하지 않은 경우를 저장한 것처럼 읽히지 않게 앞에 표시한다. */
function voteResultText(outcome, comparison) {
  const saved = comparison.vote ? CHOICE_LABEL[comparison.vote.choice] : '없음';
  const tail = `비교 id ${comparison.id}, 저장된 선택: ${saved}(revision ${comparison.vote?.revision ?? 0}). 데모 기록.`;
  switch (outcome) {
    case 'saved':
      return `[saved] 선택을 저장했어요. ${tail}`;
    case 'updated':
      return `[updated] 선택을 수정했어요. ${tail}`;
    case 'already_submitted':
      return `[already_submitted] 같은 선택이 이미 저장돼 있어요. 바뀐 것은 없어요. ${tail}`;
    case 'conflict':
      return `[conflict] 저장하지 않았어요 — 이 비교에는 이미 다른 선택이 있어요. 사용자가 바꾸기를 원하면 update_demo_preference 를 쓰세요. ${tail}`;
    case 'stale':
      return `[stale] 수정하지 않았어요 — 그사이 선택이 다른 곳에서 바뀌었어요. 최신 선택을 사용자에게 보여 주고 다시 확인하세요. ${tail}`;
    case 'no_vote':
      return `[no_vote] 수정하지 않았어요 — 아직 저장된 선택이 없어요. 처음 저장은 submit_demo_preference 로 하세요. ${tail}`;
    default:
      return tail;
  }
}

function voteResult(outcome, comparison) {
  const ok = ['saved', 'updated', 'already_submitted'].includes(outcome);
  return {
    content: [{ type: 'text', text: voteResultText(outcome, comparison) }],
    structuredContent: { ok, outcome, already_submitted: outcome === 'already_submitted', comparison },
  };
}

/** 데모 도구 설명의 머리말. live 모드에서도 이 도구들에는 그대로 맞다(실제 옷장을 읽는 도구는 따로 있다). */
const DEMO_BOUNDARY =
  '이 도구는 데모 전용: 고정된 샘플 옷장과 미리 짜 둔 샘플 코디만 다루며, 실제 사용자 옷장·실제 추천 엔진·AI 생성 결과가 아니다.';

const CHOICE_RULES =
  'submit_demo_preference 는 사용자가 대화나 화면에서 직접 A·B·둘 다 아쉬움을 밝혔을 때만 부른다 — 대신 고르지 않는다. ' +
  '이미 저장된 선택은 사용자가 바꾸겠다고 분명히 말했을 때만 update_demo_preference 로 바꾼다.';

/**
 * 서버 지시문(모델이 읽는다). 모드마다 실제로 하는 일을 그대로 말한다.
 *   - demo: 모든 도구가 샘플 데이터다.
 *   - live: 내 프로필·내 옷장은 연결된 계정의 실제 데이터(읽기 전용)이고, A/B 비교·사진은 여전히 샘플·기존 생성 사진 데모다.
 *     선택 기록은 연결된 계정에 귀속되지만 서비스 DB 가 아니라 이 서버의 데모 저장소에 남는다. 추천 엔진·옷 사진 등록은 아직 없다.
 */
export function serverInstructions(live) {
  if (!live) {
    return `데모 전용: 고정된 샘플 옷장과 미리 짜 둔 샘플 코디만 다루며, 실제 사용자 옷장·실제 추천 엔진·AI 생성 결과가 아니다. 사용자에게 결과를 전할 때 데모·샘플이라는 점을 밝힌다. ${CHOICE_RULES}`;
  }
  return (
    'Wardrobe Edit 계정 연결 서버다. ' +
    'get_my_profile·list_my_wardrobe 는 OAuth 로 연결된 사용자 본인 계정의 실제 프로필과 옷장을 읽는다(읽기 전용, 옷 사진은 싣지 않는다). ' +
    '그 밖의 A/B 비교 도구(create_demo_comparison·create_demo_photo_comparison·get_demo_comparison)는 샘플 옷장과 이미 생성해 둔 룩북 사진으로 하는 데모이며, ' +
    '사용자의 실제 옷장이나 추천 결과가 아니다 — 결과를 전할 때 실제 데이터와 데모를 구분해서 밝힌다. ' +
    'submit_demo_preference·update_demo_preference 로 저장한 선택은 연결된 계정에 귀속된 데모 기록이다(서비스 DB 가 아니라 이 서버의 데모 저장소). ' +
    (live.recommender
      ? 'recommend_my_outfit 는 실제 추천 엔진으로 내 옷장·저장된 지역의 지금 날씨·추천 이력을 써서 오늘 코디를 고른다. ' +
        '부를 때마다 AI 모델 비용이 들고 오늘의 추천이 내 추천 이력에 남으므로(착용 기록은 아니다) 사용자가 추천을 원할 때 한 번만 부르고, ' +
        '같은 조건으로 되풀이해 부르거나 실패했다고 바로 다시 부르지 않는다. 추천 결과는 데모 비교·데모 사진과 섞지 않는다. ' +
        '옷 사진 등록·코디 사진 생성은 아직 연결되지 않았다 — 해 주는 척하지 않는다. '
      : '실제 추천 엔진과 옷 사진 등록은 아직 연결되지 않았다 — 해 주는 척하지 않는다. ') +
    CHOICE_RULES
  );
}

/** 오류 결과의 기계용 세부(code·message·저장된 비교)를 싣는 `_meta` 키. */
export const ERROR_META_KEY = 'wardrobe-edit/error';

/**
 * 오류 결과. structuredContent 는 넣지 않는다 — 공식 SDK 클라이언트는 isError 결과도 출력 스키마로
 * 검사하므로 오류 모양을 넣으면 호스트에서 깨진다. 모델은 content 글자를, 화면은 `_meta` 를 읽는다.
 */
function toolError(code, message) {
  return {
    isError: true,
    content: [{ type: 'text', text: `[${code}] ${message}` }],
    _meta: { [ERROR_META_KEY]: { code, message } },
  };
}

function describeComparison(view) {
  const lines = view.options.map(
    (o) =>
      `${o.slot.toUpperCase()}: ${o.items.map((i) => `${i.name}(${i.color ?? '색 미기재'})`).join(', ')}` +
      (o.image ? ' [이전에 생성해 둔 룩북 사진 있음]' : ''),
  );
  const voted = view.vote
    ? `\n저장된 선택: ${view.vote.choice} (revision ${view.vote.revision})`
    : '\n아직 선택 없음.';
  return `${view.notice ?? DEMO_NOTICE}\n상황 ${view.context.situation_label} · 무드 ${view.context.mood_label} · ${view.context.weather_description} ${view.context.temperature}°C\n비교 id: ${view.id}\n${lines.join('\n')}${voted}`;
}

function registerUi(server, uri, uiHtml, resourceDomains) {
  const legacy = uri !== UI_URI;
  registerAppResource(
    server,
    legacy ? 'Wardrobe Edit 데모 코디 비교 화면(v1 별칭)' : 'Wardrobe Edit 데모 코디 비교 화면',
    uri,
    {
      description: legacy
        ? '이전 대화의 카드를 위한 별칭. 최신 화면(compare-v3)과 같은 HTML.'
        : '샘플 코디 두 개를 나란히 보고 A·B·둘 다 아쉬움을 고르고, 저장한 선택을 수정하는 데모 화면.',
      _meta: { ui: { prefersBorder: true } },
    },
    async () => ({
      contents: [
        {
          uri,
          mimeType: RESOURCE_MIME_TYPE,
          text: uiHtml,
          // 외부 스크립트·네트워크를 쓰지 않는다. 화면의 서버 호출은 호스트 bridge(tools/call)로만 간다.
          // 이미지는 이 서버 자신의 origin(허용 목록에 든 사진만 내보냄)에서만 받는다.
          _meta: { ui: { prefersBorder: true, csp: { connectDomains: [], resourceDomains } } },
        },
      ],
    }),
  );
}

/**
 * @param {{ store: import('./demo-store.js').DemoStore, uiHtml: string, imageOrigin?: string | null,
 *           live?: { adapter: import('./adapters/live.js').LiveAdapter, resourceMetadataUrl: string, scope: string } | null }} deps
 *   imageOrigin: 사진을 내보내는 이 서버의 origin(https://PUBLIC_HOST 또는 로컬 http). 사진이 없으면 쓰지 않는다.
 *   live: 계정 연결(OAuth) 모드. 주어지면 내 프로필·내 옷장 도구가 생기고 모든 도구가 oauth2 를 요구한다고 알린다.
 */
export function buildMcpServer({ store, uiHtml, imageOrigin = null, live = null }) {
  const photoSets = store.availablePhotoSets();
  const resourceDomains = photoSets.length > 0 && imageOrigin ? [imageOrigin] : [];
  const server = new McpServer(SERVER_INFO, { instructions: serverInstructions(live) });

  for (const uri of [UI_URI, ...LEGACY_UI_URIS]) {
    registerUi(server, uri, uiHtml, resourceDomains);
  }


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
    async ({ category }, extra) => {
      const denied = checkScope(extra, live, 'read');
      if (denied) return denied;
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
    async ({ situation, mood }, extra) => {
      const denied = checkScope(extra, live, 'write');
      if (denied) return denied;
      try {
        const view = await store.createComparison({ situation, mood, owner: ownerOf(extra) });
        return { content: [{ type: 'text', text: describeComparison(view) }], structuredContent: view };
      } catch (e) {
        if (e instanceof StoreError) return toolError(e.code, e.message);
        throw e;
      }
    },
  );

  if (photoSets.length > 0) {
    registerAppTool(
      server,
      TOOL_NAMES.photo,
      {
        title: '생성 룩북 사진 두 장 비교 열기(데모)',
        description:
          '데모 전용: 이 앱에서 이전에 이미 만들어 둔 생성 룩북 사진 두 장(각 사진은 그 착장의 아이템 목록과 짝지어 있음)으로 새 A/B 비교를 만들고 비교 화면을 띄운다. ' +
          '지금 새로 추천하거나 이미지를 생성하지 않으며 실제 사용자 옷장이 아니다. 사진이 준비된 세트만 고를 수 있다: ' +
          `${photoSets.map((k) => `${k}=${PHOTO_SETS[k].title}`).join('; ')}. ` +
          '다른 무드·상황의 사진은 없다 — 그때는 create_demo_comparison(글자·색 비교)을 쓴다. ' +
          '부를 때마다 새 비교 id 가 생기고 보여 준 두 코디가 데모 저장소에 기록된다. 선택 저장·수정은 submit/update 도구로 같다.',
        inputSchema: z.strictObject({ set: z.enum(photoSets).describe('사진 세트 키') }),
        outputSchema: comparisonSchema,
        annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: false, openWorldHint: false },
        _meta: { ui: { resourceUri: UI_URI } },
      },
      async ({ set }, extra) => {
        const denied = checkScope(extra, live, 'write');
        if (denied) return denied;
        try {
          const view = await store.createPhotoComparison({ set, owner: ownerOf(extra) });
          return { content: [{ type: 'text', text: describeComparison(view) }], structuredContent: view };
        } catch (e) {
          if (e instanceof StoreError) return toolError(e.code, e.message);
          throw e;
        }
      },
    );
  }

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
    async ({ comparison_id }, extra) => {
      const denied = checkScope(extra, live, 'read');
      if (denied) return denied;
      const view = store.getComparison(comparison_id, ownerOf(extra));
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
        `${DEMO_BOUNDARY} 사용자가 직접 밝힌 첫 선택을 데모 비교 하나에 저장한다: choice 는 a, b, neither(둘 다 아쉬움). ` +
        '사용자가 고르지 않았으면 부르지 않는다(건너뛰기는 저장할 것이 없다). 이유(reasons)와 메모(comment, 200자)는 선택 사항. ' +
        '이미 선택이 있으면 바꾸지 않는다: 같은 내용이면 outcome=already_submitted(ok=true), 다른 내용이면 outcome=conflict(ok=false, 저장 안 함)와 ' +
        '저장된 선택을 돌려준다. 바꾸기는 update_demo_preference 로만 한다. 데모 로컬 파일에만 쓰며 실제 서비스 데이터는 바꾸지 않는다.',
      inputSchema: z.strictObject({ comparison_id: comparisonId, ...voteInput }),
      outputSchema: voteResultSchema,
      // 같은 내용을 다시 보내면 결과가 같다(idempotent). 저장된 선택을 덮어쓰지 않는다(destructive 아님).
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: true, openWorldHint: false },
      _meta: { ui: { resourceUri: UI_URI } },
    },
    async ({ comparison_id, ...vote }, extra) => {
      const denied = checkScope(extra, live, 'write');
      if (denied) return denied;
      try {
        const { outcome, comparison } = await store.submitVote(comparison_id, vote, ownerOf(extra));
        return voteResult(outcome, comparison);
      } catch (e) {
        if (e instanceof StoreError) return toolError(e.code, e.message);
        throw e;
      }
    },
  );

  registerAppTool(
    server,
    TOOL_NAMES.update,
    {
      title: '데모 비교 선택 수정',
      description:
        `${DEMO_BOUNDARY} 이미 저장된 선택을 사용자가 분명히 바꾸겠다고 했을 때만 쓴다. 저장된 선택(choice·reasons·comment)을 통째로 새 값으로 바꾼다 — ` +
        '이유만 또는 메모만 바꿀 때도 남길 값을 모두 보낸다. expected_revision 에는 get_demo_comparison 등에서 본 vote.revision 을 넣는다. ' +
        '그 사이 다른 곳에서 바뀌었으면 outcome=stale(ok=false, 바꾸지 않음), 저장된 선택이 없으면 outcome=no_vote(ok=false), ' +
        '이미 같은 내용이면 outcome=already_submitted(ok=true, 바꾸지 않음). 바꾸면 outcome=updated 이고 revision 이 1 오른다. ' +
        '비교마다 선택은 하나만 남는다(이전 값은 지운다). 데모 로컬 파일에만 쓴다.',
      inputSchema: z.strictObject({
        comparison_id: comparisonId,
        expected_revision: z.number().int().min(1).max(1_000_000).describe('수정하기 전에 본 vote.revision'),
        ...voteInput,
      }),
      outputSchema: voteResultSchema,
      // 저장된 선택을 바꾼다(destructive). 같은 인자로 다시 불러도 결과 상태가 같다(idempotent).
      annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: true, openWorldHint: false },
      _meta: { ui: { resourceUri: UI_URI } },
    },
    async ({ comparison_id, expected_revision, ...vote }, extra) => {
      const denied = checkScope(extra, live, 'write');
      if (denied) return denied;
      try {
        const { outcome, comparison } = await store.updateVote(comparison_id, expected_revision, vote, ownerOf(extra));
        return voteResult(outcome, comparison);
      } catch (e) {
        if (e instanceof StoreError) return toolError(e.code, e.message);
        throw e;
      }
    },
  );

  if (live) registerLiveTools(server, live);
  declareSecuritySchemes(server, live);
  return server;
}

/**
 * 인증·권한이 모자랄 때의 도구 오류. ChatGPT 는 `_meta["mcp/www_authenticate"]` 를 보고 계정 연결(또는 권한
 * 추가) 화면을 띄운다.
 */
function authChallenge(live, { error, description, scope }) {
  return {
    isError: true,
    content: [{ type: 'text', text: `[${error}] ${description}` }],
    _meta: {
      'mcp/www_authenticate': [
        `Bearer resource_metadata="${live.resourceMetadataUrl}", error="${error}", error_description="${description}"` +
          (scope ? `, scope="${scope}"` : ''),
      ],
    },
  };
}

/**
 * live 모드에서 이 도구에 필요한 scope 가 토큰에 있는가. demo 모드(live 없음)는 통과.
 * 토큰 주체가 없으면(이론상 HTTP 층에서 이미 막힌다) 익명으로 진행하지 않고 거절한다.
 * @param {'read'|'write'|'recommend'} kind
 */
function checkScope(extra, live, kind) {
  if (!live) return null;
  if (!ownerOf(extra)) {
    return authChallenge(live, { error: 'invalid_token', description: 'Link your Wardrobe Edit account' });
  }
  const needed = { read: [live.scopes.read], write: [live.scopes.read, live.scopes.write], recommend: [live.scopes.read, live.scopes.recommend] }[kind];
  const granted = extra.authInfo.scopes ?? [];
  if (needed.every((s) => granted.includes(s))) return null;
  return authChallenge(live, {
    error: 'insufficient_scope',
    description: {
      read: 'Reading needs the wardrobe.read permission',
      write: 'Saving choices needs the comparisons.write permission',
      recommend: 'Outfit recommendations need the recommendations.write permission. Reconnect to grant it',
    }[kind],
    scope: needed.join(' '),
  });
}

// 실제 추천 인자. 웹 홈 화면 흐름과 같다: 상황은 기본 '일상'(웹 홈은 늘 일상), 성별 탭(female·male), 무드는 그
// 성별의 style_mood 목록(Rust StyleGenre 키) — 성별 없이 무드만은 받지 않는다.
export const RECOMMEND_SITUATIONS = ['일상', '출근', '데이트'];
export const RECOMMEND_GENDERS = ['female', 'male'];
export const RECOMMEND_MOODS = [
  'minimal', 'classic', 'street', 'sporty_casual',
  'amekaji', 'preppy', 'workwear', 'outdoor_casual',
  'romantic', 'modern_chic', 'bohemian', 'mannish', 'glam', 'edgy', 'pop',
];

/** 오늘의 추천·다른 조합·안 입은 옷 활용. */
const recommendItemSchema = z.object({ id: z.string(), name: z.string(), category: z.string(), reason: z.string() });
const recommendOutput = z.object({
  ok: z.boolean(),
  outcome: z.string(),
  message: z.string(),
  source: z.literal('live_recommendation_engine'),
  request: z.object({ situation: z.string(), gender: z.string().nullable(), mood: z.string().nullable() }),
  region: z.string().nullable().optional(),
  weather: z
    .object({ temperature: z.number().nullable(), apparent_temperature: z.number().nullable(), description: z.string().nullable() })
    .nullable()
    .optional(),
  weather_summary: z.string().nullable().optional(),
  notice: z.string().nullable().optional(),
  modes: z
    .array(
      z.object({
        mode: z.string(),
        label: z.string(),
        description: z.string(),
        summary: z.string(),
        reason: z.string(),
        verdict: z.string(),
        score: z.number().describe('내부 순위 점수. 확률이나 100점 만점이 아니며 100을 넘을 수 있다.'),
        tips: z.array(z.string()),
        items: z.array(recommendItemSchema),
      }),
    )
    .optional(),
  side_effects: z.string().optional(),
});

const RECOMMEND_MESSAGES = {
  empty_wardrobe: '추천할 옷이 옷장에 없어요(고른 성별 기준). 웹에서 옷을 먼저 등록하거나 성별 조건을 바꿔 주세요.',
  no_region: '날씨 기준 지역이 아직 없어요. 웹 화면에서 지역을 먼저 정해 주세요.',
  mood_not_for_gender: '고른 성별에서 쓸 수 없는 무드예요. 성별에 맞는 무드를 골라 주세요.',
  mood_needs_gender: '무드를 고르려면 성별(female 또는 male)도 같이 정해 주세요.',
  street_not_for_work: '스트리트 무드는 출근 상황에 추천하지 않아요. 상황을 일상이나 데이트로 바꿔 주세요.',
  no_candidates: '오늘 날씨에 맞는 코디 후보가 부족해요.',
  busy: '이미 추천을 만드는 중이에요. 끝난 뒤에 다시 요청해 주세요.',
  unavailable: '추천 서버에 연결할 수 없어요(서버가 꺼져 있을 수 있어요). 데모 결과로 바꾸지 않았어요.',
  timeout: '추천이 제한 시간 안에 끝나지 않아 기다리기를 멈췄어요. 서버에서는 추천이 끝나 이력이 남았을 수 있어요. 바로 다시 부르지 마세요.',
  delegation_rejected: '추천 서버가 이 연결의 위임을 받지 않았어요(서버 설정 문제). 운영자에게 알려 주세요.',
  invalid_request: '추천 서버가 이 요청 조건을 받지 않았어요(서버 설정이나 조건 문제일 수 있어요). 같은 조건으로 바로 다시 부르지 말고 운영자에게 알려 주세요.',
  upstream_error: '추천 서버가 오류를 돌려줬어요. 데모 결과로 바꾸지 않았어요.',
  foreign_item: '추천 결과에 이 계정의 옷이 아닌 항목이 있어 결과를 보여 주지 않았어요.',
};
const SIDE_EFFECTS = '오늘의 추천 1건이 내 추천 이력에 남고(다음 추천의 반복 감점에 쓰인다, 착용 기록 아님), 응답 묶음이 추천 기록(recommendation_batch)에 남는다.';

/** 실패 결과. 문장은 언제나 위의 고정 안내다 — 추천 서버의 오류 본문을 그대로 내보내지 않는다. */
function recommendFailure(request, outcome) {
  const message = RECOMMEND_MESSAGES[outcome] ?? RECOMMEND_MESSAGES.upstream_error;
  return {
    content: [{ type: 'text', text: `[${outcome}] ${message}` }],
    structuredContent: { ok: false, outcome, message, source: 'live_recommendation_engine', request },
  };
}

/**
 * Rust 응답 → 내보낼 모양. 사진(image_url, data URI·내부 경로)·이미지 프롬프트용 착용 문장(wear)·점수 내부값·batch id 는 뺀다.
 * 모든 옷 id 가 이 사용자의 것인지 확인한다 — 하나라도 아니면 결과 전체를 내보내지 않는다('foreign').
 * 모양이 어긋난 응답(카드·아이템이 객체가 아님, 아이템 없는 카드, id·이름·카테고리 없음)은 'invalid' — 성공으로 내보내지 않는다.
 */
async function shapeRecommendation(live, userId, request, body, context) {
  const str = (v, max = 600) => (typeof v === 'string' ? v.slice(0, max) : '');
  const isObj = (v) => v !== null && typeof v === 'object' && !Array.isArray(v);
  const validItem = (i) =>
    isObj(i) && [i.id, i.name, i.category].every((v) => typeof v === 'string' && v.length > 0) && i.id.length <= 64;
  const validMode = (m) => isObj(m) && Array.isArray(m.outfit) && m.outfit.length > 0 && m.outfit.every(validItem);
  if (!body.modes.every(validMode)) return 'invalid';
  const modes = body.modes.map((m) => ({
    mode: str(m.mode, 40),
    label: str(m.mode_label, 40),
    description: str(m.mode_description, 120),
    summary: str(m.recommendation),
    reason: str(m.reason),
    verdict: str(m.verdict, 40),
    score: typeof m.score === 'number' && Number.isFinite(m.score) ? m.score : 0,
    tips: Array.isArray(m.tips) ? m.tips.slice(0, 6).map((t) => str(t, 300)) : [],
    items: (Array.isArray(m.outfit) ? m.outfit : []).map((i) => ({
      id: str(i.id, 64),
      name: str(i.name, 200),
      category: str(i.category, 20),
      reason: str(i.reason, 400),
    })),
  }));
  const ids = modes.flatMap((m) => m.items.map((i) => i.id));
  const owned = await live.adapter.ownedClothingIds(userId, ids);
  if (ids.some((id) => !owned.has(id))) return 'foreign';
  const weather = body.batch_id ? await live.adapter.batchWeather(userId, body.batch_id).catch(() => null) : null;
  return {
    ok: modes.length > 0,
    outcome: modes.length > 0 ? 'recommended' : 'no_candidates',
    message: modes.length > 0 ? `실제 추천 엔진이 고른 코디 ${modes.length}개` : RECOMMEND_MESSAGES.no_candidates,
    source: 'live_recommendation_engine',
    request,
    region: context.region,
    weather,
    weather_summary: str(body.weather_summary) || null,
    notice: str(body.notice) || null,
    modes,
    side_effects: SIDE_EFFECTS,
  };
}

function recommendText(r) {
  const head = [
    `실제 추천(내 옷장 기준) · 지역 ${r.region ?? '?'}` +
      (r.weather?.temperature != null ? ` · ${r.weather.temperature}°C${r.weather.description ? ` ${r.weather.description}` : ''}` : ''),
    r.weather_summary ? `날씨: ${r.weather_summary}` : null,
    r.notice ? `안내: ${r.notice}` : null,
  ].filter(Boolean);
  const body = r.modes.map(
    (m) =>
      `■ ${m.label} (${m.verdict} ${m.score})\n${m.items.map((i) => `- [${i.category}] ${i.name} (id ${i.id}): ${i.reason}`).join('\n')}` +
      (m.summary ? `\n${m.summary}` : '') +
      (m.reason ? `\n선정 이유: ${m.reason}` : ''),
  );
  return [...head, ...body, '점수는 후보끼리 순위를 정하는 내부 점수예요(확률·100점 만점 아님).', `기록: ${r.side_effects}`].join('\n\n');
}

function registerRecommendTool(server, live) {
  server.registerTool(
    TOOL_NAMES.recommend,
    {
      title: '오늘 코디 추천(내 옷장)',
      description:
        '연결된 내 Wardrobe Edit 계정의 실제 옷장·저장된 지역의 지금 날씨·추천 이력으로 실제 추천 엔진이 오늘 코디를 고른다 ' +
        '(오늘의 추천·다른 조합·안 입은 옷 활용 중 나오는 것). 데모가 아니다. 어느 계정인지는 연결(OAuth)이 정한다. ' +
        '부작용: 부를 때마다 AI 모델 호출 비용이 들고, 오늘의 추천 1건이 추천 이력(착용 기록 아님)과 추천 기록에 남는다. ' +
        '사용자가 추천을 원할 때 한 번만 부른다. 같은 조건으로 되풀이하거나, 실패·시간 초과 뒤 바로 다시 부르지 않는다. ' +
        '사진은 돌려주지 않는다. situation 기본은 일상. mood 를 쓰려면 gender 도 정한다. ' +
        'score 는 후보끼리 순위를 정하는 내부 점수다 — 확률이나 100점 만점이 아니고 100을 넘을 수 있으며, 코디 품질을 보장하는 값이 아니다.',
      inputSchema: z.strictObject({
        situation: z.enum(RECOMMEND_SITUATIONS).optional().describe('상황. 기본 일상(웹 홈과 같다). 출근·데이트는 격식을 더 본다.'),
        gender: z.enum(RECOMMEND_GENDERS).optional().describe('옷장 성별 탭(female·male). 없으면 성별로 거르지 않는다.'),
        mood: z.enum(RECOMMEND_MOODS).optional().describe('스타일 무드 키. 그 성별의 무드만 받는다. gender 가 필요하다.'),
      }),
      outputSchema: recommendOutput,
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: false, openWorldHint: true },
    },
    async ({ situation = '일상', gender, mood }, extra) => {
      const denied = checkScope(extra, live, 'recommend');
      if (denied) return denied;
      const userId = ownerOf(extra);
      const request = { situation, gender: gender ?? null, mood: mood ?? null };
      if (mood && !gender) return recommendFailure(request, 'mood_needs_gender');
      if (mood === 'street' && situation === '출근') return recommendFailure(request, 'street_not_for_work');
      // 모델을 부르기 전에 막을 수 있는 것은 여기서 막는다.
      const context = await live.adapter.recommendContext(userId, { gender: gender ?? null, mood: mood ?? null });
      if (!context.moodOk) return recommendFailure(request, 'mood_not_for_gender');
      if (context.wardrobe === 0) return recommendFailure(request, 'empty_wardrobe');
      if (!context.region) return recommendFailure(request, 'no_region');

      const body = { occasion: situation };
      if (gender) body.gender = gender;
      if (mood) body.style_mood = mood;
      const upstream = await live.recommender.recommend(userId, body);
      if (!upstream.ok) return recommendFailure(request, upstream.outcome);
      const shaped = await shapeRecommendation(live, userId, request, upstream.body, context);
      if (shaped === 'invalid') return recommendFailure(request, 'upstream_error');
      if (shaped === 'foreign') return recommendFailure(request, 'foreign_item');
      if (!shaped.ok) {
        return {
          content: [{ type: 'text', text: `[no_candidates] ${shaped.notice ?? shaped.message}` }],
          structuredContent: shaped,
        };
      }
      return { content: [{ type: 'text', text: recommendText(shaped) }], structuredContent: shaped };
    },
  );
}

function registerLiveTools(server, live) {
  if (live.recommender) registerRecommendTool(server, live);
  const LIVE_NOTE =
    '연결된 내 Wardrobe Edit 계정의 실제 데이터를 읽는다(읽기 전용). 어느 계정인지는 연결(OAuth 토큰)이 정한다 — 사용자 id 를 인자로 받지 않는다.';
  server.registerTool(
    TOOL_NAMES.myProfile,
    {
      title: '내 Wardrobe Edit 프로필',
      description: `${LIVE_NOTE} 내 이름과 옷장 아이템 수(카테고리별)를 돌려준다.`,
      inputSchema: z.strictObject({}),
      outputSchema: z.object({
        username: z.string(),
        display_name: z.string().nullable(),
        wardrobe_count: z.number().int(),
        wardrobe_by_category: z.record(z.string(), z.number().int()),
      }),
      annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false },
    },
    async (_args, extra) => {
      const denied = checkScope(extra, live, 'read');
      if (denied) return denied;
      const userId = ownerOf(extra);
      const profile = await live.adapter.getProfile(userId);
      if (!profile) return toolError('not_found', '연결된 계정을 찾을 수 없어요. 계정을 다시 연결해 주세요.');
      return {
        content: [
          {
            type: 'text',
            text: `연결된 계정: ${profile.display_name ?? profile.username} · 옷장 ${profile.wardrobe_count}벌`,
          },
        ],
        structuredContent: profile,
      };
    },
  );
  server.registerTool(
    TOOL_NAMES.myWardrobe,
    {
      title: '내 옷장 보기',
      description: `${LIVE_NOTE} 내 옷장 아이템 목록(이름·카테고리·색·소재). 사진은 내보내지 않는다. category 로 거를 수 있다.`,
      inputSchema: z.strictObject({
        category: z.enum(CATEGORIES).optional().describe('아우터·상의·하의·신발·가방 중 하나(선택).'),
        limit: z.number().int().min(1).max(200).optional().describe('최대 개수(기본 50)'),
      }),
      outputSchema: z.object({
        items: z.array(
          z.object({
            id: z.string(),
            name: z.string(),
            category: z.string(),
            color: z.string().nullable(),
            sub_category: z.string().nullable(),
            material: z.string().nullable(),
            has_image: z.boolean(),
          }),
        ),
      }),
      annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false },
    },
    async ({ category, limit }, extra) => {
      const denied = checkScope(extra, live, 'read');
      if (denied) return denied;
      const userId = ownerOf(extra);
      const items = await live.adapter.listWardrobe(userId, { category: category ?? null, limit: limit ?? 50 });
      const text = items.length
        ? items.map((i) => `- [${i.category}] ${i.name}${i.color ? ` · ${i.color}` : ''}`).join('\n')
        : '옷장이 비어 있어요.';
      return { content: [{ type: 'text', text }], structuredContent: { items } };
    },
  );
}

/**
 * 도구마다 securitySchemes 를 알린다(OpenAI: 도구 정의의 최상위 필드). SDK 는 모르는 필드를 내보내지 않으므로
 * tools/list 응답에 덧붙인다. live 모드: 읽기 도구 oauth2(wardrobe.read), 쓰기 도구(비교 만들기·선택 저장·수정)
 * oauth2(wardrobe.read + comparisons.write). demo 모드: noauth.
 */
const WRITE_TOOLS = new Set([TOOL_NAMES.create, TOOL_NAMES.photo, TOOL_NAMES.vote, TOOL_NAMES.update]);

function declareSecuritySchemes(server, live) {
  const schemesFor = (name) => {
    if (!live) return [{ type: 'noauth' }];
    if (name === TOOL_NAMES.recommend) return [{ type: 'oauth2', scopes: [live.scopes.read, live.scopes.recommend] }];
    const scopes = WRITE_TOOLS.has(name) ? [live.scopes.read, live.scopes.write] : [live.scopes.read];
    return [{ type: 'oauth2', scopes }];
  };
  const inner = server.server;
  const original = inner._requestHandlers.get('tools/list');
  if (!original) return;
  inner.setRequestHandler(ListToolsRequestSchema, async (request, extra) => {
    const result = await original(request, extra);
    return {
      ...result,
      tools: result.tools.map((t) => {
        const schemes = schemesFor(t.name);
        return { ...t, securitySchemes: schemes, _meta: { ...(t._meta ?? {}), securitySchemes: schemes } };
      }),
    };
  });
}
