//! 실제로 입은 기록을 추천 순위에 조금 반영한다.
//!
//! 쓰는 것은 `outfit_wear.status = 'worn'` 뿐이다. 계획·취소된 기록, 추천 이력(보여 준 것), 룩북의
//! `worn` 표시는 들어오지 않는다. 입어 본 반응(`reaction`)도 쓰지 않는다 — 한 번의 말을 취향으로
//! 학습하지 않는다.
//!
//! 얼마나:
//!   - 3일 안에 입은 상의·하의·아우터 한 벌당 −2, 7일 안이면 −1.
//!   - 3일 안에 입은 신발·가방 한 개당 −1. 매일 같은 신발을 신는 것은 흔해서 작게 둔다.
//!   - 14일 안에 구성이 똑같은 코디를 입었으면 −3.
//!   - 합계는 [`WEAR_PENALTY_CAP`] 까지. 날씨 경계 감점(−10)보다 작아 날씨 판정을 뒤집지 못한다.
//!
//! 뺄 뿐 제외하지 않는다 — 작은 옷장이 비지 않게. 모두가 같은 몫을 잃으면 그 몫은 지운다
//! (`recommendation_service::finalize_scores` 와 같은 이유). `style_score` 를 건드리지 않으므로
//! 오늘의 추천 자격 단계도 그대로다.

use std::collections::HashMap;

use chrono::{Duration, NaiveDate};
use serde_json::Value;
use sqlx::MySqlPool;
use sqlx::types::Json;

use crate::models::clothing::Clothing;
use crate::models::recommendation::OutfitCandidate;
use crate::services::outfit_comparison::{ItemSnapshot, item_key};

pub const WEAR_PENALTY_CAP: i32 = 6;
const WINDOW_DAYS: i64 = 14;
const RECENT_DAYS: i64 = 3;
const WEEK_DAYS: i64 = 7;

/// 최근 입은 기록의 요약.
#[derive(Debug, Clone, Default)]
pub struct RecentWear {
    /// 옷 id → (가장 최근 입은 날, 이름)
    pub items: HashMap<String, (NaiveDate, String)>,
    /// 구성 키 → 가장 최근 입은 날
    pub outfits: HashMap<String, NaiveDate>,
}

impl RecentWear {
    pub fn add(&mut self, date: NaiveDate, key: &str, items: &[ItemSnapshot]) {
        let latest = self.outfits.entry(key.to_string()).or_insert(date);
        *latest = (*latest).max(date);
        for i in items {
            let entry = self
                .items
                .entry(i.id.clone())
                .or_insert((date, i.name.clone()));
            if date > entry.0 {
                entry.0 = date;
            }
        }
    }
}

/// 14일 안의 입은 기록을 읽는다.
pub async fn load_recent(
    db: &MySqlPool,
    user_id: &str,
    today: NaiveDate,
) -> Result<RecentWear, sqlx::Error> {
    let rows: Vec<(NaiveDate, String, Json<Vec<ItemSnapshot>>)> = sqlx::query_as(
        "SELECT wear_date, item_key, items_json FROM outfit_wear \
         WHERE user_id = ? AND status = 'worn' AND wear_date BETWEEN ? AND ? \
         ORDER BY wear_date DESC LIMIT 60",
    )
    .bind(user_id)
    .bind(today - Duration::days(WINDOW_DAYS))
    .bind(today)
    .fetch_all(db)
    .await?;
    let mut recent = RecentWear::default();
    for (date, key, items) in rows {
        recent.add(date, &key, &items.0);
    }
    Ok(recent)
}

/// 감점 계산에 쓰는 옷 한 벌(id·카테고리).
pub type WornItem<'a> = (&'a str, &'a str);

