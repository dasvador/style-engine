use std::sync::Mutex;

use anyhow::Context;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use sqlx::MySqlPool;
use tokio::sync::RwLock;

use crate::db::reference_repo;
use crate::models::reference::ReferenceMatch;

/// Cached reference entry for in-memory similarity search
#[derive(Debug, Clone)]
struct CachedReference {
    name: String,
    era: Option<String>,
    style: Option<String>,
    description: String,
    embedding: Vec<f32>,
}

/// Embedding service using fastembed (ONNX-based, local, free)
pub struct EmbeddingService {
    model: Mutex<TextEmbedding>,
    cache: RwLock<Vec<CachedReference>>,
}

impl EmbeddingService {
    /// Initialize the embedding model (downloads ~80MB model on first run)
    pub fn new() -> anyhow::Result<Self> {
        tracing::info!("Initializing embedding model (multilingual-e5-base)...");

        let mut options = InitOptions::default();
        options.model_name = EmbeddingModel::MultilingualE5Base;
        options.show_download_progress = true;

        let model = TextEmbedding::try_new(options)
            .context("Failed to initialize fastembed model")?;

        tracing::info!("Embedding model initialized");

        Ok(Self {
            model: Mutex::new(model),
            cache: RwLock::new(Vec::new()),
        })
    }

    /// Embed a single text string, returning a 384-dim vector
    pub fn embed_text(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let model = self
            .model
            .lock()
            .map_err(|e| anyhow::anyhow!("Model lock poisoned: {}", e))?;
        let mut results = model
            .embed(vec![text.to_string()], None)
            .context("Failed to embed text")?;
        results
            .pop()
            .ok_or_else(|| anyhow::anyhow!("Empty embedding result"))
    }

    /// Load all reference embeddings from DB into the in-memory cache.
    /// If any entries are missing embeddings, generate and save them first.
    pub async fn load_cache(&self, pool: &MySqlPool) -> anyhow::Result<()> {
        let refs = reference_repo::list_references(pool).await?;
        let mut entries = Vec::with_capacity(refs.len());

        for r in refs {
            let embedding = if let Some(emb_json) = &r.embedding {
                serde_json::from_value::<Vec<f32>>(emb_json.clone()).ok()
            } else {
                None
            };

            let emb = if let Some(e) = embedding {
                e
            } else {
                // Generate missing embedding and persist to DB
                tracing::info!("Generating embedding for '{}'...", &r.name);
                let e = self.embed_text(&r.description)?;
                let emb_json = serde_json::to_value(&e)?;
                reference_repo::update_reference(
                    pool, &r.id, None, None, None, None, Some(&emb_json),
                )
                .await?;
                e
            };

            entries.push(CachedReference {
                name: r.name,
                era: r.era,
                style: r.style,
                description: r.description,
                embedding: emb,
            });
        }

        tracing::info!("Loaded {} reference embeddings into cache", entries.len());
        let mut cache = self.cache.write().await;
        *cache = entries;
        Ok(())
    }

