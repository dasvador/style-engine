# Wardrobe Edit MCP 서버 (데모 PoC + 계정 연결 시험 운영)

ChatGPT에서 계정을 연결해 실제 옷장을 조회하고 코디를 추천받는 MCP 서버다. MCP Apps 기반의 샘플 코디 비교·선택 화면도 제공한다. 모드가 둘이다.

- **`demo`(기본)**: 인증 없음, 샘플 데이터만. 흐름: 샘플 옷장 보기 → 상황·무드별 샘플 코디 두 개 비교 화면(MCP Apps) → A / B / 둘 다 아쉬움 선택과 이유 저장.
- **`live`**: 운영자가 발급한 일회용 코드로 OAuth 계정 연결을 하는 **시험 운영(pilot)**. 위 데모 도구에 더해, 연결된 계정 본인의
  프로필·옷장을 **읽기 전용**으로 보여 주고, 설정하면 Rust 추천 엔진으로 **내 옷장 기준 오늘 코디 추천**(`recommend_my_outfit`)을 한다.
  옷 사진 등록·코디 사진 생성은 아직 없다. 아래 "계정 연결(live 모드)".

> **demo 모드는 데모 데이터만 쓴다.** 옷장은 `src/fixtures.js`에 고정한 17벌의 샘플이고, 코디는 상황×무드마다 손으로 짜 둔
> 두 벌이다. AI가 만든 추천이 아니며 실제 서비스 옷장·DB·추천 엔진·OpenAI API·`.env`를 읽지 않는다.
> 화면과 모든 도구 응답에 이 점을 표시한다.
>
> 로컬에 asset을 준비해 두면(아래 "생성 룩북 사진") **이 앱에서 이전에 이미 만들어 둔 룩북 사진 두 장**으로 하는
> 사진 비교도 열 수 있다. 새로 추천·생성하지 않으며, 사진 세트 두 개(팝·엣지)만 있다. 다른 무드·상황에는 사진이 없다.

Rust 본 서버(3003)와 별도 프로세스로 실행한다. 데모와 live 옷장 조회는 3003 없이 동작하지만, 실제 코디 추천은 3003이 필요하다. demo 모드는 데이터도 공유하지 않는다
(live 모드만 서비스 DB를 직접 SELECT 한다). 데모 비교와 같은 것은 데이터 계약뿐이다:
선택 값(`a`/`b`/`neither`), 이유 코드 5개, 메모 200자 규칙, "먼저 저장한 것이 남는다", 비교 응답 모양
(`id`·`context`·`options[{slot, items}]`·`vote`)이 `src/services/outfit_comparison.rs`·`ComparePage.tsx`와 같다.

## 빠른 실행

```sh
cd mcp
npm ci            # lockfile 그대로 설치(@modelcontextprotocol/sdk 1.32.1, ext-apps 1.7.5)
npm start         # UI 빌드(dist/ui.html) 후 http://127.0.0.1:8787/mcp
```

끝내기: 실행한 터미널에서 `Ctrl+C`. 백그라운드로 띄웠다면 시작 로그의 pid로 `kill <pid>`
(또는 `lsof -tiTCP:8787 -sTCP:LISTEN | xargs kill`). SIGINT/SIGTERM을 받으면 연결을 닫고 끝난다.

| 환경 변수 | 기본값 | 뜻 |
|---|---|---|
| `MCP_PORT` | `8787` | 포트. `3003`(Rust 본 서버)은 거절한다 |
| `MCP_HOST` | `127.0.0.1` | 바인딩 주소. 루프백 그대로 두고 터널로만 내보낸다 |
| `PUBLIC_HOST` | 없음 | 터널 공개 호스트 이름(예: `abcd.trycloudflare.com`). Host 헤더 허용 목록에 더한다 |
| `ALLOWED_HOSTS` | 없음 | 그 밖에 받을 Host 값(쉼표 구분, 포트 포함 그대로) |
| `ALLOWED_ORIGINS` | 없음 | `Origin` 헤더가 붙은 요청을 받을 출처(쉼표 구분). 기본은 Origin이 붙으면 모두 403 |
| `DEMO_DATA_FILE` | `mcp/.data/demo-store.json` | 데모 선택 기록 파일 |
| `DEMO_ASSETS_DIR` | `mcp/.assets` | 생성 룩북 사진 사본·manifest(로컬, gitignore). 없으면 사진 비교 도구가 나오지 않는다 |
| `DEMO_TTL_HOURS` | `168` | 비교 보존 시간(7일) |
| `DEMO_MAX_COMPARISONS` | `500` | 보존 개수 상한(넘으면 오래된 것부터 지움) |
| `MAX_BODY_BYTES` | `65536` | 요청 본문 상한 |
| `RATE_LIMIT_PER_MINUTE` | `300` | 프로세스 전체 1분 요청 수 상한(넘으면 429) |
| `WARDROBE_DELEGATION_KEY` | 없음 | live 전용. 실제 추천을 켠다. Rust 서버와 **같은** 32바이트 이상 hex 키. 없으면 `recommend_my_outfit`가 나오지 않는다. 출력·커밋하지 않는다 |
| `LIVE_RECOMMEND_PORT` | `3003` | live 전용. Rust 서버 포트. 호스트는 언제나 `127.0.0.1`(바꿀 수 없다) |
| `RECOMMEND_TIMEOUT_SECONDS` | `90` | live 전용. 추천 응답을 기다리는 최대 시간(5..300). 넘으면 끊고 다시 부르지 않는다 |
| `RECOMMEND_MAX_CONCURRENT` | `2` | live 전용. 동시에 보내는 추천 수(1..8). 한 계정은 언제나 하나 |
| `MCP_ACCESS_LOG` | 없음 | `1`이면 요청마다 진단용 한 줄(`access 메서드 경로 상태 ms rpc=메서드(도구) auth=yes/no origin=호스트 accept proto ua`)을 stderr에 남긴다. 토큰·쿼리·본문·도구 인자·결과 값은 남기지 않는다 |
| `WARDROBE_MCP_MODE` | `demo` | `demo`(샘플 데이터, 인증 없음) 또는 `live`(계정 연결 OAuth + 본인 옷장 읽기). 그 밖의 값이면 시작하지 않는다 |
| `LIVE_DATABASE_URL` | 없음 | live 전용. 옷장 DB 접속 문자열(읽기만 한다). 로그에 남기지 않는다 |
| `LIVE_LOCAL_ISSUER` | 없음 | live 전용. `1`이면 발급자를 `http://127.0.0.1:포트`로(로컬 시험용, ChatGPT 에서는 못 씀) |
| `OAUTH_REDIRECT_URIS` | 없음 | live 전용. 기본 `https://chatgpt.com/connector_platform_oauth_redirect`에 더할 정확한 콜백(쉼표 구분) |
| `AUTH_STORE_FILE` | `mcp/.data/auth-store.json` | live 전용. 클라이언트 등록·코드·토큰(해시만) |
| `LINK_CODES_FILE` | `mcp/.data/link-codes.json` | live 전용. 계정 연결 코드(해시만) |

