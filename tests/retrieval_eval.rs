//! 레퍼런스 검색 eval — 회귀 게이트가 있는 스코어카드.
//!
//! Vision 2-Pass 의 가운데 단계, Pass 1 서술로 레퍼런스를 찾는 검색만 잰다. 업로드
//! 분석과 같은 조건으로 검색한다: 승인된 레퍼런스, 같은 카테고리, 코사인 유사도 순.
//!
//! ```text
//! cargo test --test retrieval_eval -- --nocapture           # 실행 + 게이트
//! UPDATE_EVAL_BASELINE=1 cargo test --test retrieval_eval   # 기준선 갱신
//! python3 tools/retrieval-eval/embed.py                     # 벡터 스냅샷 갱신
//! ```
//!
//! CI 는 임베딩 API 를 부르지 않으므로 벡터는 `tests/eval/retrieval_vectors.json`
//! 스냅샷에서 읽는다. 스냅샷에는 임베딩한 원문도 들어 있어서, 카탈로그의 검색 문장과
//! 다르면 이 테스트가 실패한다. 그대로 두면 고치기 전 문장의 벡터로 잰 숫자가 나오고,
//! 그 숫자가 틀렸다는 표시는 어디에도 없다.
//!
//! 산출물:
//!   tests/eval/retrieval_scorecard.json — 기계 판독용 (기준선 비교 대상)
//!   tests/eval/retrieval_scorecard.md   — 사람이 읽는 요약
//!   tests/eval/retrieval_baseline.json  — 커밋된 기준선

use std::collections::BTreeMap;
use std::path::PathBuf;

use base64::Engine;
use serde::{Deserialize, Serialize};

use style_engine::services::prompts::RAG_MIN_SIMILARITY;

/// 지표가 이 값(%p)보다 더 떨어지면 회귀로 본다. 정답 있는 케이스가 30건이라 한 건이
/// 3.3%p 다 — 한 건이라도 순위가 밀리면 잡힌다.
const REGRESSION_TOLERANCE_PCT: f64 = 1.0;

/// Pass 2 가 받는 레퍼런스 수 (`analyze_clothing_image_with_rag` 의 `search_scoped(.., 5, ..)`).
const PASS2_TOP_N: usize = 5;

// ─── 입력 ───

#[derive(Deserialize)]
struct Catalog {
    references: Vec<CatalogRef>,
}

#[derive(Deserialize)]
struct CatalogRef {
    name: String,
    category: String,
    review_status: String,
    #[serde(default)]
    embedding_text: Option<String>,
    description: String,
}

impl CatalogRef {
    /// `ClothingReference::text_for_embedding` 과 같은 규칙.
    fn search_text(&self) -> &str {
        self.embedding_text
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| self.description.trim())
    }
}

#[derive(Deserialize)]
struct Cases {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    group: String,
    category: String,
    #[serde(default)]
    expected: Option<String>,
    query: String,
}

#[derive(Deserialize)]
struct Snapshot {
    model: String,
    references: BTreeMap<String, SnapshotEntry>,
    queries: BTreeMap<String, SnapshotEntry>,
}

#[derive(Deserialize)]
struct SnapshotEntry {
    text: String,
    vector: String,
}

// ─── 스코어카드 ───

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Scorecard {
    model: String,
    references_searched: usize,
    positive_cases: usize,
    negative_cases: usize,
    recall_at_1_pct: f64,
    recall_at_5_pct: f64,
    mrr: f64,
    /// 레퍼런스를 쓰기 전에 쓴 질의만 — 표현 누수가 없는 쪽.
    before_recall_at_1_pct: f64,
    /// 정답이 있는데 1위 유사도가 임계값보다 낮아 폴백되는 케이스 수.
    positives_below_threshold: usize,
    /// 정답이 없는데 임계값을 넘어 엉뚱한 레퍼런스를 받는 케이스 수.
    negatives_above_threshold: usize,
    lowest_positive_top1: f64,
    highest_negative_top1: f64,
    threshold: f64,
}

struct CaseResult<'a> {
    case: &'a Case,
    top1_name: String,
    top1_sim: f32,
    rank: Option<usize>,
    pool: usize,
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn eval_path(name: &str) -> PathBuf {
    root().join("tests/eval").join(name)
}

fn decode(b64: &str) -> Vec<f32> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .expect("벡터 base64 디코딩 실패");
    let (chunks, rest) = bytes.as_chunks::<4>();
    assert!(rest.is_empty(), "벡터 바이트 수가 4의 배수가 아니다");
    chunks.iter().map(|c| f32::from_le_bytes(*c)).collect()
}

