//! 룩북 사진의 몸 방향·손·시선·표정(2026-10-18).
//!
//! 예전에는 자세 다섯 중 하나를 고르고, 모든 사진에 "머리나 얼굴을 만지지 않는다" 를 걸었다.
//! 손은 늘 옆에 붙고 표정은 무드 문장 하나로 고정돼, 서로 다른 코디가 같은 사람을 같은 자리에
//! 세운 사진처럼 보였다. 사용자가 윙크·입술 살짝 오므리기와 자유로운 손동작을 승인했다.
//!
//! 독립된 무작위 지시를 여러 개 붙이지 않는다. 걸으면서 손을 허리에 얹거나, 먼 곳을 보며
//! 윙크하는 식으로 서로 맞지 않는 조합이 나온다. 그래서 순서대로 고른다:
//! 1. 몸 방향(시선이 정해진다)
//! 2. 그 몸 방향과 이 착장이 받쳐 주는 손동작
//! 3. 그 시선에 맞는 표정
//!
//! - 착장이 받쳐 주지 않는 손동작은 고르지 않는다.
//!   - 아우터가 없으면 앞섶 잡기가 없다.
//!   - 주머니를 알 수 없으면 손 넣기가 없다.
//!   - 가방이 없으면 가방 잡기가 없다.
//!   - 기댈 것이 없는 장소면 기대기가 없다.
//!   - 짧은 크롭 머리면 귀 뒤로 넘기기가 없다.
//! - 입력은 이미지용 텍스트(이름 + 재질 설명)뿐이다. 판정은 낱말 기준이라, 모르면 안전한 쪽(넣지 않음)으로 간다.
//!
//! 해시: 출신(상위 32비트)·헤어(16~31)·장소(0~15)·빛(40~47)이 이미 원래 해시의 자리를 쓴다.
//! 자세·손·표정은 원래 해시에 단계마다 다른 소금을 섞은 값(`salted`)으로 고른다. 출신이나
//! 헤어와 표정이 늘 함께 묶이지 않고, 같은 착장은 늘 같은 결과다.

/// 얼굴이 카메라를 향하는 정도.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gaze {
    /// 카메라를 보거나 카메라 쪽으로 얼굴을 돌렸다 — 윙크 같은 장난스러운 표정이 읽힌다.
    Camera,
    /// 고개만 살짝 카메라 쪽 — 얼굴 일부만 보인다.
    Side,
    /// 먼 곳을 본다.
    Away,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Standing,
    Walking,
    Leaning,
}

#[derive(Debug)]
pub struct Stance {
    pub key: &'static str,
    pub text: &'static str,
    pub gaze: Gaze,
    pub motion: Motion,
}

/// 몸 방향. 걷는 자세 말고는 두 발을 디딘 채 서 있다고 못 박는다 — 몸을 튼 자세에 걸음이
/// 섞이면 두 다리가 엇갈린 그림이 나왔다(2026-10-16 사용자 지적).
pub const STANCES: [Stance; 6] = [
    Stance {
        key: "frontal",
        text: "a natural near-frontal stance, standing still with both feet planted, weight \
               on one leg, eyes resting near the camera",
        gaze: Gaze::Camera,
        motion: Motion::Standing,
    },
    Stance {
        key: "turned",
        text: "standing still, body turned twenty to thirty degrees from the camera with both \
               feet planted and pointing the same way as her hips, face turned back towards it",
        gaze: Gaze::Camera,
        motion: Motion::Standing,
    },
    Stance {
        key: "walking",
        text: "walking slowly towards the side her toes point, caught mid-step, only her head \
               turning slightly towards the camera",
        gaze: Gaze::Side,
        motion: Motion::Walking,
    },
    // 걷다가 돌아보기. 등이 카메라를 향하면 옷이 안 보인다 — 몸은 비스듬히, 앞이 보이게 둔다.
    Stance {
        key: "look_back",
        text: "walking across the frame at an easy pace and glancing back towards the camera \
               over her near shoulder, her body still angled so the front of the outfit stays \
               in view",
        gaze: Gaze::Camera,
        motion: Motion::Walking,
    },
    Stance {
        key: "looking_off",
        text: "standing still for a moment with both feet planted, looking off down the street",
        gaze: Gaze::Away,
        motion: Motion::Standing,
    },
    Stance {
        key: "leaning",
        text: "leaning lightly with one shoulder against the wall or railing beside her, the \
               clothes neither pressed flat nor hidden",
        gaze: Gaze::Camera,
        motion: Motion::Leaning,
    },
];

