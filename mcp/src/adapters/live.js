// 실서비스 연결 경계 — 비활성. 어떤 도구에도 연결돼 있지 않고, config 가 demo 외 모드를 거절한다.
//
// 나중에 실제 Wardrobe Edit 비교(Rust /api/comparisons)에 붙이려면 아래 인터페이스를 채운다.
// 지금 막혀 있는 이유(README "실서비스 연결 경계" 참고):
//   1. Rust 의 AuthUser 는 잘못된 토큰도 `default` 사용자로 받아 준다 → 원격 호출을 사용자별로 가를 수 없다.
//   2. 전체 옷장 API 에 소유권 격리가 없다 → 옷장 조회 도구를 열면 남의 옷장이 보인다.
//   3. 비교는 추천 묶음(batch_id)에서만 만들 수 있고, 묶음은 유료 모델을 부르는 추천 요청이 만든다.
// 이 셋이 풀리기 전에는 3003 이나 운영 DB 를 이 MCP 서버 뒤에 두지 않는다.
//
// 연결 시 기대하는 모양(데모 저장소 DemoStore 와 같은 메서드):
//   listWardrobe(userId, category?)          → { items: ItemSnapshot[] }
//   createComparison(userId, { batchId })    → ComparisonView   (POST /api/comparisons)
//   getComparison(userId, id)                → ComparisonView | null (GET /api/comparisons/{id})
//   submitVote(userId, id, vote)             → { outcome, comparison } (POST /api/comparisons/{id}/vote)
// userId 는 MCP OAuth 로 확인한 사용자여야 하고, 요청 본문·도구 인자로 받지 않는다.

export class LiveAdapterDisabled extends Error {
  constructor() {
    super('실서비스 어댑터는 비활성이에요. 이 MCP 서버는 데모 모드만 지원해요.');
  }
}

export function createLiveAdapter() {
  throw new LiveAdapterDisabled();
}
