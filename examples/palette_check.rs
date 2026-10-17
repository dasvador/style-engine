//! 색 다양성 성별·무드(`mood_palette::targets`)의 후보 목록을 기본 방식과 색 계열 방식으로
//! 나란히 본다(진단용). LLM 을 부르지 않는다. DB 는 읽기만 한다.
//!
//! 컨텍스트는 홈 추천(`routes::recommendation`)과 같게 둔다: 최근 이력 7개의 옷 id, 달력 기준
//! 계절(`current_season_label` 과 같은 규칙), 상황 '일상'.
//!
//!     cargo run --example palette_check -- <성별> <무드> <기온> [사용자 id — 최근 이력 반영]
use std::collections::HashSet;

use sqlx::mysql::MySqlPoolOptions;
use style_engine::db::clothing_repo;
use style_engine::db::recommendation_history_repo::RecommendationHistoryRepo;
use style_engine::models::clothing::Clothing;
use style_engine::models::style_vocab::StyleGenre;
use style_engine::services::mood_palette::colour_family;
use style_engine::services::shortlist::{
    ShortlistContext, build_all_shortlists, build_all_shortlists_with_palette,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let args: Vec<String> = std::env::args().collect();
    let gender = args.get(1).cloned().unwrap_or_else(|| "female".into());
    let mood = StyleGenre::from_alias(args.get(2).map(String::as_str).unwrap_or(""))
        .ok_or_else(|| anyhow::anyhow!("무드"))?;
    let temp: f64 = args.get(3).map(String::as_str).unwrap_or("19.6").parse()?;
    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("DATABASE_URL")?)
        .await?;
    let clothes = clothing_repo::list_clothing_filtered(&pool, Some(&gender), Some(mood)).await?;
    let mut recent = HashSet::new();
    if let Some(user) = args.get(4) {
        // 홈의 후보 목록과 같은 7개(`recent_ids`).
        for r in RecommendationHistoryRepo::find_recent_by_user(&pool, user, 7).await? {
            for id in [r.top_id, r.bottom_id, r.outer_id, r.shoes_id, r.bag_id]
                .into_iter()
                .flatten()
            {
                recent.insert(id);
            }
        }
    }
    // 홈과 같은 달력 기준 계절(`routes::recommendation::current_season_label`). 기온으로
    // 추정하지 않는다.
    use chrono::Datelike;
    let season = match chrono::Local::now().month() {
        3..=5 => "봄",
        6..=8 => "여름",
        9..=11 => "가을",
        _ => "겨울",
    };
    let ctx = ShortlistContext {
        temperature: temp,
        situation: Some("일상"),
        current_season: Some(season),
        recent_item_ids: &recent,
    };
    let show = |label: &str, v: &[&Clothing]| {
        let fams: Vec<String> = v
            .iter()
            .filter_map(|c| colour_family(c).map(|f| format!("{}({f})", c.name)))
            .collect();
        println!(
            "  {label} {}벌 · 컬러 {}: {}",
            v.len(),
            fams.len(),
            fams.join(", ")
        );
    };
    let wardrobe_colour: Vec<&Clothing> = clothes
        .iter()
        .filter(|c| matches!(c.category.as_str(), "상의" | "아우터") && colour_family(c).is_some())
        .collect();
    println!(
        "{gender} {mood} {temp}°C · 옷장 {}벌 · 컬러 상의·아우터 {}벌",
        clothes.len(),
        wardrobe_colour.len()
    );
    let base = build_all_shortlists(&clothes, &ctx);
    let pal = build_all_shortlists_with_palette(&clothes, &ctx);
    show("기본 상의", &base.tops);
    show("기본 아우터", &base.outers);
    show("색 계열 상의", &pal.tops);
    show("색 계열 아우터", &pal.outers);
    let removed: Vec<&str> = base
        .tops
        .iter()
        .chain(base.outers.iter())
        .filter(|c| {
            !pal.tops
                .iter()
                .chain(pal.outers.iter())
                .any(|x| x.id == c.id)
        })
        .map(|c| c.name.as_str())
        .collect();
    println!("  빠진 옷: {}", removed.join(", "));
    if std::env::var("SHOW_GROUPED").is_ok() {
        let g = pal.to_grouped();
        println!("  [모델이 받는 상의 줄]\n{}", g.tops.join("\n"));
        println!("  [모델이 받는 아우터 줄]\n{}", g.outers.join("\n"));
    }
    Ok(())
}
