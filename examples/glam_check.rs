//! 글램 무드 옷장(`seeds/demo/glam.sql` 을 넣은 DB)으로 만들 수 있는 코디를 잰다.
//!
//! LLM 을 부르지 않으므로 결과가 매번 같다. 무드 옷장은 운영과 같은 조회
//! (`list_clothing_filtered`)로 읽는다.
//!
//!     cargo run --example glam_check -- explain "상의|하의|아우터|신발|가방"
//!     cargo run --example glam_check -- pool <기온>
//!     cargo run --example glam_check -- shadow <기온> [무드]   # 무드 없는 평가 vs 글램 규칙
//!
//! 전체 조합을 늘어놓는 진단 도구다 — 좋은 조합이 "있다" 는 것을 보일 뿐, 실제 추천이
//! 그 조합을 고른다는 증거가 아니다.
use std::collections::HashSet;

use sqlx::mysql::MySqlPoolOptions;
use style_engine::db::clothing_repo;
use style_engine::models::clothing::Clothing;
use style_engine::models::outfit::{OutfitContext, OutfitSlot, SlotKind};
use style_engine::models::style_vocab::{Role, StyleGenre};
use style_engine::services::serving_ranker::compute_today_fit;
use style_engine::services::shortlist::{ShortlistContext, build_all_shortlists};
use style_engine::services::style_engine as engine_v1;
use style_engine::services::style_engine_v2::TodayFitLevel;
use style_engine::services::{mood_rules, mood_shadow};

/// (무드 없는 운영 평가, 글램 규칙, S2, 글램 규칙, 코디) — 같은 조합의 점수들.
type Row<'a> = (i32, i32, i32, i32, Vec<&'a Item>);

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

/// 홈의 피드백 보정(`recommendation::feedback_bonus`)과 같은 계산.
fn feedback_bonus(
    items: &[&Clothing],
    item_adj: &std::collections::HashMap<String, i32>,
    prefs: &std::collections::HashMap<String, i32>,
) -> i32 {
    let mut bonus: i32 = items.iter().filter_map(|i| item_adj.get(&i.name)).sum();
    for tag in style_engine::services::outfit_scorer::detect_outfit_tags_pub(items) {
        bonus += prefs.get(tag.as_str()).copied().unwrap_or(0);
    }
    bonus
}