## 도구

| 도구 | 하는 일 | 부작용 | annotation |
|---|---|---|---|
| `browse_sample_wardrobe` | 샘플 옷장 목록(`category` 선택) | 없음 | readOnly, idempotent |
| `create_demo_comparison` | `situation`(`daily`·`office`·`weekend`) × `mood`(`amekaji`·`minimal`)의 샘플 두 코디로 새 비교를 만들고 화면을 띄움. 좌우는 처음 한 번 무작위 | 비교(노출) 1건 기록 | 쓰기, 비멱등, 비파괴 |
| `get_demo_comparison` | 비교 하나와 저장된 선택을 다시 읽고 화면을 띄움 | 없음 | readOnly, idempotent |
| `submit_demo_preference` | 첫 선택 저장: `choice`(`a`·`b`·`neither`), `reasons`(선택), `comment`(선택, 200자) | 선택이 없을 때만 저장. 이미 있으면 바꾸지 않음 | 쓰기, 멱등, 비파괴 |
| `update_demo_preference` | 저장된 선택 바꾸기: `expected_revision` + 바꾼 뒤의 `choice`·`reasons`·`comment` 전체 | 본 revision이 지금과 같을 때만 바꾸고 revision +1 | 쓰기, 멱등, **파괴(덮어씀)** |
| `create_demo_photo_comparison` | `set`(사진이 준비된 세트만: `pop_color_pattern`·`edgy_leather`)의 이미 생성해 둔 룩북 사진 두 장으로 새 비교. **asset이 있을 때만 나온다** | 비교(노출) 1건 기록 | 쓰기, 비멱등, 비파괴 |

### 선택 저장·수정 결과

`submit`·`update`는 예상 가능한 결과를 모두 **오류가 아닌 정상 결과**로 돌려준다
(`structuredContent = { ok, outcome, already_submitted, comparison }`, `content[0].text = "[outcome] …"`).
호스트가 `isError`를 자체 오류로 바꿔 던지는 경우(ChatGPT에서 본 `MCP error -32000 …`)를 피하려는 것이다.

| outcome | ok | 뜻 |
|---|---|---|
| `saved` | true | 첫 선택을 저장(revision 1) |
| `updated` | true | 선택을 바꿈(revision +1) |
| `already_submitted` | true | 이미 같은 내용 — 바뀐 것 없음(같은 요청 재시도 포함) |
| `conflict` | false | submit 했는데 다른 선택이 이미 있음 — 저장 안 함. 바꾸려면 update |
| `stale` | false | update 했는데 그사이 revision이 바뀜 — 바꾸지 않고 최신 상태를 돌려줌 |
| `no_vote` | false | update 했는데 저장된 선택이 없음 — submit 먼저 |

선택은 비교당 늘 하나다(`vote.revision`·`updated_at`·처음 저장 시각 `submitted_at`). 수정하면 이전 값은 남기지 않는다.
이유만, 메모만 바꿀 때도 `update`에 남길 값을 모두 보낸다(빠진 `reasons`·`comment`는 지운다는 뜻이다).

위 데모 도구는 `openWorldHint: false`이고 모르는 인자를 거절한다(`additionalProperties: false`).
"건너뛰기"는 화면에만 있는 동작이며 아무것도 저장하지 않는다. "둘 다 아쉬움"(`neither`)은 저장되는 선택이다.

그 밖의 오류(없는 비교 등)는 `isError: true`, `content[0].text = "[code] 메시지"`, 세부는 `_meta["wardrobe-edit/error"]`
(`code`: `not_found`·`invalid_vote`·`unknown_scenario`)에 싣는다. 0.1.0의 `conflict` 오류는 위 `outcome: conflict`로 바뀌었다.
`structuredContent`에 넣지 않는 이유: 공식 SDK 클라이언트가 오류 결과도 출력 스키마로 검사해 호스트에서 깨진다.
인자 형식 오류(enum·id 모양·길이)는 SDK 입력 검증이 `isError` 결과로 돌려준다.

UI 리소스: `ui://wardrobe-edit/compare-v3.html` (`text/html;profile=mcp-app`). CSP `connectDomains`는 비어 있고,
`resourceDomains`는 사진 asset이 있을 때만 이 서버 자신의 origin 하나(`https://PUBLIC_HOST`, 로컬이면 `http://127.0.0.1:포트`)다.
v2는 선택 수정, v3는 사진이 생기며 올렸다 — 호스트가 옛 HTML을 캐시해 두었어도 새 도구 결과는 v3 화면을 받는다.
`compare-v1.html`·`compare-v2.html`은 이미 대화에 남은 옛 카드를 위한 별칭으로 남겨 같은 새 HTML·CSP를 돌려준다(새 화면은 옛 결과 모양도 읽는다).
호스트가 옛 HTML을 계속 캐시하면 옛 카드는 옛 화면 그대로일 수 있다 — 새로 만든 카드에서 확인할 것.
화면은 외부 네트워크를 쓰지 않고 호스트 bridge(`tools/call`)로만 `get`·`submit`·`update`를 부른다.

