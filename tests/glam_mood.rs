//! 글램 무드 데모 시드(`seeds/demo/glam.sql`)로 만든 코디가 기존 엔진의 기준을 지키는지.
//!
//! 시드 SQL 을 직접 읽어 옷을 만든다 — 시드를 고치면 이 테스트가 같이 본다. DB 와 LLM 은
//! 쓰지 않으므로 결과가 매번 같다. 포인트를 받쳐 주는 기존 옷 17벌(화이트 티·블랙 슬랙스
//! 등)은 시드가 태그만 붙이므로, 2026-10-16 DB 값을 여기 그대로 옮겨 둔다.

use std::collections::{HashMap, HashSet};

use style_engine::models::clothing::Clothing;
use style_engine::models::outfit::{OutfitContext, OutfitSlot, SlotKind};
use style_engine::models::style_vocab::{
    Role, Saturation, Silhouette, Style, StyleGenre, Thickness, Tone, Weight,
};
use style_engine::services::mood_gate;
use style_engine::services::serving_ranker::compute_today_fit;
use style_engine::services::shortlist::{ShortlistContext, build_all_shortlists};
use style_engine::services::style_engine as engine;
use style_engine::services::style_engine_v2::TodayFitLevel;

struct Item {
    c: Clothing,
    seasons: Vec<String>,
    worlds: Vec<String>,
}

/// SQL 값 목록 하나를 칸으로 나눈다. 따옴표 안의 쉼표는 나누지 않는다.
fn split_values(tuple: &str) -> Vec<Option<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut was_quoted = false;
    for ch in tuple.chars() {
        match ch {
            '\'' => {
                quoted = !quoted;
                was_quoted = true;
            }
            ',' if !quoted => {
                let v = cur.trim().to_string();
                out.push(if !was_quoted && v == "NULL" {
                    None
                } else {
                    Some(v)
                });
                cur.clear();
                was_quoted = false;
            }
            _ => cur.push(ch),
        }
    }
    let v = cur.trim().to_string();
    out.push(if !was_quoted && v == "NULL" {
        None
    } else {
        Some(v)
    });
    out
}

