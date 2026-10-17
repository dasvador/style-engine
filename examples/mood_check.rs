//! 무드 옷장(데모 시드를 넣은 DB)으로 만들 수 있는 코디를 운영 엔진으로 채점해, 무드 요소가
//! 있는 코디가 어떤 감점 때문에 밀리는지 본다. LLM 을 부르지 않으므로 결과가 매번 같다.
//!
//!     cargo run --example mood_check -- <무드> <기온> [규칙: plain|mood]
//!
//! `plain` 은 무드 없는 운영 평가, `mood` 는 `mood_rules::evaluate_for_mood` 다. 날씨
//! 판정은 홈과 같다(실패는 빼고, 경계는 −10). 전체 조합을 늘어놓는 진단 도구다 — 좋은
//! 조합이 "있다" 는 것을 보일 뿐, 실제 추천이 그 조합을 고른다는 증거가 아니다.
use std::collections::{BTreeMap, HashSet};

use sqlx::mysql::MySqlPoolOptions;
use style_engine::db::clothing_repo;
use style_engine::models::clothing::Clothing;
use style_engine::models::outfit::{OutfitContext, OutfitSlot, SlotKind};
use style_engine::models::style_vocab::StyleGenre;
use style_engine::services::serving_ranker::{WEATHER_BORDERLINE_PENALTY, compute_today_fit};
use style_engine::services::shortlist::{ShortlistContext, build_all_shortlists};
use style_engine::services::style_engine as engine_v1;
use style_engine::services::style_engine_v2::TodayFitLevel;
use style_engine::services::{mood_gate, mood_rules};

struct Item {
    c: Clothing,
    seasons: Vec<String>,
    worlds: Vec<String>,
}

fn kind(category: &str) -> SlotKind {
    match category {
        "상의" => SlotKind::Top,
        "하의" => SlotKind::Bottom,
        "아우터" => SlotKind::Outer,
        "신발" => SlotKind::Shoes,
        _ => SlotKind::Bag,
    }
}

