//! 스타일 메타데이터의 표준 어휘.
//!
//! 이 프로젝트는 같은 버그를 두 번 냈다. 두 번 다 원인은 "역할·톤·스타일을 문자열로
//! 비교한다"는 것이었다.
//!
//! 1. 2026-05 여성 아이템 시드가 `role`에 `base`/`accent`/`outer`를, `style`에 무드 분류를
//!    넣었다. 엔진은 `베이스`/`포인트`를 찾으므로 141건이 규칙에 투명해졌다.
//! 2. 2026-08 커밋 `67b710b`이 밥/반찬 → 베이스/포인트 이름 변경을 하면서 테스트 fixture
//!    55건을 빠뜨렸다. hard filter 정확도가 6.3%p 낮게 측정되고 있었다.
//!
//! 둘 다 컴파일러도 테스트도 잡지 못했다. `Option<String>`인 한 오타든 다른 어휘든 그냥
//! "일치하지 않음"으로 조용히 흘러가기 때문이다.
//!
//! 그래서 이 어휘들을 타입으로 승격한다. 이후로는
//!
//! - 존재하지 않는 변형을 쓰면 **컴파일이 실패한다.**
//! - DB에 표준 밖의 값이 있으면 **행 디코딩이 실패한다** (조용히 무시되지 않는다).
//! - LLM이 표준 밖의 값을 반환하면 **파싱이 실패하고 재시도된다**
//!   ([`crate::services::llm::LlmClient::chat_json`] 참고).
//!
//! 새 값을 추가하려면 여기 variant를 넣어야 하고, 그러면 이 값을 다루는 모든 `match`가
//! 컴파일 에러로 드러난다.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// 표준 어휘에 없는 값을 파싱하려 했을 때.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("'{value}'은(는) {vocabulary}의 표준 값이 아닙니다. 허용: {allowed}")]
pub struct VocabError {
    pub vocabulary: &'static str,
    pub value: String,
    pub allowed: String,
}