    /// Search for top-N most similar references given a query text
    pub async fn search(&self, query: &str, top_n: usize) -> anyhow::Result<Vec<ReferenceMatch>> {
        let query_embedding = {
            let text = query.to_string();
            // Run embedding in blocking context since ONNX is CPU-bound
            let model_ref = &self.model;
            let model = model_ref
                .lock()
                .map_err(|e| anyhow::anyhow!("Model lock poisoned: {}", e))?;
            let mut results = model
                .embed(vec![text], None)
                .context("Failed to embed query")?;
            results
                .pop()
                .ok_or_else(|| anyhow::anyhow!("Empty embedding result"))?
        };

        let cache = self.cache.read().await;

        let mut scored: Vec<(f32, &CachedReference)> = cache
            .iter()
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
                era: entry.era.clone(),
                style: entry.style.clone(),
                description: entry.description.clone(),
                similarity: sim,
            })
            .collect();

        Ok(results)
    }

    /// Seed reference data if the table is empty
    pub async fn seed_if_empty(&self, pool: &MySqlPool) -> anyhow::Result<()> {
        let count = reference_repo::count_references(pool).await?;
        if count > 0 {
            tracing::info!(
                "clothing_reference table has {} entries, skipping seed",
                count
            );
            return Ok(());
        }

        tracing::info!("Seeding clothing reference data...");

        let seeds = seed_data();
        let mut seeded = 0;

        for (name, era, style, description) in &seeds {
            let embedding = self.embed_text(description)?;
            let emb_json = serde_json::to_value(&embedding)?;

            reference_repo::insert_reference(
                pool,
                name,
                Some(era),
                Some(style),
                description,
                Some(&emb_json),
            )
            .await?;

            seeded += 1;
        }

        tracing::info!("Seeded {} reference entries", seeded);
        Ok(())
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
    pub fn search_wardrobe(
        &self,
        query: &str,
        clothes: &[crate::models::clothing::Clothing],
        top_n: usize,
    ) -> anyhow::Result<Vec<WardrobeMatch>> {
        let query_emb = self.embed_text(query)?;

        let mut scored: Vec<(f32, &crate::models::clothing::Clothing)> = clothes
            .iter()
            .map(|c| {
                // 아이템 설명 텍스트 구성
                let desc = format!(
                    "{} {} {} {} {}",
                    c.name,
                    c.color.as_deref().unwrap_or(""),
                    c.style.as_deref().unwrap_or(""),
                    c.material_primary.as_deref().unwrap_or(""),
                    c.sub_category.as_deref().unwrap_or(""),
                );
                let item_emb = self.embed_text(&desc).unwrap_or_default();
                let sim = if item_emb.is_empty() { 0.0 } else { cosine_similarity(&query_emb, &item_emb) };
                (sim, c)
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        Ok(scored.into_iter().take(top_n).map(|(sim, c)| {
            WardrobeMatch {
                name: c.name.clone(),
                category: c.category.clone(),
                similarity: sim,
            }
        }).collect())
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
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

/// Amekaji / vintage clothing seed data: (name, era, style, description)
fn seed_data() -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    vec![
        (
            "M-51 피쉬테일 파카",
            "1950s 한국전쟁",
            "밀리터리",
            "M-51 피쉬테일 파카는 1950년대 한국전쟁 시기 미군 방한용 외투이다. 가장 큰 특징은 뒷자락이 물고기 꼬리(fishtail)처럼 두 갈래로 갈라진 롱 기장이다. 후드가 내장되어 있으며, 허리에 드로스트링이 있다. 프론트는 대형 지퍼와 스냅 버튼 플래킷(storm flap)으로 이중 잠금된다. 소재는 코튼 사틴 또는 나일론 쉘에 모직 또는 합성 라이너를 탈부착한다. 기장은 무릎 근처까지 내려오는 롱 파카로, M-65 필드 자켓(허리~엉덩이 기장)과 확연히 구분된다. 등 하단의 피쉬테일 디자인은 바람과 추위 차단용이며, 이 형태는 M-51 파카만의 고유한 식별 포인트이다.",
        ),
        (
            "M-65 필드 자켓",
            "1965~ 베트남전 이후",
            "밀리터리",
            "M-65 필드 자켓은 1965년 미군에 도입된 필드 자켓으로, 현재까지 가장 대중적인 밀리터리 아우터이다. 핵심 식별 포인트: (1) 스탠드 칼라(stand collar) 안에 후드 수납 지퍼가 있다 (칼라 뒷면 지퍼를 열면 후드가 나온다), (2) 가슴 2개 + 허리 2개 = 총 4개의 플랩 포켓, (3) 벨크로(초기형은 스냅) 커프스, (4) 허리와 밑단에 드로스트링. 소재는 나일론-코튼 혼방(50/50)이 기본이며, 기장은 허리~엉덩이를 덮는 미디엄 렝스이다. M-43과의 차이: M-65는 스탠드 칼라+후드 수납, M-43은 셔츠형 칼라+후드 없음. 정글 퍼티그와의 차이: M-65는 두꺼운 나일론-코튼, 정글 퍼티그는 얇은 립스탑.",
        ),
        (
            "정글 퍼티그 자켓",
            "1960s 베트남전",
            "밀리터리",
            "정글 퍼티그 자켓(Jungle Fatigue Jacket)은 베트남전 열대 전장용으로 개발된 경량 전투복 상의이다. 핵심 식별 포인트: (1) 셔츠형 포인트 칼라(뾰족한 옷깃, 셔츠처럼 접히는 칼라), (2) 버튼 프론트(지퍼가 아닌 버튼으로 여밈), (3) 가슴 2개 + 허리 2개 = 4개의 대형 벨로우즈 포켓(부풀어 오르는 입체 포켓), (4) 얇고 가벼운 립스탑(ripstop) 또는 포플린 소재 — 열대 기후용이라 매우 얇다. (5) 가스 플랩(gas flap)이 프론트 안쪽에 있다. 기장은 엉덩이까지 오는 정도. M-43과의 핵심 차이: 정글 퍼티그는 립스탑(얇고 가벼움), M-43은 백사틴(두껍고 묵직함). 같은 4포켓+셔츠칼라여도 소재 무게감으로 구분한다.",
        ),
        (
            "M-43 필드 자켓",
            "1943 2차 세계대전",
            "밀리터리",
            "M-43 필드 자켓은 2차 세계대전 유럽 전장용 미군 필드 자켓이다. 핵심 식별 포인트: (1) 셔츠형 칼라(포인트 칼라, 정글 퍼티그와 유사하나 소재가 다름), (2) 4개 포켓(가슴 2 + 허리 2), (3) 에폴렛(어깨 견장)이 있다, (4) OD(올리브 드랩) 코튼 백사틴(sateen) 소재 — 두껍고 묵직하며 광택이 약간 있다. (5) 비교적 심플하고 클린한 디자인. 기장은 엉덩이를 덮는 정도. 정글 퍼티그와의 핵심 차이: M-43은 백사틴(두꺼움, 묵직함, 약간의 광택), 정글 퍼티그는 립스탑(얇음, 가벼움, 무광). 같은 셔츠칼라+4포켓 구성이지만 소재의 두께와 질감이 확연히 다르다. M-65와의 차이: M-43은 셔츠칼라+후드 없음, M-65는 스탠드 칼라+후드 수납.",
        ),
        (
            "MA-1 봄버 자켓",
            "1950s~",
            "밀리터리",
            "MA-1 봄버 자켓은 1950년대 미 공군 비행 자켓으로, 가장 대중적인 봄버 자켓이다. 핵심 식별 포인트: (1) 리브 니트 칼라, 커프스, 밑단 — 목, 손목, 허리 모두 니트 리브로 마감되어 몸에 밀착, (2) 지퍼 프론트(버튼이 아닌 풀 지퍼), (3) 왼쪽 상완에 유틸리티 포켓(지퍼 또는 펜슬 포켓), (4) 나일론 아우터 쉘, (5) 인터내셔널 오렌지 안감(리버시블 구조, 긴급구조 시 뒤집어 입는 용도). 숏~미디엄 기장으로 허리 정도까지 온다. A-2 플라이트 자켓과의 차이: MA-1은 니트 칼라+나일론, A-2는 셔츠형 가죽 칼라+가죽 소재. 봄버 스타일의 대표 아이콘이다.",
        ),
        (
            "N-1 데크 자켓",
            "1940s~1950s 2차 대전/한국전",
            "밀리터리",
            "N-1 데크 자켓은 미 해군 갑판 근무용 방한 자켓이다. 핵심 식별 포인트: (1) 숏 기장(허리선 정도), (2) 안감이 알파카 파일 또는 보아(양털처럼 보송보송한 두꺼운 안감), (3) 치노스트랩 칼라 — 칼라 안쪽에도 알파카/보아 안감이 있어 세워서 여밀 수 있다, (4) 지퍼 프론트 + 윈드 플랩(스냅 버튼), (5) 코튼 준커클로스(jungle cloth) 또는 코튼 사틴 아우터 쉘, 네이비(곤색) 컬러가 기본. 다른 밀리터리 자켓 대비 확연히 짧은 기장과 보아/알파카 안감이 즉시 구분되는 포인트이다. 갑판(deck) 위 강풍과 추위에 대응하기 위한 헤비웨이트 자켓.",
        ),
        (
            "A-2 플라이트 자켓",
            "1930s~1940s 2차 대전",
            "밀리터리",
            "A-2 플라이트 자켓은 미 육군 항공대(AAF) 파일럿 가죽 자켓이다. 핵심 식별 포인트: (1) 셔츠형 칼라(뾰족한 옷깃, 가죽 칼라가 접혀 있음), (2) 리브 니트 커프스와 밑단(허리, 손목이 니트), (3) 가죽 소재 — 홀스하이드(말가죽) 또는 고트스킨(염소가죽), (4) 두 개의 플랩 포켓 또는 슬래시 포켓, (5) 지퍼 또는 스냅 프론트. 숏~미디엄 기장. MA-1과의 차이: A-2는 가죽+셔츠칼라, MA-1은 나일론+니트 칼라. B-15와의 차이: A-2는 가죽+셔츠칼라, B-15는 나일론/코튼+무톤 칼라. 2차 대전 시대 비행 자켓의 원형이며, 클래식한 가죽 비행 자켓의 대명사이다.",
        ),
        (
            "B-15 플라이트 자켓",
            "1940s~1950s 2차 대전/한국전",
            "밀리터리",
            "B-15 플라이트 자켓은 미 육군 항공대의 동계 비행 자켓이다. 핵심 식별 포인트: (1) 무톤(양모) 칼라 — 크고 푹신한 무톤 퍼 칼라가 가장 눈에 띄는 특징, (2) 지퍼 프론트, (3) 나일론 또는 코튼 아우터 쉘(가죽이 아님), (4) 알파카 또는 울 파일 안감, (5) 왼팔 유틸리티 포켓(펜슬 포켓). 숏~미디엄 기장. A-2와의 차이: B-15는 무톤 칼라+나일론/코튼, A-2는 셔츠 칼라+가죽. MA-1과의 차이: B-15는 무톤 칼라, MA-1은 니트 칼라. B-15는 MA-1의 전신으로, MA-1이 B-15의 무톤 칼라를 니트로 교체하여 발전시킨 것이다.",
        ),
        (
            "P-41/P-47 HBT 유틸리티 자켓",
            "1940s 2차 대전",
            "밀리터리/워크웨어",
            "P-41(초기형) 및 P-47(후기형) 유틸리티 자켓은 2차 대전 미군의 작업복/전투복 상의이다. 핵심 식별 포인트: (1) HBT(헤링본 트윌, Herringbone Twill) 소재 — 물고기뼈 패턴의 능직 코튼, OD(올리브 드랩) 또는 OD7(녹갈색) 컬러, (2) 심플한 버튼 프론트(13스타 메탈 버튼), (3) 가슴 포켓 1~2개, 허리 포켓 없거나 1개, (4) 워크웨어 스타일의 단순한 구조, (5) 에폴렛 없음(P-41) 또는 있음(P-47). 기장은 허리~엉덩이. M-43과의 차이: HBT 유틸리티는 헤링본 트윌 소재+심플 구조, M-43은 백사틴 소재+4포켓+에폴렛. 워크웨어와 밀리터리 스타일의 크로스오버 아이템으로, 빈티지/아메카지 패션에서 인기가 높다.",
        ),
        (
            "N-3B 스노클 파카",
            "1950s~ 한국전/냉전",
            "밀리터리",
            "N-3B 스노클 파카는 미군 극한지 방한 파카이다. 핵심 식별 포인트: (1) 스노클 후드 — 후드 앞부분이 얼굴을 감싸는 터널 형태로 돌출되어 있어 '스노클(잠수경)'처럼 보인다. 퍼(코요테/합성) 트림이 후드 테두리를 장식한다. (2) 히프~무릎 중간까지의 롱 기장, (3) 두꺼운 나일론 아우터 쉘 + 알파카/합성 파일 안감 — 극한 방한용, (4) 지퍼 + 스냅 버튼 플래킷(storm flap) 이중 잠금, (5) 대형 플랩 포켓과 핸드워머 포켓. M-51 피쉬테일 파카와의 차이: N-3B는 스노클 후드+퍼 트림, M-51은 피쉬테일(물고기 꼬리) 뒷자락. N-3B는 후드가 터널형으로 얼굴을 감싸지만, M-51은 후드가 일반적인 형태이다. N-1 데크 자켓과의 차이: N-3B는 롱 기장+스노클 후드, N-1은 숏 기장+무톤 칼라.",
        ),
        // --- 아메카지 / 캐주얼 ---
        (
            "셀비지 데님 진",
            "1950s~ 클래식",
            "아메카지/데님",
            "셀비지 데님(selvedge denim) 진은 구식 셔틀 직기(shuttle loom)로 직조한 데님 원단으로 만든 청바지이다. 핵심 식별 포인트: (1) 셀비지 라인 — 원단 가장자리에 빨간색/컬러 라인이 있는 마감(아웃심을 말아올리면 보임), (2) 인디고 염색 — 진한 인디고 블루에서 경년 변화(페이딩)가 진행됨, 히게(수염 자국), 하치노스(벌집 주름), 아타리(돌출부 색 빠짐) 등의 독특한 에이징 패턴, (3) 리벳(구리/은색 금속 보강), (4) 가죽 패치(뒷 허리), (5) 체인 스티치 헴라인(밑단을 둥근 바늘로 박아 로프 같은 물결 효과). 주요 브랜드: Warehouse, Levi's Vintage Clothing(LVC), Fullcount, orSlow, Kapital. 일반 청바지와의 차이: 셀비지 데님은 빈티지 직기로 직조해 원단 질감이 거칠고 불균일(슬럽)하며, 시간이 지남에 따라 착용자 고유의 에이징이 나타남. 일반 데님은 대량생산 직기로 균일하고 셀비지 라인이 없다.",
        ),
        (
            "빈티지 스웻셔츠",
            "1930s~ 클래식",
            "아메카지/스웻",
            "빈티지 스타일 스웻셔츠(crew neck sweatshirt)는 클래식 아메리칸 캐주얼의 핵심 아이템이다. 핵심 식별 포인트: (1) 크루넥(둥근 목) — V자 거싯(gusset, 목 앞의 삼각형 보강 원단)이 있으면 빈티지 디테일, (2) 이중직(loopback) 또는 역편직(reverse weave) 안감 — 루프 형태의 내면, (3) 리브 니트 커프스/밑단/목 — 탄력 있는 리브 마감, (4) 세트인 슬리브 또는 래글런 슬리브, (5) 코튼 플리스 또는 루프 테리 소재 — 두툼하고 묵직한 느낌. 주요 브랜드: Loopwheeler(일본, 구식 편직기), Champion(Reverse Weave), Warehouse, BARNS Outfitters. 일반 스웻셔츠와의 차이: 빈티지 스웻은 두꺼운 이중직 원단, V거싯, 리브가 두껍고 느슨한 핏. 현대 패스트패션 스웻은 얇고 가벼우며 거싯이 없다.",
        ),
        (
            "레트로 스니커",
            "1970s~ 클래식",
            "아메카지/스니커",
            "레트로/빈티지 스타일 스니커즈는 아메카지 코디의 필수 신발이다. 핵심 식별 포인트: (1) 클래식한 로우/미드 실루엣 — 70~90년대 디자인을 계승, (2) 스웨이드 또는 메쉬 어퍼, (3) 두꺼운 러닝화 스타일의 미드솔(EVA/엔캡), (4) 레트로 컬러웨이 — 그레이, 네이비, 버건디, 그린 등 빈티지 색 조합, (5) 모델 넘버/시리즈명이 유명(예: 574, 990, 993, Old Skool, Era). 주요 브랜드: New Balance(574, 990, 993, 996, 1300 시리즈 — Made in USA/UK 라인이 프리미엄), Vans(Old Skool, Authentic, Era — 캔버스+스웨이드 스케이트 스니커). 구별 포인트: New Balance는 'N' 로고 사이드 패널+러닝화 솔, Vans는 사이드 스트라이프(재즈 스트라이프)+와플 솔. 하이엔드 운동화나 하이테크 스니커와의 차이: 레트로 스니커는 단순한 실루엣, 클래식 컬러, 복고 디자인이 핵심이다.",
        ),
    ]
}
