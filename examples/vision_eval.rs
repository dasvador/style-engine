//! Vision 분석 품질 평가 — 같은 사진을 레퍼런스 없이 / 레퍼런스 사용으로 각각 분석한다.
//!
//! ```text
//! cargo run --example vision_eval -- <manifest.json> <out.json>
//! ```
//!
//! manifest 는 `[{"id": "...", "path": "..."}]`. 사진은 저장소 밖에 둔다 — 평가에 쓰는
//! 외부 사진은 저작권이 있으므로 커밋하지 않는다.
//!
//! 레퍼런스 사용 경로는 `prompts::analyze_clothing_image_with_rag` 와 같은 순서로 돌되,
//! 그 함수가 버리는 중간 결과(1차 카테고리, 받은 레퍼런스와 유사도, 폴백 이유)를 남긴다.
//! 폴백할 때는 레퍼런스 없는 분석과 같은 함수를 부르므로, 비용을 아끼려고 그 결과를
//! 재사용한다.
//!
//! 업로드 경로와 달리 DB 에 아무것도 쓰지 않는다.

use std::sync::Arc;

use base64::Engine;
use serde::{Deserialize, Serialize};
use sqlx::mysql::MySqlPoolOptions;

use style_engine::models::clothing::VisionAnalysisResult;
use style_engine::services::embedding::{EmbeddingService, SearchScope};
use style_engine::services::llm::LlmClient;
use style_engine::services::prompts::{self, RAG_MIN_SIMILARITY};

#[derive(Deserialize)]
struct Entry {
    id: String,
    path: String,
}

#[derive(Serialize, Deserialize)]
struct Analysis {
    is_clothing: bool,
    name: Option<String>,
    category: Option<String>,
    style_genres: Vec<String>,
}

impl From<&VisionAnalysisResult> for Analysis {
    fn from(r: &VisionAnalysisResult) -> Self {
        Self {
            is_clothing: r.is_clothing,
            name: r.name.clone(),
            category: r.category.clone(),
            style_genres: r.style_genres.clone(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct RetrievedRef {
    name: String,
    similarity: f32,
}

#[derive(Serialize, Deserialize)]
struct Row {
    id: String,
    /// 분석이 실패했으면 그 메시지. 업로드였다면 사용자가 오류를 받는 경우다.
    error: Option<String>,
    general: Option<Analysis>,
    pass1_category: Option<String>,
    pass1_description: String,
    retrieved: Vec<RetrievedRef>,
    /// `None` 이면 레퍼런스를 써서 2차 분석했다.
    fallback: Option<String>,
    rag: Option<Analysis>,
}

fn data_url(path: &str) -> anyhow::Result<String> {
    let bytes = std::fs::read(path)?;
    // 확장자가 아니라 내용으로 판단한다 — 받아 온 파일 중에 이름만 .jpg 인 PNG 가 있었다.
    let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else {
        "image/jpeg"
    };
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let args: Vec<String> = std::env::args().collect();
    let (manifest, out) = match args.as_slice() {
        [_, m, o] => (m.clone(), o.clone()),
        _ => anyhow::bail!("사용법: cargo run --example vision_eval -- <manifest.json> <out.json>"),
    };
    let entries: Vec<Entry> = serde_json::from_str(&std::fs::read_to_string(&manifest)?)?;

    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("DATABASE_URL")?)
        .await?;
    let llm = Arc::new(LlmClient::from_env(reqwest::Client::new()));
    let embedding = Arc::new(EmbeddingService::new(llm.clone())?);
    embedding.load_cache(&pool).await?;

    // 중간에 끊겨도 다시 돌리면 이어서 한다 — 끝난 사진은 건너뛴다.
    let mut rows: Vec<Row> = std::fs::read_to_string(&out)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    for (i, e) in entries.iter().enumerate() {
        if rows.iter().any(|r| r.id == e.id) {
            continue;
        }
        let url = data_url(&e.path)?;
        let mut row = Row {
            id: e.id.clone(),
            error: None,
            general: None,
            pass1_category: None,
            pass1_description: String::new(),
            retrieved: Vec::new(),
            fallback: None,
            rag: None,
        };
        // 한 장이 실패해도 나머지는 잰다. 실패 자체가 측정 대상이다.
        if let Err(err) = analyze_one(&llm, &embedding, &url, &mut row).await {
            row.error = Some(err.to_string());
        }
        eprintln!(
            "[{}/{}] {} 1차={:?} 레퍼런스1위={} 폴백={:?} 오류={}",
            i + 1,
            entries.len(),
            e.id,
            row.pass1_category,
            row.retrieved.first().map_or("-", |r| r.name.as_str()),
            row.fallback,
            row.error
                .as_deref()
                .map_or("-".to_string(), |m| m.chars().take(80).collect()),
        );
        rows.push(row);
        std::fs::write(&out, serde_json::to_string_pretty(&rows)?)?;
    }

    std::fs::write(&out, serde_json::to_string_pretty(&rows)?)?;
    eprintln!("완료: {} 건 → {out}", rows.len());
    Ok(())
}

async fn analyze_one(
    llm: &LlmClient,
    embedding: &EmbeddingService,
    url: &str,
    row: &mut Row,
) -> anyhow::Result<()> {
    {
        let general = prompts::analyze_clothing_image(llm, url).await;
        // 레퍼런스 없는 분석이 실패해도 레퍼런스 경로는 따로 잰다.
        let general = match general {
            Ok(g) => {
                row.general = Some(Analysis::from(&g));
                Some(g)
            }
            Err(err) => {
                row.error = Some(format!("레퍼런스 없는 분석 실패: {err}"));
                None
            }
        };

        let pass1 = prompts::analyze_clothing_pass1(llm, url).await?;
        row.pass1_category = pass1.category.clone();
        row.pass1_description = pass1.description.clone();
        let scope = SearchScope {
            category: pass1.search_category(),
            include_drafts: false,
        };
        let refs = embedding
            .search_scoped(&pass1.description, 5, &scope)
            .await?;
        row.retrieved = refs
            .iter()
            .map(|r| RetrievedRef {
                name: r.name.clone(),
                similarity: r.similarity,
            })
            .collect();
        let top = refs.first().map(|r| r.similarity).unwrap_or(0.0);

        if refs.is_empty() || top < RAG_MIN_SIMILARITY {
            row.fallback = Some(if refs.is_empty() {
                "카테고리에 레퍼런스 없음".to_string()
            } else {
                format!("유사도 {top:.3} < {RAG_MIN_SIMILARITY}")
            });
            // 폴백은 레퍼런스 없는 분석과 같은 함수다.
            row.rag = general.as_ref().map(Analysis::from);
        } else {
            let r = prompts::analyze_clothing_pass2(llm, url, &refs).await?;
            row.rag = Some(Analysis::from(&r));
        }
    }
    Ok(())
}
