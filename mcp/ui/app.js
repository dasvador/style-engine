// 데모 코디 비교 화면(MCP Apps view). ChatGPT·Claude 같은 호스트가 iframe 으로 띄우고, 공식
// `App` bridge(postMessage JSON-RPC: ui/initialize · ui/notifications/tool-result · tools/call)로만
// 서버와 이야기한다. 화면이 직접 네트워크를 쓰지 않는다.
//
// 기존 ComparePage.tsx 와 같은 규칙: 두 코디는 똑같은 모양, 이유·메모는 선택, "둘 다 아쉬움"은
// 저장되는 선택이고 "건너뛰기"는 아무것도 저장하지 않는다.

import { App } from '@modelcontextprotocol/ext-apps';

const TOOL_GET = 'get_demo_comparison';
const TOOL_VOTE = 'submit_demo_preference';
const TOOL_UPDATE = 'update_demo_preference';

const REASONS = [
  ['color_combo', '색 조합'],
  ['silhouette', '실루엣'],
  ['personal_taste', '내 취향'],
  ['practicality', '실용성/착용 편의'],
  ['weather_fit', '날씨 적합성'],
];
const REASON_LABEL = Object.fromEntries(REASONS);
const COMMENT_MAX = 200;
const CATEGORY_ORDER = ['아우터', '상의', '하의', '신발', '가방'];

/** 옷장 색 이름 → 색 점. 프론트 lib/colorSwatch.ts 에서 샘플 옷장에 나오는 색만 옮겼다. */
const SWATCHES = [
  // 사진 세트(photo-sets.js)의 색. 더 구체적인 이름을 앞에 둔다(체리레드가 레드보다, 옐로그린이 그린보다).
  ['체리레드', '#a3222f'],
  ['옐로그린', '#b5c24a'],
  ['코발트블루', '#2f4fb0'],
  ['코발트', '#1f4fbf'],
  ['버건디', '#6e2632'],
  ['그린', '#3a9a4a'],
  ['레드', '#b8312f'],
  ['다크브라운', '#4a3426'],
  ['브라운', '#6e4b33'],
  ['라이트블루', '#a9c4e0'],
  ['스카이블루', '#8fc1e6'],
  ['인디고', '#33416b'],
  ['헤더그레이', '#a9a8a4'],
  ['오프화이트', '#f2efe6'],
  ['화이트', '#f7f7f4'],
  ['아이보리', '#f1ead9'],
  ['오트밀', '#d9ccb4'],
  ['베이지', '#d4c2a2'],
  ['올리브', '#6b6b3f'],
  ['차콜', '#3d3f42'],
  ['블랙', '#1f1f1f'],
  ['그레이', '#8d8d8d'],
];
function swatch(color) {
  if (!color) return null;
  const compact = color.replace(/\s+/g, '');
  const hit = SWATCHES.find(([name]) => compact.includes(name));
  return hit ? hit[1] : null;
}

// ─── 상태 ───

/**
 * phase: connecting · waiting(도구 실행 중) · syncing(저장 상태 확인 중) · ready(고르는 중)
 *        · saving · saved · skipped · error
 * editing: 저장된 선택을 고치는 중(ready·saving 과 함께 쓴다).
 */
const state = {
  phase: 'connecting',
  comparison: null,
  editing: false,
  choice: null,
  reasons: new Set(),
  comment: '',
  message: null,
  retry: null,
  banner: null,
  bannerTone: 'ok',
};

function set(patch) {
  Object.assign(state, patch);
  render();
}

/** 화면이 연 조회·저장 요청 번호. 응답이 오기 전에 새 요청이 나가면 옛 응답은 버린다. */
let seq = 0;
/** 호스트가 보낸 결과로 시작한 자동 조회 횟수(비교 id 별). 호스트가 결과를 되풀이해 보내도 끝없이 돌지 않게. */
const autoSyncs = new Map();
const AUTO_SYNC_MAX = 5;

