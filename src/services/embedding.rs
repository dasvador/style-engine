use std::sync::Arc;

use sqlx::MySqlPool;
use tokio::sync::RwLock;

use crate::db::reference_repo;
use crate::models::reference::ReferenceMatch;
use crate::services::llm::{LlmClient, LlmTask};

/// 임베딩 모델의 출력 차원.
/// 저장된 벡터가 이 길이와 다르면 다른 모델이 만든 것이므로 폐기하고 다시 만든다.
/// cosine_similarity 는 길이가 다르면 0.0 을 돌려주므로, 그대로 두면 검색이
/// 에러 없이 조용히 전부 실패한다.
///
/// 임베딩 모델이 설정으로 바뀔 수 있게 된 뒤로 이 값도 모델을 따라가야 한다.
/// `LLM_TASK_EMBEDDING`으로 다른 차원의 모델을 지정했다면 `LLM_EMBEDDING_DIM`도 함께 바꿔야
/// 기존 벡터가 폐기되고 재생성된다.
const DEFAULT_EMBEDDING_DIM: usize = 1536;

/// Cached reference entry for in-memory similarity search
#[derive(Debug, Clone)]
struct CachedReference {
    name: String,
    category: Option<String>,
    era: Option<String>,
    style: Option<String>,
    description: String,
    review_status: String,
    embedding: Vec<f32>,
}

/// 검색 범위. 기본값은 기존 동작과 같다 — 전체 레퍼런스, 승인된 것만.
#[derive(Debug, Clone, Default)]
pub struct SearchScope {
    /// 이 카테고리의 레퍼런스만 본다. `None` 이면 전체.
    ///
    /// 2026-10-11 측정: 오답 6건이 전부 **다른 카테고리** 레퍼런스에 걸렸다
    /// (블라우스·스커트·힐·가방 → 청바지/스웻셔츠). 카테고리를 좁히면 그중
    /// 라피아 토트백은 후보가 0개가 되어 확실한 폴백이 된다.
    ///
    /// 다만 이것만으로 분리가 되지는 않는다. 같은 카테고리에 레퍼런스가 하나뿐이면
    /// (현재 `하의` = 셀비지 데님 1개) 미디 스커트가 그것과 0.527 로 비교된다.
    /// 분리는 커버리지가 만든다.
    pub category: Option<String>,
    /// 검수 전 데이터까지 포함한다. 평가 하네스용이고, 업로드 분석 경로는 false 다.
    pub include_drafts: bool,
}

/// 레퍼런스 임베딩 캐시와 유사도 검색.
///
/// 임베딩 호출 자체는 [`LlmClient`]에 위임한다 — 어떤 provider의 어떤 모델을 쓰는지,
/// 재시도·계측을 어떻게 하는지는 이 서비스의 관심사가 아니다.
pub struct EmbeddingService {
    llm: Arc<LlmClient>,
    expected_dim: usize,
    cache: RwLock<Vec<CachedReference>>,
}

impl EmbeddingService {
    pub fn new(llm: Arc<LlmClient>) -> anyhow::Result<Self> {
        let expected_dim = std::env::var("LLM_EMBEDDING_DIM")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_EMBEDDING_DIM);

        tracing::info!(
            model = %llm.config().task(LlmTask::Embedding).model,
            expected_dim,
            "Initializing embedding service"
        );

