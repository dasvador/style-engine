// 데모 전용 샘플 옷장과 손으로 짠 샘플 코디.
//
// 실제 서비스 옷장·DB·추천 엔진과 아무 관계가 없다. 아이템 id 는 모두 `sample-` 로 시작해
// 실제 의류 id(UUID)와 섞이지 않는다. 코디는 모델이 만든 것이 아니라 아래 표에 고정해 둔 것이다.
// 아이템 모양은 Rust `ItemSnapshot`(src/services/outfit_comparison.rs)과 같은 필드를 쓴다.

/** 위에서 아래로 입는 순서 — 프론트 ComparePage 의 CATEGORY_ORDER 와 같다. */
export const CATEGORIES = ['아우터', '상의', '하의', '신발', '가방'];

/** @typedef {{id:string,name:string,category:string,color:string|null,sub_category:string|null,material:string|null,has_image:boolean}} SampleItem */

/** @type {SampleItem[]} */
export const SAMPLE_WARDROBE = [
  item('sample-outer-01', 'M-65 필드 재킷', '아우터', '올리브', '필드 재킷', '코튼 나일론'),
  item('sample-outer-02', '울 싱글 코트', '아우터', '차콜', '코트', '울'),
  item('sample-outer-03', '데님 트러커 재킷', '아우터', '인디고', '재킷', '데님'),
  item('sample-top-01', '옥스퍼드 버튼다운 셔츠', '상의', '라이트블루', '셔츠', '코튼'),
  item('sample-top-02', '크루넥 니트', '상의', '오트밀', '니트', '울'),
  item('sample-top-03', '포켓 티셔츠', '상의', '화이트', '티셔츠', '코튼'),
  item('sample-top-04', '헤비웨이트 스웨트셔츠', '상의', '헤더그레이', '스웨트셔츠', '코튼'),
  item('sample-top-05', '샴브레이 워크 셔츠', '상의', '스카이블루', '셔츠', '코튼'),
  item('sample-bottom-01', '셀비지 데님', '하의', '인디고', '데님 팬츠', '데님'),
  item('sample-bottom-02', '퍼티그 팬츠', '하의', '올리브', '카고 팬츠', '코튼'),
  item('sample-bottom-03', '울 슬랙스', '하의', '차콜', '슬랙스', '울'),
  item('sample-bottom-04', '치노 팬츠', '하의', '베이지', '치노', '코튼'),
  item('sample-shoes-01', '가죽 서비스 슈즈', '신발', '브라운', '구두', '가죽'),
  item('sample-shoes-02', '캔버스 스니커즈', '신발', '오프화이트', '스니커즈', '캔버스'),
  item('sample-shoes-03', '더비 슈즈', '신발', '블랙', '구두', '가죽'),
  item('sample-bag-01', '캔버스 토트백', '가방', '아이보리', '토트백', '캔버스'),
  item('sample-bag-02', '가죽 브리프케이스', '가방', '다크브라운', '브리프케이스', '가죽'),
];

function item(id, name, category, color, sub_category, material) {
  return { id, name, category, color, sub_category, material, has_image: false };
}

/** 상황 — 기존 앱의 occasion 값과 같은 한국어 이름. */
export const SITUATIONS = /** @type {const} */ ({
  daily: '일상',
  office: '출근',
  weekend: '주말 나들이',
});

/** 무드 — 기존 앱의 mood_key 와 같은 키를 쓴다. */
export const MOODS = /** @type {const} */ ({
  amekaji: '아메카지',
  minimal: '미니멀',
});

/**
 * 상황 × 무드마다 손으로 고른 두 코디와 샘플 날씨 문구. 날씨는 실제 날씨 조회가 아니라 고정 문구다.
 * 두 코디 중 어느 쪽이 "더 좋은" 것으로 정해 두지 않았다 — 순위나 점수가 없다.
 */
export const SAMPLE_PAIRS = {
  'daily:amekaji': {
    weather: { temperature: 16, weather_description: '맑음(샘플)' },
    outfits: [
      ['sample-outer-01', 'sample-top-03', 'sample-bottom-01', 'sample-shoes-01'],
      ['sample-outer-03', 'sample-top-04', 'sample-bottom-02', 'sample-shoes-02'],
    ],
  },
  'daily:minimal': {
    weather: { temperature: 16, weather_description: '맑음(샘플)' },
    outfits: [
      ['sample-top-02', 'sample-bottom-04', 'sample-shoes-02', 'sample-bag-01'],
      ['sample-top-03', 'sample-bottom-03', 'sample-shoes-03'],
    ],
  },
  'office:amekaji': {
    weather: { temperature: 12, weather_description: '흐림(샘플)' },
    outfits: [
      ['sample-outer-01', 'sample-top-01', 'sample-bottom-04', 'sample-shoes-01', 'sample-bag-02'],
      ['sample-top-05', 'sample-top-02', 'sample-bottom-01', 'sample-shoes-01'],
    ],
  },
  'office:minimal': {
    weather: { temperature: 12, weather_description: '흐림(샘플)' },
    outfits: [
      ['sample-outer-02', 'sample-top-01', 'sample-bottom-03', 'sample-shoes-03', 'sample-bag-02'],
      ['sample-outer-02', 'sample-top-02', 'sample-bottom-04', 'sample-shoes-01'],
    ],
  },
  'weekend:amekaji': {
    weather: { temperature: 19, weather_description: '구름 조금(샘플)' },
    outfits: [
      ['sample-top-05', 'sample-bottom-02', 'sample-shoes-02', 'sample-bag-01'],
      ['sample-outer-03', 'sample-top-03', 'sample-bottom-04', 'sample-shoes-01'],
    ],
  },
  'weekend:minimal': {
    weather: { temperature: 19, weather_description: '구름 조금(샘플)' },
    outfits: [
      ['sample-top-04', 'sample-bottom-03', 'sample-shoes-02'],
      ['sample-top-01', 'sample-bottom-04', 'sample-shoes-03', 'sample-bag-01'],
    ],
  },
};

const BY_ID = new Map(SAMPLE_WARDROBE.map((i) => [i.id, i]));

/** 샘플 아이템을 값으로 복사한다(저장 뒤 fixture 를 바꿔도 그때 보여 준 값이 남게). */
export function snapshotItems(ids) {
  return ids.map((id) => {
    const found = BY_ID.get(id);
    if (!found) throw new Error(`fixture 에 없는 아이템: ${id}`);
    return { ...found };
  });
}

/** Rust `item_key` 와 같다: 정렬·중복 제거한 아이템 id 를 `|` 로 잇는다. */
export function itemKey(ids) {
  return [...new Set(ids)].sort().join('|');
}
