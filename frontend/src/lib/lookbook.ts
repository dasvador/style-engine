/**
 * 기존 UI(`src/routes/home.rs`)의 순수 표현 로직을 그대로 옮긴 것.
 *
 * 규칙과 문자열 목록을 바꾸지 않았다 — 화면에 뜨는 결과가 달라지지 않아야 하기 때문이다.
 */

import type { ChatItem, OutfitItem, Role } from '../types/api';

/** 아이템 이름들로 룩북 제목을 고른다. 순서가 곧 우선순위다. */
export function generateLookbookTitle(items: readonly { name: string }[]): string {
  const s = items.map((i) => i.name).join(' ');
  if (s.includes('워크') && s.includes('데님')) return 'Worn-In Workwear';
  if (s.includes('밀리터리') || s.includes('유틸리티')) return 'Muted Utility';
  if (s.includes('워크')) return 'Soft Workwear';
  if (s.includes('코듀로이') || s.includes('부츠')) return 'Grounded Vintage';
  if (s.includes('니트') || s.includes('하링턴')) return 'Faded Ivy';
  if (s.includes('슬랙스') || s.includes('옥스포드')) return 'Clean Minimal';
  if (s.includes('린넨') || s.includes('리넨')) return 'Dry Linen Layer';
  if (s.includes('올리브') || s.includes('카키')) return 'Olive Field';
  if (s.includes('데님')) return 'Washed Indigo';
  return 'Curated Daily';
}

export function generateMoodSubtitle(items: readonly { name: string }[]): string {
  const s = items.map((i) => i.name).join(' ');
  const tones: string[] = [];
  if (s.includes('네이비') || s.includes('인디고')) tones.push('navy');
  if (s.includes('올리브') || s.includes('카키')) tones.push('olive');
  if (s.includes('베이지') || s.includes('크림') || s.includes('오트밀')) tones.push('warm neutral');
  if (s.includes('차콜') || s.includes('그레이')) tones.push('charcoal');
  if (s.includes('블랙')) tones.push('black');

  const textures: string[] = [];
  if (s.includes('워시드') || s.includes('페이디드')) textures.push('washed');
  if (s.includes('데님')) textures.push('denim');
  if (s.includes('캔버스')) textures.push('canvas');
  if (s.includes('린넨') || s.includes('리넨')) textures.push('linen');
  if (s.includes('코튼')) textures.push('cotton');
  if (s.includes('니트') || s.includes('스웻')) textures.push('knit');

  const tone = tones.slice(0, 2).join(' + ') || 'muted tone';
  const tex = textures.slice(0, 2).join(' · ') || 'natural texture';
  return `${tone} balance · ${tex}`;
}

export function extractTextureTags(items: readonly { name: string }[]): string[] {
  const tags = new Set<string>();
  for (const i of items) {
    const n = i.name;
    if (n.includes('워시드') || n.includes('페이디드')) tags.add('washed');
    if (n.includes('슬러브') || n.includes('멜란지')) tags.add('slubby');
    if (n.includes('스웨이드')) tags.add('suede');
    if (n.includes('데님')) tags.add('denim');
    if (n.includes('캔버스')) tags.add('canvas');
    if (n.includes('린넨') || n.includes('리넨')) tags.add('dry linen');
    if (n.includes('코튼')) tags.add('cotton');
    if (n.includes('나일론')) tags.add('nylon');
    if (n.includes('레더')) tags.add('leather');
    if (n.includes('코듀로이')) tags.add('corduroy');
    if (n.includes('울')) tags.add('wool');
    if (n.includes('니트')) tags.add('knit');
    if (n.includes('스웻')) tags.add('fleece');
    if (n.includes('트윌')) tags.add('twill');
  }
  return [...tags].slice(0, 5);
}

export function extractSilhouetteTags(items: readonly ChatItem[]): string[] {
  const tags = new Set<string>();
  const s = items.map((i) => i.name).join(' ');
  if (s.includes('부츠') || s.includes('워크')) tags.add('grounded');
  if (s.includes('블루종') || s.includes('코치') || s.includes('자켓')) tags.add('structured');
  if (s.includes('슬랙스') || s.includes('치노')) tags.add('clean line');
  if (s.includes('스니커') || s.includes('슬립온') || s.includes('트레이너')) tags.add('lightweight');
  if (s.includes('스웻') || s.includes('후디')) tags.add('relaxed');
  if (s.includes('가디건') || s.includes('셔츠')) tags.add('layered');
  if (items.some((i) => i.slot === 'bag')) tags.add('utility carry');
  return [...tags].slice(0, 4);
}

/**
 * Style Note 정리.
 *
 * LLM 이 마크다운 강조와 아이템 나열을 섞어 보내는 일이 있어, 기존 UI 가 하던 대로
 * 굵게 표시와 슬롯 나열 줄을 걷어낸다. 결과는 텍스트로만 렌더링한다.
 */
export function cleanStyleNote(desc: string): string {
  return desc
    .replace(/\*\*/g, '')
    .replace(/- /g, '')
    .replace(/\n\n/g, '\n')
    .replace(/^.*?[:：]\s*/gm, (m) =>
      m.includes('상의') || m.includes('하의') || m.includes('신발') || m.includes('가방') || m.includes('아우터')
        ? ''
        : m,
    )
    .split('\n')
    .filter((l) => !/^[·•]\s*(상의|하의|아우터|신발|가방)/.test(l))
    .join('\n')
    .trim();
}