/// `embedding.rs::cosine_similarity` 와 같은 계산.
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let (mut dot, mut na, mut nb) = (0f32, 0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    let d = na.sqrt() * nb.sqrt();
    if d == 0.0 { 0.0 } else { dot / d }
}

fn round(x: f64, places: i32) -> f64 {
    let m = 10f64.powi(places);
    (x * m).round() / m
}

fn load() -> (Vec<CatalogRef>, Vec<Case>, Snapshot) {
    let catalog: Catalog = toml::from_str(
        &std::fs::read_to_string(root().join("data/clothing_references.toml"))
            .expect("카탈로그 읽기 실패"),
    )
    .expect("카탈로그 파싱 실패");
    let cases: Cases = toml::from_str(
        &std::fs::read_to_string(root().join("tests/fixtures/retrieval_cases.toml"))
            .expect("케이스 읽기 실패"),
    )
    .expect("케이스 파싱 실패");
    let snapshot: Snapshot = serde_json::from_str(
        &std::fs::read_to_string(eval_path("retrieval_vectors.json"))
            .expect("벡터 스냅샷이 없다. python3 tools/retrieval-eval/embed.py 를 실행할 것"),
    )
    .expect("벡터 스냅샷 파싱 실패");
    (catalog.references, cases.cases, snapshot)
}

/// 스냅샷이 현재 카탈로그·케이스와 같은 원문으로 만들어졌는지 본다.
fn stale_entries(refs: &[CatalogRef], cases: &[Case], snap: &Snapshot) -> Vec<String> {
    let mut stale = Vec::new();
    for r in refs {
        match snap.references.get(&r.name) {
            Some(e) if e.text == r.search_text() => {}
            Some(_) => stale.push(format!("레퍼런스 '{}' 의 검색 문장이 바뀌었다", r.name)),
            None => stale.push(format!("레퍼런스 '{}' 의 벡터가 없다", r.name)),
        }
    }
    for c in cases {
        match snap.queries.get(&c.id) {
            Some(e) if e.text == c.query.trim() => {}
            Some(_) => stale.push(format!("질의 {} 의 문장이 바뀌었다", c.id)),
            None => stale.push(format!("질의 {} 의 벡터가 없다", c.id)),
        }
    }
    stale
}

fn evaluate<'a>(refs: &[CatalogRef], cases: &'a [Case], snap: &Snapshot) -> Vec<CaseResult<'a>> {
    let ref_vecs: Vec<(&CatalogRef, Vec<f32>)> = refs
        .iter()
        .filter(|r| r.review_status == "approved")
        .map(|r| (r, decode(&snap.references[&r.name].vector)))
        .collect();

    cases
        .iter()
        .map(|case| {
            let q = decode(&snap.queries[&case.id].vector);
            let mut pool: Vec<(f32, &str)> = ref_vecs
                .iter()
                .filter(|(r, _)| r.category == case.category)
                .map(|(r, v)| (cosine(&q, v), r.name.as_str()))
                .collect();
            pool.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            let rank = case
                .expected
                .as_deref()
                .and_then(|want| pool.iter().position(|(_, n)| *n == want))
                .map(|i| i + 1);
            let (top1_sim, top1_name) = pool
                .first()
                .map(|(s, n)| (*s, n.to_string()))
                .unwrap_or((0.0, "(후보 없음)".into()));
            CaseResult {
                case,
                top1_name,
                top1_sim,
                rank,
                pool: pool.len(),
            }
        })
        .collect()
}

fn scorecard(model: &str, searched: usize, results: &[CaseResult]) -> Scorecard {
    let pos: Vec<&CaseResult> = results
        .iter()
        .filter(|r| r.case.expected.is_some())
        .collect();
    let neg: Vec<&CaseResult> = results
        .iter()
        .filter(|r| r.case.expected.is_none())
        .collect();
    let pct = |n: usize, d: usize| {
        if d == 0 {
            0.0
        } else {
            round(n as f64 / d as f64 * 100.0, 1)
        }
    };
    let before: Vec<&&CaseResult> = pos.iter().filter(|r| r.case.group == "before").collect();
    let threshold = RAG_MIN_SIMILARITY;

    Scorecard {
        model: model.to_string(),
        references_searched: searched,
        positive_cases: pos.len(),
        negative_cases: neg.len(),
        recall_at_1_pct: pct(pos.iter().filter(|r| r.rank == Some(1)).count(), pos.len()),
        recall_at_5_pct: pct(
            pos.iter()
                .filter(|r| r.rank.is_some_and(|k| k <= PASS2_TOP_N))
                .count(),
            pos.len(),
        ),
        mrr: round(
            pos.iter()
                .map(|r| r.rank.map_or(0.0, |k| 1.0 / k as f64))
                .sum::<f64>()
                / pos.len().max(1) as f64,
            3,
        ),
        before_recall_at_1_pct: pct(
            before.iter().filter(|r| r.rank == Some(1)).count(),
            before.len(),
        ),
        positives_below_threshold: pos.iter().filter(|r| r.top1_sim < threshold).count(),
        negatives_above_threshold: neg.iter().filter(|r| r.top1_sim >= threshold).count(),
        lowest_positive_top1: round(
            pos.iter()
                .map(|r| r.top1_sim as f64)
                .fold(f64::MAX, f64::min),
            3,
        ),
        highest_negative_top1: round(neg.iter().map(|r| r.top1_sim as f64).fold(0.0, f64::max), 3),
        threshold: round(threshold as f64, 3),
    }
}

