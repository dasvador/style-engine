// 사진 비교 세트 — 이 앱에서 이전에 이미 만들어 둔 생성 룩북 사진 두 장으로 하는 A/B 비교.
//
// 새로 추천하거나 새로 생성하지 않는다. 각 사진은 그 사진을 만들 때 실제로 보낸 아이템 목록과
// 짝지어 있다 — 아래 items 는 그 목록의 이름을 그대로 옮긴 것이다(색 이름은 이름의 앞부분).
// 사진 파일 자체와 원본 파일·출처 대응표는 저장소에 넣지 않는다: 로컬 `mcp/.assets/manifest.json`
// (gitignore, scripts/import-image-assets.mjs 가 만든다)에 asset 키로 있다. 파일이 없거나 검사에
// 실패한 세트는 도구에 나오지 않는다(사진 없는 사진 비교를 만들지 않는다).
//
// 아이템 모양은 fixtures.js 의 샘플 아이템과 같다. id 는 `sample-photo-` 로 시작해 샘플 옷장·실제
// 의류 id 와 섞이지 않는다.

/** @typedef {{ key: string, items: import('./fixtures.js').SampleItem[], alt: string }} PhotoSide */

function it(id, name, category, color, material) {
  return { id: `sample-photo-${id}`, name, category, color, sub_category: null, material, has_image: false };
}

export const PHOTO_MOODS = { pop: '팝', edgy: '엣지' };

/**
 * @type {Record<string, { title: string, mood: keyof typeof PHOTO_MOODS, situation: 'daily',
 *   temperature: number, weather_description: string, sides: [PhotoSide, PhotoSide] }>}
 */
export const PHOTO_SETS = {
  pop_color_pattern: {
    title: '팝 — 색 대비 vs 체커보드 무늬',
    mood: 'pop',
    situation: 'daily',
    temperature: 19.6,
    weather_description: '사진 생성 당시 기준(샘플)',
    sides: [
      {
        key: 'pop-red-trucker-cobalt-knit',
        alt: '레드 크롭 트러커 재킷, 코발트 블루 크롭 니트, 인디고 와이드 데님, 블랙 앵클부츠, 화이트 캔버스 토트백을 입은 생성 룩북 사진',
        items: [
          it('pop1-outer', '레드 크롭 트러커 재킷', '아우터', '레드', 'cotton twill'),
          it('pop1-top', '코발트 블루 크롭 니트', '상의', '코발트 블루', 'soft cotton-blend knit'),
          it('pop1-bottom', '인디고 와이드 데님', '하의', '인디고', 'denim'),
          it('pop1-shoes', '블랙 레더 앵클부츠', '신발', '블랙', 'leather'),
          it('pop1-bag', '화이트 캔버스 토트백', '가방', '화이트', 'canvas'),
        ],
      },
      {
        key: 'pop-green-checker-cobalt-pants',
        alt: '그린 화이트 체커보드 니트, 코발트 와이드 팬츠, 화이트 레더 로우 스니커즈, 화이트 캔버스 토트백을 입은 생성 룩북 사진',
        items: [
          it('pop2-top', '그린 화이트 체커보드 니트', '상의', '그린 화이트', 'intarsia knit'),
          it('pop2-bottom', '코발트 와이드 팬츠', '하의', '코발트', 'cotton twill'),
          it('pop2-shoes', '화이트 레더 로우 스니커즈', '신발', '화이트', 'leather'),
          it('pop2-bag', '화이트 캔버스 토트백', '가방', '화이트', 'canvas'),
        ],
      },
    ],
  },
  edgy_leather: {
    title: '엣지 — 버건디 레더 vs 블랙 라이더',
    mood: 'edgy',
    situation: 'daily',
    temperature: 17.4,
    weather_description: '사진 생성 당시 기준(샘플)',
    sides: [
      {
        key: 'edgy-burgundy-leather-barrel',
        alt: '버건디 박시 크롭 레더 재킷, 옐로그린 탱크, 차콜 울 배럴 레그 팬츠, 블랙 니하이 롱부츠, 버건디 박스 크로스백을 입은 생성 룩북 사진',
        items: [
          it('edgy1-outer', '버건디 박시 크롭 레더 재킷', '아우터', '버건디', 'lambskin leather'),
          it('edgy1-top', '옐로그린 비대칭 넥 코튼 탱크', '상의', '옐로그린', 'ribbed cotton'),
          it('edgy1-bottom', '차콜 울 배럴 레그 팬츠', '하의', '차콜', 'wool blend'),
          it('edgy1-shoes', '블랙 레더 니하이 롱부츠', '신발', '블랙', 'leather'),
          it('edgy1-bag', '버건디 레더 박스 크로스백', '가방', '버건디', 'leather'),
        ],
      },
      {
        key: 'edgy-black-rider-grey-denim',
        alt: '블랙 레더 라이더 자켓, 체리 레드 보트넥 저지 탑, 그레이 워시드 크로스오버 데님, 블랙 앵클부츠, 블랙 지오메트릭 숄더백을 입은 생성 룩북 사진',
        items: [
          it('edgy2-outer', '블랙 레더 라이더 자켓', '아우터', '블랙', 'leather'),
          it('edgy2-top', '체리 레드 사선 헴 보트넥 저지 탑', '상의', '체리 레드', 'cotton jersey'),
          it('edgy2-bottom', '그레이 워시드 크로스오버 웨이스트 데님', '하의', '그레이', 'washed denim'),
          it('edgy2-shoes', '블랙 레더 앵클부츠', '신발', '블랙', 'leather'),
          it('edgy2-bag', '블랙 레더 지오메트릭 숄더백', '가방', '블랙', 'leather'),
        ],
      },
    ],
  },
};

/** 모든 세트가 쓰는 asset 키(가져오기 스크립트와 manifest 검사가 쓴다). */
export const PHOTO_ASSET_KEYS = Object.values(PHOTO_SETS).flatMap((s) => s.sides.map((side) => side.key));
