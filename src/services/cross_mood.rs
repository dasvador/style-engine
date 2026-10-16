//! 무드를 바꿔 다시 요청해도 같은 코디가 나오지 않게 한다.
//!
//! 홈 화면은 무드마다 따로 추천을 요청한다. 화이트 셔츠처럼 여러 장르에 들어가는 옷이
//! 있어서, 클래식에서 본 코디가 모던 시크에서 그대로 다시 나왔다. 서버는 앞서 무엇을
//! 보여 줬는지 몰랐고, 화면도 무드가 다르면 다른 코디로 여겼다.
//!
//! 기준은 "아이템이 하나 이하만 다르면 같은 코디" 다. 아이템 하나를 공유하는 것은
//! 괜찮다 — 같은 셔츠라도 실루엣·배색·소재 조합이 다르면 다른 코디다(무드 구분에 관한
//! 사용자 결정). 하나만 바뀐 코디는 사실상 같은 코디로 보인다.
//!
//! 대체 후보가 너무 나쁘면 빼지 않는다. 겹침을 피하려고 품질을 크게 내주면 안 된다 —
//! "비슷한 조합이 겹치면 품질이 비슷한 대체 후보를 고른다" 가 요구다.

use std::collections::HashSet;

use crate::models::recommendation::OutfitCandidate;

/// 겹치지 않는 후보 중 최고점이 전체 최고점보다 이만큼 넘게 낮으면 거르지 않는다.
/// 판정 등급 한 칸 정도의 폭이다.
pub const QUALITY_MARGIN: i32 = 10;

fn item_ids(c: &OutfitCandidate) -> HashSet<&str> {
    [&c.top_id, &c.bottom_id, &c.outer_id, &c.shoes_id, &c.bag_id]
        .into_iter()
        .filter_map(|id| id.as_deref())
        .collect()
}

/// 이미 보여 준 코디와 아이템이 하나 이하만 다른가.
pub fn repeats(candidate: &OutfitCandidate, shown: &[String]) -> bool {
    let ids = item_ids(candidate);
    let shown: HashSet<&str> = shown.iter().map(String::as_str).collect();
    if ids.is_empty() || shown.is_empty() {
        return false;
    }
    let shared = ids.intersection(&shown).count();
    let larger = ids.len().max(shown.len());
    // 두 벌 이상 공유해야 같은 코디로 본다 — 두 아이템짜리 코디가 한 벌만 같아도
    // 겹친다고 하지 않도록.
    shared >= 2 && shared + 1 >= larger
}

/// 이미 보여 준 코디와 겹치는 후보를 뺀다. 순서는 유지한다.
///
/// 거르지 않고 그대로 돌려주는 경우: 보여 준 코디가 없을 때, 모든 후보가 겹칠 때,
/// 겹치지 않는 후보의 최고점이 전체 최고점보다 [`QUALITY_MARGIN`] 넘게 낮을 때.
pub fn drop_repeats(
    candidates: Vec<OutfitCandidate>,
    shown_outfits: &[Vec<String>],
) -> Vec<OutfitCandidate> {
    if shown_outfits.is_empty() || candidates.is_empty() {
        return candidates;
    }
    let is_repeat = |c: &OutfitCandidate| shown_outfits.iter().any(|s| repeats(c, s));
    let best = candidates.iter().map(|c| c.final_score).max().unwrap_or(0);
    let best_fresh = candidates
        .iter()
        .filter(|c| !is_repeat(c))
        .map(|c| c.final_score)
        .max();
    match best_fresh {
        Some(f) if best - f <= QUALITY_MARGIN => {
            candidates.into_iter().filter(|c| !is_repeat(c)).collect()
        }
        _ => candidates,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(ids: [&str; 4], score: i32) -> OutfitCandidate {
        OutfitCandidate {
            top_id: Some(ids[0].into()),
            bottom_id: Some(ids[1].into()),
            outer_id: None,
            shoes_id: Some(ids[2].into()),
            bag_id: Some(ids[3].into()),
            style_score: score,
            feedback_bonus: 0,
            recency_penalty: 0,
            diversity_bonus: 0,
            final_score: score,
            ai_candidate_index: 0,
        }
    }

    fn shown(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_same_outfit_repeats() {
        assert!(repeats(
            &cand(["t", "b", "s", "g"], 90),
            &shown(&["t", "b", "s", "g"])
        ));
    }

    #[test]
    fn one_swapped_item_still_repeats() {
        assert!(repeats(
            &cand(["t", "b", "s", "g"], 90),
            &shown(&["t", "b", "s", "x"])
        ));
    }

    /// 화이트 셔츠 하나를 공유하는 것은 다른 코디다.
    #[test]
    fn sharing_one_or_two_items_is_a_different_outfit() {
        assert!(!repeats(
            &cand(["t", "b", "s", "g"], 90),
            &shown(&["t", "x", "y", "z"])
        ));
        assert!(!repeats(
            &cand(["t", "b", "s", "g"], 90),
            &shown(&["t", "b", "y", "z"])
        ));
    }

    #[test]
    fn an_added_outer_on_the_same_outfit_repeats() {
        let mut c = cand(["t", "b", "s", "g"], 90);
        c.outer_id = Some("o".into());
        assert!(repeats(&c, &shown(&["t", "b", "s", "g"])));
    }

    #[test]
    fn repeats_are_dropped_when_a_close_alternative_exists() {
        let out = drop_repeats(
            vec![
                cand(["t", "b", "s", "g"], 90),
                cand(["a", "c", "d", "e"], 84),
            ],
            &[shown(&["t", "b", "s", "g"])],
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].final_score, 84);
    }

    /// 겹침을 피하려고 품질을 크게 내주지 않는다.
    #[test]
    fn repeats_are_kept_when_the_alternative_is_much_worse() {
        let out = drop_repeats(
            vec![
                cand(["t", "b", "s", "g"], 90),
                cand(["a", "c", "d", "e"], 70),
            ],
            &[shown(&["t", "b", "s", "g"])],
        );
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn nothing_is_dropped_when_every_candidate_repeats_or_nothing_was_shown() {
        let only = vec![cand(["t", "b", "s", "g"], 90)];
        assert_eq!(
            drop_repeats(only.clone(), &[shown(&["t", "b", "s", "g"])]).len(),
            1
        );
        assert_eq!(drop_repeats(only, &[]).len(), 1);
    }
}
