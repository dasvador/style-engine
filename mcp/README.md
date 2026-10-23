# Wardrobe Edit 데모 MCP 서버 (로컬 PoC)

ChatGPT·Claude 대화 안에서 Wardrobe Edit의 "둘 중 고르기"를 불러 볼 수 있는지 확인하는 **데모 전용** MCP 서버다.
흐름: 샘플 옷장 보기 → 상황·무드별 샘플 코디 두 개 비교 화면(MCP Apps) → A / B / 둘 다 아쉬움 선택과 이유 저장.

> **데모 데이터만 쓴다.** 옷장은 `src/fixtures.js`에 고정한 17벌의 샘플이고, 코디는 상황×무드마다 손으로 짜 둔
> 두 벌이다. AI가 만든 추천이 아니며 실제 서비스 옷장·DB·추천 엔진·OpenAI API·`.env`를 읽지 않는다.
> 화면과 모든 도구 응답에 이 점을 표시한다.

Rust 본 서버(3003)와 코드·프로세스·데이터를 공유하지 않는다. 같은 것은 데이터 계약뿐이다:
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
| `DEMO_TTL_HOURS` | `168` | 비교 보존 시간(7일) |
| `DEMO_MAX_COMPARISONS` | `500` | 보존 개수 상한(넘으면 오래된 것부터 지움) |
| `MAX_BODY_BYTES` | `65536` | 요청 본문 상한 |
| `RATE_LIMIT_PER_MINUTE` | `300` | 프로세스 전체 1분 요청 수 상한(넘으면 429) |
| `WARDROBE_MCP_MODE` | `demo` | `demo`만 있다. 다른 값이면 시작하지 않는다 |

## 도구

| 도구 | 하는 일 | 부작용 | annotation |
|---|---|---|---|
| `browse_sample_wardrobe` | 샘플 옷장 목록(`category` 선택) | 없음 | readOnly, idempotent |
| `create_demo_comparison` | `situation`(`daily`·`office`·`weekend`) × `mood`(`amekaji`·`minimal`)의 샘플 두 코디로 새 비교를 만들고 화면을 띄움. 좌우는 처음 한 번 무작위 | 비교(노출) 1건 기록 | 쓰기, 비멱등, 비파괴 |
| `get_demo_comparison` | 비교 하나와 저장된 선택을 다시 읽고 화면을 띄움 | 없음 | readOnly, idempotent |
| `submit_demo_preference` | `choice`(`a`·`b`·`neither`), `reasons`(선택), `comment`(선택, 200자) 저장 | 비교당 1회. 같은 내용 재전송은 `already_submitted`, 다른 내용은 `conflict` 오류 | 쓰기, 멱등, 비파괴 |

모든 도구는 `openWorldHint: false`이고 모르는 인자를 거절한다(`additionalProperties: false`).
"건너뛰기"는 화면에만 있는 동작이며 아무것도 저장하지 않는다. "둘 다 아쉬움"(`neither`)은 저장되는 선택이다.

오류 결과는 `isError: true`, `content[0].text = "[code] 메시지"`, 세부는 `_meta["wardrobe-edit/error"]`
(`code`: `not_found`·`conflict`·`invalid_vote`·`unknown_scenario`, conflict면 저장된 `comparison` 포함)에 싣는다.
`structuredContent`에 넣지 않는 이유: 공식 SDK 클라이언트가 오류 결과도 출력 스키마로 검사해 호스트에서 깨진다.
인자 형식 오류(enum·id 모양·길이)는 SDK 입력 검증이 `isError` 결과로 돌려준다.

UI 리소스: `ui://wardrobe-edit/compare-v1.html` (`text/html;profile=mcp-app`, CSP `connectDomains`·`resourceDomains` 비어 있음).
화면은 외부 네트워크를 쓰지 않고 호스트 bridge(`tools/call`)로만 `submit_demo_preference`를 부른다.
OpenAI 전용 `_meta`(`openai/*`)는 쓰지 않는다 — MCP Apps 표준만 쓴다.

## 데모 기록 파일

- 위치: `mcp/.data/demo-store.json`(gitignore). 형식은 `src/demo-store.js` 머리 주석 참고(`version: 1`).
- 수명: 비교마다 7일, 최대 500개. 서버를 다시 켜도 남는다. 파일을 지우면 모두 사라진다.
- 한 프로세스만 이 파일을 쓴다고 가정한다(같은 파일로 서버 두 개를 띄우지 말 것).
- 인증이 없다. 공개 URL을 아는 사람은 누구나 데모 비교를 만들 수 있고, 비교 id(128비트 무작위)를 알면 읽고 선택할 수 있다.
  목록 도구는 없다. 개인 정보를 적지 말 것.