/// 이 코디에서 글램을 읽을 수 있는 요소의 수 — 빛을 받는 소재나 조형적인 실루엣.
fn glam_points(items: &[&Item]) -> usize {
    let words = [
        "satin",
        "metallic",
        "lurex",
        "velvet",
        "one-shoulder",
        "draped",
        "asymmetric",
        "lustrous",
    ];
    items
        .iter()
        .filter(|i| {
            let k = i.c.texture_keywords.as_deref().unwrap_or("").to_lowercase();
            words.iter().any(|w| k.contains(w))
        })
        .count()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let args: Vec<String> = std::env::args().collect();
    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("DATABASE_URL")?)
        .await?;
    let clothes =
        clothing_repo::list_clothing_filtered(&pool, Some("female"), Some(StyleGenre::Glam))
            .await?;
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
    let find = |name: &str| items.iter().find(|i| i.c.name == name);

    match args.get(1).map(String::as_str) {
        Some("explain") => {
            let names: Vec<&str> = args[2].split('|').collect();
            let outfit: Vec<&Item> = names.iter().filter_map(|n| find(n)).collect();
            let season = args.get(3).map(String::as_str);
            let e = engine_v1::evaluate(&ctx(&outfit), season);
            println!("점수 {} · 글램 요소 {}", e.score, glam_points(&outfit));
            for p in &e.problems {
                println!("  -{} {}: {}", p.deduction, p.rule, p.detail);
            }
        }
        Some("pool") => {
            let temp: f64 = args[2].parse()?;
            // 운영과 같은 기온 필터(shortlist)를 지난 옷만 쓰고, 기온에 맞는 계절로 평가한다.
            let season = if temp >= 23.0 {
                "여름"
            } else if temp >= 12.0 {
                "가을"
            } else {
                "겨울"
            };
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
            let (tops, bottoms, outers, shoes, bags) =
                (by("상의"), by("하의"), by("아우터"), by("신발"), by("가방"));
            println!(
                "무드 옷장: 상의 {} 하의 {} 아우터 {} 신발 {} 가방 {}",
                tops.len(),
                bottoms.len(),
                outers.len(),
                shoes.len(),
                bags.len()
            );
            let mut good = Vec::new();
            let mut best: std::collections::BTreeMap<String, (i32, String)> = Default::default();
            let mut outer_choices: Vec<Option<&Item>> = vec![None];
            outer_choices.extend(outers.iter().map(|o| Some(*o)));
            for t in &tops {
                for b in &bottoms {
                    for o in &outer_choices {
                        for s in &shoes {
                            for g in &bags {
                                let mut v: Vec<&Item> = vec![t, b, s, g];
                                if let Some(o) = o {
                                    v.push(o);
                                }
                                let c = ctx(&v);
                                if compute_today_fit(&c, temp) == TodayFitLevel::Fail {
                                    continue;
                                }
                                let e = engine_v1::evaluate(&c, Some(season));
                                for i in &v {
                                    let entry = best
                                        .entry(format!("{} {}", i.c.category, i.c.name))
                                        .or_insert((i32::MIN, String::new()));
                                    if e.score > entry.0 {
                                        *entry = (
                                            e.score,
                                            e.problems
                                                .iter()
                                                .map(|p| format!("-{} {}", p.deduction, p.rule))
                                                .collect::<Vec<_>>()
                                                .join(", "),
                                        );
                                    }
                                }
                                let accents =
                                    v.iter().filter(|i| i.c.role == Some(Role::Accent)).count();
                                if e.score >= 80 && glam_points(&v) >= 1 {
                                    good.push((e.score, accents, v));
                                }
                            }
                        }
                    }
                }
            }
            good.sort_by_key(|g| std::cmp::Reverse(g.0));
            let distinct = |f: fn(&Vec<&Item>) -> String| {
                good.iter()
                    .map(|(_, _, v)| f(v))
                    .collect::<HashSet<_>>()
                    .len()
            };
            println!(
                "{temp}°C ({season}) 에서 80점 이상·글램 요소 있는 코디 {}개",
                good.len()
            );
            println!(
                "  서로 다른 상의 {} · 하의 {} · 신발 {} · 가방 {}",
                distinct(|v| v[0].c.name.clone()),
                distinct(|v| v[1].c.name.clone()),
                distinct(|v| v[2].c.name.clone()),
                distinct(|v| v[3].c.name.clone()),
            );
            println!(
                "  포인트 2개 이상인 코디: {}",
                good.iter().filter(|(_, a, _)| *a >= 2).count()
            );
            if std::env::var("BEST").is_ok() {
                for (name, (score, why)) in &best {
                    println!("  최고 {score:>3} {name} [{why}]");
                }
            }
            for (score, _, v) in good.iter().step_by((good.len() / 6).max(1)).take(6) {
                println!(
                    "  {score} · {}",
                    v.iter()
                        .map(|i| i.c.name.as_str())
                        .collect::<Vec<_>>()
                        .join(" / ")
                );
            }
        }
        Some("accents") => {
            // 포인트(role = 포인트) 2개 이상 조합이 왜 상위에 없는가. 옷장·기온 필터·점수
            // 단계별로 센다. 집계 대상은 기온 필터를 지난 **모든** 조합이다.
            let temp: f64 = args[2].parse()?;
            let season = if temp >= 23.0 {
                "여름"
            } else if temp >= 12.0 {
                "가을"
            } else {
                "겨울"
            };
            let accent = |i: &Item| i.c.role == Some(Role::Accent);
            for cat in ["상의", "하의", "아우터", "신발", "가방"] {
                let all: Vec<_> = items.iter().filter(|i| i.c.category == cat).collect();
                let names: Vec<_> = all
                    .iter()
                    .filter(|i| accent(i))
                    .map(|i| i.c.name.as_str())
                    .collect();
                println!(
                    "  옷장 {cat}: {}벌 중 포인트 {}벌 {:?}",
                    all.len(),
                    names.len(),
                    names
                );
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
            let mut outers: Vec<Option<&Item>> = vec![None];
            outers.extend(by("아우터").into_iter().map(Some));
            let (mut total, mut two, mut two_fit) = (0, 0, 0);
            let mut scores: Vec<i32> = Vec::new();
            let mut one_scores: Vec<i32> = Vec::new();
            let mut deductions: std::collections::BTreeMap<String, usize> = Default::default();
            for t in by("상의") {
                for b in by("하의") {
                    for o in &outers {
                        for sh in by("신발") {
                            for g in by("가방") {
                                let mut v: Vec<&Item> = vec![t, b, sh, g];
                                v.extend(*o);
                                total += 1;
                                let n = v.iter().filter(|i| accent(i)).count();
                                let c = ctx(&v);
                                let fit = compute_today_fit(&c, temp) != TodayFitLevel::Fail;
                                let e = engine_v1::evaluate(&c, Some(season));
                                if n >= 2 {
                                    two += 1;
                                    if fit {
                                        two_fit += 1;
                                        scores.push(e.score);
                                        for p in &e.problems {
                                            *deductions
                                                .entry(format!("{} −{}", p.rule, p.deduction))
                                                .or_default() += 1;
                                        }
                                    }
                                } else if fit {
                                    one_scores.push(e.score);
                                }
                            }
                        }
                    }
                }
            }
            scores.sort_unstable();
            one_scores.sort_unstable();
            let med = |v: &Vec<i32>| v.get(v.len() / 2).copied().unwrap_or(0);
            println!(
                "{temp}°C: 기온 필터 뒤 조합 {total}개 중 포인트 2개 이상 {two}개, 그중 오늘 입기 통과 {two_fit}개"
            );
            println!(
                "  엔진 점수 — 포인트 2개 이상: 중앙값 {} 최고 {} · 1개 이하: 중앙값 {} 최고 {}",
                med(&scores),
                scores.last().copied().unwrap_or(0),
                med(&one_scores),
                one_scores.last().copied().unwrap_or(0)
            );
            let mut d: Vec<_> = deductions.into_iter().collect();
            d.sort_by_key(|x| std::cmp::Reverse(x.1));
            println!(
                "  포인트 2개 이상 조합의 감점(건수): {:?}",
                &d[..d.len().min(6)]
            );
        }
        Some("shadow") => {
            // 같은 조합을 운영 점수와 shadow 점수(운영 + mood_shadow 보정)로 각각 줄 세워
            // 상위 코디가 어떻게 달라지는지 본다. 날씨 판정은 둘 다 운영 그대로다.
            let temp: f64 = args[2].parse()?;
            let mood = args
                .get(3)
                .and_then(|m| StyleGenre::from_alias(m))
                .unwrap_or(StyleGenre::Glam);
            let season = if temp >= 23.0 {
                "여름"
            } else if temp >= 12.0 {
                "가을"
            } else {
                "겨울"
            };
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
            let mut outers: Vec<Option<&Item>> = vec![None];
            outers.extend(by("아우터").into_iter().map(Some));
            let item_adj: std::collections::HashMap<String, i32> =
                style_engine::db::feedback_repo::get_item_adjustments(&pool, "default")
                    .await?
                    .into_iter()
                    .map(|s| (s.item_name, s.score_adjustment))
                    .collect();
            let prefs: std::collections::HashMap<String, i32> =
                style_engine::db::feedback_repo::get_preference_scores(&pool, "default")
                    .await?
                    .into_iter()
                    .map(|s| (s.reason_tag, s.score))
                    .collect();
            let mut rows = Vec::new();
            for t in by("상의") {
                for b in by("하의") {
                    for o in &outers {
                        for sh in by("신발") {
                            for g in by("가방") {
                                let mut v: Vec<&Item> = vec![t, b, sh, g];
                                v.extend(*o);
                                let c = ctx(&v);
                                if compute_today_fit(&c, temp) == TodayFitLevel::Fail {
                                    continue;
                                }
                                let e = engine_v1::evaluate(&c, Some(season));
                                let w = mood_shadow::waiver(&c, &e, Some(mood));
                                // 운영과 같은 피드백 보정(아이템 누적 + 이유 태그). 홈은
                                // style_score = 엔진 점수 + 이 값이다.
                                let pieces: Vec<&Clothing> = v.iter().map(|i| &i.c).collect();
                                let fb = feedback_bonus(&pieces, &item_adj, &prefs);
                                let g = mood_rules::evaluate_for_mood(&c, Some(season), Some(mood))
                                    .score;
                                rows.push((
                                    e.score + fb,
                                    g + fb,
                                    e.score + fb + w.inner,
                                    g + fb,
                                    v,
                                ));
                            }
                        }
                    }
                }
            }
            // 동점이 많아 "상위 20개" 는 순회 순서에 좌우된다. 85점 이상 묶음의 구성과
            // 강한 상의별 최고점을 본다.
            let summarize = |label: &str, key: fn(&Row) -> i32| {
                let gate = rows.iter().filter(|r| key(r) >= 70).count();
                let high: Vec<_> = rows.iter().filter(|r| key(r) >= 85).collect();
                let skirts = high
                    .iter()
                    .filter(|r| {
                        r.4.iter()
                            .any(|i| i.c.category == "하의" && i.c.name.contains("스커트"))
                    })
                    .count();
                println!(
                    "  {label}: 70점 이상(오늘의 추천 1차 조건) {gate}/{} · 85점 이상 중 스커트 {skirts}/{}",
                    rows.len(),
                    high.len()
                );
                let statement = high
                    .iter()
                    .filter(|r| {
                        r.4.iter()
                            .any(|i| i.c.category == "상의" && i.c.role == Some(Role::Accent))
                    })
                    .count();
                let glam = high.iter().filter(|r| glam_points(&r.4) >= 1).count();
                let strong2 = high
                    .iter()
                    .filter(|r| {
                        r.4.iter()
                            .filter(|i| i.c.role == Some(Role::Accent))
                            .count()
                            >= 2
                    })
                    .count();
                let tops: HashSet<_> = high
                    .iter()
                    .flat_map(|r| {
                        r.4.iter()
                            .filter(|i| i.c.category == "상의")
                            .map(|i| &i.c.name)
                    })
                    .collect();
                println!(
                    "  {label}: 85점 이상 {}개 — 강한 상의 포함 {statement} · 글램 요소 {glam} · 포인트 2개 이상 {strong2} · 서로 다른 상의 {}",
                    high.len(),
                    tops.len()
                );
                let mut best: std::collections::BTreeMap<&str, i32> = Default::default();
                for r in &rows {
                    for i in
                        r.4.iter()
                            .filter(|i| i.c.category == "상의" && i.c.role == Some(Role::Accent))
                    {
                        let e = best.entry(i.c.name.as_str()).or_insert(i32::MIN);
                        *e = (*e).max(key(r));
                    }
                }
                println!(
                    "      강한 상의 최고점: {}",
                    best.iter()
                        .map(|(n, s)| format!("{n} {s}"))
                        .collect::<Vec<_>>()
                        .join(" · ")
                );
            };
            println!(
                "{temp}°C ({season}) · {} · 입을 수 있는 조합 {}개",
                mood.as_str(),
                rows.len()
            );
            summarize("운영(무드 없음)", |r| r.0);
            summarize("글램 규칙     ", |r| r.1);
            summarize("S2 이너만     ", |r| r.2);
        }
        _ => eprintln!(
            "사용법: glam_check explain \"이름|이름|…\" | pool <기온> | shadow <기온> [무드]"
        ),
    }
    Ok(())
}
