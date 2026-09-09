use sqlx::MySqlPool;
use uuid::Uuid;

use crate::models::clothing::Clothing;
use crate::models::style_vocab::{Role, Saturation, Style, StyleGenre, Thickness, Tone, Weight};

const SELECT_COLS: &str = "id, name, category, gender, style_mood, color, thickness, image_url, tone, saturation, style, weight, role, color_temperature, versatility, statement_level, formality_level, visual_weight, texture_depth, visual_weight_v2, texture_depth_v2, grounding_score, shadow_tone, silhouette_volume, material_primary, sub_category, floating_score, strong_style_score, texture_keywords, created_at, updated_at";

#[allow(clippy::too_many_arguments)] // 테이블 컬럼을 그대로 받는 저장 함수
pub async fn insert_clothing(
    pool: &MySqlPool,
    name: &str,
    category: &str,
    color: Option<&str>,
    thickness: Thickness,
    image_url: Option<&str>,
    tone: Option<Tone>,
    saturation: Option<Saturation>,
    style: Option<Style>,
    weight: Option<Weight>,
    role: Option<Role>,
    color_temperature: Option<&str>,
    versatility: Option<&str>,
    statement_level: Option<i8>,
    formality_level: Option<i8>,
) -> Result<Clothing, sqlx::Error> {
    let id = Uuid::new_v4().to_string();

    sqlx::query(
        "INSERT INTO clothing (id, name, category, color, thickness, image_url, tone, saturation, style, weight, role, color_temperature, versatility, statement_level, formality_level) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(name)
    .bind(category)
    .bind(color)
    .bind(thickness)
    .bind(image_url)
    .bind(tone)
    .bind(saturation)
    .bind(style)
    .bind(weight)
    .bind(role)
    .bind(color_temperature)
    .bind(versatility)
    .bind(statement_level)
    .bind(formality_level)
    .execute(pool)
    .await?;

    sqlx::query_as::<_, Clothing>(&format!(
        "SELECT {} FROM clothing WHERE id = ?",
        SELECT_COLS
    ))
    .bind(&id)
    .fetch_one(pool)
    .await
}

pub async fn insert_seasons(
    pool: &MySqlPool,
    clothing_id: &str,
    seasons: &[String],
) -> Result<(), sqlx::Error> {
    for season in seasons {
        sqlx::query("INSERT IGNORE INTO clothing_season (clothing_id, season) VALUES (?, ?)")
            .bind(clothing_id)
            .bind(season)
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub async fn get_seasons(pool: &MySqlPool, clothing_id: &str) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT season FROM clothing_season WHERE clothing_id = ?")
            .bind(clothing_id)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

pub async fn insert_texture_worlds(
    pool: &MySqlPool,
    clothing_id: &str,
    worlds: &[String],
) -> Result<(), sqlx::Error> {
    for w in worlds {
        sqlx::query(
            "INSERT IGNORE INTO clothing_texture_world (clothing_id, texture_world) VALUES (?, ?)",
        )
        .bind(clothing_id)
        .bind(w)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn get_texture_worlds(
    pool: &MySqlPool,
    clothing_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT texture_world FROM clothing_texture_world WHERE clothing_id = ?")
            .bind(clothing_id)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

pub async fn delete_texture_worlds(pool: &MySqlPool, clothing_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM clothing_texture_world WHERE clothing_id = ?")
        .bind(clothing_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_clothing(pool: &MySqlPool) -> Result<Vec<Clothing>, sqlx::Error> {
    sqlx::query_as::<_, Clothing>(&format!(
        "SELECT {} FROM clothing ORDER BY created_at DESC",
        SELECT_COLS
    ))
    .fetch_all(pool)
    .await
}

/// 장르를 아이템 속성으로 표현한 조건.
///
/// **장르가 지정되지 않은 아이템**(`style_mood IS NULL`)을 어느 장르의 후보로
/// 볼지 정한다. 조건은 전부 이미 채워져 있는 컬럼만 쓴다 — `style`,
/// `sub_category`, `material_primary`, `formality_level`, `saturation`.
/// 새 속성을 만들어 넣지 않는다.
///
/// 왜 태그가 아니라 조건인가: 등록 경로(`create_clothing`)가 `style_mood` 를
/// 쓰지 않아 새로 올라오는 아이템은 전부 미분류다. 태그를 심어두는 방식은 이미
/// 있는 데이터에만 통하고 앞으로 들어오는 아이템에는 통하지 않는다 — 같은
/// 구멍이 다시 생긴다. 등록 시 장르를 정하는 경로가 생기면 그때부터 그 아이템은
/// 태그로 걸리고 이 조건을 타지 않는다.
///
/// 여성 전용 장르(로맨틱 페미닌·보헤미안 등)는 `None` 이다. 레이스·리본·플로럴
/// 같은 판단 근거가 스키마에 없어서 조건으로 옮길 수 없고, 여성 아이템은 시드가
/// 장르를 제대로 붙여 두었으므로 태그만으로 충분하다.
///
/// 장르는 서로 겹친다. 옥스퍼드 셔츠는 아메카지이면서 프레피이고 스마트 캐주얼
/// 이다. 한 아이템이 여러 장르 조건에 걸리는 것은 의도된 동작이다.
fn genre_predicate(genre: StyleGenre) -> Option<&'static str> {
    Some(match genre {
        // 절제된 색상, 로고·장식 없는 기본 아이템. 채도가 판단 기준이다.
        StyleGenre::MinimalClassic => {
            "style IN ('베이직','포멀') AND saturation = '낮음' \
             AND sub_category NOT IN ('cargo','bdu')"
        }
        // 출근·데이트에 쓰는 단정함. 셔츠·니트·슬랙스·재킷과 formality 로 가른다.
        StyleGenre::SmartCasual => {
            "style IN ('베이직','포멀') AND formality_level >= 2 \
             AND sub_category IN ('shirt','knit','longsleeve','slacks','chino','linen',\
             'coat','blazer','harrington','blouson','loafer','derby','sneaker','trainer',\
             'shoulder','tote','crossbody')"
        }
        // 아이비리그 계열 아이템. 스마트 캐주얼보다 아이템 목록이 좁다.
        StyleGenre::Preppy => {
            "style IN ('베이직','포멀') \
             AND sub_category IN ('shirt','knit','chino','slacks','blazer','harrington',\
             'coat','loafer','derby','canvas_sneaker','tote')"
        }
        // 작업복에서 온 것들. `style='워크'` 가 이미 이 축을 담고 있고,
        // 데님·트러커·워크부츠는 스타일 값과 무관하게 워크웨어 아이템이다.
        StyleGenre::Workwear => {
            "style = '워크' OR sub_category IN ('trucker','work_boots','overshirt','denim')"
        }
        // 그래픽·여유로운 실루엣·스니커즈. 격식 낮은 캐주얼 아이템으로 잡는다.
        StyleGenre::Street => {
            "formality_level <= 2 \
             AND sub_category IN ('sweat','zip_up','tee','cargo','bomber','coach',\
             'canvas_sneaker','sneaker','trainer','backpack','crossbody','sweatpants')"
        }
        // 기능성 소재와 아웃도어 아이템. 다만 셸 재킷 안에는 평범한 티셔츠를
        // 입으므로, 격식 낮은 기본 상의도 후보에 넣는다 — 이걸 빼면 이 장르는
        // 상의가 한 벌도 없어 코디 자체가 만들어지지 않는다.
        StyleGenre::OutdoorCasual => {
            "material_primary IN ('nylon','cordura','polyester','fleece') \
             OR sub_category IN ('parka','coach','deck','cargo','trainer','runner','backpack') \
             OR (sub_category IN ('tee','longsleeve','henley','sweat','zip_up','knit') \
                 AND formality_level <= 2)"
        }
        // 운동복 계열. 아웃도어와 달리 트레이닝·러닝 쪽이다.
        StyleGenre::SportyCasual => {
            "style = '스포츠' OR material_primary = 'sweat' \
             OR sub_category IN ('sweat','sweatpants','zip_up','runner','trainer',\
             'canvas_sneaker','backpack','crossbody')"
        }
        // 데님·치노·스웨트·필드재킷·워크셔츠로 이어지는 아메리칸 캐주얼 전반.
        // 이 옷장에서는 포멀 테일러링(블레이저·더비·울 슬랙스)과 순수 운동복만
        // 빠진다 — 둘 다 아메카지가 아니다.
        StyleGenre::Amekaji => "style IN ('베이직','워크','밀리터리')",
        // 여성 전용 장르 — 근거가 스키마에 없다. 태그로만 고른다.
        StyleGenre::RomanticFeminine
        | StyleGenre::ModernChic
        | StyleGenre::Bohemian
        | StyleGenre::ModelOffDuty
        | StyleGenre::Mannish => return None,
    })
}

pub async fn list_clothing_filtered(
    pool: &MySqlPool,
    gender: Option<&str>,
    style_mood: Option<StyleGenre>,
) -> Result<Vec<Clothing>, sqlx::Error> {
    let mut sql = format!("SELECT {} FROM clothing WHERE 1=1", SELECT_COLS);
    if gender.is_some() {
        sql.push_str(" AND gender = ?");
    }
    if let Some(genre) = style_mood {
        // 장르가 지정된 아이템은 그 태그로, 지정되지 않은 아이템은 속성 조건으로.
        //
        // 두 절은 서로 겹치지 않는다 — `style_mood` 가 NULL 이면 첫 절이 절대
        // 참이 될 수 없고, NULL 이 아니면 둘째 절이 참이 될 수 없다. 그래서 장르가
        // 제대로 붙어 있는 아이템(여성 시드 전체, 남성 스포티·아웃도어 시드)은
        // 조건 쪽 영향을 전혀 받지 않는다.
        match genre_predicate(genre) {
            Some(pred) => {
                sql.push_str(&format!(
                    " AND (style_mood = ? OR (style_mood IS NULL AND ({pred})))"
                ));
            }
            None => sql.push_str(" AND style_mood = ?"),
        }
    }
    sql.push_str(" ORDER BY created_at DESC");

    let mut q = sqlx::query_as::<_, Clothing>(&sql);
    if let Some(g) = gender {
        q = q.bind(g);
    }
    if let Some(m) = style_mood {
        q = q.bind(m);
    }
    q.fetch_all(pool).await
}

pub async fn get_clothing_by_id(
    pool: &MySqlPool,
    id: &str,
) -> Result<Option<Clothing>, sqlx::Error> {
    sqlx::query_as::<_, Clothing>(&format!(
        "SELECT {} FROM clothing WHERE id = ?",
        SELECT_COLS
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}

#[allow(clippy::too_many_arguments)] // 테이블 컬럼을 그대로 받는 저장 함수
pub async fn update_clothing(
    pool: &MySqlPool,
    id: &str,
    name: Option<&str>,
    category: Option<&str>,
    color: Option<&str>,
    thickness: Option<Thickness>,
    image_url: Option<&str>,
    tone: Option<Tone>,
    saturation: Option<Saturation>,
    style: Option<Style>,
    weight: Option<Weight>,
    role: Option<Role>,
    color_temperature: Option<&str>,
    versatility: Option<&str>,
    statement_level: Option<i8>,
    formality_level: Option<i8>,
) -> Result<Option<Clothing>, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE clothing SET
            name              = COALESCE(?, name),
            category          = COALESCE(?, category),
            color             = COALESCE(?, color),
            thickness         = COALESCE(?, thickness),
            image_url         = COALESCE(?, image_url),
            tone              = COALESCE(?, tone),
            saturation        = COALESCE(?, saturation),
            style             = COALESCE(?, style),
            weight            = COALESCE(?, weight),
            role              = COALESCE(?, role),
            color_temperature = COALESCE(?, color_temperature),
            versatility       = COALESCE(?, versatility),
            statement_level   = COALESCE(?, statement_level),
            formality_level   = COALESCE(?, formality_level),
            updated_at        = NOW()
        WHERE id = ?
        "#,
    )
    .bind(name)
    .bind(category)
    .bind(color)
    .bind(thickness)
    .bind(image_url)
    .bind(tone)
    .bind(saturation)
    .bind(style)
    .bind(weight)
    .bind(role)
    .bind(color_temperature)
    .bind(versatility)
    .bind(statement_level)
    .bind(formality_level)
    .bind(id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Ok(None);
    }

    sqlx::query_as::<_, Clothing>(&format!(
        "SELECT {} FROM clothing WHERE id = ?",
        SELECT_COLS
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn delete_seasons(pool: &MySqlPool, clothing_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM clothing_season WHERE clothing_id = ?")
        .bind(clothing_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_clothing(pool: &MySqlPool, id: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM clothing WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}