/** 이미지 생성 API 가 받는 한 줄 설명으로 변환. */
const IMAGE_SLOT_LABEL: Record<string, string> = {
  inner: 'top/inner',
  outer: 'outerwear',
  bottom: 'pants',
  shoes: 'shoes',
  bag: 'bag',
};

export function buildImagePrompt(items: readonly { slot: string; name: string; material?: string | null }[]): string {
  return items
    .map((i) => {
      const slot = IMAGE_SLOT_LABEL[i.slot] ?? i.slot;
      const mat = i.material ? ` (${i.material})` : '';
      return `${slot}: ${i.name}${mat}`;
    })
    .join(', ');
}

/** 추천 결과(카테고리 기반)를 이미지 생성용 슬롯으로 매핑. */
const CATEGORY_TO_SLOT: Record<string, string> = {
  상의: 'inner',
  아우터: 'outer',
  하의: 'bottom',
  신발: 'shoes',
  가방: 'bag',
};

export function outfitToImageItems(
  outfit: readonly OutfitItem[],
): { slot: string; name: string; material: string | null }[] {
  return outfit.map((o) => ({
    slot: CATEGORY_TO_SLOT[o.category] ?? o.category,
    name: o.name,
    material: o.material ?? null,
  }));
}

export function getWeatherEmoji(desc: string | undefined): string {
  if (!desc) return '';
  if (desc.includes('맑')) return '☀️';
  if (desc.includes('구름') || desc.includes('흐림')) return '☁️';
  if (desc.includes('비') || desc.includes('소나기')) return '🌧️';
  if (desc.includes('눈')) return '🌨️';
  if (desc.includes('안개')) return '🌫️';
  return '🌤️';
}

export function getRoleExplanation(role: Role | string): string {
  const explanations: Record<string, string> = {
    베이스: "이 아이템은 '베이스' 역할입니다 — 대부분 코디의 중심을 잡아주는 기본 아이템이에요.",
    포인트: "이 아이템은 '포인트' 역할입니다 — 코디에 포인트와 개성을 더해주는 아이템이에요.",
    약한포인트: "이 아이템은 '약한포인트' 역할입니다 — 부드럽게 포인트를 더하는 보조 아이템이에요.",
    구조템: "이 아이템은 '구조템' 역할입니다 — 전체 실루엣과 핏을 잡아주는 아이템이에요.",
    연결템: "이 아이템은 '연결템' 역할입니다 — 다른 아이템들 사이를 자연스럽게 연결해주는 아이템이에요.",
  };
  return explanations[role] ?? `이 아이템의 역할: ${role}`;
}

export function getRoleChipClass(role: Role | string): string {
  switch (role) {
    case '베이스':
      return '';
    case '포인트':
      return 'chip-warning';
    case '구조템':
      return 'chip-success';
    default:
      return 'chip-neutral';
  }
}

export function scoreClass(score: number): string {
  if (score >= 88) return 'score-great';
  if (score >= 73) return 'score-good';
  if (score >= 55) return 'score-okay';
  return 'score-awkward';
}

export function verdictColor(score: number): string {
  if (score >= 88) return 'var(--success)';
  if (score >= 73) return 'var(--primary)';
  if (score >= 55) return 'var(--warning)';
  return 'var(--danger)';
}

export const THICKNESS_LABEL: Record<string, string> = {
  thin: '얇은',
  medium: '보통',
  thick: '두꺼운',
};

export const MODE_ICON: Record<string, string> = {
  todays_pick: '⭐',
  variation: '🔄',
  dormant_revival: '👀',
};

// 피드백 사유 (기존 UI 와 동일)
export const LIKE_REASONS = [
  'good_texture_balance',
  'good_grounding',
  'good_color_depth',
  'good_denim_bridge',
  'good_body_balance',
] as const;

export const DISLIKE_REASONS = [
  'too_military',
  'too_dark',
  'too_flat',
  'too_light',
  'floating_balance',
  'color_repetition',
  'style_overload',
] as const;

export const REASON_LABELS: Record<string, string> = {
  good_texture_balance: '질감 좋음',
  good_grounding: '안정감 좋음',
  good_color_depth: '색감 좋음',
  good_denim_bridge: '데님 조합 좋음',
  good_body_balance: '체형 밸런스 좋음',
  too_military: '너무 군복 같음',
  too_dark: '너무 어두움',
  too_flat: '너무 밋밋함',
  too_light: '너무 가벼움',
  floating_balance: '떠 보임',
  color_repetition: '색 반복',
  style_overload: '스타일 과다',
};

export const LIKE_MESSAGES = [
  '다음에도 이런 조합 위주로 추천할게요',
  '취향 반영했어요',
  '비슷한 스타일 더 찾아볼게요',
];

export const DISLIKE_MESSAGES = [
  '다음엔 이런 조합은 줄일게요',
  '반영했어요, 다른 방향으로 시도해볼게요',
  '알겠어요, 조정할게요',
];