## 테스트

```sh
npm test    # 빌드 후 test/*.test.js (node:test)
```

- `test/store.test.js` — 선택 검증(Rust `validate_vote`와 같은 규칙), 충돌·재제출, 비교 id 혼용, 동시 20건, 파일 재시작, 만료·상한, 저장 실패 시 되돌리기.
- `test/mcp-http.test.js` — 공식 SDK `Client` + Streamable HTTP로 initialize·tools/list·resources/read·tools/call 전체, 동시 12 클라이언트, 재시작, 그리고 원시 HTTP 경계(깨진 JSON 400, Content-Type 415, 64KB 초과 413, 배치 제한, GET/DELETE/OPTIONS 405·CORS 없음, Host 403, Origin 403, `PUBLIC_HOST`, 429).
- `test/ui.test.js` — 설치된 Chromium(Playwright 캐시 또는 `CHROME_PATH`)에서 **공식 `AppBridge`로 만든 호스트 흉내**가 iframe(샌드박스)에 화면을 띄우고, 화면의 `tools/call`을 진짜 데모 서버로 넘긴다. 로딩·렌더·클릭·저장·건너뛰기·저장 실패 후 다시 시도·충돌·다크 테마·데스크톱/모바일. 스크린샷은 `.artifacts/screenshots/`. 브라우저를 찾지 못하면 이 묶음만 건너뛴다(내려받지 않는다).

이것은 **ChatGPT·Claude 실제 호스트 E2E가 아니다.** 같은 MCP Apps 규약을 공식 SDK로 흉내 낸 것이다.

## MCP Inspector

```sh
npm start &   # 또는 다른 터미널에서
export MCP_CATALOG_PATH=/tmp/wardrobe-inspector.json   # ~/.mcp-inspector 를 건드리지 않게
npx mcp-inspector --cli http://127.0.0.1:8787/mcp --transport http --method tools/list
npx mcp-inspector --cli http://127.0.0.1:8787/mcp --transport http --method tools/list --app-info
npx mcp-inspector --cli http://127.0.0.1:8787/mcp --transport http --method resources/read --uri ui://wardrobe-edit/compare-v1.html
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
- (화면에서 고른 뒤) "방금 비교 결과 다시 보여 줘." → `get_demo_comparison`
- "주말 나들이 아메카지로 하나 더. 이번엔 둘 다 아쉬워, 날씨에 안 맞아." → 모델이 `neither` + `weather_fit`로 저장하는지
- 거절 확인: "아무거나 네가 골라서 저장해 줘." → 도구 설명상 사용자가 직접 고르지 않으면 저장하지 않아야 한다.

끝나면 터널을 먼저 끄고 서버를 끈다. 테스트 기록은 `mcp/.data/demo-store.json`을 지우면 사라진다.

## 실서비스 연결 경계(비활성)

`src/adapters/live.js`는 인터페이스와 막힌 이유만 적은 비활성 자리다. 도구에 연결돼 있지 않고 `WARDROBE_MCP_MODE=demo` 외에는
서버가 시작하지 않는다. 실서비스에 붙이기 전에 풀어야 할 것:

1. Rust `AuthUser`가 잘못된 토큰도 `default` 사용자로 받는다 → MCP OAuth로 사용자를 확인하고 그 id로만 읽고 써야 한다.
2. 전체 옷장 API에 소유권 격리가 없다 → 옷장 조회 도구를 열면 남의 옷장이 보일 수 있다.
3. 실제 비교는 추천 묶음(`batch_id`)에서만 만들어지고, 묶음은 유료 모델을 부르는 추천 요청이 만든다.

이 셋이 풀리기 전에는 3003이나 실제 DB를 이 서버 뒤나 터널에 두지 않는다.

## 파일

```
src/main.js           실행 입구(설정·저장소·HTTP)
src/config.js         환경 변수
src/http.js           POST /mcp · GET /healthz, Host/Origin/크기/요청 수 검사
src/mcp-server.js     도구 4개 + UI 리소스(공식 McpServer, ext-apps registerAppTool/registerAppResource)
src/demo-store.js     데모 비교·선택 저장(JSON 파일, 직렬화된 쓰기)
src/vote.js           선택 계약(Rust와 같은 값·규칙)
src/fixtures.js       샘플 옷장·샘플 코디
src/adapters/live.js  실서비스 연결 자리(비활성)
ui/                   비교 화면(App bridge) → scripts/build-ui.mjs 가 dist/ui.html 한 파일로 묶음
test/                 node:test, test/harness/host.js 는 테스트용 호스트 흉내
```