        Ok(Self {
            llm,
            expected_dim,
            cache: RwLock::new(Vec::new()),
        })
    }

    /// Embed multiple texts in a single API call. Returns vectors in input order.
    pub async fn embed_batch(&self, texts: Vec<String>) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self.llm.embed(&texts).await?.vectors)
    }

    /// Embed a single text string.
    pub async fn embed_text(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let mut results = self.embed_batch(vec![text.to_string()]).await?;
        results
            .pop()
            .filter(|e| !e.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Empty embedding result"))
    }

    /// Load all reference embeddings from DB into the in-memory cache.
    /// If any entries are missing embeddings, generate and save them first.
    pub async fn load_cache(&self, pool: &MySqlPool) -> anyhow::Result<()> {
        let refs = reference_repo::list_references(pool).await?;
        let mut entries = Vec::with_capacity(refs.len());

        for r in refs {
            let embedding = if let Some(emb_json) = &r.embedding {
                serde_json::from_value::<Vec<f32>>(emb_json.clone())
                    .ok()
                    .filter(|e| {
                        let ok = e.len() == self.expected_dim;
                        if !ok {
                            tracing::warn!(
                                "Discarding stale embedding for '{}': {} dims, expected {}",
                                &r.name,
                                e.len(),
                                self.expected_dim
                            );
                        }
                        ok
                    })
            } else {
                None
            };

            let emb = if let Some(e) = embedding {
                e
            } else {
                // Generate missing embedding and persist to DB
                tracing::info!("Generating embedding for '{}'...", &r.name);
                let e = self.embed_text(r.text_for_embedding()).await?;
                let emb_json = serde_json::to_value(&e)?;
                reference_repo::update_reference(
                    pool,
                    &r.id,
                    None,
                    None,
                    None,
                    None,
                    Some(&emb_json),
                )
                .await?;
                e
            };

            entries.push(CachedReference {
                name: r.name,
                category: r.category,
                era: r.era,
                style: r.style,
                description: r.description,
                review_status: r.review_status,
                embedding: emb,
            });
        }

        tracing::info!("Loaded {} reference embeddings into cache", entries.len());
        let mut cache = self.cache.write().await;
        *cache = entries;
        Ok(())
    }

    /// Search for top-N most similar references given a query text.
    /// 범위는 기본값 — 전체 카테고리, 승인된 레퍼런스만.
    pub async fn search(&self, query: &str, top_n: usize) -> anyhow::Result<Vec<ReferenceMatch>> {
        self.search_scoped(query, top_n, &SearchScope::default())
            .await
    }

    /// 범위를 좁혀 검색한다.
    ///
    /// 후보가 0개면 빈 벡터를 돌려준다. 호출자는 이것을 "레퍼런스 없음" 으로 읽어야
    /// 하고, 유사도 임계값보다 확실한 신호다 — 임계값은 분포가 겹치지만 빈 후보는
    /// 겹칠 수가 없다.
    pub async fn search_scoped(
        &self,
        query: &str,
        top_n: usize,
        scope: &SearchScope,
    ) -> anyhow::Result<Vec<ReferenceMatch>> {
        let query_embedding = self.embed_text(query).await?;

        let cache = self.cache.read().await;

        let mut scored: Vec<(f32, &CachedReference)> = cache
            .iter()
            .filter(|entry| scope.include_drafts || entry.review_status == "approved")
            .filter(|entry| match (&scope.category, &entry.category) {
                (None, _) => true,
                (Some(want), Some(got)) => want == got,
                // 카테고리를 좁혀 달라고 했는데 레퍼런스에 카테고리가 없으면 제외한다.
                // 넣어 두면 어느 종류인지 모르는 레퍼런스가 모든 질의에 따라붙는다.
                (Some(_), None) => false,
            })
            .map(|entry| {
                let sim = cosine_similarity(&query_embedding, &entry.embedding);
                (sim, entry)
            })
            .collect();

        // Sort descending by similarity
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let results: Vec<ReferenceMatch> = scored
            .into_iter()
            .take(top_n)
            .map(|(sim, entry)| ReferenceMatch {
                name: entry.name.clone(),
                category: entry.category.clone(),
                era: entry.era.clone(),
                style: entry.style.clone(),
                description: entry.description.clone(),
                similarity: sim,
            })
            .collect();

        Ok(results)
    }
}

/// Wardrobe 아이템 시맨틱 검색 결과
pub struct WardrobeMatch {
    pub name: String,
    pub category: String,
    pub similarity: f32,
}

impl EmbeddingService {
    /// Wardrobe 아이템을 시맨틱 검색. clothing 목록에서 query와 가장 유사한 아이템 top-k 반환.
    /// query + 모든 아이템 설명을 한 번의 배치 API 호출로 임베딩하여 비용/지연을 최소화한다.
    pub async fn search_wardrobe(
        &self,
        query: &str,
        clothes: &[crate::models::clothing::Clothing],
        top_n: usize,
    ) -> anyhow::Result<Vec<WardrobeMatch>> {
        if clothes.is_empty() {
            return Ok(Vec::new());
        }

        // [query, item1, item2, ...] 를 한 번에 임베딩
        let mut texts: Vec<String> = Vec::with_capacity(clothes.len() + 1);
        texts.push(query.to_string());
        for c in clothes {
            texts.push(format!(
                "{} {} {} {} {}",
                c.name,
                c.color.as_deref().unwrap_or(""),
                c.style.map(|s| s.as_str()).unwrap_or(""),
                c.material_primary.as_deref().unwrap_or(""),
                c.sub_category.as_deref().unwrap_or(""),
            ));
        }

        let embeddings = self.embed_batch(texts).await?;
        let query_emb = embeddings
            .first()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing query embedding"))?;

        let mut scored: Vec<(f32, &crate::models::clothing::Clothing)> = clothes
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let item_emb = embeddings.get(i + 1);
                let sim = item_emb
                    .map(|e| cosine_similarity(&query_emb, e))
                    .unwrap_or(0.0);
                (sim, c)
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        Ok(scored
            .into_iter()
            .take(top_n)
            .map(|(sim, c)| WardrobeMatch {
                name: c.name.clone(),
                category: c.category.clone(),
                similarity: sim,
            })
            .collect())
    }
}

/// Cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;

    for (ai, bi) in a.iter().zip(b.iter()) {
        dot += ai * bi;
        norm_a += ai * ai;
        norm_b += bi * bi;
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 { 0.0 } else { dot / denom }
}