const MSG = {
  saveFailed: '저장하지 못했어요. 연결을 확인하고 다시 시도해 주세요. 고른 내용은 그대로 있어요.',
  syncFailed: '저장 상태를 확인하지 못했어요. 잠시 후 다시 시도해 주세요.',
  generic: '요청을 처리하지 못했어요. 잠시 후 다시 시도해 주세요.',
  connect: '호스트와 연결하지 못했어요. 카드를 다시 열어 주세요.',
  cancelled: '도구 실행이 취소됐어요.',
};

// ─── bridge ───

const app = new App({ name: 'wardrobe-edit-demo-compare', version: '0.3.0' }, {});

/** 불러오지 못한 사진 URL. 다시 그려도 깨진 사진을 또 띄우지 않고 대체 문구를 보여 준다. */
const failedImages = new Set();

app.ontoolinput = () => {
  if (!state.comparison) set({ phase: 'waiting', message: null });
};
app.ontoolcancelled = () => {
  if (!state.comparison) set({ phase: 'error', message: MSG.cancelled, retry: null });
};
app.ontoolresult = (result) => onHostResult(result);
app.onhostcontextchanged = (ctx) => applyHostContext(ctx);

/** 결과에서 비교를 꺼낸다. 비교 결과·선택 결과·(v1 서버의) conflict 오류 모양을 모두 읽는다. */
function comparisonOf(result) {
  const sc = result?.structuredContent;
  return sc?.comparison ?? (sc?.options ? sc : null) ?? result?._meta?.['wardrobe-edit/error']?.comparison ?? null;
}

/** 사용자에게 보여도 되는 짧은 오류 문장. 서버가 _meta 에 넣은 한국어 문장만 쓰고, 원문 오류는 쓰지 않는다. */
function friendlyError(result) {
  const message = result?._meta?.['wardrobe-edit/error']?.message;
  return typeof message === 'string' && message.length <= 120 ? message : MSG.generic;
}

function revisionOf(c) {
  return c?.vote ? (Number.isInteger(c.vote.revision) ? c.vote.revision : 1) : 0;
}

/**
 * 호스트가 보낸 도구 결과. 대화에 남은 옛 카드를 다시 열면 그때의 결과(선택 전 스냅샷)가 다시 온다 —
 * 그대로 그리지 않고 서버에서 지금 상태를 읽어 그린다.
 */
function onHostResult(result) {
  const snap = comparisonOf(result);
  if (!snap?.id) {
    // 비교가 없는 오류 결과. 이미 보여 주는 비교가 있으면 그대로 둔다.
    if (!state.comparison) set({ phase: 'error', message: friendlyError(result), retry: null });
    return;
  }
  const same = state.comparison?.id === snap.id;
  // 고치거나 저장하는 중이면 입력을 지우지 않는다.
  if (same && (state.phase === 'saving' || state.editing)) return;
  if (!same) state.comparison = null;

  const canCall = Boolean(app.getHostCapabilities()?.serverTools);
  const count = autoSyncs.get(snap.id) ?? 0;
  if (canCall && count < AUTO_SYNC_MAX) {
    autoSyncs.set(snap.id, count + 1);
    void sync(snap.id);
  } else {
    // 서버를 부를 수 없는 호스트이거나 너무 자주 왔다 — 받은 스냅샷이 지금 것보다 새로울 때만 쓴다.
    if (!same || revisionOf(snap) >= revisionOf(state.comparison)) showLatest(snap);
  }
}

/** 서버에서 비교의 지금 상태를 읽어 그린다. */
async function sync(id) {
  const my = ++seq;
  set({ phase: 'syncing', message: null, retry: null });
  let result = null;
  try {
    result = await callTool(TOOL_GET, { comparison_id: id });
  } catch {
    result = null;
  }
  if (my !== seq) return;
  const latest = result && !result.isError ? comparisonOf(result) : null;
  if (latest?.id === id) return showLatest(latest);
  const message = result?.isError ? friendlyError(result) : MSG.syncFailed;
  set({ phase: 'error', message, retry: () => sync(id) });
}