화면 동작:
- 호스트가 도구 결과를 보내면(대화에 남은 옛 카드를 다시 연 경우 포함) 그대로 그리지 않고 `get_demo_comparison`으로 지금 상태를 읽는다
  ("저장 상태를 확인하는 중..."). 이미 선택이 있으면 처음부터 저장된 화면. 실패하면 짧은 안내와 다시 시도.
  같은 비교에 대한 자동 조회는 5번까지(호스트가 결과를 되풀이해 보내도 끝없이 돌지 않음), 늦게 온 옛 응답은 버린다.
- 저장된 화면의 **선택 수정** → 저장된 선택·이유·메모가 채워진 폼. 바뀐 것이 없으면 "수정 저장"이 꺼져 있다. **취소**하면 원래 화면.
- 저장·수정 호출이 예외로 끝나면(호스트가 오류로 감쌈·시간 초과·연결 끊김) 원문을 보여 주지 않고 `get`으로 다시 읽어
  이미 반영됐으면 성공, 다른 선택이 있으면 conflict/stale 안내, 아무것도 안 바뀌었으면 입력을 그대로 두고 "다시 시도".
OpenAI 전용 `_meta`(`openai/*`)는 쓰지 않는다 — MCP Apps 표준만 쓴다.

## 생성 룩북 사진(로컬 asset)

사진 비교는 이 앱이 예전에 이미 만들어 둔 생성 룩북 사진(`static/images/`의 생성 결과, `outfit_image` 행)을 다시 쓴다.
새로 생성하지 않고 유료 API를 부르지 않는다. 사진 파일과 원본 대응표는 **저장소에 넣지 않는다** — 로컬 `mcp/.assets/`(gitignore)에만 둔다.

- 세트 정의(커밋됨): `src/photo-sets.js` — 세트마다 두 쪽, 각 쪽 = 그 사진을 만들 때 보낸 아이템 목록(이름 그대로)과 asset 키·alt.
  아이템과 사진은 1:1이다. 사진을 다른 아이템 묶음에 끼우지 않는다.
- 로컬 대응표(커밋 안 함): `mcp/.assets/sources.json` — `{ "assets": { "<asset 키>": { "static_file": "<UUID>.png", "source": {근거} } } }`.
- 가져오기: `node scripts/import-image-assets.mjs` → `static/images/<UUID>.png`(일반 파일·PNG만)을 읽는다.
  **원본 PNG를 바이트 그대로** 복사한다. 줄이거나 다시 인코딩하거나 메타데이터를 지우지 않는다 — 생성 원본의
  C2PA 출처 정보(`caBX` 청크)를 그대로 보존해 내보낸다. 이름만 `mcp/.assets/images/<sha256 앞 32자>.png`로 바꾸고
  (원본 경로·UUID는 밖으로 나가지 않는다), 복사본 sha256이 원본과 같은지 확인한 뒤 `manifest.json`을 만든다
  (`provenance: { byte_identical_to_original, c2pa_chunk_present }`). manifest에 없는 옛 파일은 지운다.
  한 장 상한은 8MB(원본 약 2.1~2.3MB, 1024×1536).
- 서버는 시작할 때 manifest를 검사한다(이름 모양·심볼릭 링크 거절·디렉터리 안·서명=MIME·sha256). 통과한 사진만 허용 목록에 넣는다.
  두 장이 모두 있는 세트만 도구에 나온다. 시작 로그에 `생성 룩북 사진: asset N개, 사진 세트 …`와 제외 이유가 찍힌다.
- 내보내는 경로는 `GET|HEAD /assets/img/<해시>.png` 하나다. 허용 목록 이름만 조회하고 경로로 파일을 찾지 않는다. 그 밖(원본 `static/images`·
  manifest·sources·`..`)은 404다. `content-type`은 실제 형식이고, `ETag`·`Cache-Control: immutable`·`nosniff`·`Cross-Origin-Resource-Policy: cross-origin`을 붙인다.
- 사진 URL은 `PUBLIC_HOST`로만 만든다(`https://PUBLIC_HOST/assets/img/…`). 요청의 `Host`·`X-Forwarded-*`는 믿지 않는다.
  `PUBLIC_HOST`가 없으면 로컬 검사용 `http://127.0.0.1:포트`라 ChatGPT·Claude에서는 사진이 안 보인다(시작 로그에 경고).
- 저장 파일에는 사진 URL이 아니라 asset 키만 남긴다. 나중에 사진이 빠지면 그 비교는 `image: null`로 글자·색 점만 보인다(다른 사진으로 바꾸지 않는다).
- 화면: 두 카드에 같은 크기(2:3) 사진 칸, `object-fit: contain`(전신이 잘리지 않음). 모바일에서도 A·B를 나란히 둔다.
  사진을 못 불러오면 칸 크기는 그대로 두고 대체 문구를 보여 주며, 카드는 아이템 글자로 계속 고를 수 있다.

다른 컴퓨터에서 준비하기: 같은 생성 결과 파일이 `static/images/`에 있어야 한다. `mcp/.assets/sources.json`을 위 모양으로 쓰고
가져오기를 돌린다. 파일이 없으면 사진 비교 도구가 나오지 않을 뿐 나머지는 그대로 동작한다.

## 데모 기록 파일

- 위치: `mcp/.data/demo-store.json`(gitignore). 형식은 `src/demo-store.js` 머리 주석 참고(`version: 1`).
  0.2.0에서 선택에 `revision`·`updated_at`이 더해졌다. 0.1.0 파일은 그대로 읽힌다(없는 값은 revision 1, updated_at = submitted_at).
- 수명: 비교마다 7일, 최대 500개. 서버를 다시 켜도 남는다. 파일을 지우면 모두 사라진다.
- 한 프로세스만 이 파일을 쓴다고 가정한다(같은 파일로 서버 두 개를 띄우지 말 것).
- 인증이 없다. 공개 URL을 아는 사람은 누구나 데모 비교를 만들 수 있고, 비교 id(128비트 무작위)를 알면 읽고 선택할 수 있다.
  목록 도구는 없다. 개인 정보를 적지 말 것.

