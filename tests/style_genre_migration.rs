//! 장르 마이그레이션과 [`StyleGenre`] 어휘가 어긋나지 않는지 본다.
//!
//! `style_mood` 테이블은 화면에 나갈 장르 목록을 갖고, `clothing.style_mood` 는
//! 그 키로 아이템을 태깅한다. 서버는 두 값을 모두 `StyleGenre` 로 디코딩한다.
//! 그래서 마이그레이션이 넣는 키 중 하나라도 enum 에 없으면, 그 장르를 고른
//! 사용자의 요청은 행 디코딩 실패로 떨어진다 — 컴파일러도 다른 테스트도
//! 잡아주지 못하는 자리다. `style_vocab` 모듈 주석이 적어둔, 이미 두 번 난 버그와
//! 같은 종류다.
//!
//! DB 없이 도는 테스트다. 마이그레이션 SQL 을 텍스트로 읽어 키만 뽑아 검사한다.

use style_engine::models::style_vocab::StyleGenre;

/// 현재 장르 목록을 정의하는 마이그레이션.
///
/// 예전 재편 마이그레이션(`20260910000004`, `20260910000005`)이 아니라 가장 최근
/// 것을 본다. 지난 마이그레이션들은 그 시점의 키를 넣었고 그 뒤 이름이 바뀌었으므로,
/// 지금의 어휘와 대조하면 당연히 어긋난다. 목록의 현재 주인은 이 파일 하나다.
const GENRE_MIGRATION: &str =
    include_str!("../migrations/20261006000001_split_minimal_and_classic.sql");

/// 데모 시드가 넣는 무드 칩. 글램은 옷과 함께 시드로만 들어간다 — 옷 없이 칩만 운영에
/// 뜨지 않게 하려는 것이다(`seeds/demo/glam.sql` 머리 주석).
const DEMO_SEED_MOODS: &str = include_str!("../seeds/demo/glam.sql");
const EDGY_SEED_MOODS: &str = include_str!("../seeds/demo/edgy.sql");

/// `('male', 'preppy', ...` 같은 INSERT 행에서 두 번째 따옴표 값(장르 키)을 뽑는다.
fn inserted_genre_keys(sql: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for gender in ["male", "female", "unisex"] {
        // 정렬용 공백이 들어가도 읽히게 한다 — VALUES 행은 보기 좋게 칸을 맞춰 적는다.
        let marker = format!("('{gender}',");
        let mut rest = sql;
        while let Some(at) = rest.find(&marker) {
            rest = &rest[at + marker.len()..];
            let tail = rest.trim_start();
            let Some(tail) = tail.strip_prefix('\'') else {
                continue;
            };
            if let Some(end) = tail.find('\'') {
                found.push((gender.to_string(), tail[..end].to_string()));
            }
        }
    }
    found
}

/// 마이그레이션이 `style_mood` 에 넣는 키는 모두 표준 식별자여야 한다.
#[test]
fn every_inserted_mood_key_is_a_canonical_genre() {
    let male: Vec<_> = inserted_genre_keys(GENRE_MIGRATION)
        .into_iter()
        .filter(|(g, _)| g == "male")
        .collect();
    let female: Vec<_> = inserted_genre_keys(GENRE_MIGRATION)
        .into_iter()
        .filter(|(g, _)| g == "female")
        .collect();
    assert_eq!(male.len(), 8, "남성 장르는 8개여야 한다: {male:?}");
    assert_eq!(female.len(), 8, "여성 장르는 8개여야 한다: {female:?}");

    for (gender, key) in inserted_genre_keys(GENRE_MIGRATION).into_iter() {
        assert!(
            key.parse::<StyleGenre>().is_ok(),
            "{gender} 목록의 '{key}' 가 StyleGenre 에 없다. \
             이 장르를 고른 사용자의 요청은 행 디코딩에서 실패한다."
        );
    }
}

/// 마이그레이션이 옮겨 놓는 목적지 값들. 여기 오타가 나면 아이템이 아무도
/// 고를 수 없는 장르로 사라진다 — DELETE 가 아니라 UPDATE 라 조용히 남는다.
#[test]
fn migration_targets_are_canonical_genres() {
    for sql in [GENRE_MIGRATION] {
        for line in sql.lines() {
            let line = line.trim();
            if line.starts_with("--") {
                continue;
            }
            // "SET style_mood = 'x'" / "SET mood = 'x'" / "SET mood_key = 'x'"
            for marker in ["SET style_mood = '", "SET mood = '", "SET mood_key = '"] {
                if let Some(at) = line.find(marker) {
                    let rest = &line[at + marker.len()..];
                    let key = &rest[..rest.find('\'').expect("닫는 따옴표")];
                    assert!(
                        key.parse::<StyleGenre>().is_ok(),
                        "마이그레이션이 '{key}' 로 옮기는데 그건 표준 장르가 아니다"
                    );
                }
            }
        }
    }
}