/** 비교를 그린다. 선택이 있으면 저장된 화면, 없으면 고르는 화면(같은 비교면 고르던 내용 유지). */
function showLatest(c, banner = null, bannerTone = 'ok') {
  if (c.vote) {
    return set({ phase: 'saved', comparison: c, editing: false, banner, bannerTone, message: null, retry: null });
  }
  const same = state.comparison?.id === c.id && !state.editing;
  set({
    phase: 'ready',
    comparison: c,
    editing: false,
    choice: same ? state.choice : null,
    reasons: same ? state.reasons : new Set(),
    comment: same ? state.comment : '',
    message: null,
    retry: null,
    banner: null,
  });
}

function applyHostContext(ctx) {
  if (ctx?.theme) document.documentElement.dataset.theme = ctx.theme;
  const inset = ctx?.safeAreaInsets;
  if (inset) {
    document.body.style.padding = `${inset.top}px ${inset.right}px ${inset.bottom}px ${inset.left}px`;
  }
}

async function callTool(name, args) {
  return app.callServerTool({ name, arguments: args });
}

// ─── 저장·수정 ───

function draftVote() {
  return {
    choice: state.choice,
    reasons: REASONS.map(([code]) => code).filter((code) => state.reasons.has(code)),
    comment: state.comment.trim(),
  };
}

/** 저장된 선택과 지금 입력이 같은지. 서버는 이유를 같은 순서로 정렬하고 메모 앞뒤 공백을 뗀다. */
function sameAsSaved(saved, draft) {
  return (
    saved.choice === draft.choice &&
    (saved.comment ?? '') === draft.comment &&
    saved.reasons.length === draft.reasons.length &&
    saved.reasons.every((r, i) => r === draft.reasons[i])
  );
}

const OUTCOME_BANNER = {
  saved: ['선택을 저장했어요.', 'ok'],
  updated: ['선택을 수정했어요.', 'ok'],
  conflict: ['이 비교에는 이미 다른 선택이 저장돼 있어요. 바꾸려면 ‘선택 수정’을 눌러 주세요.', 'caution'],
  stale: ['그사이 다른 화면에서 선택이 바뀌었어요. 최신 선택을 보여 드려요. 바꾸려면 다시 ‘선택 수정’을 눌러 주세요.', 'caution'],
};

function afterVote(outcome, comparison, { editing }) {
  if (outcome === 'no_vote' || !comparison.vote) {
    // 고치려던 선택이 없어졌다(만료 후 다시 만든 경우 등). 처음 고르는 화면으로.
    return set({
      phase: 'ready',
      comparison,
      editing: false,
      message: '저장된 선택이 없어요. 다시 골라 저장해 주세요.',
      retry: null,
    });
  }
  const [banner, tone] =
    outcome === 'already_submitted'
      ? [editing ? '수정한 선택이 저장돼 있어요.' : '같은 선택이 이미 저장돼 있었어요.', 'ok']
      : OUTCOME_BANNER[outcome] ?? [null, 'ok'];
  showLatest(comparison, banner, tone);
  if (outcome === 'saved' || outcome === 'updated') void tellModel(comparison);
}

/** v1 서버 결과에는 outcome 이 없다 — ok·already_submitted 로 읽는다. */
function outcomeOf(result) {
  const sc = result?.structuredContent;
  if (!result || result.isError) {
    return result?._meta?.['wardrobe-edit/error']?.code === 'conflict' ? 'conflict' : null;
  }
  if (sc?.outcome) return sc.outcome;
  if (sc?.ok === true) return sc.already_submitted ? 'already_submitted' : 'saved';
  return null;
}

