//! 무게(weight) 속성이 추천에 주는 영향을 잰다.
//!
//! 여성 옷장 전체로 "지금 값" 과 "무게를 비운 값(None)" 을 나란히 돌려 기온별로
//! 무엇이 달라지는지 센다. 무게는 세 곳에서 쓰인다 — 기온별 후보 추리기(shortlist),
//! 오늘 입기 적합도의 보온 계산(today-fit), 스타일 엔진의 하드 필터·점수.
//! LLM 을 부르지 않으므로 결과가 매번 같다.
//!
//!     cargo run --example weight_effect
use std::collections::HashSet;

use sqlx::mysql::MySqlPoolOptions;
use style_engine::db::clothing_repo;
use style_engine::models::clothing::Clothing;
use style_engine::models::outfit::{OutfitContext, OutfitSlot, SlotKind};
use style_engine::services::candidate_pipeline::evaluate_v2;
use style_engine::services::shortlist::{ShortlistContext, build_all_shortlists};

const TEMPS: [f64; 7] = [0.0, 5.0, 10.0, 15.0, 20.0, 25.0, 30.0];

struct Item {
    c: Clothing,
    seasons: Vec<String>,
    worlds: Vec<String>,
}

fn slot(kind: SlotKind, it: &Item, strip: bool) -> OutfitSlot {
    let mut c = it.c.clone();
    if strip {
        c.weight = None;
    }
    OutfitSlot {
        slot: kind,
        clothing: c,
        seasons: it.seasons.clone(),
        texture_worlds: it.worlds.clone(),
    }
}

fn ids(v: &[&Clothing]) -> HashSet<String> {
    v.iter().map(|c| c.id.clone()).collect()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("DATABASE_URL")?)
        .await?;
    let clothes = clothing_repo::list_clothing_filtered(&pool, Some("female"), None).await?;
    let with_weight = clothes.iter().filter(|c| c.weight.is_some()).count();
    println!(
        "여성 옷 {}벌, 무게 값 있음 {}벌",
        clothes.len(),
        with_weight
    );

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

    // ── 1. 후보 추리기 ──
    let stripped: Vec<Clothing> = clothes
        .iter()
        .cloned()
        .map(|mut c| {
            c.weight = None;
            c
        })
        .collect();
    let recent = HashSet::new();
    println!(
        "\n[후보 추리기] 기온별로 슬롯 후보에서 빠지거나 새로 들어온 옷 수 (상의/하의/아우터/신발/가방)"
    );
    for situation in [None, Some("출근")] {
        for t in TEMPS {
            let ctx = ShortlistContext {
                temperature: t,
                situation,
                current_season: None,
                recent_item_ids: &recent,
            };
            let a = build_all_shortlists(&clothes, &ctx);
            let b = build_all_shortlists(&stripped, &ctx);
            let d =
                |x: &[&Clothing], y: &[&Clothing]| ids(x).symmetric_difference(&ids(y)).count() / 2;
            println!(
                "  {:>4} {:>4.0}도: {}/{}/{}/{}/{}",
                situation.unwrap_or("기본"),
                t,
                d(&a.tops, &b.tops),
                d(&a.bottoms, &b.bottoms),
                d(&a.outers, &b.outers),
                d(&a.shoes, &b.shoes),
                d(&a.bags, &b.bags)
            );
        }
    }

    // ── 2·3. 착장 평가 ──
    // 상의×하의 전부, 아우터는 없음 + 하나(순서대로 돌려 가며). 신발·가방도 돌려 가며 고른다.
    let mut outfits: Vec<Vec<(SlotKind, &Item)>> = Vec::new();
    let mut k = 0usize;
    for t in &tops {
        for b in &bottoms {
            let base = vec![
                (SlotKind::Top, *t),
                (SlotKind::Bottom, *b),
                (SlotKind::Shoes, shoes[k % shoes.len()]),
                (SlotKind::Bag, bags[k % bags.len()]),
            ];
            let mut with_outer = base.clone();
            with_outer.push((SlotKind::Outer, outers[k % outers.len()]));
            outfits.push(base);
            outfits.push(with_outer);
            k += 1;
        }
    }
    println!(
        "\n[착장 평가] 착장 {}개 × 기온 {}개",
        outfits.len(),
        TEMPS.len()
    );
    println!("  기온 | 적합도 바뀜 (통과→경계/실패 · 반대) | 하드필터 바뀜 | 스타일 점수 바뀜");
    let level = |l: style_engine::services::style_engine_v2::TodayFitLevel| l as i32;
    for t in TEMPS {
        let (mut worse, mut better, mut hard, mut score, mut dsum) = (0, 0, 0, 0, 0i64);
        let mut examples = Vec::new();
        for o in &outfits {
            let mk = |strip| OutfitContext {
                slots: o.iter().map(|(k, i)| slot(*k, i, strip)).collect(),
                situation: None,
            };
            let a = evaluate_v2(&mk(false), None, t);
            let b = evaluate_v2(&mk(true), None, t);
            let (la, lb) = (level(a.today_fit), level(b.today_fit));
            if la > lb {
                worse += 1;
                if examples.len() < 3 {
                    examples.push((o, a.today_fit, b.today_fit));
                }
            }
            if la < lb {
                better += 1;
            }
            if a.hard.pass != b.hard.pass {
                hard += 1;
            }
            if a.style_score != b.style_score {
                score += 1;
                dsum += (a.style_score - b.style_score) as i64;
            }
        }
        println!(
            "  {:>4.0}도 | 나빠짐 {:>4} · 좋아짐 {:>4} | {:>4} | {:>4} (평균 {:+.1})",
            t,
            worse,
            better,
            hard,
            score,
            if score > 0 {
                dsum as f64 / score as f64
            } else {
                0.0
            }
        );
        for (o, a, b) in examples {
            let names: Vec<String> = o
                .iter()
                .filter(|(k, _)| matches!(k, SlotKind::Top | SlotKind::Outer))
                .map(|(_, i)| format!("{}({:?}/{:?})", i.c.name, i.c.thickness, i.c.weight))
                .collect();
            println!(
                "        예: {} → 무게 있음 {:?}, 없음 {:?}",
                names.join(" + "),
                a,
                b
            );
        }
    }
    Ok(())
}
