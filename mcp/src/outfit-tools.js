// 코디 흐름 도구: 추천 뒤에 한 벌 바꾸기 → 입을 계획 → 실제 착용 기록 → 내역.
//
// 코디는 언제나 Rust 가 만든 참조(outfit_ref)로만 가리킨다. 모델이 적은 옷 이름·코디 목록은 받지 않는다 — 입력에 그런 칸이 없다.
// '2번 코디' 같은 말은 대화마다 뜻이 달라서 받지 않는다. 계정은 OAuth 주체로만 정해지고, 소유자 확인은 Rust 가 요청마다 한다.
//
// 권한: 바꾸기는 추천(recommendations.write, 제안만 남기고 기록하지 않는다), 계획·착용·수정은 outfits.write, 내역은 wardrobe.read.
// 결과 문장은 이 파일의 고정 안내다. Rust 의 오류 본문을 그대로 내보내지 않는다.

import * as z from 'zod';

import { CATEGORIES } from './fixtures.js';

export const OUTFIT_TOOL_NAMES = {
  replaceItem: 'replace_my_outfit_item',
  planOutfit: 'plan_my_outfit',
  recordWorn: 'record_worn_outfit',
  updateRecord: 'update_outfit_record',
  history: 'list_my_outfit_history',
};

/** 도구별 필요 권한(live.scopes 의 키). */
export const OUTFIT_TOOL_SCOPE = {
  [OUTFIT_TOOL_NAMES.replaceItem]: ['read', 'recommend'],
  [OUTFIT_TOOL_NAMES.planOutfit]: ['read', 'outfits'],
  [OUTFIT_TOOL_NAMES.recordWorn]: ['read', 'outfits'],
  [OUTFIT_TOOL_NAMES.updateRecord]: ['read', 'outfits'],
  [OUTFIT_TOOL_NAMES.history]: ['read'],
};

export const OUTFIT_INSTRUCTIONS =
  '추천 뒤의 흐름: 코디는 결과의 outfit_ref 를 그대로 써서 가리킨다. replace_my_outfit_item 은 한 벌만 바꾼 대안을 제안할 뿐 저장·기록하지 않는다. ' +
  "plan_my_outfit 은 '오늘 이거 입을게' 같은 입을 계획이고 착용이 아니다. record_worn_outfit 은 사용자가 '입었어'라고 직접 말했을 때만 부른다 — " +
  '계획했다고, 추천을 좋아했다고 입었다고 기록하지 않는다. 잘못 남겼으면 update_outfit_record 의 undo 로 되돌린다. 내역은 list_my_outfit_history. ';

const PREFERENCES = ['comfortable', 'warmer', 'lighter', 'darker', 'brighter', 'more_formal', 'more_casual'];
const REACTIONS = ['comfortable', 'uncomfortable', 'too_cold', 'too_hot', 'liked', 'disliked'];
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

const outfitRef = z.string().regex(UUID).describe('추천·바꾸기 결과의 outfit_ref 를 한 글자도 바꾸지 않고 그대로 넣는다.');
const recordId = z.string().regex(UUID).describe('계획·착용 결과나 내역의 record_id.');
const date = z
  .string()
  .regex(/^\d{4}-\d{2}-\d{2}$/)
  .describe('YYYY-MM-DD. 사용자가 날짜를 말했으면 그 날짜. 없으면 서울 기준 오늘.');
const comment = z.string().max(200).describe('사용자가 직접 말한 짧은 메모(200자).');
const reaction = z.enum(REACTIONS).describe('입어 본 뒤 사용자가 직접 말한 반응. 지어내지 않는다. 기록일 뿐 추천에 자동 반영되지 않는다.');
const revision = z.number().int().min(1).max(1_000_000).describe('최근에 본 기록의 revision.');