fn ctx(items: &[&Item]) -> OutfitContext {
    OutfitContext {
        slots: items
            .iter()
            .map(|i| OutfitSlot {
                slot: kind(&i.c.category),
                clothing: i.c.clone(),
                seasons: i.seasons.clone(),
                texture_worlds: i.worlds.clone(),
            })
            .collect(),
        situation: Some("일상".to_string()),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let args: Vec<String> = std::env::args().collect();
    let mood = StyleGenre::from_alias(args.get(1).map(String::as_str).unwrap_or(""))
        .ok_or_else(|| anyhow::anyhow!("사용법: mood_check <무드> <기온> [plain|mood]"))?;
    // `explain`: "이름|이름|…" 한 벌의 감점을 하나씩 보여 준다.
    let explain = args.get(2).filter(|a| a.contains('|')).cloned();
    let temp: f64 = if explain.is_some() {
        args.get(3).map(String::as_str).unwrap_or("16").parse()?
    } else {
        args.get(2).map(String::as_str).unwrap_or("16").parse()?
    };
    let with_rules = args.get(3).map(String::as_str) != Some("plain");
    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("DATABASE_URL")?)
        .await?;
    let clothes = clothing_repo::list_clothing_filtered(&pool, Some("female"), Some(mood)).await?;
    let mut items = Vec::new();
    for c in &clothes {
        items.push(Item {
            seasons: clothing_repo::get_seasons(&pool, &c.id)
                .await
                .unwrap_or_default(),
            worlds: clothing_repo::get_texture_worlds(&pool, &c.id)
                .await
                .unwrap_or_default(),
            c: c.clone(),
        });
    }
    let season = if temp >= 23.0 {
        "여름"
    } else if temp >= 12.0 {
        "가을"
    } else {
        "겨울"
    };
    if let Some(names) = explain {
        let v: Vec<&Item> = names
            .split('|')
            .filter_map(|n| items.iter().find(|i| i.c.name == n))
            .collect();
        let c = ctx(&v);
        let pieces: Vec<&Clothing> = v.iter().map(|i| &i.c).collect();
        for (label, e) in [
            ("운영", engine_v1::evaluate(&c, Some(season))),
            (
                "무드",
                mood_rules::evaluate_for_mood(&c, Some(season), Some(mood)),
            ),
        ] {
            println!(
                "{label} {}점 · 날씨 {:?} · 무드 {:?}",
                e.score,
                compute_today_fit(&c, temp),
                mood_gate::reads_as_at(mood, &pieces, Some(temp))
            );
            for p in &e.problems {
                println!("  -{} {:?} {}: {}", p.deduction, p.code, p.rule, p.detail);
            }
        }
        return Ok(());
    }
    let none = HashSet::new();
    let sl = build_all_shortlists(
        &clothes,
        &ShortlistContext {
            temperature: temp,
            situation: Some("일상"),
            current_season: Some(season),
            recent_item_ids: &none,
        },
    );
    let allowed: HashSet<&str> = [&sl.tops, &sl.bottoms, &sl.outers, &sl.shoes, &sl.bags]
        .iter()
        .flat_map(|v| v.iter().map(|c| c.id.as_str()))
        .collect();
    let by = |cat: &str| {
        items
            .iter()
            .filter(|i| i.c.category == cat && allowed.contains(i.c.id.as_str()))
            .collect::<Vec<_>>()
    };
    println!(
        "{mood} {temp}°C ({season}) · 옷장 {} · 후보 목록 상의 {} 하의 {} 아우터 {} 신발 {} 가방 {}",
        items.len(),
        sl.tops.len(),
        sl.bottoms.len(),
        sl.outers.len(),
        sl.shoes.len(),
        sl.bags.len()
    );
    for cat in ["상의", "하의", "아우터"] {
        let names: Vec<String> = by(cat)
            .iter()
            .map(|i| {
                let mark = if mood_gate::is_mood_piece(mood, &i.c) == Some(true) {
                    "*"
                } else {
                    ""
                };
                format!("{}{mark}", i.c.name)
            })
            .collect();
        println!("  {cat}: {}", names.join(", "));
    }

    let mut outers: Vec<Option<&Item>> = vec![None];
    outers.extend(by("아우터").into_iter().map(Some));
    // (최종 점수, 무드를 읽는가, 감점 코드, 코디)
    let mut rows: Vec<(i32, bool, Vec<String>, Vec<&Item>)> = Vec::new();
    let mut failed = 0;
    for t in by("상의") {
        for b in by("하의") {
            for o in &outers {
                for sh in by("신발") {
                    for g in by("가방") {
                        let mut v: Vec<&Item> = vec![t, b, sh, g];
                        v.extend(*o);
                        let c = ctx(&v);
                        let fit = compute_today_fit(&c, temp);
                        if fit == TodayFitLevel::Fail {
                            failed += 1;
                            continue;
                        }
                        let e = if with_rules {
                            mood_rules::evaluate_for_mood(&c, Some(season), Some(mood))
                        } else {
                            engine_v1::evaluate(&c, Some(season))
                        };
                        let penalty = if fit == TodayFitLevel::Borderline {
                            WEATHER_BORDERLINE_PENALTY
                        } else {
                            0
                        };
                        let pieces: Vec<&Clothing> = v.iter().map(|i| &i.c).collect();
                        let reads =
                            mood_gate::reads_as_at(mood, &pieces, Some(temp)).unwrap_or(true);
                        let codes = e
                            .problems
                            .iter()
                            .map(|p| format!("{:?}-{}", p.code, p.deduction))
                            .collect();
                        rows.push((e.score - penalty, reads, codes, v));
                    }
                }
            }
        }
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.0));
    let reading = rows.iter().filter(|r| r.1).count();
    println!(
        "조합 {} (날씨 실패로 뺀 것 {failed}) · 무드를 읽는 조합 {reading}",
        rows.len()
    );
    let best = |f: bool| rows.iter().find(|r| r.1 == f).map(|r| r.0);
    println!(
        "최고점: 무드 요소 있음 {:?} · 없음 {:?}",
        best(true),
        best(false)
    );
    let top: Vec<_> = rows.iter().take(60).collect();
    println!(
        "상위 60개 중 무드를 읽는 조합 {}",
        top.iter().filter(|r| r.1).count()
    );
    // 무드를 읽는 조합이 받은 감점 — 어느 규칙이 반복해서 깎는지 본다.
    let mut freq: BTreeMap<String, usize> = BTreeMap::new();
    for r in rows.iter().filter(|r| r.1) {
        for code in &r.2 {
            *freq.entry(code.clone()).or_default() += 1;
        }
    }
    let mut freq: Vec<_> = freq.into_iter().collect();
    freq.sort_by_key(|f| std::cmp::Reverse(f.1));
    println!("무드를 읽는 조합의 감점(상위):");
    for (code, n) in freq.iter().take(12) {
        println!("  {code}: {n}");
    }
    // 무드를 읽는 조합에서 하의·아우터의 형태가 얼마나 갈리는가(80점 이상).
    let mut shapes: BTreeMap<String, usize> = BTreeMap::new();
    for r in rows.iter().filter(|r| r.1 && r.0 >= 80) {
        let pick = |cat: &str| {
            r.3.iter()
                .find(|i| i.c.category == cat)
                .map(|i| i.c.sub_category.clone().unwrap_or_default())
                .unwrap_or_else(|| "없음".into())
        };
        *shapes
            .entry(format!("하의 {} · 아우터 {}", pick("하의"), pick("아우터")))
            .or_default() += 1;
    }
    println!("80점 이상 무드 조합의 하의·아우터 종류:");
    for (k, n) in &shapes {
        println!("  {k}: {n}");
    }
    println!("상위 8개:");
    for r in rows.iter().take(8) {
        let names: Vec<&str> = r.3.iter().map(|i| i.c.name.as_str()).collect();
        println!(
            "  {} {} {} | {}",
            r.0,
            if r.1 { "무드" } else { "    " },
            names.join(" / "),
            r.2.join(" ")
        );
    }
    Ok(())
}