## 테스트

```sh
npm test    # 빌드 후 test/*.test.js (node:test)
```

- `test/store.test.js` — 선택 검증(Rust `validate_vote`와 같은 규칙), 첫 저장의 충돌·재제출, 명시 수정(revision·stale·no_vote·이유만/메모만·동시 10건 중 1건), 0.1.0 파일 호환, 비교 id 혼용, 동시 20건, 파일 재시작, 만료·상한, 저장 실패 시 되돌리기.
- `test/mcp-http.test.js` — 공식 SDK `Client` + Streamable HTTP로 initialize·tools/list·resources/read·tools/call 전체, 동시 12 클라이언트, 재시작, 그리고 원시 HTTP 경계(깨진 JSON 400, Content-Type 415, 64KB 초과 413, 배치 제한, GET/DELETE/OPTIONS 405·CORS 없음, Host 403, Origin 403, `PUBLIC_HOST`, 429).
- `test/images.test.js` — asset 검사(손상·서명 불일치·심볼릭 링크·이름·없는 파일), 이미지 GET/HEAD·MIME·ETag·304·405, 경로 조작·원본 static·manifest·`.env` 404, 다른 Host 403, 사진 도구(준비된 세트만)·CSP 자기 origin·https 절대 URL·쪽별 사진=아이템 대응, 사진 비교의 저장·수정, 글자 비교는 `image: null`, 사진 없음·일부·나중에 빠짐, 로컬 http fallback. 테스트가 만든 작은 PNG로 돈다.
- `test/live-auth.test.js` — live 모드: discovery·S256·같은 issuer, 401+`WWW-Authenticate`, DCR redirect 허용 목록, A·B 계정 격리(옷장·프로필·데모 비교/선택), 동의 화면(5회·Origin·거절), 연결 코드(일회·만료·없는 계정), 인가 거절(redirect·resource·S256·scope), 토큰 거절(PKCE·redirect·resource·코드 재사용 시 폐기), 리프레시 회전·재사용 폐기·revoke·만료, 해시 저장·재시작 지속. `TEST_DATABASE_URL`이 있으면 실제 DB 어댑터로 A·B 옷장 격리와 추천 사전 확인·옷 소유 확인·추천 날씨 조회의 계정 분리도 잰다(없으면 skip 으로 표시).
- `test/recommend.test.js` — 실제 추천: 가짜 Rust 서버(같은 위임 서명 검증)로 두 계정 격리, 예전 scope 재동의 요청·리프레시 확대 거절, 인자 검증, 빈 옷장·지역 없음, 후보 부족·404·401·HTML 500·리다이렉트·남의 옷 id, 시간 초과(재시도 없음)·서버 꺼짐, 동시 요청, 응답에서 사진·키·내부 값 제외.
- `test/import.test.js` — 가져오기 스크립트: 원본 PNG 바이트 그대로 복사(C2PA `caBX` 보존)·해시 이름·provenance 기록, 옛 파일 정리, UUID.png·일반 파일·PNG만, 모르는 키 거절.
- `test/ui.test.js` — 설치된 Chromium(Playwright 캐시 또는 `CHROME_PATH`)에서 **공식 `AppBridge`로 만든 호스트 흉내**가 iframe(샌드박스)에 화면을 띄우고, 화면의 `tools/call`을 진짜 데모 서버로 넘긴다. 로딩·렌더·클릭·저장·건너뛰기·저장 실패 후 다시 시도·충돌·다크 테마·데스크톱/모바일, 그리고 옛 create 카드 재열기→저장된 화면, 상태 조회 실패→다시 시도, 선택 수정(B→둘 다 아쉬움·이유만·메모만·A)·취소, 두 화면 stale 수정 방지, 같은 iframe의 새 비교, 저장 후 시간 초과(한 번만 반영), ChatGPT형 `-32000` 래핑 conflict(원문 숨김), 네트워크 실패 시 입력 유지, 사진 비교(테스트 PNG + `mcp/.assets`가 있으면 실제 생성 사진: `naturalWidth`·같은 칸 크기·contain·alt·저장/수정·사진 실패 대체·글자 비교 전환). 스크린샷은 `.artifacts/screenshots/`. 브라우저를 찾지 못하면 이 묶음만 건너뛴다(내려받지 않는다).

이것은 **ChatGPT·Claude 실제 호스트 E2E가 아니다.** 같은 MCP Apps 규약을 공식 SDK로 흉내 낸 것이다.

## MCP Inspector

```sh
npm start &   # 또는 다른 터미널에서
export MCP_CATALOG_PATH=/tmp/wardrobe-inspector.json   # ~/.mcp-inspector 를 건드리지 않게
npx mcp-inspector --cli http://127.0.0.1:8787/mcp --transport http --method tools/list
npx mcp-inspector --cli http://127.0.0.1:8787/mcp --transport http --method tools/list --app-info
npx mcp-inspector --cli http://127.0.0.1:8787/mcp --transport http --method resources/read --uri ui://wardrobe-edit/compare-v3.html
npx mcp-inspector --cli http://127.0.0.1:8787/mcp --transport http --method tools/call \
  --tool-name create_demo_comparison --tool-arg situation=office mood=minimal
npx mcp-inspector --cli http://127.0.0.1:8787/mcp --transport http --method tools/call \
  --tool-name submit_demo_preference --tool-arg comparison_id=<위 결과의 id> choice=b 'reasons=["color_combo"]'
```

웹 UI는 `npm run inspector`로 켤 수 있지만 이번에 실행해 보지는 않았다(설치 때 Inspector의 postinstall 스크립트가 npm 정책으로 건너뛰어졌다 — CLI는 정상). Inspector 웹이 브라우저에서 `Origin`을 붙여 직접 부르는 방식이면 403이 난다 —
그때만 `ALLOWED_ORIGINS=<Inspector 주소>`를 주고 다시 켠다.
`--strict`로 보면 출력 스키마의 nullable 필드(`["string","null"]`)에 이식성 경고가 15개 나온다(입력 스키마는 0개). 합법 JSON Schema이고 SDK 변환기가 만드는 모양이다.