const itemOut = z.object({ id: z.string(), name: z.string(), category: z.string(), color: z.string().nullable() });
const recordOut = z.object({
  record_id: z.string(),
  outfit_ref: z.string(),
  status: z.enum(['planned', 'worn', 'cancelled']),
  date: z.string(),
  items: z.array(itemOut),
  occasion: z.string().nullable(),
  mood_key: z.string().nullable(),
  temperature: z.number().nullable(),
  reaction: z.string().nullable(),
  comment: z.string().nullable(),
  revision: z.number().int(),
});
const writeOutput = z.object({
  ok: z.boolean(),
  outcome: z.string(),
  message: z.string(),
  record: recordOut.optional(),
  current_revision: z.number().int().optional(),
});

/** 쓰기 결과 안내. already_* 는 재시도 — 새로 남긴 것이 없다. */
const WRITE_MESSAGES = {
  planned: '입을 계획으로 저장했어요. 아직 입은 기록은 아니에요.',
  already_planned: '같은 날 같은 코디 계획이 이미 있어요. 새로 저장하지 않았어요.',
  worn: '입은 기록으로 남겼어요.',
  already_worn: '같은 날 같은 코디를 입은 기록이 이미 있어요. 새로 남기거나 바꾸지 않았어요(수정은 update_outfit_record).',
  cancelled: '계획을 취소했어요. 기록은 남지만 입은 내역에는 들어가지 않아요.',
  already_cancelled: '이미 취소된 기록이에요.',
  undone_to_planned: '입은 기록을 되돌려 다시 계획 상태로 뒀어요. 반응은 지웠어요.',
  undone_to_cancelled: '입은 기록을 되돌렸어요. 계획이 없던 기록이라 취소 상태가 됐어요.',
  nothing_to_undo: '입은 기록이 아니라 되돌릴 것이 없어요.',
  updated: '기록을 고쳤어요.',
  unchanged: '이미 같은 내용이라 바꾸지 않았어요.',
};

const FAILURE_MESSAGES = {
  ref_not_found: '이 코디 참조를 찾을 수 없어요. 이 계정의 추천·바꾸기 결과에 나온 outfit_ref 를 그대로 써 주세요.',
  record_not_found: '이 기록을 찾을 수 없어요.',
  item_gone: '이 코디의 옷 중 지금 옷장에 없는 것이 있어 처리하지 않았어요.',
  item_not_owned: '내 옷장에서 이 옷을 찾을 수 없어요. list_my_wardrobe 의 id 만 쓸 수 있어요.',
  wrong_category: '이 옷은 고른 자리의 카테고리가 아니에요.',
  same_item: '지금 코디에 들어 있는 옷이에요. 다른 옷을 골라 주세요.',
  invalid_category: '카테고리는 상의·하의·아우터·신발·가방 중 하나예요.',
  invalid_date: '날짜는 YYYY-MM-DD 형식의 실제 날짜로 적어 주세요.',
  date_in_future: '아직 오지 않은 날짜에는 입었다고 기록할 수 없어요. 날짜를 확인해 주세요.',
  date_in_past: '지난 날짜에는 입을 계획을 세울 수 없어요. 입었다면 record_worn_outfit 을 쓰세요.',
  date_too_far: '입을 계획은 60일 뒤까지만 세울 수 있어요.',
  date_too_old: '365일보다 오래된 날짜는 기록할 수 없어요.',
  invalid_range: '조회 날짜 범위가 올바르지 않아요(시작 ≤ 끝, 500일 이내).',
  comment_too_long: '메모는 200자 이내로 적어 주세요.',
  invalid_reaction: '지원하지 않는 반응이에요.',
  reaction_needs_worn: '반응은 입은 기록에만 남길 수 있어요.',
  invalid_status: '상태는 planned·worn·cancelled 중 하나예요.',
  invalid_transition: '지금 상태에서는 할 수 없는 일이에요. 입은 기록은 undo, 취소된 기록은 새 계획으로 처리해 주세요.',
  revision_conflict: '그사이 기록이 바뀌었어요. 내역을 다시 읽고 사용자에게 확인한 뒤 진행해 주세요.',
  duplicate_on_date: '그 날짜에 같은 코디 기록이 이미 있어요.',
  invalid_request: '요청 값이 맞지 않아요.',
  busy: '요청이 몰려 있어요. 잠시 뒤에 다시 시도해 주세요.',
  // 연결이 응답 전에 끊긴 경우도 여기로 온다. 서버가 이미 저장했을 수 있어 "저장 안 됨" 이라고 말하지 않는다.
  unavailable: '코디 서버의 응답을 받지 못했어요. 쓰기 요청이었다면 저장됐는지 알 수 없으니, 다시 부르지 말고 list_my_outfit_history 로 확인한 뒤 진행해 주세요.',
  timeout: '서버 응답을 기다리다 멈췄어요. 저장됐는지 알 수 없으니 list_my_outfit_history 로 확인한 뒤 진행해 주세요.',
  delegation_rejected: '코디 서버가 이 연결의 위임을 받지 않았어요(서버 설정 문제). 운영자에게 알려 주세요.',
  upstream_error: '코디 서버가 오류를 돌려줬어요. 저장되지 않았을 수 있어요.',
  foreign_item: '결과에 이 계정의 옷이 아닌 항목이 있어 보여 주지 않았어요.',
};

