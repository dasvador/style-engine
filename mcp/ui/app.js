// 데모 코디 비교 화면(MCP Apps view). ChatGPT·Claude 같은 호스트가 iframe 으로 띄우고, 공식
// `App` bridge(postMessage JSON-RPC: ui/initialize · ui/notifications/tool-result · tools/call)로만
// 서버와 이야기한다. 화면이 직접 네트워크를 쓰지 않는다.
//
// 기존 ComparePage.tsx 와 같은 규칙: 두 코디는 똑같은 모양, 이유·메모는 선택, "둘 다 아쉬움"은
// 저장되는 선택이고 "건너뛰기"는 아무것도 저장하지 않는다.

import { App } from '@modelcontextprotocol/ext-apps';

const TOOL_VOTE = 'submit_demo_preference';

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
];
function swatch(color) {
  if (!color) return null;
  const hit = SWATCHES.find(([name]) => color.includes(name));
  return hit ? hit[1] : null;
}

// ─── 상태 ───

/**
 * phase: connecting · waiting(도구 실행 중) · ready(고르는 중) · saving · saved · skipped · error
 * @type {{ phase: string, comparison: any, choice: string|null, reasons: Set<string>, comment: string,
 *          message: string|null, retry: (() => void)|null, banner: string|null }}
 */
