# Style Engine Eval Scorecard

케이스 96건 (`tests/fixtures/recommendation_cases.toml`)

| 지표 | 값 |
|---|---|
| Hard filter 정확도 | **94.8%** |
| — false positive (엔진 거절 / 사람 수용) | 3 |
| — false negative (엔진 통과 / 사람 거절) | 2 |
| — 거절 정밀도 / 재현율 | 75.0% / 81.8% |
| Today-fit 정확도 (3-class) | **74.0%** |
| 선호도 순위 정확도 (Accept > Reject 쌍) | **74.9%** |
| — Accept 평균 / Reject 평균 / 간격 | 96.7 / 91.9 / 4.8 |
| Hard + fit 동시 일치 | **70.8%** |

## False positive를 유발한 룰

| 룰 | 건수 |
|---|---|
| `WarmMonotoneNoStructure` | 2 |
| `LackOfStructure` | 1 |

## Today-fit 오분류 (expected→actual)

| 전이 | 건수 |
|---|---|
| Borderline→Pass | 14 |
| Fail→Pass | 7 |
| Borderline→Fail | 2 |
| Fail→Borderline | 1 |
| Pass→Borderline | 1 |

> `cargo test --test eval_scorecard`로 재생성됩니다. 기준선 대비 1.0%p 이상 하락하면 테스트가 실패합니다.