function failure(outcome, extra = {}) {
  const known = Object.hasOwn(FAILURE_MESSAGES, outcome) ? outcome : 'upstream_error';
  const message = FAILURE_MESSAGES[known];
  return {
    content: [{ type: 'text', text: `[${known}] ${message}` }],
    structuredContent: { ok: false, outcome: known, message, ...extra },
  };
}

const str = (v, max) => (typeof v === 'string' ? v.slice(0, max) : null);
const num = (v) => (typeof v === 'number' && Number.isFinite(v) ? v : null);

function shapeItem(i) {
  return { id: str(i?.id, 64) ?? '', name: str(i?.name, 200) ?? '', category: str(i?.category, 20) ?? '', color: str(i?.color, 60) };
}

/** Rust 기록 → 내보낼 모양. 모양이 어긋나면 null. */
function shapeRecord(r) {
  const valid =
    r && typeof r === 'object' && typeof r.record_id === 'string' && typeof r.outfit_ref === 'string' &&
    ['planned', 'worn', 'cancelled'].includes(r.status) && typeof r.date === 'string' &&
    Array.isArray(r.items) && Number.isInteger(r.revision);
  if (!valid) return null;
  return {
    record_id: r.record_id,
    outfit_ref: r.outfit_ref,
    status: r.status,
    date: r.date.slice(0, 10),
    items: r.items.map(shapeItem),
    occasion: str(r.occasion, 50),
    mood_key: str(r.mood_key, 50),
    temperature: num(r.temperature),
    reaction: str(r.reaction, 20),
    comment: str(r.comment, 200),
    revision: r.revision,
  };
}

const STATUS_LABEL = { planned: '계획', worn: '입음', cancelled: '취소' };

function recordLine(r) {
  const items = r.items.map((i) => `${i.category} ${i.name}`).join(', ');
  return (
    `${r.date} [${STATUS_LABEL[r.status]}] ${items}` +
    (r.reaction ? ` · 반응 ${r.reaction}` : '') +
    (r.comment ? ` · 메모 ${r.comment}` : '') +
    ` (record_id ${r.record_id}, revision ${r.revision})`
  );
}

function writeResult(upstream) {
  if (!upstream.ok) {
    return failure(
      upstream.outcome,
      upstream.current_revision !== undefined ? { current_revision: upstream.current_revision } : {},
    );
  }
  const outcome = upstream.body.outcome;
  const record = shapeRecord(upstream.body.record);
  if (!record || !Object.hasOwn(WRITE_MESSAGES, outcome)) return failure('upstream_error');
  const message = WRITE_MESSAGES[outcome];
  return {
    content: [{ type: 'text', text: `${message}\n${recordLine(record)}` }],
    structuredContent: { ok: true, outcome, message, record },
  };
}