/// 손동작.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hand {
    /// 두 팔을 편하게 내린다. 차렷이 아니라 팔꿈치가 살짝 굽은 상태.
    Loose,
    /// 걸음에 맞춰 팔이 자연스럽게 흔들린다(걷는 자세만).
    Swing,
    Pocket,
    /// 머리카락을 귀 뒤로 넘기거나 바람에 날린 머리를 정리한다.
    Hair,
    /// 아우터 앞섶·깃을 가볍게 잡는다.
    Lapel,
    /// 한 손을 허리에, 체중은 한쪽 다리에(서 있는 자세만).
    Hip,
    /// 목록의 가방을 가볍게 잡는다.
    Bag,
}

impl Hand {
    pub fn key(self) -> &'static str {
        match self {
            Hand::Loose => "loose",
            Hand::Swing => "swing",
            Hand::Pocket => "pocket",
            Hand::Hair => "hair",
            Hand::Lapel => "lapel",
            Hand::Hip => "hip",
            Hand::Bag => "bag",
        }
    }
}

/// 이 착장·장소·헤어가 받쳐 주는 손동작의 근거. 입력 텍스트에서 낱말로만 판정한다.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Support {
    /// 주머니가 있는 옷의 이름("jeans", "trousers", "coat" …). 모르면 `None`.
    pub pocket_on: Option<&'static str>,
    /// 앞섶을 잡을 수 있는 아우터의 이름("jacket", "coat" …). 아우터가 없으면 `None`.
    pub outer: Option<&'static str>,
    /// 가방: 클러치면 `Some(true)`, 끈·손잡이가 있으면 `Some(false)`, 없으면 `None`.
    pub bag_is_clutch: Option<bool>,
    pub lean: bool,
    pub hair_tuck: bool,
}

fn has_any(text: &str, words: &[&str]) -> bool {
    let lower = text.to_lowercase();
    words.iter().any(|w| lower.contains(w))
}

/// 아우터 종류. 목록에 없는 종류면 "outer layer" 로 둔다.
fn outer_noun(desc: &str) -> &'static str {
    let table: [(&[&str], &str); 7] = [
        (&["blazer", "블레이저"], "blazer"),
        (&["trench", "트렌치"], "trench coat"),
        (&["cardigan", "가디건", "카디건"], "cardigan"),
        (&["parka", "파카", "anorak", "아노락"], "parka"),
        (&["coat", "코트"], "coat"),
        (
            &[
                "jacket",
                "재킷",
                "자켓",
                "점퍼",
                "jumper",
                "블루종",
                "blouson",
            ],
            "jacket",
        ),
        (&["vest", "베스트", "조끼", "gilet"], "vest"),
    ];
    table
        .iter()
        .find(|(ws, _)| has_any(desc, ws))
        .map_or("outer layer", |(_, n)| n)
}