## ChatGPT·Claude에서 써 보기(공개 HTTPS 필요)

두 호스트 모두 원격 MCP 서버는 **공개 HTTPS URL**로만 붙는다. 로컬 서버를 터널로 내보낸다.

1. 서버를 켠다: `npm start` (루프백 8787).
2. **8787 포트만** 터널로 내보낸다. 3003(Rust 본 서버)이나 MySQL 포트는 내보내지 않는다. 예:
   - `cloudflared tunnel --url http://127.0.0.1:8787`
   - `ngrok http 8787`
3. 터널이 준 호스트 이름으로 서버를 다시 켠다: `PUBLIC_HOST=abcd.trycloudflare.com npm start`
   (터널이 Host를 바꾸지 않고 넘기므로 필요하다. 안 하면 로그에 `거절(host)`가 찍히고 403).
4. 연결 URL: `https://<공개 호스트>/mcp`
   - **ChatGPT**: Plugins(앱) 설정 → **Add custom MCP server** → 이 HTTPS URL, 인증 없음(OpenAI app quickstart 기준).
   - **Claude**(웹/데스크톱 원격 커넥터): 설정 → 커넥터 → 커스텀 커넥터 추가 → URL 입력. 인증 없음.
   - 계정 설정·플랜에 따라 메뉴 이름이 다를 수 있다. Claude Desktop의 로컬 stdio 연결은 이 PoC 범위가 아니다.
5. 연결이 403으로 실패하고 로그에 `거절(origin): https://...`가 찍히면 그 값을 `ALLOWED_ORIGINS`에 넣고 다시 켠다.

시험 문장 예:

- "Wardrobe Edit 데모로 샘플 옷장 보여 줘."
- "Wardrobe Edit 데모에서 출근·미니멀 코디 두 개 비교 화면 열어 줘."
- (사진 asset이 있을 때) "Wardrobe Edit 데모에서 예전에 만든 팝 무드 룩북 사진 두 장으로 비교해 줘." → `create_demo_photo_comparison`(`pop_color_pattern`)
- "엣지 사진 비교도 보여 줘." → `edgy_leather`. "글램 사진으로 비교해 줘." → 사진 세트가 없으니 글자 비교를 제안하거나 없다고 말해야 한다.
- (화면에서 고른 뒤) "방금 비교 결과 다시 보여 줘." → `get_demo_comparison`
- 위로 스크롤해 처음 열린 비교 카드를 다시 연다 → 선택 전 화면이 아니라 "저장 상태를 확인하는 중..." 뒤 저장된 화면이 나와야 한다.
- 저장된 화면에서 "선택 수정" → 다른 선택으로 "수정 저장". 또는 대화로 "아까 비교 B 말고 둘 다 아쉬움으로 바꿔 줘." → `update_demo_preference`
- "주말 나들이 아메카지로 하나 더. 이번엔 둘 다 아쉬워, 날씨에 안 맞아." → 모델이 `neither` + `weather_fit`로 저장하는지
- 거절 확인: "아무거나 네가 골라서 저장해 줘." → 도구 설명상 사용자가 직접 고르지 않으면 저장하지 않아야 한다.

끝나면 터널을 먼저 끄고 서버를 끈다. 테스트 기록은 `mcp/.data/demo-store.json`을 지우면 사라진다.

## 계정 연결(live 모드)

`WARDROBE_MCP_MODE=live`로 켜면 ChatGPT 대화에서 **내 Wardrobe Edit 계정**을 연결하고, 그 계정의 프로필과 옷장만 읽는다.
기본 `demo` 모드(지금 8787)는 그대로다.

### 현재 상태(시험 운영)
- **운영자 발급 일회용 코드 방식의 pilot**이다. 사용자가 스스로 가입·로그인해서 연결하는 흐름은 없다(아래 "계정 보유 증명").
- live 서버는 **Rust 서버(3003)를 거치지 않고** 서비스 DB를 직접 읽는다(`src/adapters/live.js`, SELECT 만). 3003이 꺼져 있어도 프로필·옷장 읽기가 된다.
- 확인한 것:
  - 로컬 테스트: OAuth·계정 격리·scope·동의 화면(실제 Chromium) 테스트(`npm test`).
  - 실제 ChatGPT: 커스텀 MCP 서버로 OAuth 연결(동의 화면 → 토큰 발급)에 성공했다.
  - 연결된 커넥터에서 `get_my_profile`·`list_my_wardrobe` 직접 호출: 연결 계정의 옷장 수·목록이 정상으로 돌아왔다.
- 이전에 ChatGPT 웹 채팅에서 응답 표시 오류가 있었으나, 이후 사용자가 대화 테스트의 정상 동작을 확인했다. 이번 실제 추천 도구는 별도로 로컬에서만 검증했다.
- 실제 추천(`recommend_my_outfit`, 2026-10-30): 로컬에서만 확인했다(루프백 3003 + 로컬 발급자 MCP, OAuth 연결 → 도구 호출, 실제 모델 호출 2회).
  ChatGPT 커넥터에서는 아직 부르지 않았다 — 새 권한이라 **기존 연결은 재동의가 필요하다**(아래 "실제 추천").
- 남은 일: 사용자 스스로 하는 로그인·연결(외부 IdP 또는 서비스 로그인), 고정 배포 주소(지금은 임시 터널), ChatGPT에서 추천 확인, 옷 사진 등록·코디 사진 생성, 실제 A/B 비교 연결.