async function save() {
  const c = state.comparison;
  if (!c || !state.choice || state.phase === 'saving') return;
  const draft = draftVote();
  const editing = state.editing;
  const base = revisionOf(c);
  const args = { comparison_id: c.id, choice: draft.choice, reasons: draft.reasons };
  if (draft.comment) args.comment = draft.comment;
  const my = ++seq;
  set({ phase: 'saving', message: null, retry: null });

  let result = null;
  try {
    result = editing
      ? await callTool(TOOL_UPDATE, { ...args, expected_revision: base })
      : await callTool(TOOL_VOTE, args);
  } catch {
    // 호스트가 오류로 감쌌거나(예: MCP error -32000 …) 시간이 지났다. 원문은 보여 주지 않는다.
    result = null;
  }
  if (my !== seq) return;
  const outcome = outcomeOf(result);
  const returned = comparisonOf(result);
  if (outcome && returned?.id === c.id) return afterVote(outcome, returned, { editing });
  // 결과를 믿을 수 없다. 서버에 이미 반영됐는지 다시 읽어 판정한다.
  await verifyAfterFailure(c.id, draft, base, editing, my);
}

async function verifyAfterFailure(id, draft, base, editing, my) {
  let result = null;
  try {
    result = await callTool(TOOL_GET, { comparison_id: id });
  } catch {
    result = null;
  }
  if (my !== seq) return;
  const latest = result && !result.isError ? comparisonOf(result) : null;
  if (latest?.id !== id) return set({ phase: 'ready', message: MSG.saveFailed, retry: save });
  if (latest.vote && sameAsSaved(latest.vote, draft)) {
    // 응답만 잃었고 저장은 됐다.
    return afterVote(editing ? 'updated' : 'saved', latest, { editing });
  }
  if (!latest.vote) {
    if (editing) return afterVote('no_vote', latest, { editing });
    return set({ phase: 'ready', comparison: latest, message: MSG.saveFailed, retry: save });
  }
  if (!editing) return afterVote('conflict', latest, { editing });
  if (revisionOf(latest) !== base) return afterVote('stale', latest, { editing });
  // 아무것도 바뀌지 않았다 — 고치던 내용 그대로 다시 시도할 수 있게.
  set({ phase: 'ready', message: MSG.saveFailed, retry: save });
}

function startEdit() {
  const v = state.comparison?.vote;
  if (!v) return;
  set({
    phase: 'ready',
    editing: true,
    choice: v.choice,
    reasons: new Set(v.reasons),
    comment: v.comment ?? '',
    message: null,
    retry: null,
    banner: null,
  });
}

function cancelEdit() {
  seq += 1; // 혹시 남은 응답이 화면을 바꾸지 않게
  set({ phase: 'saved', editing: false, message: null, retry: null, banner: null });
}

/** 호스트가 지원하면 저장한 선택을 모델 맥락에 알린다. 지원하지 않으면 조용히 넘어간다. */
async function tellModel(comparison) {
  if (!comparison?.vote || !app.getHostCapabilities()?.updateModelContext) return;
  const v = comparison.vote;
  const choice = v.choice === 'neither' ? '둘 다 아쉬움' : `${v.choice.toUpperCase()} 코디`;
  const reasons = v.reasons.map((r) => REASON_LABEL[r]).join(', ') || '없음';
  const verb = revisionOf(comparison) > 1 ? '로 수정했고' : '를 골랐고';
  try {
    await app.updateModelContext({
      content: [
        {
          type: 'text',
          text: `[데모 비교 ${comparison.id}] 사용자가 화면에서 "${choice}"${verb} 데모 기록에 저장됐다(revision ${revisionOf(comparison)}). 이유: ${reasons}.${v.comment ? ` 메모: ${v.comment}` : ''}`,
        },
      ],
    });
  } catch {
    /* 선택은 이미 저장됐다 — 모델 맥락 알림은 덤이다. */
  }
}

// ─── 그리기 ───

const root = document.getElementById('root');

function h(tag, attrs = {}, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === false || v === null || v === undefined) continue;
    if (k === 'class') el.className = v;
    else if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else if (k === 'style') el.style.cssText = v;
    else el.setAttribute(k, v === true ? '' : v);
  }
  for (const child of children.flat()) {
    if (child === null || child === undefined || child === false) continue;
    el.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return el;
}