/// 마이그레이션이 옮겨 내보내는 **출발** 값들은 서버도 알아들어야 한다.
///
/// 마이그레이션은 DB 를 고치지만, 예전 클라이언트가 보내는 요청 본문과 아직
/// 이전되지 않은 환경은 여전히 예전 값을 들고 온다. `from_alias` 가 그 값들을
/// 받아주지 않으면 사용자는 자기가 고른 장르에서 400 을 받는다.
#[test]
fn legacy_values_the_migration_moves_are_still_accepted_by_the_server() {
    let legacy = [
        // 남성 재편 이전
        "minimal",
        "minimal_casual",
        "streetwear",
        "street_style",
        "gorpcore",
        "outdoor",
        "athleisure",
        "sportswear",
        "sporty",
        // 여성 재편 이전
        "quiet_luxury",
        "coquette",
        "feminine_casual",
        "office_siren",
        "boho",
        "boho_revival",
        "vintage",
        "off_duty",
        "boyish",
    ];
    for raw in legacy {
        assert!(
            StyleGenre::from_alias(raw).is_some(),
            "마이그레이션이 '{raw}' 를 옮기는데 서버는 이 값을 거부한다"
        );
    }
}

/// 화면에 나가는 목록(마이그레이션 8+8, 데모 시드의 글램·엣지)과 enum 이 일치해야 한다.
/// 어느 쪽에도 노출되지 않는 장르가 남으면 그 장르로 태깅된 아이템은 영영 후보에
/// 들어가지 못한다.
#[test]
fn exposed_genres_cover_the_whole_vocabulary() {
    let mut exposed: Vec<String> = inserted_genre_keys(GENRE_MIGRATION)
        .into_iter()
        .chain(inserted_genre_keys(DEMO_SEED_MOODS))
        .chain(inserted_genre_keys(EDGY_SEED_MOODS))
        .map(|(_, key)| key)
        .collect();
    exposed.sort();
    exposed.dedup();

    let mut all: Vec<String> = StyleGenre::ALL.iter().map(|g| g.to_string()).collect();
    all.sort();

    assert_eq!(exposed, all, "노출 목록과 StyleGenre 가 어긋난다");
}

/// 남녀 공용 장르는 성별마다 행이 따로 있어야 한다. 한쪽에만 있으면 그 성별의
/// 사용자는 그 장르를 고를 수 없다.
#[test]
fn shared_genres_appear_in_both_gender_lists() {
    let male: Vec<String> = inserted_genre_keys(GENRE_MIGRATION)
        .into_iter()
        .filter(|(g, _)| g == "male")
        .map(|(_, k)| k)
        .collect();
    let female: Vec<String> = inserted_genre_keys(GENRE_MIGRATION)
        .into_iter()
        .filter(|(g, _)| g == "female")
        .map(|(_, k)| k)
        .collect();

    for shared in ["minimal", "classic", "street", "sporty_casual"] {
        assert!(
            male.contains(&shared.to_string()),
            "남성 목록에 {shared} 없음"
        );
        assert!(
            female.contains(&shared.to_string()),
            "여성 목록에 {shared} 없음"
        );
    }
}

/// 문자열에서 `'xxxxxxxx-xxxx-…'` 꼴의 옷 id 를 순서대로 뽑는다.
fn clothing_ids(sql: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = sql;
    while let Some(at) = rest.find('\'') {
        rest = &rest[at + 1..];
        let Some(end) = rest.find('\'') else { break };
        let v = &rest[..end];
        if v.len() == 36 && v.chars().filter(|c| *c == '-').count() == 4 {
            ids.push(v.to_string());
        }
        rest = &rest[end + 1..];
    }
    ids
}

/// 엣지 롤백은 시드가 엣지 태그를 붙인 기존 옷만 지운다 — 두 파일의 기존 옷 목록이 같아야
/// 한다. 목록이 어긋나면 시드 태그가 남거나, 다른 시드의 엣지 태그를 지운다.
#[test]
fn edgy_rollback_targets_exactly_the_seeded_existing_items() {
    let rollback = include_str!("../seeds/demo/edgy_rollback.sql");
    let tag_block = &EDGY_SEED_MOODS[EDGY_SEED_MOODS
        .find("SELECT id, 'edgy', 'seed' FROM clothing")
        .expect("태그 블록")..];
    let mut seeded = clothing_ids(tag_block);
    let mut rolled = clothing_ids(rollback);
    seeded.sort();
    rolled.sort();
    assert_eq!(seeded.len(), 17);
    assert_eq!(seeded, rolled);
    // 엣지 전체를 지우는 문장이 없다.
    for stmt in rollback.split(';') {
        let s = stmt.to_lowercase();
        if s.contains("style_genre = 'edgy'") {
            assert!(
                s.contains("clothing_id like '0ffd0c00") && s.contains("clothing_id in"),
                "{stmt}"
            );
        }
    }
}
