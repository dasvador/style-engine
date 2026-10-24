//! 한 카테고리 옷의 역할(role)을 바꾸면 운영 엔진 점수가 어떻게 움직이는지 잰다.
//!
//! 검수 페이지(`tools/world-review`)의 `[[role]]` 항목에 붙는 `impact` 를 만든다. 그 옷이
//! 들어간 착장(상의×하의 전부, 아우터는 없음/있음, 신발은 돌려 가며)을 역할마다 평가해
//! 지금 역할과의 운영 엔진(v1) 점수 차이와, shadow 하드필터 판정이 바뀐 수를 센다.
//! LLM 을 부르지 않으므로 결과가 매번 같다.
//!
//!     cargo run --example role_effect -- <성별> <카테고리> <out.json>
use std::collections::BTreeMap;

use sqlx::mysql::MySqlPoolOptions;
use style_engine::db::clothing_repo;
use style_engine::models::clothing::Clothing;
use style_engine::models::outfit::{IssueCode, OutfitContext, OutfitSlot, SlotKind};
use style_engine::models::style_vocab::Role;
use style_engine::services::style_engine as engine_v1;
use style_engine::services::style_engine_v2::run_hard_filter;

struct Item {
    c: Clothing,
    seasons: Vec<String>,
    worlds: Vec<String>,
}

fn slot(kind: SlotKind, it: &Item, role: Option<Option<Role>>) -> OutfitSlot {
    let mut c = it.c.clone();
    if let Some(r) = role {
        c.role = r;
    }
    OutfitSlot {
        slot: kind,
        clothing: c,
        seasons: it.seasons.clone(),
        texture_worlds: it.worlds.clone(),
    }
}

fn slot_kind(category: &str) -> SlotKind {
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
    let args: Vec<String> = std::env::args().collect();
    let (gender, category, out) = (&args[1], &args[2], &args[3]);
    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("DATABASE_URL")?)
        .await?;
    let clothes =
        clothing_repo::list_clothing_filtered(&pool, "default", Some(gender), None).await?;
    let mut items = Vec::new();
    for c in &clothes {
        let seasons = clothing_repo::get_seasons(&pool, &c.id)
            .await
            .unwrap_or_default();
        let worlds = clothing_repo::get_texture_worlds(&pool, &c.id)
            .await
            .unwrap_or_default();
        items.push(Item {
            c: c.clone(),
            seasons,
            worlds,
        });
    }
    let by = |cat: &str| {
        items
            .iter()
            .filter(|i| i.c.category == cat)
            .collect::<Vec<_>>()
    };
    let (tops, bottoms, outers, shoes, bags) =
        (by("상의"), by("하의"), by("아우터"), by("신발"), by("가방"));
    let target_kind = slot_kind(category);

    let mut report = BTreeMap::new();
    for target in by(category) {
        // 대상 옷을 고정하고 나머지 슬롯을 채운 착장들.
        let mut outfits: Vec<Vec<(SlotKind, &Item)>> = Vec::new();
        let mut k = 0usize;
        for t in &tops {
            for b in &bottoms {
                let mut o = vec![(SlotKind::Top, *t), (SlotKind::Bottom, *b)];
                o.push((SlotKind::Shoes, shoes[k % shoes.len()]));
                o.push((SlotKind::Bag, bags[k % bags.len()]));
                let mut with_outer = o.clone();
                with_outer.push((SlotKind::Outer, outers[k % outers.len()]));
                for mut v in [o, with_outer] {
                    v.retain(|(kind, _)| *kind != target_kind);
                    v.push((target_kind, target));
                    outfits.push(v);
                }
                k += 1;
            }
        }
        outfits.dedup_by(|a, b| {
            a.iter()
                .map(|(_, i)| &i.c.id)
                .eq(b.iter().map(|(_, i)| &i.c.id))
        });

        let ctx = |o: &Vec<(SlotKind, &Item)>, role: Option<Option<Role>>| OutfitContext {
            slots: o
                .iter()
                .map(|(kind, i)| {
                    let r = if std::ptr::eq(*i, target) { role } else { None };
                    slot(*kind, i, r)
                })
                .collect(),
            situation: None,
        };
        let mut per_role = BTreeMap::new();
        let base: Vec<_> = outfits
            .iter()
            .map(|o| {
                let c = ctx(o, None);
                let e = engine_v1::evaluate(&c, None);
                let accents = e
                    .problems
                    .iter()
                    .any(|p| p.code == IssueCode::TooManyAccents);
                (e.score, run_hard_filter(&c, None).pass, accents)
            })
            .collect();
        for &r in Role::ALL {
            let (mut sum, mut up, mut down, mut hard, mut accents) = (0i64, 0, 0, 0, 0);
            for (o, (s0, h0, _)) in outfits.iter().zip(&base) {
                let c = ctx(o, Some(Some(r)));
                let e = engine_v1::evaluate(&c, None);
                let d = (e.score - s0) as i64;
                sum += d;
                if d > 0 {
                    up += 1;
                }
                if d < 0 {
                    down += 1;
                }
                if run_hard_filter(&c, None).pass != *h0 {
                    hard += 1;
                }
                if e.problems
                    .iter()
                    .any(|p| p.code == IssueCode::TooManyAccents)
                {
                    accents += 1;
                }
            }
            let n = outfits.len();
            per_role.insert(
                r.as_str().to_string(),
                serde_json::json!({
                    "n": n, "avg": (sum as f64 / n as f64 * 10.0).round() / 10.0,
                    "up": up, "down": down, "hard": hard, "accents": accents,
                }),
            );
        }
        let now_accents = base.iter().filter(|b| b.2).count();
        report.insert(
            target.c.id.clone(),
            serde_json::json!({
                "name": target.c.name,
                "role": target.c.role.map(|r| r.as_str()),
                "now_accents": now_accents,
                "impact": per_role,
            }),
        );
    }
    std::fs::write(out, serde_json::to_string_pretty(&report)?)?;
    println!("{}벌 측정 → {}", report.len(), out);
    Ok(())
}