fn markdown(sc: &Scorecard, results: &[CaseResult]) -> String {
    let mut md = String::new();
    md.push_str("# 레퍼런스 검색 스코어카드\n\n");
    md.push_str(&format!(
        "업로드 분석과 같은 조건(승인된 레퍼런스 {}개, 같은 카테고리)으로 검색했다. \
         기준선 대비 {REGRESSION_TOLERANCE_PCT:.1}%p 이상 하락하면 테스트가 실패한다.\n\n",
        sc.references_searched
    ));
    md.push_str("| 지표 | 값 |\n|---|---|\n");
    md.push_str(&format!("| Recall@1 | {:.1}% |\n", sc.recall_at_1_pct));
    md.push_str(&format!(
        "| Recall@{PASS2_TOP_N} (Pass 2 가 받는 범위) | {:.1}% |\n",
        sc.recall_at_5_pct
    ));
    md.push_str(&format!("| MRR | {:.3} |\n", sc.mrr));
    md.push_str(&format!(
        "| Recall@1 (레퍼런스보다 먼저 쓴 질의만) | {:.1}% |\n",
        sc.before_recall_at_1_pct
    ));
    md.push_str(&format!(
        "| 임계값 {:.2} 에서 정답인데 폴백 | {}/{} |\n",
        sc.threshold, sc.positives_below_threshold, sc.positive_cases
    ));
    md.push_str(&format!(
        "| 임계값 {:.2} 에서 정답 없는데 통과 | {}/{} |\n",
        sc.threshold, sc.negatives_above_threshold, sc.negative_cases
    ));
    md.push_str(&format!(
        "| 정답 1위 유사도 최저 / 음성 1위 유사도 최고 | {:.3} / {:.3} |\n\n",
        sc.lowest_positive_top1, sc.highest_negative_top1
    ));

    md.push_str(&threshold_sweep(results));
    md.push_str(
        "## 1위를 놓친 케이스\n\n| id | 정답 | 1위 | 유사도 | 정답 순위 |\n|---|---|---|---|---|\n",
    );
    for r in results
        .iter()
        .filter(|r| r.case.expected.is_some() && r.rank != Some(1))
    {
        md.push_str(&format!(
            "| {} | {} | {} | {:.3} | {} |\n",
            r.case.id,
            r.case.expected.as_deref().unwrap_or(""),
            r.top1_name,
            r.top1_sim,
            r.rank.map_or("없음".into(), |k| k.to_string())
        ));
    }
    md.push_str("\n## 정답이 없는 케이스\n\n| id | 카테고리 | 1위 | 유사도 | 후보 수 |\n|---|---|---|---|---|\n");
    for r in results.iter().filter(|r| r.case.expected.is_none()) {
        md.push_str(&format!(
            "| {} | {} | {} | {:.3} | {} |\n",
            r.case.id, r.case.category, r.top1_name, r.top1_sim, r.pool
        ));
    }
    md
}

