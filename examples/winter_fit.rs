//! 같은 후보를 지금의 오늘 입기 판정으로 다시 잰다(진단용). 입력은 예전 바이너리 로그에서
//! 뽑은 후보 `[{temp, items: [이름…], old_fit}]` 이다. 상체(보온)·하체·코트 착용 상태를 따로
//! 출력해, 판정이 바뀐 후보가 왜 바뀌었는지 본다. DB 는 읽기만 한다.
//!
//!     cargo run --example winter_fit -- <후보.json>
use sqlx::mysql::MySqlPoolOptions;
use style_engine::db::clothing_repo;
use style_engine::models::outfit::{OutfitContext, OutfitSlot, SlotKind};
use style_engine::services::serving_ranker::{
    compute_today_fit, has_confirmed_insulation, is_winter_outer, lower_body_fit, outer_wear,
};

fn kind(c: &str) -> SlotKind {
    match c {
        "상의" => SlotKind::Top,
        "하의" => SlotKind::Bottom,
        "아우터" => SlotKind::Outer,
        "신발" => SlotKind::Shoes,
        _ => SlotKind::Bag,
    }
}

#[derive(serde::Deserialize)]
struct Cand {
    temp: f64,
    items: Vec<String>,
    old_fit: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let path = std::env::args().nth(1).expect("후보 파일");
    let cands: Vec<Cand> = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("DATABASE_URL")?)
        .await?;
    let clothes = clothing_repo::list_clothing_filtered(&pool, Some("female"), None).await?;
    let mut rows = Vec::new();
    for c in &cands {
        let mut slots = Vec::new();
        for n in &c.items {
            let Some(cl) = clothes.iter().find(|x| &x.name == n) else {
                continue;
            };
            slots.push(OutfitSlot {
                slot: kind(&cl.category),
                clothing: cl.clone(),
                seasons: clothing_repo::get_seasons(&pool, &cl.id)
                    .await
                    .unwrap_or_default(),
                texture_worlds: clothing_repo::get_texture_worlds(&pool, &cl.id)
                    .await
                    .unwrap_or_default(),
            });
        }
        let get = |k: SlotKind| slots.iter().find(|s| s.slot == k).map(|s| &s.clothing);
        let outer = get(SlotKind::Outer);
        let lower = lower_body_fit(get(SlotKind::Bottom), get(SlotKind::Shoes), outer, c.temp);
        let ctx = OutfitContext {
            slots: slots.clone(),
            situation: Some("일상".into()),
        };
        let new = compute_today_fit(&ctx, c.temp);
        rows.push(serde_json::json!({
            "temp": c.temp,
            "items": c.items,
            "old_fit": c.old_fit,
            "new_fit": format!("{new:?}"),
            "lower": format!("{lower:?}"),
            "outer": outer.map(|o| o.name.clone()),
            "outer_thickness": outer.map(|o| o.thickness.as_str()),
            "insulated": outer.map(has_confirmed_insulation),
            "winter_outer": outer.map(is_winter_outer),
            "top_thickness": get(SlotKind::Top).map(|t| t.thickness.as_str()),
            "wear": outer.and_then(|o| outer_wear(o, c.temp)),
        }));
    }
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}