### 구조
- **OAuth 2.1 인가 코드 + PKCE(S256)**. 인가 서버와 리소스 서버가 이 MCP 서버 하나다(`issuer = https://PUBLIC_HOST/`, `resource = https://PUBLIC_HOST/mcp`).
  공식 SDK 부품을 쓴다: `mcpAuthRouter`(메타데이터·`/authorize`·`/token`·`/register`(DCR)·`/revoke`), `requireBearerAuth`(`/mcp`의 토큰·만료·scope·resource 검사, 401 `WWW-Authenticate: … resource_metadata="…"`).
- 메타데이터: `/.well-known/oauth-authorization-server`, `/.well-known/oauth-protected-resource/mcp`(및 접미사 없는 경로).
- scope 세 개:
  - `wardrobe.read`: 내 프로필·내 옷장 읽기. `/mcp`에 들어오려면 늘 필요하다.
  - `comparisons.write`: 데모 비교 만들기와 내 선택 저장·수정.
  - `recommendations.write`: 실제 코디 추천(모델 비용, 추천 이력이 남음). 2026-10-30에 생겼다.
- scope를 보내지 않으면 모두 요청한 것으로 보고, 동의 화면에 하나하나 보여 준다. 리프레시로는 처음 받은 scope보다 넓힐 수 없다.
- 쓰기 도구(`create_demo_comparison`·`create_demo_photo_comparison`·`submit_demo_preference`·`update_demo_preference`)는 실행 때 `comparisons.write`를 확인한다.
  없으면 `isError` + `_meta["mcp/www_authenticate"]`(`error="insufficient_scope"`)로 거절하고 아무것도 저장하지 않는다.
- 도구마다 `securitySchemes`를 알린다. live: 읽기 도구 `oauth2(wardrobe.read)`, 쓰기 도구 `oauth2(wardrobe.read comparisons.write)`, 추천 `oauth2(wardrobe.read recommendations.write)`. demo: `noauth`.
- demo 서버는 토큰을 검증할 수 없다. 그래서 `Authorization`이 붙은 요청은 익명으로 처리하지 않고 400으로 거절한다.
- live 서버는 모든 `/mcp` 요청이 유효한 토큰이어야 하고, 모든 도구가 그 토큰의 주체를 쓴다(틀린 토큰이면 401).
- 클라이언트 등록은 **DCR**만(CIMD 미구현). `redirect_uri`는 허용 목록과 **정확히 같은 문자열**만 등록된다.
- 인가 코드 5분·한 번(PKCE 시도도 한 번), 액세스 토큰 1시간, 리프레시 30일·회전. 코드나 옛 리프레시가 다시 오면 그 연결에서 나간 토큰을 모두 폐기한다.
  `resource`가 이 서버가 아니면 인가·토큰 모두 `invalid_target`. 토큰·코드는 해시로만 저장하고 로그에 남기지 않는다.

### 계정 보유 증명(외부 로그인 없이)
외부 IdP가 아직 없어서, **서버 운영자 = 계정 소유자**가 로컬 터미널에서 짧은 수명의 일회용 **연결 코드**를 만든다.
OAuth 동의 화면에서 그 코드를 넣어야 연결된다.

```sh
LIVE_DATABASE_URL=... npm run link-code -- --user-id <Wardrobe Edit 사용자 id>
# 기존 로컬 옷장(default)에 연결할 때만
LIVE_DATABASE_URL=... npm run link-code -- --user-id default --allow-default
```

- 코드 원문은 그 터미널에 한 번만 나온다(파일엔 해시). 10분, 한 번만 쓸 수 있다. 동의 화면 시도는 5회까지.
- 동의 화면은 다른 사이트 Origin·프레임·캐시를 막는다.
  - `Referrer-Policy: same-origin`이다. `no-referrer`면 브라우저가 같은 출처 POST의 Origin을 `null`로 보내 403이 난다(실제 ChatGPT 연결에서 발견).
  - CSP `form-action`은 `'self'` + 그 요청의 등록된 `redirect_uri` origin 하나다. POST 뒤 콜백으로 가는 302에도 적용되기 때문이다.
  - `Origin: null`은 계속 거절한다. 실제 브라우저 테스트는 `test/consent-browser.test.js`.
- 사용자 id는 코드가 정한다. 도구 인자·화면에서 id를 받지 않는다. 그래서 id를 알아도 남의 계정에 연결할 수 없다.

### 도구(live 추가분, 읽기 전용)
| 도구 | 하는 일 |
|---|---|
| `get_my_profile` | 연결된 계정의 이름과 옷장 수(카테고리별) |
| `list_my_wardrobe` | 연결된 계정의 옷장(이름·카테고리·색·소재, 사진은 싣지 않음, `category`·`limit`) |

- 데모 도구도 그대로 쓴다. 다만 비교·선택 기록은 **만든 계정에만** 보인다(다른 계정·익명 사이에서 서로 보이지 않음).
- 옷 등록·수정·삭제, 사진, 이미지 생성 도구는 없다.

### 실제 추천(`recommend_my_outfit`, 위임 키가 있을 때만)
웹 홈의 추천과 같은 엔진(`POST /api/recommendation/multi`)을 연결된 계정 대신 부른다. 이 서버에 추천 로직은 없다.

| | 데모 비교(`create_demo_*`) | 실제 추천(`recommend_my_outfit`) |
|---|---|---|
| 옷 | 샘플 17벌 | 연결된 계정의 옷장 |
| 코디 | 미리 짜 둔 두 벌 | Rust 엔진(모델 후보 → 점수·날씨·무드·이력) |
| 날씨 | 없음 | 계정에 저장된 지역의 지금 날씨 |
| 사진 | 이미 생성해 둔 룩북 사진(있으면) | 돌려주지 않는다(옷 사진·data URI·내부 URL 제외) |
| 남는 것 | 이 서버의 데모 기록 | 서비스 DB: 추천 이력 1행 + 추천 기록 1행 |
| 비용 | 없음 | 부를 때마다 모델 호출 1회 |

- 인자(모두 선택): `situation` = `일상`(기본, 웹 홈과 같다)·`출근`·`데이트`, `gender` = `female`·`male`(웹 성별 탭), `mood` = 그 성별의 무드 키(`style_mood`).
  무드만 주면(성별 없이) 거절하고, 스트리트 + 출근도 거절한다. 사용자 id 인자는 없다.