/// 임계값을 바꿨을 때 두 종류의 오류가 어떻게 움직이는지.
///
/// 2026-10-11 보정 결과 어떤 값도 두 분포를 가르지 못했다(정답 0.30~0.94, 음성
/// 0.41~0.57). 그래서 값은 그대로 두고, 바꾸려는 사람이 같은 표를 보고 판단하도록
/// 스코어카드에 남긴다.
fn threshold_sweep(results: &[CaseResult]) -> String {
    let pos: Vec<f32> = results
        .iter()
        .filter(|r| r.case.expected.is_some())
        .map(|r| r.top1_sim)
        .collect();
    let neg: Vec<f32> = results
        .iter()
        .filter(|r| r.case.expected.is_none())
        .map(|r| r.top1_sim)
        .collect();
    let mut md = String::from(
        "## 임계값별 오류\n\n정답인데 폴백하면 맞는 레퍼런스를 버리고, 정답 없는데 통과하면 \
         같은 카테고리의 엉뚱한 레퍼런스를 Pass 2 가 받는다.\n\n\
         | 임계값 | 정답인데 폴백 | 정답 없는데 통과 | 합 |\n|---|---|---|---|\n",
    );
    for step in 0..=10 {
        let t = 0.40 + step as f32 * 0.02;
        let a = pos.iter().filter(|s| **s < t).count();
        let b = neg.iter().filter(|s| **s >= t).count();
        let mark = if (t - RAG_MIN_SIMILARITY).abs() < 1e-6 {
            " ← 현재"
        } else {
            ""
        };
        md.push_str(&format!(
            "| {t:.2}{mark} | {a}/{} | {b}/{} | {} |\n",
            pos.len(),
            neg.len(),
            a + b
        ));
    }
    md.push('\n');
    md
}

#[test]
fn the_vector_snapshot_matches_the_catalogue() {
    let (refs, cases, snap) = load();
    let stale = stale_entries(&refs, &cases, &snap);
    assert!(
        stale.is_empty(),
        "벡터 스냅샷이 현재 카탈로그·케이스와 다르다:\n  {}\n\n\
         python3 tools/retrieval-eval/embed.py 로 다시 만들고 커밋할 것.",
        stale.join("\n  ")
    );
}

#[test]
fn every_expected_reference_exists_in_its_category() {
    let (refs, cases, _) = load();
    for c in &cases {
        if let Some(want) = &c.expected {
            let r = refs.iter().find(|r| &r.name == want);
            let r = r.unwrap_or_else(|| panic!("{}: 정답 '{}' 가 카탈로그에 없다", c.id, want));
            assert_eq!(
                r.category, c.category,
                "{}: 정답 '{}' 의 카테고리({})가 질의 카테고리({})와 다르다 — 선필터에서 영원히 못 찾는다",
                c.id, want, r.category, c.category
            );
        }
    }
}

#[test]
fn retrieval_does_not_regress() {
    let (refs, cases, snap) = load();
    if !stale_entries(&refs, &cases, &snap).is_empty() {
        // 원인은 위 테스트가 알린다. 여기서 옛 벡터로 잰 숫자를 기준선과 비교하지 않는다.
        return;
    }
    let results = evaluate(&refs, &cases, &snap);
    let searched = refs
        .iter()
        .filter(|r| r.review_status == "approved")
        .count();
    let sc = scorecard(&snap.model, searched, &results);

    let json = serde_json::to_string_pretty(&sc).expect("직렬화 실패");
    std::fs::write(eval_path("retrieval_scorecard.json"), format!("{json}\n"))
        .expect("스코어카드 쓰기 실패");
    std::fs::write(eval_path("retrieval_scorecard.md"), markdown(&sc, &results))
        .expect("요약 쓰기 실패");
    println!("{}", markdown(&sc, &results));

    let baseline_path = eval_path("retrieval_baseline.json");
    if std::env::var("UPDATE_EVAL_BASELINE").is_ok() {
        std::fs::write(&baseline_path, format!("{json}\n")).expect("기준선 쓰기 실패");
        return;
    }
    let raw = std::fs::read_to_string(&baseline_path).expect(
        "기준선이 없다. 먼저 `UPDATE_EVAL_BASELINE=1 cargo test --test retrieval_eval` 를 실행할 것",
    );
    let base: Scorecard = serde_json::from_str(&raw).expect("기준선 파싱 실패");

    let mut regressions = Vec::new();
    for (label, cur, prev) in [
        ("Recall@1", sc.recall_at_1_pct, base.recall_at_1_pct),
        ("Recall@5", sc.recall_at_5_pct, base.recall_at_5_pct),
        ("MRR(×100)", sc.mrr * 100.0, base.mrr * 100.0),
    ] {
        if cur < prev - REGRESSION_TOLERANCE_PCT {
            regressions.push(format!("{label}: {prev:.1} → {cur:.1}"));
        }
    }
    if sc.negatives_above_threshold > base.negatives_above_threshold {
        regressions.push(format!(
            "정답 없는데 임계값 통과: {} → {}",
            base.negatives_above_threshold, sc.negatives_above_threshold
        ));
    }
    assert!(
        regressions.is_empty(),
        "레퍼런스 검색이 기준선보다 나빠졌다:\n  {}\n\n\
         의도한 변경이라면: UPDATE_EVAL_BASELINE=1 cargo test --test retrieval_eval",
        regressions.join("\n  ")
    );
}
