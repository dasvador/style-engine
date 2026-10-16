//! 오늘 입기에 "실패" 인 후보를 추천 자리에서 뺀다.
//!
//! 운영 엔진의 점수는 기온·상황을 감점 하나로만 반영해서, 점수가 높으면 오늘 입을 수
//! 없는 코디도 1위가 된다. 2026-10-11 실제 요청 23개 중 3개가 그랬다 — 출근에 후드티 +
//! 스웻팬츠, 출근에 차콜 후드티 + 데님, 출근에 반팔 티 + 치노.
//!
//! 판정은 `serving_ranker::compute_today_fit` 을 빌려 쓴다(shadow 엔진의 기온·상황
//! 게이트). 순위는 건드리지 않는다 — shadow 엔진 전체를 옮기는 것은 블라인드 비교에서
//! 운영 9 : shadow 6 으로 기각됐고, 여기서 가져오는 것은 "실패를 내보내지 않는다" 한
//! 가지다.
//!
//! 후보가 전부 실패면 그대로 둔다. 빈 추천보다 경계 넘은 추천이 낫다 — shadow 엔진이
//! 후보를 모두 탈락시켜 아무것도 못 고른 요청이 같은 측정에서 2개 있었다.

/// `is_unfit` 이 참인 후보를 뺀다. 남는 후보가 없으면 원래 목록을 그대로 돌려준다.
/// 두 번째 값은 뺀 후보 수다(그대로 돌려준 경우 0).
pub fn keep_wearable<T>(candidates: Vec<T>, is_unfit: impl Fn(&T) -> bool) -> (Vec<T>, usize) {
    let total = candidates.len();
    let wearable: Vec<usize> = (0..total).filter(|&i| !is_unfit(&candidates[i])).collect();
    if wearable.is_empty() || wearable.len() == total {
        return (candidates, 0);
    }
    let dropped = total - wearable.len();
    let kept = candidates
        .into_iter()
        .enumerate()
        .filter(|(i, _)| wearable.contains(i))
        .map(|(_, c)| c)
        .collect();
    (kept, dropped)
}

/// `is_unfit` 이 참인 후보를 뺀다. 전부 빠지면 빈 목록이다 — 날씨 실패를 "후보가 다
/// 떨어졌다" 는 이유로 정상 추천에 되돌리지 않는다(2026-10-17 사용자 결정). 남은 후보가
/// 없으면 호출자가 제한된 보완을 시도하고, 그래도 없으면 후보 부족으로 알린다.
pub fn keep_wearable_strict<T>(
    candidates: Vec<T>,
    is_unfit: impl Fn(&T) -> bool,
) -> (Vec<T>, usize) {
    let total = candidates.len();
    let kept: Vec<T> = candidates.into_iter().filter(|c| !is_unfit(c)).collect();
    let dropped = total - kept.len();
    (kept, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unfit_candidate_is_dropped() {
        let (kept, dropped) = keep_wearable(vec![1, 2, 3], |&c| c == 1);
        assert_eq!(kept, vec![2, 3]);
        assert_eq!(dropped, 1);
    }

    /// 전부 실패면 빈 추천 대신 원래 후보를 그대로 쓴다.
    #[test]
    fn all_unfit_keeps_everything() {
        let (kept, dropped) = keep_wearable(vec![1, 2], |_| true);
        assert_eq!(kept, vec![1, 2]);
        assert_eq!(dropped, 0);
    }

    /// 날씨 판정은 엄격하다 — 전부 실패면 아무것도 남기지 않는다.
    #[test]
    fn strict_gate_returns_nothing_when_all_fail() {
        let (kept, dropped) = keep_wearable_strict(vec![1, 2], |_| true);
        assert!(kept.is_empty());
        assert_eq!(dropped, 2);
    }

    #[test]
    fn order_is_kept() {
        let (kept, _) = keep_wearable(vec![5, 4, 3, 2, 1], |&c| c % 2 == 0);
        assert_eq!(kept, vec![5, 3, 1]);
    }
}