/// 문자열 기반 표준 어휘 enum을 정의한다.
///
/// 생성되는 것: variant, `as_str`, `ALL`, `FromStr`(엄격), `Display`,
/// serde(표준 문자열 기준), sqlx MySQL `Type`/`Decode`/`Encode`.
macro_rules! style_vocab {
    (
        $(#[$meta:meta])*
        $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident => $text:literal ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name {
            $( $(#[$vmeta])* $variant ),+
        }

        impl $name {
            /// 이 어휘의 모든 값.
            pub const ALL: &'static [$name] = &[ $( $name::$variant ),+ ];

            /// DB·API·프롬프트에서 쓰이는 표준 문자열.
            pub const fn as_str(&self) -> &'static str {
                match self {
                    $( $name::$variant => $text ),+
                }
            }

            /// 오류 메시지에 쓸 허용값 목록.
            fn allowed() -> String {
                [ $( $text ),+ ].join(" / ")
            }
        }

        impl FromStr for $name {
            type Err = VocabError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s.trim() {
                    $( $text => Ok($name::$variant), )+
                    other => Err(VocabError {
                        vocabulary: stringify!($name),
                        value: other.to_string(),
                        allowed: $name::allowed(),
                    }),
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let raw = String::deserialize(d)?;
                raw.parse().map_err(serde::de::Error::custom)
            }
        }

        // ─── sqlx (MySQL VARCHAR 컬럼) ───

        impl sqlx::Type<sqlx::MySql> for $name {
            fn type_info() -> sqlx::mysql::MySqlTypeInfo {
                <str as sqlx::Type<sqlx::MySql>>::type_info()
            }

            fn compatible(ty: &sqlx::mysql::MySqlTypeInfo) -> bool {
                <&str as sqlx::Type<sqlx::MySql>>::compatible(ty)
            }
        }

        impl<'r> sqlx::Decode<'r, sqlx::MySql> for $name {
            fn decode(
                value: sqlx::mysql::MySqlValueRef<'r>,
            ) -> Result<Self, sqlx::error::BoxDynError> {
                let raw = <&str as sqlx::Decode<sqlx::MySql>>::decode(value)?;
                // 표준 밖의 값은 조용히 넘어가지 않고 디코딩 오류가 된다.
                raw.parse::<$name>().map_err(Into::into)
            }
        }

        impl<'q> sqlx::Encode<'q, sqlx::MySql> for $name {
            fn encode_by_ref(
                &self,
                buf: &mut <sqlx::MySql as sqlx::Database>::ArgumentBuffer<'q>,
            ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
                <&str as sqlx::Encode<sqlx::MySql>>::encode(self.as_str(), buf)
            }
        }
    };
}

style_vocab! {
    /// 코디에서 아이템이 맡는 역할. 대부분의 평가 규칙이 이 구성의 균형을 본다.
    Role {
        /// 코디의 바탕이 되는 무난한 아이템.
        Base => "베이스",
        /// 존재감으로 시선을 끄는 아이템. 하나를 넘기면 산만해진다.
        Accent => "포인트",
        /// 약한 존재감의 포인트.
        SoftAccent => "약한포인트",
        /// 다른 아이템 사이를 이어주는 중간 성격.
        Connector => "연결템",
        /// 실루엣과 시각적 무게중심을 잡아주는 아이템.
        Structural => "구조템",
    }
}

style_vocab! {
    /// 전체 밝기.
    Tone {
        Bright => "밝음",
        Mid => "중간",
        Dark => "어두움",
    }
}

style_vocab! {
    /// 색상 채도.
    Saturation {
        Low => "낮음",
        Mid => "중간",
        High => "높음",
    }
}

style_vocab! {
    /// 대표 스타일. 무드(`style_mood`)와는 다른 축이다 — 이쪽은 스타일 충돌 판정에 쓰인다.
    Style {
        Basic => "베이직",
        Work => "워크",
        Military => "밀리터리",
        Formal => "포멀",
        Sport => "스포츠",
    }
}

style_vocab! {
    /// 시각적 무게감.
    Weight {
        Light => "가벼움",
        Mid => "중간",
        Heavy => "무거움",
    }
}

style_vocab! {
    /// 원단 두께. 온도 게이트가 이 값을 본다.
    ///
    /// 다른 어휘와 달리 값이 영어인 이유: 이 필드의 계약은 처음부터 `thin/medium/thick`
    /// 이었다 — 데이터 모델 문서, 등록 폼의 option value, Vision 프롬프트가 모두 그렇게
    /// 적고 있고 UI는 표시할 때만 한국어로 옮긴다. 2026-05 여성 시드가 `얇은/중간/두꺼운`
    /// 을 넣은 것이 이탈이었고, 정규화 방향은 선언된 계약 쪽이다.
    Thickness {
        Thin => "thin",
        Medium => "medium",
        Thick => "thick",
    }
}

style_vocab! {
    /// 사용자가 고르는 스타일 장르 (`clothing.style_mood`, `style_mood.mood_key`).
    ///
    /// 표시명은 여기 두지 않는다. 화면에 나가는 한국어 이름과 설명은 `style_mood`
    /// 테이블이 갖고, 이 enum 은 코드·DB·API 가 공유하는 식별자만 정의한다. 그래야
    /// 문구를 고치는 데 배포가 필요 없다.
    ///
    /// 예전 이름이나 영문 변형은 [`StyleGenre::from_alias`] 로 정규화한다.
    StyleGenre {
        // ─── 남성 · 공용 ───
        Amekaji => "amekaji",
        /// 남성/공용 "미니멀 캐주얼". 여성 쪽 [`StyleGenre::MinimalClassic`] 과는 다른 값이다.
        MinimalCasual => "minimal",
        /// 성별 공용으로 쓰인다.
        Street => "street",

        // ─── 여성 대표 장르 ───
        MinimalClassic => "minimal_classic",
        RomanticFeminine => "romantic_feminine",
        ModernChic => "modern_chic",
        Bohemian => "bohemian",
        ModelOffDuty => "model_off_duty",
        Mannish => "mannish",
        SportyCasual => "sporty_casual",
    }
}

impl StyleGenre {
    /// 바깥에서 들어온 값을 표준 식별자로 정규화한다.
    ///
    /// LLM 출력, 예전 클라이언트, 마이그레이션 이전 DB 값이 모두 여기를 지난다.
    /// 대소문자·하이픈·공백 차이를 흡수하고, 예전 이름과 영문 표기를 현재 장르로 옮긴다.
    ///
    /// 모르는 값은 `None` 이다. 조용히 기본 장르로 바꾸지 않는다 — 그렇게 하면 잘못된
    /// 장르로 추천이 나가고도 아무 신호가 남지 않는다.
    pub fn from_alias(raw: &str) -> Option<Self> {
        // "Quiet Luxury", "quiet-luxury", "quiet  luxury" 를 모두 같은 키로 만든다.
        let key: String = raw
            .trim()
            .to_ascii_lowercase()
            .chars()
            .map(|c| {
                if c == '-' || c == ' ' || c == '_' {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        let key = key
            .split('_')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("_");

        // 표준값이면 그대로.
        if let Ok(genre) = key.parse::<StyleGenre>() {
            return Some(genre);
        }

        Some(match key.as_str() {
            // 미니멀 클래식
            "quiet_luxury" | "quietluxury" | "퀴엣_럭셔리" | "콰이엇_럭셔리" => {
                StyleGenre::MinimalClassic
            }
            // 로맨틱 페미닌 — 코켓은 하위 표현으로 흡수한다.
            "coquette" | "코켓" | "feminine_casual" | "feminine" => StyleGenre::RomanticFeminine,
            // 모던 시크
            "office_siren" | "officesiren" | "오피스_사이렌" | "office" => {
                StyleGenre::ModernChic
            }
            // 보헤미안
            // 보헤미안. `vintage` 는 대표 장르가 아니지만 시드 데이터에 남아 있다 —
            // 코듀로이·스웨이드·플로럴·라탄 같은 구성이라 보헤미안으로 흡수한다.
            "boho"
            | "boho_revival"
            | "bohorevival"
            | "보호"
            | "보호_리바이벌"
            | "보헤미안"
            | "vintage"
            | "빈티지" => StyleGenre::Bohemian,
            // 모델 오프듀티
            "off_duty"
            | "offduty"
            | "model_off_duty"
            | "modeloffduty"
            | "오프듀티_모델"
            | "모델_오프듀티" => StyleGenre::ModelOffDuty,
            // 스트리트
            "streetwear" | "스트릿" | "스트리트" => StyleGenre::Street,
            // 매니시
            "boyish" | "보이시" | "매니시" => StyleGenre::Mannish,
            // 스포티 캐주얼
            "athleisure" | "sporty" | "스포티" | "스포티_캐주얼" => {
                StyleGenre::SportyCasual
            }
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_canonical_string() {
        for &r in Role::ALL {
            assert_eq!(r.as_str().parse::<Role>().unwrap(), r);
        }
        for &t in Tone::ALL {
            assert_eq!(t.as_str().parse::<Tone>().unwrap(), t);
        }
        for &s in Style::ALL {
            assert_eq!(s.as_str().parse::<Style>().unwrap(), s);
        }
        for &w in Weight::ALL {
            assert_eq!(w.as_str().parse::<Weight>().unwrap(), w);
        }
        for &s in Saturation::ALL {
            assert_eq!(s.as_str().parse::<Saturation>().unwrap(), s);
        }
        for &t in Thickness::ALL {
            assert_eq!(t.as_str().parse::<Thickness>().unwrap(), t);
        }
    }

    /// 실제로 DB를 오염시켰던 값들. 이제는 조용히 무시되지 않고 오류가 되어야 한다.
    #[test]
    fn historical_bad_values_are_rejected() {
        for bad in ["base", "accent", "outer", "밥", "반찬", "약한반찬"] {
            assert!(bad.parse::<Role>().is_err(), "{bad} 는 거부되어야 한다");
        }
        for bad in ["밝은", "어두운"] {
            assert!(bad.parse::<Tone>().is_err(), "{bad} 는 거부되어야 한다");
        }
        for bad in ["boho", "casual", "office", "street", "minimal"] {
            assert!(bad.parse::<Style>().is_err(), "{bad} 는 거부되어야 한다");
        }
        for bad in ["얇은", "중간", "두꺼운", "보통"] {
            assert!(
                bad.parse::<Thickness>().is_err(),
                "{bad} 는 거부되어야 한다"
            );
        }
    }

    /// 예전 장르명과 영문 변형이 현재 식별자로 옮겨져야 한다. 이 표가 깨지면
    /// 기존에 저장된 취향과 옷 데이터가 조용히 다른 장르로 흘러간다.
    #[test]
    fn old_genre_names_map_to_current_ones() {
        let cases = [
            ("quiet_luxury", StyleGenre::MinimalClassic),
            ("Quiet Luxury", StyleGenre::MinimalClassic),
            ("coquette", StyleGenre::RomanticFeminine),
            ("feminine_casual", StyleGenre::RomanticFeminine),
            ("office siren", StyleGenre::ModernChic),
            ("boho", StyleGenre::Bohemian),
            ("Boho-Revival", StyleGenre::Bohemian),
            ("vintage", StyleGenre::Bohemian),
            ("off_duty", StyleGenre::ModelOffDuty),
            ("model off duty", StyleGenre::ModelOffDuty),
            ("streetwear", StyleGenre::Street),
            ("boyish", StyleGenre::Mannish),
            ("athleisure", StyleGenre::SportyCasual),
        ];
        for (raw, expected) in cases {
            assert_eq!(StyleGenre::from_alias(raw), Some(expected), "{raw}");
        }
    }

    /// 표준값은 그대로 통과해야 한다 — 정규화가 이미 옳은 값을 망가뜨리면 안 된다.
    #[test]
    fn canonical_genre_ids_round_trip() {
        for genre in StyleGenre::ALL {
            assert_eq!(StyleGenre::from_alias(genre.as_str()), Some(*genre));
            assert_eq!(genre.as_str().parse::<StyleGenre>(), Ok(*genre));
        }
    }

    /// 남성/공용 `minimal` 과 여성 `minimal_classic` 은 다른 장르다.
    /// 하나로 뭉개면 남성 옷장이 여성 장르로 새어 들어간다.
    #[test]
    fn male_minimal_is_not_female_minimal_classic() {
        assert_eq!(
            StyleGenre::from_alias("minimal"),
            Some(StyleGenre::MinimalCasual)
        );
        assert_ne!(StyleGenre::MinimalCasual, StyleGenre::MinimalClassic);
    }

    /// 모르는 값을 기본 장르로 바꾸지 않는다. 그러면 사용자가 고른 장르가
    /// 무시된 채 추천이 나가고도 아무 신호가 남지 않는다.
    #[test]
    fn unknown_genre_is_rejected_not_defaulted() {
        for bad in ["y2k", "", "  ", "미니멀리즘", "quiet_luxury_2"] {
            assert_eq!(
                StyleGenre::from_alias(bad),
                None,
                "{bad} 는 거부되어야 한다"
            );
        }
    }

    #[test]
    fn error_message_lists_the_allowed_values() {
        let err = "base".parse::<Role>().unwrap_err();
        assert!(err.to_string().contains("베이스"));
        assert!(err.to_string().contains("구조템"));
    }

    #[test]
    fn whitespace_is_tolerated() {
        assert_eq!("  베이스 ".parse::<Role>().unwrap(), Role::Base);
    }

    #[test]
    fn serde_uses_canonical_strings() {
        let json = serde_json::to_string(&Role::SoftAccent).unwrap();
        assert_eq!(json, "\"약한포인트\"");
        assert_eq!(
            serde_json::from_str::<Role>("\"약한포인트\"").unwrap(),
            Role::SoftAccent
        );
        assert!(serde_json::from_str::<Role>("\"base\"").is_err());
    }
}