/// 슬롯별 이미지 설명(`("pants", "블랙 와이드 슬랙스 (polyester, wide leg)")` …), 헤어, 기댈 곳이
/// 있는 장소인지로 지원 조건을 정한다.
///
/// 주머니는 설명에 pocket/주머니/포켓이 있거나, 하의가 보통 주머니가 있는 종류(데님·청바지·
/// 치노·카고·슬랙스·trousers)일 때만 있다고 본다. 스커트·레깅스·이름만 "팬츠" 인 하의는 모른다.
pub fn support(slots: &[(String, String)], hair: &str, lean: bool) -> Support {
    let get = |slot: &str| {
        slots
            .iter()
            .find(|(s, _)| s == slot)
            .map(|(_, d)| d.as_str())
    };
    let explicit = ["pocket", "주머니", "포켓"];
    // 가슴 주머니에는 손을 넣지 않는다 — 손이 상의 앞을 가린다(트러커 재킷의 chest pockets).
    let hand_pocket = |d: &str| {
        let rest = d
            .to_lowercase()
            .replace("chest pockets", "")
            .replace("chest pocket", "")
            .replace("가슴 주머니", "")
            .replace("가슴 포켓", "");
        has_any(&rest, &explicit)
    };
    let pocket_on = match (get("pants"), get("outerwear")) {
        (Some(p), _) if hand_pocket(p) => Some(pants_noun(p)),
        (_, Some(o)) if hand_pocket(o) => Some(outer_noun(o)),
        (Some(p), _)
            if has_any(
                p,
                &[
                    "denim",
                    "데님",
                    "jeans",
                    "청바지",
                    "chino",
                    "치노",
                    "cargo",
                    "카고",
                    "slacks",
                    "슬랙스",
                    "trousers",
                ],
            ) && !has_any(p, &["skirt", "스커트", "legging", "레깅스"]) =>
        {
            Some(pants_noun(p))
        }
        _ => None,
    };
    let bag_is_clutch = get("bag").map(|b| has_any(b, &["clutch", "클러치"]));
    Support {
        pocket_on,
        outer: get("outerwear").map(outer_noun),
        bag_is_clutch,
        lean,
        hair_tuck: !has_any(hair, &["crop", "pixie", "buzz"]),
    }
}

fn pants_noun(desc: &str) -> &'static str {
    if has_any(desc, &["skirt", "스커트"]) {
        "skirt"
    } else if has_any(desc, &["denim", "데님", "jeans", "청바지"]) {
        "jeans"
    } else {
        "trousers"
    }
}

/// 이 몸 방향에서 고를 수 있는 손동작과 가중치. 두 팔을 내린 자세는 가중치를 낮게 둔다 —
/// 모든 사진이 차렷이 되지 않게.
pub fn hands_for(stance: &Stance, s: &Support) -> Vec<(Hand, u32)> {
    let mut v: Vec<(Hand, u32)> = Vec::new();
    match stance.motion {
        Motion::Walking => v.push((Hand::Swing, 3)),
        Motion::Standing | Motion::Leaning => v.push((Hand::Loose, 1)),
    }
    if s.pocket_on.is_some() {
        v.push((Hand::Pocket, 2));
    }
    if s.hair_tuck {
        v.push((Hand::Hair, 2));
    }
    if s.outer.is_some() {
        v.push((Hand::Lapel, 2));
    }
    if stance.motion == Motion::Standing {
        v.push((Hand::Hip, 2));
    }
    if s.bag_is_clutch.is_some() && stance.motion != Motion::Leaning {
        v.push((Hand::Bag, 1));
    }
    v
}