function sortItems(items) {
  const rank = (c) => {
    const i = CATEGORY_ORDER.indexOf(c);
    return i < 0 ? CATEGORY_ORDER.length : i;
  };
  return [...items].sort((a, b) => rank(a.category) - rank(b.category));
}

function isPhoto(c) {
  return c?.data_source === 'curated_generated_lookbook';
}

function demoBadge(c) {
  return h(
    'p',
    { class: 'demo-banner', role: 'note' },
    h('strong', {}, '데모'),
    isPhoto(c)
      ? ' 이 앱에서 이전에 만들어 둔 생성 룩북 사진이에요. 지금 새로 추천하거나 생성한 것이 아니고, 실제 옷장도 아니에요.'
      : ' 샘플 옷장으로 미리 짜 둔 고정 코디예요. 실제 옷장이나 AI 추천 결과가 아니에요.',
  );
}

function contextLine(c) {
  const ctx = c.context;
  return h(
    'p',
    { class: 'context' },
    `${ctx.situation_label} · ${ctx.mood_label} 무드 · ${ctx.weather_description} ${ctx.temperature}°C`,
  );
}

function itemRow(item) {
  const dot = swatch(item.color);
  return h(
    'li',
    { class: 'item' },
    h(
      'span',
      { class: 'thumb', 'aria-hidden': 'true' },
      dot ? h('i', { class: 'dot', style: `background:${dot}` }) : item.category.slice(0, 2),
    ),
    h(
      'span',
      { class: 'item-text' },
      h('span', { class: 'item-slot' }, item.category),
      h('span', { class: 'item-name' }, item.name),
      item.color ? h('span', { class: 'item-color' }, item.color) : null,
    ),
  );
}

/**
 * 사진 칸. 두 카드가 같은 크기(2:3)이고 전신이 잘리지 않게 contain 으로 넣는다. 사진을 못 불러와도
 * 칸 크기는 그대로 두고 대체 문구를 보여 준다 — 카드는 아이템 글자로 계속 고를 수 있다.
 */
function photoBox(option) {
  const img = option.image;
  if (!img) return null;
  const failed = failedImages.has(img.url);
  return h(
    'figure',
    { class: 'photo' },
    h(
      'div',
      { class: 'photo-frame' },
      failed
        ? h('span', { class: 'photo-missing' }, '사진을 불러오지 못했어요. 아래 아이템으로 골라 주세요.')
        : h('img', {
            src: img.url,
            alt: img.alt,
            width: String(img.width),
            height: String(img.height),
            decoding: 'async',
            referrerpolicy: 'no-referrer',
            draggable: 'false',
            onerror: () => {
              failedImages.add(img.url);
              render();
            },
          }),
    ),
    h('figcaption', { class: 'photo-caption' }, '이전에 생성해 둔 룩북 사진 · 아래 아이템으로 만든 사진'),
  );
}

function cardBody(option) {
  return [
    h('span', { class: 'card-head' }, option.slot.toUpperCase()),
    photoBox(option),
    h('ul', { class: 'items' }, sortItems(option.items).map(itemRow)),
  ];
}

