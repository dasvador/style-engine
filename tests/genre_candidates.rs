//! 화면에 뜨는 장르는 전부 코디를 만들 수 있어야 한다.
//!
//! 장르 칩은 `style_mood` 테이블이 만들고, 후보는 `clothing_repo::
//! list_clothing_filtered` 가 가져온다. 이 둘은 서로를 모른다. 그래서 목록에만
//! 있고 후보가 0건인 장르가 생겨도 아무 곳에서도 오류가 나지 않는다 — 사용자가
//! 칩을 눌렀을 때 빈 화면이나 실패한 추천으로만 드러난다.
//!
//! 실제로 그런 상태였다. 남성 장르는 `amekaji` 하나에 140벌이 전부 몰려 있었고
//! `minimal` 과 `street` 칩은 0건이었다.
//!
//! DB 가 필요한 테스트다. `TEST_DATABASE_URL` 이 없으면 건너뛴다.

use sqlx::mysql::MySqlPoolOptions;
use style_engine::db::clothing_repo;
use style_engine::models::style_vocab::StyleGenre;

/// 코디 한 벌을 만들려면 최소한 이 세 가지가 있어야 한다.
const REQUIRED_CATEGORIES: [&str; 3] = ["상의", "하의", "신발"];

/// 한 칸에 이보다 적으면 추천이 매번 같은 아이템을 내놓는다.
const MIN_PER_CATEGORY: usize = 2;

fn database_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL")
        .ok()
        .filter(|s| !s.is_empty())
}

/// `style_mood` 에 실제로 올라가 있는 (성별, 장르) 조합을 그대로 읽어 온다.
/// 목록을 테스트에 다시 적지 않는다 — 그러면 목록이 바뀌었을 때 테스트가
/// 같이 낡는다.
async fn exposed_genres(pool: &sqlx::MySqlPool) -> Vec<(String, StyleGenre)> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT gender, mood_key FROM style_mood ORDER BY gender, sort_order")
            .fetch_all(pool)
            .await
            .expect("style_mood 조회");

    rows.into_iter()
        .map(|(gender, key)| {
            let genre = StyleGenre::from_alias(&key)
                .unwrap_or_else(|| panic!("'{key}' 는 표준 장르가 아니다 ({gender})"));
            (gender, genre)
        })
        .collect()
}

#[tokio::test]
async fn every_exposed_genre_can_build_an_outfit() {
    let Some(url) = database_url() else {
        eprintln!("TEST_DATABASE_URL 이 없어 건너뜁니다");
        return;
    };
    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("DB 연결");

    let mut empty = Vec::new();
    let mut thin = Vec::new();

    for (gender, genre) in exposed_genres(&pool).await {
        let items = clothing_repo::list_clothing_filtered(&pool, Some(&gender), Some(genre))
            .await
            .expect("후보 조회");

        for required in REQUIRED_CATEGORIES {
            let n = items.iter().filter(|c| c.category == required).count();
            let label = format!("{gender}/{genre} {required}");
            if n == 0 {
                empty.push(label);
            } else if n < MIN_PER_CATEGORY {
                thin.push(format!("{label} = {n}"));
            }
        }
    }

    assert!(
        empty.is_empty(),
        "후보가 0건인 장르 칩이 있다 — 누르면 추천이 만들어지지 않는다: {empty:?}"
    );
    assert!(
        thin.is_empty(),
        "후보가 {MIN_PER_CATEGORY}벌 미만인 칸이 있다 — 추천이 매번 같아진다: {thin:?}"
    );
}

/// 장르를 지정하지 않으면 성별 옷장 전체가 후보여야 한다. 조건 절을 넣다가
/// 실수로 필터가 항상 걸리면 여기서 잡힌다.
#[tokio::test]
async fn no_genre_means_the_whole_wardrobe() {
    let Some(url) = database_url() else {
        eprintln!("TEST_DATABASE_URL 이 없어 건너뜁니다");
        return;
    };
    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("DB 연결");

    for gender in ["male", "female"] {
        let all = clothing_repo::list_clothing_filtered(&pool, Some(gender), None)
            .await
            .expect("후보 조회");
        assert!(!all.is_empty(), "{gender} 옷장이 비어 있다");

        // 어떤 장르를 골라도 옷장 전체보다 많아질 수는 없다.
        for (g, genre) in exposed_genres(&pool).await {
            if g != gender {
                continue;
            }
            let filtered = clothing_repo::list_clothing_filtered(&pool, Some(gender), Some(genre))
                .await
                .expect("후보 조회");
            assert!(
                filtered.len() <= all.len(),
                "{gender}/{genre} 후보({})가 옷장 전체({})보다 많다",
                filtered.len(),
                all.len()
            );
        }
    }
}