/// `INSERT … VALUES (…), (…) [AS new …];` 블록의 값 목록들. 주석 줄은 건너뛴다.
fn tuples(sql: &str, table_marker: &str) -> Vec<Vec<Option<String>>> {
    let start = sql.find(table_marker).expect(table_marker);
    let body = &sql[start..];
    let body = &body[body.find("VALUES").unwrap() + 6..];
    let end = body.find(';').unwrap();
    let body: String = body[..end]
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    let mut quoted = false;
    for ch in body.chars() {
        match ch {
            '\'' => {
                quoted = !quoted;
                if depth > 0 {
                    cur.push(ch);
                }
            }
            '(' if !quoted => {
                depth += 1;
                if depth == 1 {
                    cur.clear();
                    continue;
                }
                cur.push(ch);
            }
            ')' if !quoted => {
                depth -= 1;
                if depth == 0 {
                    out.push(split_values(&cur));
                    continue;
                }
                cur.push(ch);
            }
            _ if depth > 0 => cur.push(ch),
            _ => {}
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn clothing(
    id: &str,
    name: &str,
    category: &str,
    color: &str,
    thickness: &str,
    tone: &str,
    saturation: &str,
    style: &str,
    weight: &str,
    role: &str,
    formality: i8,
    silhouette: Option<&str>,
    material: &str,
    keywords: &str,
) -> Clothing {
    let now = chrono::NaiveDateTime::default();
    Clothing {
        id: id.into(),
        name: name.into(),
        category: category.into(),
        color: Some(color.into()),
        thickness: thickness.parse::<Thickness>().expect(name),
        image_url: None,
        tone: Some(tone.parse::<Tone>().expect(name)),
        saturation: Some(saturation.parse::<Saturation>().expect(name)),
        style: Some(style.parse::<Style>().expect(name)),
        weight: Some(weight.parse::<Weight>().expect(name)),
        role: Some(role.parse::<Role>().expect(name)),
        color_temperature: None,
        versatility: None,
        statement_level: None,
        formality_level: Some(formality),
        gender: Some("female".into()),
        style_mood: Some(StyleGenre::Glam),
        visual_weight: None,
        texture_depth: None,
        visual_weight_v2: None,
        texture_depth_v2: None,
        grounding_score: None,
        shadow_tone: None,
        silhouette_volume: silhouette.map(|s| s.parse::<Silhouette>().expect(name)),
        material_primary: Some(material.into()),
        sub_category: None,
        floating_score: None,
        strong_style_score: None,
        texture_keywords: Some(keywords.into()),
        created_at: now,
        updated_at: now,
    }
}

fn seed_sql() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/seeds/demo/glam.sql"))
        .expect("seeds/demo/glam.sql")
}

/// 시드의 새 옷 18벌 + 태그만 붙는 기존 옷 17벌. 운영의 글램 옷장과 같은 구성이다.
fn pool() -> Vec<Item> {
    let sql = seed_sql();
    let mut seasons: HashMap<String, Vec<String>> = HashMap::new();
    for t in tuples(&sql, "INSERT IGNORE INTO clothing_season") {
        seasons
            .entry(t[0].clone().unwrap())
            .or_default()
            .push(t[1].clone().unwrap());
    }
    let mut worlds: HashMap<String, Vec<String>> = HashMap::new();
    for t in tuples(&sql, "INSERT IGNORE INTO clothing_texture_world") {
        worlds
            .entry(t[0].clone().unwrap())
            .or_default()
            .push(t[1].clone().unwrap());
    }
    let mut items: Vec<Item> = tuples(&sql, "INSERT INTO clothing\n")
        .into_iter()
        .map(|t| {
            let s = |i: usize| t[i].clone().unwrap_or_default();
            let id = s(0);
            Item {
                c: clothing(
                    &id,
                    &s(1),
                    &s(2),
                    &s(5),
                    &s(6),
                    &s(7),
                    &s(8),
                    &s(9),
                    &s(10),
                    &s(11),
                    s(12).parse().unwrap(),
                    t[13].as_deref(),
                    &s(14),
                    &s(16),
                ),
                seasons: seasons.get(&id).cloned().unwrap_or_default(),
                worlds: worlds.get(&id).cloned().unwrap_or_default(),
            }
        })
        .collect();
    // 2026-10-16 DB 값 그대로. 시드는 이 옷들에 글램 태그만 더한다.
    let reused = |id: &str,
                  name: &str,
                  cat: &str,
                  color: &str,
                  th: &str,
                  tone: &str,
                  sat: &str,
                  style: &str,
                  weight: &str,
                  role: &str,
                  f: i8,
                  sil: Option<&str>,
                  mat: &str,
                  kw: &str,
                  ss: &[&str],
                  ws: &[&str]| Item {
        c: clothing(
            id, name, cat, color, th, tone, sat, style, weight, role, f, sil, mat, kw,
        ),
        seasons: ss.iter().map(|s| s.to_string()).collect(),
        worlds: ws.iter().map(|s| s.to_string()).collect(),
    };
    items.extend([
        reused(
            "d892c832-546b-11f1-af43-264d116ef584",
            "블랙 미니 체인 숄더백",
            "가방",
            "블랙",
            "medium",
            "어두움",
            "낮음",
            "베이직",
            "중간",
            "포인트",
            3,
            None,
            "leather",
            "leather, chain strap, mini",
            &["가을", "겨울", "봄", "여름"],
            &["leather"],
        ),
        reused(
            "8c00537e-598c-11f1-af43-264d116ef584",
            "실버 체인 미니 크로스백",
            "가방",
            "실버",
            "thin",
            "밝음",
            "낮음",
            "베이직",
            "가벼움",
            "포인트",
            2,
            None,
            "leather",
            "leather, silver chain, mini",
            &["가을", "겨울", "봄", "여름"],
            &["leather", "minimal"],
        ),
        reused(
            "0ffd0700-0000-0000-0000-000000000003",
            "블랙 리브 탱크탑",
            "상의",
            "블랙",
            "thin",
            "어두움",
            "낮음",
            "베이직",
            "가벼움",
            "베이스",
            1,
            Some("slim"),
            "cotton",
            "ribbed cotton, fitted tank",
            &["봄", "여름"],
            &["minimal"],
        ),
        reused(
            "d8929e84-546b-11f1-af43-264d116ef584",
            "블랙 리브드 슬림 니트",
            "상의",
            "블랙",
            "medium",
            "어두움",
            "낮음",
            "베이직",
            "중간",
            "베이스",
            2,
            Some("slim"),
            "knit",
            "ribbed knit, slim fit",
            &["가을", "겨울"],
            &["minimal"],
        ),
        reused(
            "8e2f729e-598a-11f1-af43-264d116ef584",
            "아이보리 새틴 셔츠 블라우스",
            "상의",
            "아이보리",
            "thin",
            "밝음",
            "낮음",
            "포멀",
            "가벼움",
            "베이스",
            4,
            Some("regular"),
            "satin",
            "satin, silky sheen",
            &["가을", "봄", "여름"],
            &["romantic"],
        ),
        reused(
            "0ffd0700-0000-0000-0000-000000000000",
            "화이트 코튼 크루넥 티셔츠",
            "상의",
            "화이트",
            "thin",
            "밝음",
            "낮음",
            "베이직",
            "가벼움",
            "베이스",
            1,
            Some("regular"),
            "cotton",
            "cotton jersey, relaxed crewneck",
            &["봄", "여름"],
            &["minimal"],
        ),
        reused(
            "8c00504a-598c-11f1-af43-264d116ef584",
            "누드베이지 스퀘어토 플랫 슈즈",
            "신발",
            "누드베이지",
            "medium",
            "밝음",
            "낮음",
            "베이직",
            "중간",
            "베이스",
            3,
            None,
            "leather",
            "leather, square toe, flat",
            &["가을", "겨울", "봄", "여름"],
            &["leather", "minimal"],
        ),
        reused(
            "8e2f8144-598a-11f1-af43-264d116ef584",
            "베이지 스퀘어토 스틸레토 힐",
            "신발",
            "베이지",
            "medium",
            "밝음",
            "낮음",
            "포멀",
            "중간",
            "베이스",
            4,
            None,
            "leather",
            "leather, square toe, stiletto",
            &["가을", "봄"],
            &["leather"],
        ),
        reused(
            "0ffd0700-0000-0000-0000-00000000000c",
            "블랙 레더 앵클부츠",
            "신발",
            "블랙",
            "medium",
            "어두움",
            "낮음",
            "베이직",
            "중간",
            "연결템",
            3,
            None,
            "leather",
            "leather, block heel ankle boot",
            &["가을", "겨울"],
            &["leather", "minimal"],
        ),
        reused(
            "8c00513a-598c-11f1-af43-264d116ef584",
            "블랙 슬링백 키튼힐",
            "신발",
            "블랙",
            "medium",
            "어두움",
            "낮음",
            "베이직",
            "중간",
            "베이스",
            4,
            None,
            "leather",
            "leather, slingback, kitten heel",
            &["가을", "겨울", "봄", "여름"],
            &["leather", "minimal"],
        ),
        reused(
            "0ffd0700-0000-0000-0000-000000000008",
            "블랙 레더 라이더 자켓",
            "아우터",
            "블랙",
            "medium",
            "어두움",
            "낮음",
            "베이직",
            "무거움",
            "구조템",
            2,
            Some("regular"),
            "leather",
            "leather, cropped biker, silver hardware",
            &["가을", "봄"],
            &["leather", "minimal"],
        ),
        reused(
            "d892ac44-546b-11f1-af43-264d116ef584",
            "블랙 싱글 테일러드 자켓",
            "아우터",
            "블랙",
            "medium",
            "어두움",
            "낮음",
            "베이직",
            "중간",
            "구조템",
            4,
            Some("regular"),
            "polyester",
            "structured, single breasted",
            &["가을", "봄"],
            &["tailoring"],
        ),
        reused(
            "8c004e42-598c-11f1-af43-264d116ef584",
            "블랙 미디 새틴 롱스커트",
            "하의",
            "블랙",
            "thin",
            "어두움",
            "낮음",
            "베이직",
            "가벼움",
            "베이스",
            3,
            Some("regular"),
            "satin",
            "satin, midi length",
            &["가을", "봄", "여름"],
            &["minimal"],
        ),
        reused(
            "8e2f7e38-598a-11f1-af43-264d116ef584",
            "블랙 스트레이트 울혼방 슬랙스",
            "하의",
            "블랙",
            "medium",
            "어두움",
            "낮음",
            "포멀",
            "중간",
            "베이스",
            4,
            Some("relaxed"),
            "wool",
            "wool blend, straight leg",
            &["가을", "겨울"],
            &["tailoring"],
        ),
        reused(
            "d892aeb0-546b-11f1-af43-264d116ef584",
            "블랙 와이드 슬랙스",
            "하의",
            "블랙",
            "medium",
            "어두움",
            "낮음",
            "베이직",
            "중간",
            "베이스",
            4,
            Some("oversized"),
            "polyester",
            "wide leg, clean drape",
            &["가을", "봄"],
            &["minimal"],
        ),
        reused(
            "d892c198-546b-11f1-af43-264d116ef584",
            "워시드 다크블루 와이드 데님",
            "하의",
            "다크블루",
            "medium",
            "어두움",
            "낮음",
            "베이직",
            "중간",
            "베이스",
            2,
            Some("oversized"),
            "denim",
            "washed denim, dark, wide leg",
            &["가을", "봄"],
            &["denim"],
        ),
        reused(
            "8c004cbc-598c-11f1-af43-264d116ef584",
            "화이트 와이드 데님 팬츠",
            "하의",
            "화이트",
            "medium",
            "밝음",
            "낮음",
            "베이직",
            "중간",
            "베이스",
            2,
            Some("oversized"),
            "denim",
            "white denim, wide leg",
            &["가을", "봄"],
            &["denim", "minimal"],
        ),
    ]);
    items
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

fn ctx(outfit: &[&Item]) -> OutfitContext {
    OutfitContext {
        slots: outfit
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

fn by_name<'a>(items: &'a [Item], name: &str) -> &'a Item {
    items
        .iter()
        .find(|i| i.c.name == name)
        .unwrap_or_else(|| panic!("{name}"))
}

/// 기온에 맞는 계절. 운영은 달력으로 정하지만 여기서는 기온 조건을 고정하려고 맞춘다.
fn season_for(temp: f64) -> &'static str {
    if temp >= 23.0 {
        "여름"
    } else if temp >= 12.0 {
        "가을"
    } else {
        "겨울"
    }
}

/// 이 기온에서 입을 수 있고(오늘 입기 실패 아님), 글램 요소가 있는 코디를 점수순으로.
fn wearable_glam_outfits(items: &[Item], temp: f64) -> Vec<(i32, Vec<&Item>)> {
    let season = season_for(temp);
    let none = HashSet::new();
    let clothes: Vec<Clothing> = items.iter().map(|i| i.c.clone()).collect();
    let sl = build_all_shortlists(
        &clothes,
        &ShortlistContext {
            temperature: temp,
            situation: Some("일상"),
            current_season: Some(season),
            recent_item_ids: &none,
        },
    );
    let allowed: HashSet<String> = [&sl.tops, &sl.bottoms, &sl.outers, &sl.shoes, &sl.bags]
        .iter()
        .flat_map(|v| v.iter().map(|c| c.id.clone()))
        .collect();
    let by = |cat: &str| {
        items
            .iter()
            .filter(|i| i.c.category == cat && allowed.contains(&i.c.id))
            .collect::<Vec<_>>()
    };
    let mut outers: Vec<Option<&Item>> = vec![None];
    outers.extend(by("아우터").into_iter().map(Some));
    let mut out = Vec::new();
    for t in by("상의") {
        for b in by("하의") {
            for o in &outers {
                for s in by("신발") {
                    for g in by("가방") {
                        let mut v = vec![t, b, s, g];
                        v.extend(*o);
                        let c = ctx(&v);
                        if compute_today_fit(&c, temp) == TodayFitLevel::Fail {
                            continue;
                        }
                        let pieces: Vec<&Clothing> = v.iter().map(|i| &i.c).collect();
                        if mood_gate::reads_as(StyleGenre::Glam, &pieces) != Some(true) {
                            continue;
                        }
                        out.push((engine::evaluate(&c, Some(season)).score, v));
                    }
                }
            }
        }
    }
    out.sort_by_key(|o| std::cmp::Reverse(o.0));
    out
}

/// 시드의 값이 전부 표준 어휘다. 하나라도 틀리면 DB 행 디코딩이 실패해 그 무드 옷장
/// 전체가 읽히지 않는다.
#[test]
fn seed_rows_use_the_standard_vocabulary() {
    let items = pool();
    let new: Vec<_> = items
        .iter()
        .filter(|i| i.c.id.starts_with("0ffd0b00"))
        .collect();
    assert_eq!(new.len(), 18);
    for i in &new {
        assert!(!i.seasons.is_empty(), "{} 계절이 비었다", i.c.name);
    }
    // 원피스를 넣지 않는다 — 하의 자리의 원피스 위에 상의가 하나 더 얹힌다.
    assert!(
        new.iter()
            .all(|i| !i.c.name.contains("원피스") && !i.c.name.contains("드레스"))
    );
}

/// 가방·신발까지 한 벌로 고정되지 않는다. 선선한 날과 더운 날 모두, 서로 다른 상·하의·
/// 신발·가방으로 된 입을 수 있는 글램 코디가 여럿 나온다.
#[test]
fn warm_and_cool_days_both_have_several_wearable_glam_outfits() {
    let items = pool();
    for temp in [27.0, 14.0] {
        let good: Vec<_> = wearable_glam_outfits(&items, temp)
            .into_iter()
            .filter(|(score, _)| *score >= 80)
            .collect();
        let distinct = |cat: &str| {
            good.iter()
                .flat_map(|(_, v)| {
                    v.iter()
                        .filter(|i| i.c.category == cat)
                        .map(|i| i.c.name.clone())
                })
                .collect::<HashSet<_>>()
                .len()
        };
        assert!(good.len() >= 20, "{temp}°C: {}개", good.len());
        for cat in ["상의", "하의", "신발", "가방"] {
            assert!(distinct(cat) >= 2, "{temp}°C {cat} {}종", distinct(cat));
        }
    }
}

/// 날씨는 글램이라고 느슨해지지 않는다. 더운 날 코디에는 가을·겨울 옷(메탈릭 니트·벨벳)이
/// 끼지 않고, 선선한 날 얇은 상의 하나로는 오늘 입기를 통과하지 못한다.
#[test]
fn weather_rules_still_apply() {
    let items = pool();
    let warm = wearable_glam_outfits(&items, 27.0);
    let top_warm: Vec<_> = warm.iter().take(30).collect();
    for (_, v) in &top_warm {
        for i in v {
            assert!(
                !i.c.name.contains("메탈릭 니트") && !i.c.name.contains("벨벳"),
                "27°C 상위 코디에 {}",
                i.c.name
            );
        }
    }
    let thin_alone = ctx(&[
        by_name(&items, "에메랄드 새틴 드레이프 블라우스"),
        by_name(&items, "샴페인 새틴 슬립 미디스커트"),
        by_name(&items, "블랙 새틴 포인티드 펌프스"),
        by_name(&items, "블랙 새틴 미니 클러치"),
    ]);
    assert_eq!(compute_today_fit(&thin_alone, 8.0), TodayFitLevel::Fail);
    let layered = ctx(&[
        by_name(&items, "화이트 코튼 크루넥 티셔츠"),
        by_name(&items, "샴페인 새틴 슬립 미디스커트"),
        by_name(&items, "블랙 벨벳 테일러드 블레이저"),
        by_name(&items, "블랙 레더 니하이 롱부츠"),
        by_name(&items, "블랙 새틴 미니 클러치"),
    ]);
    assert_ne!(compute_today_fit(&layered, 14.0), TodayFitLevel::Fail);
}

/// 강한 요소를 겹치면 기존 규칙이 감점한다. 메탈릭 상의에 메탈릭 가방을 더한 코디는
/// 같은 상의에 담백한 가방을 든 코디보다 낮다.
#[test]
fn stacking_strong_pieces_scores_lower() {
    let items = pool();
    let base = [
        by_name(&items, "에메랄드 새틴 드레이프 블라우스"),
        by_name(&items, "블랙 와이드 슬랙스"),
        by_name(&items, "블랙 레더 앵클부츠"),
    ];
    let with = |bag: &str| {
        let mut v = base.to_vec();
        v.push(by_name(&items, bag));
        engine::evaluate(&ctx(&v), Some("가을")).score
    };
    assert!(with("실버 메탈릭 미니 숄더백") < with("블랙 새틴 미니 클러치"));
}

/// 무드 게이트는 기본 아이템만으로 된 코디를 글램으로 치지 않는다.
#[test]
fn plain_basics_alone_do_not_read_as_glam() {
    let items = pool();
    let plain: Vec<&Clothing> = [
        "화이트 코튼 크루넥 티셔츠",
        "워시드 다크블루 와이드 데님",
        "블랙 레더 앵클부츠",
        "블랙 미니 체인 숄더백",
    ]
    .iter()
    .map(|n| &by_name(&items, n).c)
    .collect();
    assert_eq!(mood_gate::reads_as(StyleGenre::Glam, &plain), Some(false));
}

/// 테스트의 받침 옷 목록이 시드의 태그 목록과 같다. 시드에 옷을 더하거나 빼면 여기도
/// 따라와야 위 판정이 운영 옷장을 잰다.
#[test]
fn reused_items_match_the_seed_tag_list() {
    let sql = seed_sql();
    let block = &sql[sql.find("OR id IN (").unwrap()..];
    let block = &block[..block.find(");").unwrap()];
    // 줄 끝 주석(옷 이름)과 주석 줄의 따옴표는 id 가 아니다.
    let block: String = block
        .lines()
        .map(|l| l.split("--").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    let mut in_seed: Vec<String> = block
        .split('\'')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect();
    in_seed.sort();
    let mut in_test: Vec<String> = pool()
        .into_iter()
        .map(|i| i.c.id)
        .filter(|id| !id.starts_with("0ffd0b00"))
        .collect();
    in_test.sort();
    assert_eq!(in_seed, in_test);
}