const replaceOutput = z.object({
  ok: z.boolean(),
  outcome: z.string(),
  message: z.string(),
  reason: z.string().optional().describe('needs_clarification 의 이유'),
  supported_preferences: z.array(z.string()).optional(),
  original_outfit_ref: z.string().optional(),
  category: z.string().optional(),
  replaced_item: itemOut.optional(),
  alternatives: z
    .array(
      z.object({
        outfit_ref: z.string().describe('이 대안의 새 참조. 계획·착용 기록에 그대로 쓴다.'),
        new_item: itemOut,
        items: z.array(itemOut),
        rationale: z.array(z.string()),
        weather_fit: z.string(),
      }),
    )
    .optional(),
  weather: z.object({ temperature: z.number().nullable(), source: z.string().nullable() }).optional(),
  notice: z.string().nullable().optional(),
  limitations: z.array(z.string()).optional(),
});

async function shapeReplacement(live, userId, body) {
  const base = { original_outfit_ref: str(body.outfit_ref, 36) ?? undefined, category: str(body.category, 10) ?? undefined };
  if (body.outcome === 'needs_clarification') {
    const message = str(body.message, 600) ?? '';
    return {
      content: [{ type: 'text', text: `[needs_clarification] ${message}` }],
      structuredContent: {
        ok: false,
        outcome: 'needs_clarification',
        message,
        reason: str(body.reason, 40) ?? 'unknown',
        supported_preferences: Array.isArray(body.supported_preferences) ? body.supported_preferences.map((p) => str(p, 30) ?? '') : [],
        ...base,
      },
    };
  }
  if (body.outcome === 'no_candidates' || body.outcome === 'weather_unavailable') {
    const message = str(body.message, 600) ?? '';
    return {
      content: [{ type: 'text', text: `[${body.outcome}] ${message}` }],
      structuredContent: { ok: false, outcome: body.outcome, message, ...base, alternatives: [] },
    };
  }
  if (body.outcome !== 'proposed' || !Array.isArray(body.alternatives) || body.alternatives.length === 0) {
    return failure('upstream_error');
  }
  const alternatives = body.alternatives.slice(0, 5).map((a) => ({
    outfit_ref: str(a?.outfit_ref, 36) ?? '',
    new_item: shapeItem(a?.new_item),
    items: Array.isArray(a?.items) ? a.items.map(shapeItem) : [],
    rationale: Array.isArray(a?.rationale) ? a.rationale.slice(0, 6).map((t) => str(t, 300) ?? '') : [],
    weather_fit: str(a?.weather_fit, 20) ?? 'ok',
  }));
  if (alternatives.some((a) => !UUID.test(a.outfit_ref) || a.items.length === 0)) return failure('upstream_error');
  const ids = alternatives.flatMap((a) => a.items.map((i) => i.id));
  const owned = await live.adapter.ownedClothingIds(userId, ids);
  if (ids.some((id) => !owned.has(id))) return failure('foreign_item');
  const result = {
    ok: true,
    outcome: 'proposed',
    message: str(body.message, 200) ?? '',
    ...base,
    replaced_item: shapeItem(body.replaced_item),
    alternatives,
    weather: { temperature: num(body.weather?.temperature), source: str(body.weather?.source, 20) },
    notice: str(body.notice, 300),
    limitations: Array.isArray(body.limitations) ? body.limitations.slice(0, 6).map((t) => str(t, 300) ?? '') : [],
  };
  const text = [
    `${result.message}. 고르면 그 outfit_ref 로 계획하거나 입은 기록을 남겨요. 지금은 아무것도 저장·기록하지 않았어요.`,
    ...alternatives.map(
      (a) => `■ ${a.new_item.name} (옷 id ${a.new_item.id}, 코디 참조 ${a.outfit_ref})\n${a.items.map((i) => `- [${i.category}] ${i.name}`).join('\n')}\n이유: ${a.rationale.join(' ')}`,
    ),
    result.notice ? `안내: ${result.notice}` : null,
    result.limitations.length ? `한계: ${result.limitations.join(' ')}` : null,
  ].filter(Boolean);
  return { content: [{ type: 'text', text: text.join('\n\n') }], structuredContent: result };
}