/// 손동작 문장. 한 팔만 말하면 다른 팔을 모델이 정하다가 꼬이므로 두 팔을 다 적는다.
pub fn hand_text(hand: Hand, stance: &Stance, s: &Support) -> String {
    let walking = stance.motion == Motion::Walking;
    match hand {
        Hand::Loose => "her arms hang loosely with a slight bend at the elbows, hands relaxed \
                        and not pressed to her sides"
            .to_string(),
        Hand::Swing => "her arms swing naturally with her stride".to_string(),
        Hand::Pocket => format!(
            "one hand slipped casually into a pocket of her {}, the other arm {}",
            s.pocket_on.unwrap_or("trousers"),
            if walking {
                "swinging easily"
            } else {
                "hanging relaxed"
            }
        ),
        Hand::Hair => {
            if walking {
                "a light breeze in her hair, one hand brushing a few strands back from her face \
                 without hiding it, the other arm moving with her step"
                    .to_string()
            } else {
                "one hand tucking a loose strand of hair behind her ear, her face still fully \
                 visible, the other arm relaxed"
                    .to_string()
            }
        }
        Hand::Lapel => format!(
            "one hand lightly holding the front edge of her {} near the waist, the other arm \
             relaxed",
            s.outer.unwrap_or("outer layer")
        ),
        Hand::Hip => "one hand resting on her hip with her weight shifted onto one leg, the \
                      other arm relaxed"
            .to_string(),
        Hand::Bag => match s.bag_is_clutch {
            Some(true) => "one hand holding her clutch loosely at hip height, the other arm \
                           relaxed"
                .to_string(),
            _ => "one hand resting lightly on the strap or handle of her bag at hip height, \
                  the other arm relaxed"
                .to_string(),
        },
    }
}

#[derive(Debug)]
pub struct Expression {
    pub key: &'static str,
    pub text: &'static str,
    pub playful: bool,
    /// 얼굴이 카메라를 향해야 읽히는 표정.
    pub needs_camera: bool,
    /// 손이 얼굴 가까이 있으면 쓰지 않는다(윙크가 손에 가린다).
    pub needs_clear_face: bool,
}

pub const EXPRESSIONS: [Expression; 7] = [
    Expression {
        key: "easy_smile",
        text: "a relaxed, easy smile",
        playful: false,
        needs_camera: false,
        needs_clear_face: false,
    },
    Expression {
        key: "half_laugh",
        text: "a small, genuine half-laugh, lips just parted",
        playful: false,
        needs_camera: false,
        needs_clear_face: false,
    },
    Expression {
        key: "laugh_beginning",
        text: "caught just as a laugh begins — her eyes creasing a little, the smile not yet full",
        playful: false,
        needs_camera: false,
        needs_clear_face: false,
    },
    Expression {
        key: "sidelong_smile",
        text: "a sidelong glance towards the camera with a quiet smile",
        playful: false,
        needs_camera: false,
        needs_clear_face: false,
    },
    Expression {
        key: "calm",
        text: "a calm, unhurried look",
        playful: false,
        needs_camera: false,
        needs_clear_face: false,
    },
    Expression {
        key: "wink",
        text: "a light, quick wink at the camera with a half smile — brief and natural, one eye \
               only",
        playful: true,
        needs_camera: true,
        needs_clear_face: true,
    },
    Expression {
        key: "soft_purse",
        text: "her lips softly pursed for a moment, as if about to say something, her eyes \
               amused — a fleeting expression rather than a pout",
        playful: true,
        needs_camera: true,
        needs_clear_face: false,
    },
];

/// 장난스러운 표정의 비중. 팝·엣지·스포티는 조금 더 자주, 미니멀·클래식·매니시는 자연스러운
/// 표정 중심이다.
fn playful_weight(genre: crate::models::style_vocab::StyleGenre) -> u32 {
    use crate::models::style_vocab::StyleGenre as G;
    match genre {
        G::Pop | G::Edgy | G::SportyCasual => 4,
        G::Minimal | G::Classic | G::Mannish => 1,
        _ => 2,
    }
}

pub fn expressions_for(
    genre: crate::models::style_vocab::StyleGenre,
    stance: &Stance,
    hand: Hand,
) -> Vec<(&'static Expression, u32)> {
    EXPRESSIONS
        .iter()
        .filter(|e| !e.needs_camera || stance.gaze == Gaze::Camera)
        // 곁눈질은 카메라 쪽을 보는 표정이라 먼 곳을 보는 자세와 맞지 않는다.
        .filter(|e| e.key != "sidelong_smile" || stance.gaze != Gaze::Away)
        .filter(|e| !e.needs_clear_face || hand != Hand::Hair)
        .map(|e| {
            let w = if e.playful {
                playful_weight(genre)
            } else if e.key == "calm" {
                4
            } else {
                5
            };
            (e, w)
        })
        .collect()
}