/// 이 구성의 감점. 상한은 적용했고, 후보끼리 공통인 몫은 아직 빼지 않은 값이다.
pub fn raw_penalty(items: &[WornItem<'_>], recent: &RecentWear, today: NaiveDate) -> i32 {
    let mut points = 0;
    for (id, category) in items {
        let Some((date, _)) = recent.items.get(*id) else {
            continue;
        };
        let days = (today - *date).num_days();
        let garment = matches!(*category, "상의" | "하의" | "아우터");
        points += match (garment, days) {
            (true, d) if d <= RECENT_DAYS => 2,
            (true, d) if d <= WEEK_DAYS => 1,
            (false, d) if d <= RECENT_DAYS => 1,
            _ => 0,
        };
    }
    let key = item_key(items.iter().map(|(id, _)| *id));
    if let Some(date) = recent.outfits.get(&key)
        && (today - *date).num_days() <= WINDOW_DAYS
    {
        points += 3;
    }
    points.min(WEAR_PENALTY_CAP)
}

fn candidate_items<'a>(c: &'a OutfitCandidate, clothes: &'a [Clothing]) -> Vec<WornItem<'a>> {
    [&c.top_id, &c.bottom_id, &c.outer_id, &c.shoes_id, &c.bag_id]
        .iter()
        .filter_map(|id| id.as_ref())
        .filter_map(|id| clothes.iter().find(|x| &x.id == id))
        .map(|c| (c.id.as_str(), c.category.as_str()))
        .collect()
}

/// 후보들에 감점을 얹는다. 기존 반복 감점(`recency_penalty`)에 더해, 카드 선택 경로가 모두 같은 값을 본다.
/// 모두에게 공통인 몫은 빼고, 순서는 호출자가 다시 정렬한다. 후보를 빼지 않는다.
pub fn apply_penalty(
    candidates: &mut [OutfitCandidate],
    clothes: &[Clothing],
    recent: &RecentWear,
    today: NaiveDate,
) {
    let raw: Vec<i32> = candidates
        .iter()
        .map(|c| raw_penalty(&candidate_items(c, clothes), recent, today))
        .collect();
    let shared = raw.iter().copied().min().unwrap_or(0);
    for (c, p) in candidates.iter_mut().zip(raw) {
        let p = p - shared;
        c.wear_penalty = p;
        c.recency_penalty += p;
        c.final_score -= p;
    }
}

/// 사용자에게 보여 줄 사실 한 줄. 최근 7일 안에 입은 옷이 없으면 `None`.
pub fn wear_note(
    item_ids: &[&str],
    recent: &RecentWear,
    today: NaiveDate,
    penalty: i32,
) -> Option<String> {
    let mut worn: Vec<(NaiveDate, &str)> = item_ids
        .iter()
        .filter_map(|id| recent.items.get(*id))
        .filter(|(date, _)| (today - *date).num_days() <= WEEK_DAYS)
        .map(|(date, name)| (*date, name.as_str()))
        .collect();
    if worn.is_empty() {
        return None;
    }
    worn.sort_by_key(|w| std::cmp::Reverse(w.0));
    let list = worn
        .iter()
        .map(|(date, name)| format!("{name}({} 입음)", date.format("%-m/%-d")))
        .collect::<Vec<_>>()
        .join(", ");
    Some(if penalty > 0 {
        format!("최근 입은 옷이 들어 있어요: {list}. 순위를 조금 낮췄어요.")
    } else {
        format!("최근 입은 옷이 들어 있어요: {list}.")
    })
}

/// 저장된 날씨 맥락에서 기온을 꺼낸다.
pub fn context_temperature(context: &Value) -> Option<f64> {
    context.get("weather")?.get("temperature")?.as_f64()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::recommendation_service::finalize_scores;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn snap(id: &str) -> ItemSnapshot {
        ItemSnapshot {
            id: id.into(),
            name: format!("옷 {id}"),
            category: "상의".into(),
            color: None,
            sub_category: None,
            material: None,
            has_image: false,
        }
    }

    fn clothing(id: &str, category: &str) -> Clothing {
        let t = chrono::NaiveDateTime::default();
        Clothing {
            id: id.into(),
            name: format!("옷 {id}"),
            category: category.into(),
            gender: None,
            style_mood: None,
            color: None,
            thickness: crate::models::style_vocab::Thickness::Medium,
            image_url: None,
            tone: None,
            saturation: None,
            style: None,
            weight: None,
            role: None,
            color_temperature: None,
            versatility: None,
            statement_level: None,
            formality_level: None,
            visual_weight: None,
            texture_depth: None,
            visual_weight_v2: None,
            texture_depth_v2: None,
            grounding_score: None,
            shadow_tone: None,
            silhouette_volume: None,
            material_primary: None,
            sub_category: None,
            floating_score: None,
            strong_style_score: None,
            texture_keywords: None,
            created_at: t,
            updated_at: t,
        }
    }

    fn cand(top: &str, bottom: &str, idx: usize, style: i32) -> OutfitCandidate {
        let mut c =
            OutfitCandidate::new(Some(top.into()), Some(bottom.into()), None, None, None, idx);
        c.style_score = style;
        c
    }

    fn wardrobe() -> Vec<Clothing> {
        vec![
            clothing("t1", "상의"),
            clothing("t2", "상의"),
            clothing("b1", "하의"),
            clothing("b2", "하의"),
        ]
    }

    fn sort(c: &mut [OutfitCandidate]) {
        c.sort_by_key(|x| std::cmp::Reverse(x.final_score));
    }

    /// 고정된 후보에서: 어제 입은 옷을 쓰는 동점 후보가 뒤로 밀린다.
    #[test]
    fn recently_worn_item_loses_a_tie() {
        let today = d("2026-11-04");
        let mut recent = RecentWear::default();
        recent.add(d("2026-11-03"), "b9|t1", &[snap("t1"), snap("b9")]);
        let mut c = vec![cand("t1", "b1", 0, 80), cand("t2", "b2", 1, 80)];
        finalize_scores(&mut c);
        apply_penalty(&mut c, &wardrobe(), &recent, today);
        sort(&mut c);
        assert_eq!(c[0].ai_candidate_index, 1);
        assert_eq!(c[1].wear_penalty, 2);
        assert_eq!(c[1].final_score, 78);
    }

    /// 감점은 작다. 스타일 점수가 분명히 높은 후보는 어제 입은 옷이 있어도 앞선다.
    #[test]
    fn penalty_cannot_beat_a_real_style_gap() {
        let today = d("2026-11-04");
        let mut recent = RecentWear::default();
        recent.add(today, &item_key(["t1", "b1"]), &[snap("t1"), snap("b1")]);
        let mut c = vec![cand("t1", "b1", 0, 80), cand("t2", "b2", 1, 70)];
        finalize_scores(&mut c);
        apply_penalty(&mut c, &wardrobe(), &recent, today);
        sort(&mut c);
        assert_eq!(c[0].ai_candidate_index, 0);
        assert_eq!(c[0].wear_penalty, WEAR_PENALTY_CAP, "상한");
        assert_eq!(c.len(), 2, "후보를 빼지 않는다");
        assert_eq!(c[0].style_score, 80, "스타일 점수는 그대로");
    }

    /// 작은 옷장: 모든 후보가 같은 옷을 쓰면 공통 몫은 지워 점수가 깎이지 않는다.
    #[test]
    fn shared_wear_cancels_out() {
        let today = d("2026-11-04");
        let mut recent = RecentWear::default();
        recent.add(d("2026-11-03"), "x", &[snap("t1")]);
        let mut c = vec![cand("t1", "b1", 0, 80), cand("t1", "b2", 1, 75)];
        finalize_scores(&mut c);
        apply_penalty(&mut c, &wardrobe(), &recent, today);
        assert_eq!(c[0].final_score, 80);
        assert_eq!(c[1].final_score, 75);
    }

    #[test]
    fn old_wear_has_no_effect_and_recency_decays() {
        let today = d("2026-11-04");
        let mut recent = RecentWear::default();
        recent.add(d("2026-10-20"), "x", &[snap("t1")]);
        assert_eq!(raw_penalty(&[("t1", "상의")], &recent, today), 0);
        let mut recent = RecentWear::default();
        recent.add(d("2026-10-30"), "x", &[snap("t1")]);
        assert_eq!(raw_penalty(&[("t1", "상의")], &recent, today), 1);
        assert_eq!(raw_penalty(&[("t1", "신발")], &recent, today), 0);
    }

    #[test]
    fn same_full_outfit_within_two_weeks_is_counted() {
        let today = d("2026-11-04");
        let mut recent = RecentWear::default();
        recent.add(
            d("2026-10-22"),
            &item_key(["t1", "b1"]),
            &[snap("t1"), snap("b1")],
        );
        assert_eq!(
            raw_penalty(&[("t1", "상의"), ("b1", "하의")], &recent, today),
            3
        );
    }

    #[test]
    fn note_states_the_fact_and_the_effect() {
        let today = d("2026-11-04");
        let mut recent = RecentWear::default();
        recent.add(d("2026-11-02"), "x", &[snap("t1")]);
        let with = wear_note(&["t1", "b1"], &recent, today, 2).unwrap();
        assert!(with.contains("옷 t1(11/2 입음)"), "{with}");
        assert!(with.contains("순위"));
        let shared = wear_note(&["t1"], &recent, today, 0).unwrap();
        assert!(!shared.contains("순위"), "{shared}");
        assert!(wear_note(&["b1"], &recent, today, 0).is_none());
    }
}