function renderForm(c) {
  const disabled = state.phase === 'saving';
  const radio = (value) => ({
    type: 'radio',
    name: 'choice',
    value,
    checked: state.choice === value,
    disabled,
    onchange: () => set({ choice: value, message: null }),
  });

  const options = h(
    'fieldset',
    { class: 'choice' },
    h('legend', { class: 'legend' }, state.editing ? '선택 수정 — 어느 쪽이 더 끌리나요?' : '어느 쪽이 더 끌리나요?'),
    h(
      'div',
      { class: isPhoto(c) ? 'grid grid-photo' : 'grid' },
      c.options.map((o) =>
        h(
          'label',
          { class: `option${state.choice === o.slot ? ' selected' : ''}`, 'data-slot': o.slot },
          h('input', { class: 'sr-only', ...radio(o.slot), 'aria-label': `코디 ${o.slot.toUpperCase()}` }),
          cardBody(o),
          h('span', { class: 'option-cta' }, state.choice === o.slot ? '선택함' : '이 코디 고르기'),
        ),
      ),
    ),
    h(
      'label',
      { class: `neither${state.choice === 'neither' ? ' selected' : ''}` },
      h('input', radio('neither')),
      '둘 다 아쉬워요',
    ),
  );

  const reasonsLegend = state.choice === 'neither' ? '아쉬운 점 (선택)' : '고른 이유 (선택)';
  const reasons = h(
    'fieldset',
    { class: 'reasons' },
    h('legend', { class: 'legend' }, reasonsLegend),
    h(
      'div',
      { class: 'reason-list' },
      REASONS.map(([code, label]) =>
        h(
          'label',
          { class: `reason${state.reasons.has(code) ? ' on' : ''}` },
          h('input', {
            type: 'checkbox',
            value: code,
            checked: state.reasons.has(code),
            disabled: disabled || !state.choice,
            onchange: (e) => {
              const next = new Set(state.reasons);
              if (e.target.checked) next.add(code);
              else next.delete(code);
              set({ reasons: next });
            },
          }),
          label,
        ),
      ),
    ),
  );

  const counter = h('span', { class: 'count' }, `${[...state.comment].length}/${COMMENT_MAX}`);
  const comment = h(
    'div',
    { class: 'comment' },
    h('label', { class: 'legend', for: 'comment' }, '한 줄 메모 (선택)'),
    h('textarea', {
      id: 'comment',
      rows: '2',
      maxlength: String(COMMENT_MAX),
      placeholder: '예: 바지 핏이 좋아요',
      disabled: disabled || !state.choice,
      oninput: (e) => {
        // 다시 그리지 않는다(입력 중 커서가 튀지 않게). 글자 수만 고친다.
        state.comment = e.target.value;
        counter.textContent = `${[...state.comment].length}/${COMMENT_MAX}`;
        if (state.editing) syncEditButton();
      },
    }),
    counter,
  );
  comment.querySelector('textarea').value = state.comment;

  const unchanged = state.editing && state.choice && sameAsSaved(c.vote, draftVote());
  const actions = state.editing
    ? h(
        'div',
        { class: 'actions' },
        h(
          'button',
          { class: 'btn btn-primary', type: 'button', disabled: !state.choice || disabled || unchanged, onclick: save },
          disabled ? '저장하는 중...' : '수정 저장',
        ),
        h('button', { class: 'btn btn-ghost', type: 'button', disabled, onclick: cancelEdit }, '취소'),
        h(
          'span',
          { class: 'hint' },
          unchanged ? '바뀐 내용이 없어요. 선택·이유·메모 중 하나를 바꿔 주세요.' : '저장하면 이전 선택을 이 내용으로 바꿔요. 취소하면 그대로 둬요.',
        ),
      )
    : h(
        'div',
        { class: 'actions' },
        h(
          'button',
          { class: 'btn btn-primary', type: 'button', disabled: !state.choice || disabled, onclick: save },
          disabled ? '저장하는 중...' : '선택 저장',
        ),
        h(
          'button',
          { class: 'btn btn-ghost', type: 'button', disabled, onclick: () => set({ phase: 'skipped', message: null }) },
          '건너뛰기',
        ),
        h('span', { class: 'hint' }, '건너뛰면 아무것도 저장하지 않아요. ‘둘 다 아쉬워요’는 선택으로 저장돼요.'),
      );

  return [
    options,
    reasons,
    comment,
    state.message ? errorBox(state.message, state.retry) : null,
    actions,
  ];
}

/** 수정 중 메모를 칠 때는 다시 그리지 않으므로 저장 버튼만 고친다. */
function syncEditButton() {
  const btn = root.querySelector('.actions .btn-primary');
  const hint = root.querySelector('.actions .hint');
  const unchanged = sameAsSaved(state.comparison.vote, draftVote());
  if (btn) btn.disabled = unchanged;
  if (hint) {
    hint.textContent = unchanged
      ? '바뀐 내용이 없어요. 선택·이유·메모 중 하나를 바꿔 주세요.'
      : '저장하면 이전 선택을 이 내용으로 바꿔요. 취소하면 그대로 둬요.';
  }
}