- 결과: `structuredContent`에 카드(오늘의 추천·다른 조합·안 입은 옷 활용 중 나온 것)마다 옷 id·이름·카테고리·이유, 선정 이유, 판정·점수, 팁.
  지역 이름과 Rust가 실제로 쓴 기온·날씨(그 추천 기록에서 읽음), 모델이 쓴 날씨 문장도 싣는다. 모든 옷 id가 그 계정 것인지 DB로 확인하고, 하나라도 아니면 결과를 내보내지 않는다.
  `score`는 후보끼리 순위를 정하는 내부 점수다 — 확률이나 100점 만점이 아니고 100을 넘을 수 있다(88 이상이 "훌륭해요"). 코디 품질을 보장하는 값이 아니다.
  카드가 객체가 아니거나 아이템이 비었거나 id·이름·카테고리가 없는 응답은 성공으로 보이지 않고 `upstream_error`로 돌려준다.
- 이해할 수 있는 실패(`ok: false` + `outcome`, 오류가 아님): `empty_wardrobe`·`no_region`(모델 호출 전에 확인), `mood_needs_gender`·`mood_not_for_gender`·`street_not_for_work`,
  `no_candidates`, `busy`, `timeout`, `unavailable`(3003 꺼짐), `upstream_error`, `delegation_rejected`, `foreign_item`. 어떤 실패도 데모 결과로 바꾸지 않는다.
  실패 문장은 고정 안내뿐이다 — 추천 서버의 오류 본문(외부 API 오류에 접속·설정 정보가 섞일 수 있다)은 내보내지 않는다.
- 남는 기록(직접 확인): 오늘의 추천 카드 1건이 `outfit_recommendation_history`에 남는다(다음 추천의 반복 감점·"안 입은 옷" 판정에 쓰인다).
  응답 전체가 `recommendation_batch`에 1행 남는다. **둘 다 착용 기록이 아니다** — 이 서비스에는 착용 기록 표가 없다.
- 반복 호출 억제: annotation `readOnlyHint=false`, `idempotentHint=false`, `openWorldHint=true`. 도구 설명·서버 지시문에 "한 번만, 실패 뒤 바로 다시 부르지 않기"를 적었다.
  서버는 자동으로 다시 부르지 않고, 계정마다 한 번에 하나, 전체 동시 2개, 90초 제한이다.
  Rust LLM 층은 기본으로 실패 시 최대 2회 재시도한다(`LLM_MAX_RETRIES`, 웹도 같다). 유료 재시도를 막으려면 3003을 `LLM_MAX_RETRIES=0`으로 띄운다.
- **신원 전달(위임 헤더)**: ChatGPT OAuth 토큰과 Rust `api_token`은 별개다. 기존 `default` 계정은 토큰이 무효화돼 있어 `api_token`을 대신 보낼 수도 없다.
  그래서 MCP가 OAuth로 검증된 주체를 공유 키로 서명해 `X-Wardrobe-Delegation: v1.<user_id>.<만료>.<HMAC-SHA256>` 헤더로 보낸다(서명 대상: 메서드·경로·사용자·만료, 수명 60초).
  Rust(`src/middleware/delegation.rs`)는 키가 설정돼 있고, 이 컴퓨터에서 직접 온 요청이며(루프백 연결·로컬 Host·전달 헤더 없음), `Authorization`이 같이 오지 않았고,
  `POST /api/recommendation/multi`이며, 서명·만료가 맞고, 그 사용자가 있을 때만 받는다. 어떤 실패도 401이고 `default`로 넘어가지 않는다.
  추천 주소는 `http://127.0.0.1:<LIVE_RECOMMEND_PORT>` 고정이고 리다이렉트는 따라가지 않는다.
- **켜는 법**: 같은 키를 두 서버에 준다(파일·환경 변수에만, 출력·커밋 금지).
  ```sh
  # Rust(3003): 이 컴퓨터에서만 받게
  WARDROBE_BIND_HOST=127.0.0.1 WARDROBE_DELEGATION_KEY=<hex> LLM_MAX_RETRIES=0 cargo run
  # MCP live
  WARDROBE_MCP_MODE=live PUBLIC_HOST=<공개 호스트> LIVE_DATABASE_URL=... WARDROBE_DELEGATION_KEY=<같은 hex> npm start
  ```
  3003은 터널로 내보내지 않는다(터널은 MCP 포트만). 3003이 꺼져 있으면 추천만 `unavailable`이고 프로필·옷장 읽기는 그대로다.
- **기존 ChatGPT 연결은 재연결(재동의)이 필요하다.** 예전 토큰에는 `recommendations.write`가 없다. 도구는 `insufficient_scope`(`scope="wardrobe.read recommendations.write"`)로 답하고,
  ChatGPT가 다시 연결을 띄운다(운영자가 새 연결 코드를 발급해야 한다). 리프레시로 몰래 넓혀 주지 않는다.
  재동의 때 ChatGPT가 이 두 scope만 요청하면 새 토큰에는 `comparisons.write`가 없다 — 그때 데모 선택 저장은 다시 권한을 요청한다.

### 필요한 것
- (실제 추천을 쓸 때) 루프백에서 도는 Rust 서버와 같은 위임 키. 위 "실제 추천".
- Rust 마이그레이션 세 개가 적용된 DB. Rust 서버(3003)를 한 번 켜면 sqlx가 자동 적용한다.
  - `20261024000001_clothing_owner.sql`(옷장 `user_id` 컬럼): 없으면 live 모드가 시작을 거부한다.
  - `20261024000002_outfit_image_owner_key.sql`, `20261024000003_region_owner.sql`: Rust 쪽 사진 캐시·지역의 계정 분리.
- 읽기 전용 DB 접속 문자열(`LIVE_DATABASE_URL=mysql://<user>:<password>@<host>:<port>/<db>`). 저장소에 넣거나 로그에 남기지 않는다.
- 공개 HTTPS 주소(`PUBLIC_HOST`). 발급자·리소스 URL은 이 값으로만 만들고 요청의 `Host`·`X-Forwarded-*`는 쓰지 않는다.