/// 원래 해시에 단계별 소금을 섞는다(splitmix64). 같은 입력은 늘 같은 값이다.
pub fn salted(hash: u64, salt: u64) -> u64 {
    let mut z = hash ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

const SALT_STANCE: u64 = 0x5057_0001;
const SALT_HAND: u64 = 0x5057_0002;
const SALT_EXPRESSION: u64 = 0x5057_0003;

fn weighted<T: Copy>(choices: &[(T, u32)], h: u64) -> T {
    let total: u64 = choices.iter().map(|(_, w)| *w as u64).sum();
    let mut slot = h % total;
    for (c, w) in choices {
        if slot < *w as u64 {
            return *c;
        }
        slot -= *w as u64;
    }
    choices[0].0
}

/// 고른 자세·손·표정.
#[derive(Debug)]
pub struct PhotoPose {
    pub stance: &'static Stance,
    pub hand: Hand,
    pub expression: &'static Expression,
    pub pose_line: String,
}

pub fn pick(
    genre: crate::models::style_vocab::StyleGenre,
    hash: u64,
    support: &Support,
) -> PhotoPose {
    let stances: Vec<(&'static Stance, u32)> = STANCES
        .iter()
        .filter(|s| s.motion != Motion::Leaning || support.lean)
        .map(|s| (s, 1))
        .collect();
    let stance = weighted(&stances, salted(hash, SALT_STANCE));
    let hand = weighted(&hands_for(stance, support), salted(hash, SALT_HAND));
    let expression = weighted(
        &expressions_for(genre, stance, hand),
        salted(hash, SALT_EXPRESSION),
    );
    PhotoPose {
        stance,
        hand,
        expression,
        pose_line: format!("{}; {}", stance.text, hand_text(hand, stance, support)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::style_vocab::StyleGenre;
    use std::collections::HashMap;

    fn slots(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    fn hashes(n: u64) -> impl Iterator<Item = u64> {
        (0..n).map(|i| i.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (i << 7))
    }

    /// 근거가 있을 때만 그 손동작이 있다.
    #[test]
    fn support_follows_the_listed_items() {
        let s = support(
            &slots(&[
                ("top/inner", "화이트 티 (cotton jersey)"),
                ("pants", "인디고 와이드 데님 (denim, wide leg)"),
                (
                    "outerwear",
                    "블랙 레더 라이더 자켓 (leather, cropped biker)",
                ),
                ("bag", "블랙 미니 숄더백 (leather)"),
            ]),
            "long straight hair worn down",
            true,
        );
        assert_eq!(s.pocket_on, Some("jeans"));
        assert_eq!(s.outer, Some("jacket"));
        assert_eq!(s.bag_is_clutch, Some(false));
        assert!(s.hair_tuck && s.lean);

        // 스커트·주머니 언급 없는 "팬츠"·레깅스는 주머니를 모른다. 아우터·가방이 없다.
        for bottom in [
            "샴페인 새틴 슬립 미디스커트 (satin, bias cut)",
            "코발트 와이드 팬츠 (crepe, wide leg)",
            "블랙 레깅스 (stretch jersey)",
        ] {
            let s = support(
                &slots(&[("top/inner", "블랙 리브 탱크탑 (rib)"), ("pants", bottom)]),
                "short textured crop",
                false,
            );
            assert_eq!(s.pocket_on, None, "{bottom}");
            assert_eq!(s.outer, None);
            assert_eq!(s.bag_is_clutch, None);
            assert!(!s.hair_tuck && !s.lean);
        }
        // 설명에 주머니가 적혀 있으면 있다.
        let s = support(
            &slots(&[("pants", "올리브 미니스커트 (cotton, patch pockets)")]),
            "bob",
            false,
        );
        assert_eq!(s.pocket_on, Some("skirt"));
        // 가슴 주머니뿐이면 손을 넣을 주머니가 아니다.
        let s = support(
            &slots(&[
                ("pants", "체리레드 A라인 미디 스커트 (cotton twill, a-line)"),
                (
                    "outerwear",
                    "레드 크롭 트러커 재킷 (cotton twill, chest pockets)",
                ),
            ]),
            "bob",
            false,
        );
        assert_eq!(s.pocket_on, None);
        let s = support(
            &slots(&[("outerwear", "카멜 코트 (wool, chest pocket, side pockets)")]),
            "bob",
            false,
        );
        assert_eq!(s.pocket_on, Some("coat"));
        let s = support(
            &slots(&[("bag", "블랙 새틴 미니 클러치 (satin)")]),
            "bob",
            false,
        );
        assert_eq!(s.bag_is_clutch, Some(true));
    }

    /// 아무 근거도 없는 착장·장소에서도 고른 손동작은 늘 받쳐 주는 것뿐이고, 차렷만 남지 않는다.
    #[test]
    fn unsupported_gestures_are_never_picked() {
        let bare = Support::default();
        for g in StyleGenre::ALL {
            for h in hashes(5000) {
                let p = pick(*g, h, &bare);
                assert!(
                    !matches!(p.hand, Hand::Pocket | Hand::Lapel | Hand::Hair | Hand::Bag),
                    "{g} {h}: {:?}",
                    p.hand
                );
                assert_ne!(p.stance.motion, Motion::Leaning);
                assert!(!p.pose_line.contains("pocket"));
                assert!(!p.pose_line.contains("front edge"));
                assert!(!p.pose_line.contains("bag"));
                assert!(!p.pose_line.contains("clutch"));
                assert!(!p.pose_line.contains("leaning"));
                assert!(!p.pose_line.contains("hair"));
                // 걷는데 허리에 손을 얹지 않는다.
                if p.stance.motion == Motion::Walking {
                    assert_ne!(p.hand, Hand::Hip);
                }
            }
        }
        let loose = hashes(5000)
            .filter(|&h| pick(StyleGenre::Minimal, h, &bare).hand == Hand::Loose)
            .count();
        assert!(loose < 2500, "두 팔을 내린 자세가 절반을 넘는다: {loose}");
    }

    /// 윙크·입술 오므리기는 얼굴이 카메라를 향한 자세에서만, 윙크는 손이 얼굴 곁에 없을 때만
    /// 고른다. 먼 곳을 보는 자세에 곁눈질 미소가 붙지 않는다.
    #[test]
    fn expressions_match_the_gaze_and_hands() {
        let full = Support {
            pocket_on: Some("jeans"),
            outer: Some("coat"),
            bag_is_clutch: Some(false),
            lean: true,
            hair_tuck: true,
        };
        for g in StyleGenre::ALL {
            for h in hashes(5000) {
                let p = pick(*g, h, &full);
                if p.expression.playful {
                    assert_eq!(p.stance.gaze, Gaze::Camera, "{g} {h}");
                }
                if p.expression.key == "wink" {
                    assert_ne!(p.hand, Hand::Hair, "{g} {h}");
                }
                if p.stance.gaze == Gaze::Away {
                    assert_ne!(p.expression.key, "sidelong_smile");
                }
            }
        }
    }

    /// 대규모 결정론 분포: 팝·엣지·스포티는 장난스러운 표정이 더 잦고, 미니멀·클래식·매니시는
    /// 자연스러운 표정 중심이다. 모든 표정·손동작·몸 방향이 실제로 나온다. 같은 해시는 같은 결과다.
    /// (프롬프트 분포일 뿐 이미지가 그렇게 나온다는 보장은 아니다.)
    #[test]
    fn distribution_by_mood() {
        let full = Support {
            pocket_on: Some("trousers"),
            outer: Some("jacket"),
            bag_is_clutch: Some(false),
            lean: true,
            hair_tuck: true,
        };
        let n = 20_000u64;
        let rate = |g: StyleGenre| {
            hashes(n)
                .filter(|&h| pick(g, h, &full).expression.playful)
                .count() as f64
                / n as f64
        };
        let high: Vec<f64> = [StyleGenre::Pop, StyleGenre::Edgy, StyleGenre::SportyCasual]
            .map(rate)
            .to_vec();
        let low: Vec<f64> = [
            StyleGenre::Minimal,
            StyleGenre::Classic,
            StyleGenre::Mannish,
        ]
        .map(rate)
        .to_vec();
        let mid = rate(StyleGenre::Romantic);
        for (&hi, &lo) in high.iter().zip(&low) {
            assert!(
                hi > mid && mid > lo,
                "high {hi:.3} mid {mid:.3} low {lo:.3}"
            );
        }
        assert!(high.iter().all(|&r| (0.12..0.35).contains(&r)), "{high:?}");
        assert!(low.iter().all(|&r| r < 0.10), "{low:?}");

        let mut stances: HashMap<&str, u64> = HashMap::new();
        let mut hands: HashMap<&str, u64> = HashMap::new();
        let mut exprs: HashMap<&str, u64> = HashMap::new();
        for h in hashes(n) {
            let p = pick(StyleGenre::Pop, h, &full);
            *stances.entry(p.stance.key).or_default() += 1;
            *hands.entry(p.hand.key()).or_default() += 1;
            *exprs.entry(p.expression.key).or_default() += 1;
            assert_eq!(p.pose_line, pick(StyleGenre::Pop, h, &full).pose_line);
        }
        assert_eq!(stances.len(), STANCES.len(), "{stances:?}");
        assert_eq!(hands.len(), 7, "{hands:?}");
        assert_eq!(exprs.len(), EXPRESSIONS.len(), "{exprs:?}");
        // 어느 하나가 절반을 넘지 않는다.
        for m in [&stances, &hands, &exprs] {
            assert!(m.values().all(|&c| c < n / 2), "{m:?}");
        }
    }

    /// 출신·헤어가 쓰는 원래 해시 자리와 표정이 묶이지 않는다: 출신 자리(상위 32비트)와 헤어
    /// 자리(16~31비트) 값마다 장난스러운 표정 비율이 전체와 비슷하다.
    #[test]
    fn expressions_are_independent_of_origin_and_hair_bits() {
        let full = Support {
            pocket_on: Some("jeans"),
            outer: Some("coat"),
            bag_is_clutch: Some(false),
            lean: true,
            hair_tuck: true,
        };
        let all: Vec<u64> = hashes(40_000).collect();
        let playful = |h: u64| pick(StyleGenre::Pop, h, &full).expression.playful;
        let overall = all.iter().filter(|&&h| playful(h)).count() as f64 / all.len() as f64;
        for (name, bucket) in [
            (
                "origin",
                Box::new(|h: u64| (h >> 32) % 8) as Box<dyn Fn(u64) -> u64>,
            ),
            ("hair", Box::new(|h: u64| ((h >> 16) & 0xFFFF) % 8)),
            ("old_pose", Box::new(|h: u64| ((h >> 48) & 0xFF) % 5)),
        ] {
            let mut by: HashMap<u64, (u64, u64)> = HashMap::new();
            for &h in &all {
                let e = by.entry(bucket(h)).or_default();
                e.0 += 1;
                e.1 += playful(h) as u64;
            }
            for (b, (n, p)) in by {
                let r = p as f64 / n as f64;
                assert!(
                    (r - overall).abs() < 0.03,
                    "{name} {b}: {r:.3} vs {overall:.3}"
                );
            }
        }
    }
}
