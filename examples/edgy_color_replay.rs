//! 저장한 엣지 실측 후보를 지금의 카드 선택으로 다시 고른다(진단용). LLM 을 부르지 않는다.
//!
//! 입력은 실측 스크립트가 남긴 JSON(`[{temp, response, log}]`)이다. 로그의 `mood_shadow` 에
//! 후보마다 옷 이름·스타일·반복 감점·최종 점수·추천 가능 여부가 있다. 날씨 경계 감점은 지금의
//! 판정(`compute_today_fit`)으로 다시 잰다. 실측 당시의 최종 카드(`production`)와, 지금 코드의
//! 카드를 밝은색 비선호 꺼짐/켜짐 두 경우로 비교한다.
//!
//!     cargo run --example edgy_color_replay -- <실측.json>...
use sqlx::mysql::MySqlPoolOptions;
use style_engine::db::clothing_repo;
use style_engine::models::clothing::Clothing;
use style_engine::models::outfit::{OutfitContext, OutfitSlot, SlotKind};
use style_engine::models::recommendation::OutfitCandidate;
use style_engine::models::style_vocab::StyleGenre;
use style_engine::services::recommendation_service as svc;
use style_engine::services::serving_ranker::{WEATHER_BORDERLINE_PENALTY, compute_today_fit};
use style_engine::services::style_engine_v2::TodayFitLevel;

fn kind(category: &str) -> SlotKind {
    match category {
        "상의" => SlotKind::Top,
        "하의" => SlotKind::Bottom,
        "아우터" => SlotKind::Outer,
        "신발" => SlotKind::Shoes,
        _ => SlotKind::Bag,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("DATABASE_URL")?)
        .await?;
    let clothes = clothing_repo::list_clothing_filtered(
        &pool,
        "default",
        Some("female"),
        Some(StyleGenre::Edgy),
    )
    .await?;
    let mut seasons = std::collections::HashMap::new();
    for c in &clothes {
        seasons.insert(
            c.id.clone(),
            clothing_repo::get_seasons(&pool, &c.id)
                .await
                .unwrap_or_default(),
        );
    }
    let name_of = |id: &Option<String>| {
        id.as_ref()
            .and_then(|id| clothes.iter().find(|c| &c.id == id))
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "-".into())
    };
    let colour = |c: &OutfitCandidate, t: f64| {
        svc::visible_color(c, &clothes, t)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "무채색".into())
    };
    let (mut total, mut changed_on, mut changed_off) = (0, 0, 0);
    for path in std::env::args().skip(1) {
        let runs: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        for r in runs {
            let t = r["temp"].as_f64().unwrap_or(16.0);
            let log = r["log"].as_str().unwrap_or("");
            let Some(at) = log.find("mood_shadow=") else {
                continue;
            };
            let line = &log[at + "mood_shadow=".len()..];
            let line = &line[..line.find('\n').unwrap_or(line.len())];
            let shadow: serde_json::Value = serde_json::from_str(line)?;
            let mut servable = Vec::new();
            for c in shadow["candidates"].as_array().into_iter().flatten() {
                if !c["servable"].as_bool().unwrap_or(false) {
                    continue;
                }
                let items: Vec<&Clothing> = c["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|n| clothes.iter().find(|x| Some(x.name.as_str()) == n.as_str()))
                    .collect();
                let id = |k: SlotKind| {
                    items
                        .iter()
                        .find(|x| kind(&x.category) == k)
                        .map(|x| x.id.clone())
                };
                let mut cand = OutfitCandidate::new(
                    id(SlotKind::Top),
                    id(SlotKind::Bottom),
                    id(SlotKind::Outer),
                    id(SlotKind::Shoes),
                    id(SlotKind::Bag),
                    c["idx"].as_u64().unwrap_or(0) as usize,
                );
                cand.style_score = c["style"].as_i64().unwrap_or(0) as i32;
                cand.recency_penalty = c["recency"].as_i64().unwrap_or(0) as i32;
                let fin = c["final"].as_i64().unwrap_or(0) as i32;
                cand.diversity_bonus = (fin - cand.style_score + cand.recency_penalty).max(0);
                let ctx = OutfitContext {
                    slots: items
                        .iter()
                        .map(|x| OutfitSlot {
                            slot: kind(&x.category),
                            clothing: (*x).clone(),
                            seasons: seasons.get(&x.id).cloned().unwrap_or_default(),
                            texture_worlds: Vec::new(),
                        })
                        .collect(),
                    situation: Some("일상".into()),
                };
                cand.weather_penalty = if compute_today_fit(&ctx, t) == TodayFitLevel::Borderline {
                    WEATHER_BORDERLINE_PENALTY
                } else {
                    0
                };
                cand.final_score = fin - cand.weather_penalty;
                servable.push(cand);
            }
            servable.sort_by(|a, b| {
                b.final_score
                    .cmp(&a.final_score)
                    .then_with(|| b.style_score.cmp(&a.style_score))
            });
            let base_t = svc::select_todays_pick(&servable, t, &clothes);
            let base_v = base_t
                .as_ref()
                .and_then(|x| svc::select_variation(&servable, &x.candidate));
            let idx = |r: &Option<svc::ModeSelectionResult>| {
                r.as_ref().map(|r| r.candidate.ai_candidate_index)
            };
            let off = (idx(&base_t), idx(&base_v));
            let on_base_t = svc::select_todays_pick(&servable, t, &clothes);
            let on_base_v = on_base_t
                .as_ref()
                .and_then(|x| svc::select_variation(&servable, &x.candidate));
            let (on_t, on_v) =
                svc::prefer_visible_color(&servable, on_base_t, on_base_v, t, &clothes);
            let on = (idx(&on_t), idx(&on_v));
            let recorded = (
                shadow["production"][0].as_u64().map(|v| v as usize),
                shadow["production"][1].as_u64().map(|v| v as usize),
            );
            total += 1;
            changed_on += usize::from(on != recorded);
            changed_off += usize::from(off != recorded);
            let describe = |r: &Option<svc::ModeSelectionResult>| match r {
                Some(r) => format!(
                    "#{} {}·{}[{}] style={} final={}",
                    r.candidate.ai_candidate_index,
                    name_of(&r.candidate.top_id),
                    name_of(&r.candidate.outer_id),
                    colour(&r.candidate, t),
                    r.candidate.style_score,
                    r.scoring.final_score
                ),
                None => "-".into(),
            };
            println!("== {path} {t}°C  실측 카드 {recorded:?}");
            println!(
                "   제한 없음(켜짐) {on:?}  오늘 {} | 다른 {}",
                describe(&on_t),
                describe(&on_v)
            );
            println!(
                "   밝은색 비선호(꺼짐) {off:?}  오늘 {} | 다른 {}",
                describe(&base_t),
                describe(&base_v)
            );
        }
    }
    println!("요청 {total} · 실측 카드와 다른 경우: 켜짐 {changed_on} · 꺼짐 {changed_off}");
    Ok(())
}