### Rust 쪽 변화(같은 작업)
- `AuthUser`가 **닫혀 있다**. 토큰이 없거나 틀리면 401, DB 오류면 503이다. `default`로 바꾸지 않는다.
- 로컬 화면용 예외는 `WARDROBE_AUTH_MODE=dev-default-loopback`를 명시했을 때 하나뿐이다. 이 컴퓨터에서 직접 온 요청(루프백·localhost Host·전달 헤더 없음·로컬 Origin)만 `default`로 본다.
- 옷장(`clothing.user_id`)을 계정별로 거른다(목록·조회·수정·삭제·장르·코디 평가·추천·채팅). 기존 옷은 모두 `default` 소유로 남는다.
- 생성 사진: `/static`은 개발 모드의 로컬 요청에만 열린다. 토큰 사용자는 `GET /api/images/{file}`로 **본인 사진만** 받는다.
- 마이그레이션이 저장소에 평문으로 있던 `default`의 개발 토큰을 무효화한다.
- 생성 사진 캐시(`outfit_image`)의 유니크 키에 소유자를 넣었다(마이그레이션 20261024000002, 행 변경 없음).
  같은 착장 문구라도 계정마다 따로 캐시한다. 룩북 저장·핀·사진 요청·편집 기준 사진도 내 행만 읽고 쓴다.
  사진 접근권은 캐시 소유자만이다(룩북 경로는 근거가 아니다).
- MCP 위임 헤더(`src/middleware/delegation.rs`, `WARDROBE_DELEGATION_KEY`)로 추천 경로 하나만 연결된 계정 대신 부를 수 있다. `WARDROBE_BIND_HOST`로 바인딩 주소를 정한다(기본 `0.0.0.0` 그대로).
- 판정 라벨(`Verdict::from_score`)이 100을 넘는 점수(피드백·다양성 가산)를 "아쉬워요"로 떨어뜨리던 것을 고쳤다. 88 이상은 모두 "훌륭해요"(점수·가중치는 그대로).
- 홈 추천 카드의 옷마다 붙는 이유가 늘 "옷장에서 골라 넣은 아이템"으로 나오던 문제를 고쳤다. 모델의 참조(`T1`…)를 옷 id로 풀어 이유를 붙인다(웹 홈 추천에도 적용).
- 지역(`region_setting`)은 계정별이다(20261024000003, 기존 행은 default). 레퍼런스 카탈로그 수정은 관리자만 한다(`WARDROBE_ADMIN_USERS` 또는 개발 모드 로컬).

## TODO: 계정 연결 → 사진 등록

2026-10-24 사용자 요청. 아래 순서로 진행한다.

- [ ] **우선: ChatGPT와 Wardrobe Edit 계정 연결.** _(2026-10-25 시험 운영: 운영자 발급 코드로 실제 ChatGPT OAuth 연결과 프로필·옷장 읽기 확인. 위 "현재 상태(시험 운영)". 남음: 사용자 스스로 하는 로그인, 고정 배포, 웹 채팅 응답 표시 오류)_ 인증된 사용자를 서비스 계정에 연결하고, 옷장·사진·선택·착용 기록을 사용자별로 격리한다. 위 실서비스 연결 경계의 인증·소유권 문제 해결을 포함한다.
- [ ] **계정 연결 후: ChatGPT 첨부 사진으로 옷 등록.** 사용자가 전달한 사진을 기존 의류 분석 기능으로 연결하고, 분석 결과 미리보기·수정·등록 확인을 거쳐 본인 옷장에 저장한다. 임시 파일 URL 자체를 영구 사진 주소로 사용하지 않는다.
- [ ] **완료 확인:** 새 대화에서도 같은 계정의 옷장을 조회할 수 있고, 다른 계정의 데이터에는 접근할 수 없는지 확인한다. 사진 업로드부터 분석·확인·저장까지 실제 ChatGPT에서 검증한다.

## 파일

```
src/main.js           실행 입구(설정·저장소·HTTP)
src/config.js         환경 변수
src/http.js           POST /mcp · GET /healthz · GET|HEAD /assets/img/<해시>, Host/Origin/크기/요청 수 검사
src/mcp-server.js     데모 도구 5~6개, live 도구 2~3개 + UI 리소스(공식 McpServer, ext-apps registerAppTool/registerAppResource)
src/photo-sets.js     사진 비교 세트(아이템 목록 ↔ asset 키). 사진 파일·원본 대응표는 로컬 .assets 에만
src/image-assets.js   로컬 asset manifest 검사·허용 목록·사진 URL
scripts/import-image-assets.mjs  기존 생성 사진 → .assets(원본 PNG 바이트 그대로·C2PA 보존·해시 이름·manifest)
src/demo-store.js     데모 비교·선택 저장(JSON 파일, 직렬화된 쓰기)
src/vote.js           선택 계약(Rust와 같은 값·규칙)
src/fixtures.js       샘플 옷장·샘플 코디
src/adapters/live.js  live 모드: 연결된 계정의 프로필·본인 옷장 읽기, 추천 사전 확인·옷 소유·추천 날씨(mysql2, SELECT 만)
src/recommend.js      live 모드: Rust 추천 호출(루프백 고정·위임 서명·시간 제한·동시 제한)
src/auth/oauth.js     OAuth provider(코드·토큰·동의 화면) + SDK 라우터·Bearer 검사
src/auth/store.js     OAuth 상태 저장(해시만, JSON 파일)
src/auth/link-codes.js 계정 연결 코드(발급·확인)
src/auth/setup.js     live 모드 조립
scripts/issue-link-code.mjs  계정 연결 코드 발급 CLI
ui/                   비교 화면(App bridge) → scripts/build-ui.mjs 가 dist/ui.html 한 파일로 묶음
test/                 node:test, test/harness/host.js 는 테스트용 호스트 흉내
```
