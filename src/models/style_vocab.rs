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