const state = {
  phase: 'connecting',
  comparison: null,
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

// ─── bridge ───

const app = new App({ name: 'wardrobe-edit-demo-compare', version: '0.1.0' }, {});

app.ontoolinput = () => {
  if (!state.comparison) set({ phase: 'waiting', message: null });
};
app.ontoolcancelled = () => {
  if (!state.comparison) set({ phase: 'error', message: '도구 실행이 취소됐어요.', retry: null });
};
app.ontoolresult = (result) => applyResult(result, { fromHost: true });
app.onhostcontextchanged = (ctx) => applyHostContext(ctx);

/** 도구 결과(호스트가 보낸 것이든 화면이 부른 것이든)를 화면 상태로 바꾼다. */
function applyResult(result, { fromHost = false, retry = null } = {}) {
  const sc = result?.structuredContent;
  if (result?.isError) {
    // 서버 오류 세부는 _meta 에 온다(src/mcp-server.js ERROR_META_KEY). SDK 입력 검증 오류에는 글자만 있다.
    const err = result._meta?.['wardrobe-edit/error'];
    const msg = err?.message ?? textOf(result) ?? '요청을 처리하지 못했어요.';
    if (err?.code === 'conflict' && err.comparison) {
      // 다른 선택이 먼저 저장돼 있다. 저장된 쪽을 보여 준다.
      return set({ phase: 'saved', comparison: err.comparison, banner: msg, bannerTone: 'caution', message: null, retry: null });
    }
    // 저장 실패면 고르던 화면으로 돌아가 내용을 지키고, 그 밖에는 오류 화면.
    if (state.phase === 'saving' && !fromHost) return set({ phase: 'ready', message: msg, retry });
    return set({ phase: 'error', message: msg, retry });
  }
  const comparison = sc?.comparison ?? (sc?.options ? sc : null);
  if (!comparison) {
    return set({ phase: 'error', message: '이 화면이 읽을 수 없는 결과예요.', retry: null });
  }
  if (comparison.vote) {
    const banner = sc?.already_submitted ? '같은 선택이 이미 저장돼 있었어요.' : sc?.ok ? '선택을 저장했어요.' : null;
    return set({ phase: 'saved', comparison, banner, bannerTone: 'ok', message: null, retry: null });
  }
  // 같은 비교의 새 결과가 와도 고르던 내용은 지우지 않는다.
  const same = state.comparison?.id === comparison.id;
  set({
    phase: 'ready',
    comparison,
    choice: same ? state.choice : null,
    reasons: same ? state.reasons : new Set(),
    comment: same ? state.comment : '',
    message: null,
    retry: null,
    banner: null,
  });
}

function textOf(result) {
  return result?.content?.find?.((c) => c.type === 'text')?.text ?? null;
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

async function save() {
  const c = state.comparison;
  if (!c || !state.choice || state.phase === 'saving') return;
  const args = {
    comparison_id: c.id,
    choice: state.choice,
    reasons: REASONS.map(([code]) => code).filter((code) => state.reasons.has(code)),
  };
  const comment = state.comment.trim();
  if (comment) args.comment = comment;
  set({ phase: 'saving', message: null, retry: null });
  try {
    const result = await callTool(TOOL_VOTE, args);
    applyResult(result, { retry: save });
    if (!result.isError) void tellModel(result.structuredContent?.comparison);
  } catch (e) {
    // 호스트·네트워크 문제. 고른 내용은 그대로 두고 다시 보낼 수 있게 한다(같은 내용이면 서버가 한 번만 남긴다).
    set({ phase: 'ready', message: `저장하지 못했어요: ${errorText(e)}`, retry: save });
  }
}

/** 호스트가 지원하면 저장한 선택을 모델 맥락에 알린다. 지원하지 않으면 조용히 넘어간다. */
async function tellModel(comparison) {
  if (!comparison?.vote || !app.getHostCapabilities()?.updateModelContext) return;
  const v = comparison.vote;
  const choice = v.choice === 'neither' ? '둘 다 아쉬움' : `${v.choice.toUpperCase()} 코디`;
  const reasons = v.reasons.map((r) => REASON_LABEL[r]).join(', ') || '없음';
  try {
    await app.updateModelContext({
      content: [
        {
          type: 'text',
          text: `[데모 비교 ${comparison.id}] 사용자가 화면에서 "${choice}"를 골랐고 데모 기록에 저장됐다. 이유: ${reasons}.${v.comment ? ` 메모: ${v.comment}` : ''}`,
        },
      ],
    });
  } catch {
    /* 선택은 이미 저장됐다 — 모델 맥락 알림은 덤이다. */
  }
}

function errorText(e) {
  return e instanceof Error ? e.message : String(e);
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

function demoBadge() {
  return h(
    'p',
    { class: 'demo-banner', role: 'note' },
    h('strong', {}, '데모'),
    ' 샘플 옷장으로 미리 짜 둔 고정 코디예요. 실제 옷장이나 AI 추천 결과가 아니에요.',
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

function cardBody(option) {
  return [
    h('span', { class: 'card-head' }, option.slot.toUpperCase()),
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
    h('legend', { class: 'legend' }, '어느 쪽이 더 끌리나요?'),
    h(
      'div',
      { class: 'grid' },
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
      },
    }),
    counter,
  );
  comment.querySelector('textarea').value = state.comment;

  const actions = h(
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
      { class: 'grid grid-static' },
      c.options.map((o) => h('article', { class: `card${v.choice === o.slot ? ' chosen' : ''}`, 'aria-label': `코디 ${o.slot.toUpperCase()}` }, cardBody(o))),
    ),
    h('p', { class: 'result-line' }, `${reasonTitle}: ${v.reasons.map((r) => REASON_LABEL[r]).join(', ') || '고르지 않음'}`),
    v.comment ? h('p', { class: 'result-line' }, `메모: ${v.comment}`) : null,
    h('p', { class: 'fine' }, `데모 기록 · 비교 id ${c.id}`),
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
  const scrollTop = state.phase !== lastPhase && ['saved', 'skipped', 'error'].includes(state.phase);
  lastPhase = state.phase;
  const body = [];
  body.push(h('h1', { class: 'title' }, '둘 중 고르기'));
  body.push(demoBadge());
  if (c) body.push(contextLine(c));

  if (state.phase === 'connecting' || state.phase === 'waiting') {
    body.push(h('p', { class: 'loading', role: 'status' }, '샘플 코디를 불러오는 중...'));
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
  .catch((e) => set({ phase: 'error', message: `호스트와 연결하지 못했어요: ${errorText(e)}`, retry: null }));