export function registerOutfitTools(server, live, { checkScope, ownerOf }) {
  const call = (action, extra, body) => live.recommender.outfit(action, ownerOf(extra), body);

  server.registerTool(
    OUTFIT_TOOL_NAMES.replaceItem,
    {
      title: '코디에서 한 벌만 바꾸기',
      description:
        '이전 결과의 코디(outfit_ref)에서 한 자리(category)만 내 옷장의 같은 카테고리 옷으로 바꾼 대안을 최대 3개 제안한다. 나머지 옷은 id 그대로다. ' +
        '날씨·조합 평가를 다시 거치고, 모델을 부르지 않으며, 없는 옷을 만들지 않는다. 제안만 하므로 원래 코디와 기록은 바뀌지 않고 착용으로 남지 않는다. ' +
        '대안마다 새 outfit_ref 가 있어 사용자가 고른 것을 그대로 계획·착용 기록에 쓴다. ' +
        '옷을 직접 고르면 replacement_item_id(list_my_wardrobe 의 id), 조건으로 찾으면 preference. ' +
        '사용자가 조건을 말했으면 원문을 request_text 에 함께 넣는다 — 판단할 수 없는 조건(방수·비 오는 날·미끄럼 방지·세탁 상태·구매)을 버리고 부르지 않는다. ' +
        'outcome=needs_clarification 이면 지어내지 말고 사용자에게 되묻는다. 편한(comfortable)은 신발 종류로만 판단한다(착화감 데이터 없음).',
      inputSchema: z.strictObject({
        outfit_ref: outfitRef,
        category: z.enum(CATEGORIES).describe('바꿀 자리 하나.'),
        replacement_item_id: z.string().min(1).max(64).optional().describe('사용자가 고른 내 옷의 id. preference 와 같이 쓰지 않는다.'),
        preference: z.enum(PREFERENCES).optional().describe('조건: comfortable=편한 신발, warmer·lighter=두께, darker·brighter=밝기, more_formal·more_casual=격식.'),
        request_text: z.string().max(200).optional().describe('사용자가 말한 조건 원문. 알아들을 수 없으면 서버가 되묻는다.'),
      }),
      outputSchema: replaceOutput,
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: true, openWorldHint: false },
    },
    async (args, extra) => {
      const denied = checkScope(extra, live, 'recommend');
      if (denied) return denied;
      const upstream = await call('replacement', extra, args);
      if (!upstream.ok) return failure(upstream.outcome);
      return shapeReplacement(live, ownerOf(extra), upstream.body);
    },
  );

  server.registerTool(
    OUTFIT_TOOL_NAMES.planOutfit,
    {
      title: '입을 코디로 저장(계획)',
      description:
        "사용자가 '오늘 이거 입을게'처럼 입겠다는 뜻을 밝혔을 때 코디 하나를 그날의 계획으로 저장한다. 실제 착용 기록이 아니다. 코디는 outfit_ref 로만 가리키며 서버가 옷을 다시 확인한다. " +
        '같은 날 같은 구성을 다시 보내면 새로 만들지 않는다(already_planned). 구성이 다른 코디는 하루에 여러 개 저장할 수 있다. 날짜는 오늘부터 60일 뒤까지.',
      inputSchema: z.strictObject({ outfit_ref: outfitRef, date: date.optional(), comment: comment.optional() }),
      outputSchema: writeOutput,
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: true, openWorldHint: false },
    },
    async (args, extra) => {
      const denied = checkScope(extra, live, 'outfits');
      if (denied) return denied;
      return writeResult(await call('plan', extra, args));
    },
  );

  server.registerTool(
    OUTFIT_TOOL_NAMES.recordWorn,
    {
      title: '실제로 입은 코디 기록',
      description:
        "사용자가 '오늘 이거 입었어'처럼 실제로 입었다고 직접 말했을 때만 부른다. 계획했다는 이유로, 추천이 마음에 들었다는 이유로, 추측으로 부르지 않는다. " +
        '이 기록만 다음 추천의 최근 착용 감점에 쓰인다. 계획이 있으면 record_id + expected_revision 으로 확정하고, 없으면 outfit_ref 로 바로 남긴다(같은 날 같은 구성의 계획이 있으면 그 계획을 확정한다). ' +
        '둘 중 하나만 보낸다. 재시도는 already_worn 으로 끝나고 중복되지 않는다. 날짜는 미래가 될 수 없다. 잘못 남겼으면 update_outfit_record 의 undo.',
      inputSchema: z.strictObject({
        outfit_ref: outfitRef.optional(),
        record_id: recordId.optional(),
        expected_revision: revision.optional(),
        date: date.optional(),
        reaction: reaction.optional(),
        comment: comment.optional(),
      }),
      outputSchema: writeOutput,
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: true, openWorldHint: false },
    },
    async (args, extra) => {
      const denied = checkScope(extra, live, 'outfits');
      if (denied) return denied;
      const byRef = args.outfit_ref !== undefined && args.record_id === undefined && args.expected_revision === undefined;
      const byRecord = args.outfit_ref === undefined && args.record_id !== undefined && args.expected_revision !== undefined;
      if (!byRef && !byRecord) return failure('invalid_request');
      return writeResult(await call('worn', extra, args));
    },
  );

  server.registerTool(
    OUTFIT_TOOL_NAMES.updateRecord,
    {
      title: '코디 기록 고치기·취소·되돌리기',
      description:
        '사용자가 요청했을 때 기록 하나를 바꾼다. action: cancel=계획 취소(입은 기록은 취소 대신 undo), undo=입은 기록 되돌리기(계획에서 왔으면 계획으로, 바로 남긴 것이면 취소로), edit=날짜·반응·메모 고치기(null 을 보내면 지운다). ' +
        'expected_revision 에 최근에 본 revision 을 넣는다. 그사이 바뀌었으면 outcome=revision_conflict 로 바꾸지 않는다. 행은 지우지 않는다.',
      inputSchema: z.strictObject({
        record_id: recordId,
        expected_revision: revision,
        action: z.enum(['cancel', 'undo', 'edit']),
        date: date.optional(),
        reaction: reaction.nullable().optional(),
        comment: comment.nullable().optional(),
      }),
      outputSchema: writeOutput,
      // 저장된 기록의 상태·내용을 바꾼다(destructive). 같은 값으로 다시 불러도 결과가 같다(idempotent).
      annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: true, openWorldHint: false },
    },
    async (args, extra) => {
      const denied = checkScope(extra, live, 'outfits');
      if (denied) return denied;
      return writeResult(await call('record', extra, args));
    },
  );

  server.registerTool(
    OUTFIT_TOOL_NAMES.history,
    {
      title: '내 코디 기록 보기',
      description:
        '연결된 내 계정의 입을 계획·입은 기록·취소된 기록을 최근 날짜부터 읽는다. status 로 거를 수 있다(worn 만 실제 착용). 읽기만 한다. 기본 범위는 30일 전부터 60일 뒤까지, 최대 100건.',
      inputSchema: z.strictObject({
        from: date.optional().describe('시작일 YYYY-MM-DD'),
        to: date.optional().describe('끝일 YYYY-MM-DD'),
        status: z.enum(['planned', 'worn', 'cancelled']).optional(),
        limit: z.number().int().min(1).max(100).optional(),
      }),
      outputSchema: z.object({
        ok: z.boolean(),
        outcome: z.string(),
        message: z.string(),
        today: z.string().optional(),
        records: z.array(recordOut).optional(),
        has_more: z.boolean().optional(),
      }),
      annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false },
    },
    async (args, extra) => {
      const denied = checkScope(extra, live, 'read');
      if (denied) return denied;
      const upstream = await call('history', extra, args);
      if (!upstream.ok) return failure(upstream.outcome);
      if (!Array.isArray(upstream.body.records)) return failure('upstream_error');
      const records = upstream.body.records.map(shapeRecord);
      if (records.some((r) => r === null)) return failure('upstream_error');
      const hasMore = upstream.body.has_more === true;
      const message = records.length ? `기록 ${records.length}건${hasMore ? ' (더 있어요 — 날짜를 좁혀 주세요)' : ''}` : '이 범위에 기록이 없어요.';
      return {
        content: [{ type: 'text', text: [message, ...records.map(recordLine)].join('\n') }],
        structuredContent: { ok: true, outcome: 'ok', message, today: str(upstream.body.today, 10) ?? undefined, records, has_more: hasMore },
      };
    },
  );
}