function renderResult(c) {
  const v = c.vote;
  const summary =
    v.choice === 'a' ? 'A 코디를 골랐어요.' : v.choice === 'b' ? 'B 코디를 골랐어요.' : '둘 다 아쉽다고 남겼어요.';
  const reasonTitle = v.choice === 'neither' ? '아쉬운 점' : '고른 이유';
  return [
    state.banner ? h('p', { class: `msg msg-${state.bannerTone}`, role: 'status' }, state.banner) : null,
    h('p', { class: 'result-summary' }, summary),
    h(
      'div',
      { class: isPhoto(c) ? 'grid grid-static grid-photo' : 'grid grid-static' },
      c.options.map((o) => h('article', { class: `card${v.choice === o.slot ? ' chosen' : ''}`, 'aria-label': `코디 ${o.slot.toUpperCase()}` }, cardBody(o))),
    ),
    h('p', { class: 'result-line' }, `${reasonTitle}: ${v.reasons.map((r) => REASON_LABEL[r]).join(', ') || '고르지 않음'}`),
    v.comment ? h('p', { class: 'result-line' }, `메모: ${v.comment}`) : null,
    h(
      'div',
      { class: 'actions result-actions' },
      h('button', { class: 'btn btn-outline', type: 'button', onclick: startEdit }, '선택 수정'),
    ),
    h(
      'p',
      { class: 'fine' },
      `데모 기록 · 비교 id ${c.id}${revisionOf(c) > 1 ? ` · ${revisionOf(c) - 1}번 수정함` : ''}`,
    ),
  ];
}

function errorBox(message, retry) {
  return h(
    'div',
    { class: 'msg msg-error', role: 'alert' },
    h('span', {}, message),
    retry ? h('button', { class: 'btn btn-outline btn-sm', type: 'button', onclick: retry }, '다시 시도') : null,
  );
}

let lastPhase = null;

function render() {
  const c = state.comparison;
  root.dataset.phase = state.phase;
  // 결과·오류·건너뛰기로 바뀌면 맨 위로 — 높이가 고정된 호스트에서도 요약이 먼저 보이게.
  const scrollTop = state.phase !== lastPhase && ['saved', 'skipped', 'error', 'syncing'].includes(state.phase);
  lastPhase = state.phase;
  const body = [];
  body.push(h('h1', { class: 'title' }, '둘 중 고르기'));
  body.push(demoBadge(c));
  if (c) body.push(contextLine(c));

  if (state.phase === 'connecting' || state.phase === 'waiting') {
    body.push(h('p', { class: 'loading', role: 'status' }, '샘플 코디를 불러오는 중...'));
  } else if (state.phase === 'syncing') {
    body.push(h('p', { class: 'loading', role: 'status' }, '저장 상태를 확인하는 중...'));
  } else if (state.phase === 'error') {
    body.push(errorBox(state.message ?? '문제가 생겼어요.', state.retry));
  } else if (state.phase === 'skipped') {
    body.push(
      h('p', { class: 'msg', role: 'status' }, '건너뛰었어요. 아무것도 저장하지 않았어요.'),
      h('button', { class: 'btn btn-outline btn-sm', type: 'button', onclick: () => set({ phase: 'ready' }) }, '다시 고르기'),
    );
  } else if (state.phase === 'saved' && c?.vote) {
    body.push(...renderResult(c));
  } else if (c) {
    body.push(...renderForm(c));
  }
  root.replaceChildren(...body.filter(Boolean));
  if (scrollTop) window.scrollTo(0, 0);
}

render();
app
  .connect()
  .then(() => {
    applyHostContext(app.getHostContext());
    if (state.phase === 'connecting') set({ phase: 'waiting' });
  })
  .catch(() => set({ phase: 'error', message: MSG.connect, retry: null }));
