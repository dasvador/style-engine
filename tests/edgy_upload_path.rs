//! 업로드한 옷이 엣지 판정까지 이어지는가. 실제 등록 경로(`clothes::analyze_and_create`)와
//! 같은 저장 함수로 옷을 넣고, 운영과 같은 조회로 다시 읽어 판정한다.
//!
//! 등록 경로는 소재(`material_primary`)·설명(`texture_keywords`)·세부 종류를 저장하지
//! 않는다. 그래서 시드처럼 소재를 채운 옷으로는 이 경로를 검증할 수 없다 — 여기서는 저장된
//! 값만 쓴다. 근거는 분석이 "색상 + 소재 + 아이템명" 으로 짓는 이름이다.
//!
//! DB 가 필요한 테스트다. `TEST_DATABASE_URL` 이 없으면 건너뛴다. 넣은 행은 판정 전에 지운다.

use sqlx::mysql::MySqlPoolOptions;
use style_engine::db::clothing_repo;
use style_engine::models::clothing::Clothing;
use style_engine::models::style_vocab::{Role, StyleGenre, Thickness, Tone};
use style_engine::services::shortlist::{ShortlistContext, build_all_shortlists};
use style_engine::services::{mood_gate, mood_rules};

#[tokio::test]
async fn uploaded_leather_garment_reads_as_edgy_without_stored_material() {
    let Some(url) = std::env::var("TEST_DATABASE_URL")
        .ok()
        .filter(|s| !s.is_empty())
    else {
        eprintln!("TEST_DATABASE_URL 이 없어 건너뜁니다");
        return;
    };
    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("DB 연결");

    // 분석이 낼 법한 값만 넣는다. 소재·설명은 등록 경로가 저장하지 않으므로 없다.
    let uploads = [
        ("블랙 레더 미디 스커트", "하의", Thickness::Medium),
        ("블랙 레더 트리밍 니트", "상의", Thickness::Medium),
        ("블랙 리브드 니트", "상의", Thickness::Medium),
        ("블랙 기본 탑", "상의", Thickness::Thin),
        ("블랙 레더 앵클부츠", "신발", Thickness::Medium),
        ("블랙 레더 숄더백", "가방", Thickness::Medium),
    ];
    let mut ids = Vec::new();
    for (name, category, thickness) in uploads {
        let c = clothing_repo::insert_clothing(
            &pool,
            "default",
            name,
            category,
            Some("블랙"),
            thickness,
            Some("data:image/png;base64,AAAA"),
            Some(Tone::Dark),
            None,
            None,
            None,
            Some(Role::Base),
            None,
            None,
            None,
            Some(2),
        )
        .await
        .expect("등록");
        clothing_repo::set_gender(&pool, &c.id, "female")
            .await
            .expect("성별");
        clothing_repo::set_style_genres(&pool, &c.id, &["edgy".to_string()], "vision")
            .await
            .expect("장르");
        ids.push(c.id);
    }
    let listed = clothing_repo::list_clothing_filtered(
        &pool,
        "default",
        Some("female"),
        Some(StyleGenre::Edgy),
    )
    .await
    .expect("조회");
    let saved: Vec<Clothing> = listed.into_iter().filter(|c| ids.contains(&c.id)).collect();
    // 판정 전에 지운다 — 단언이 실패해도 개발 DB 에 남지 않는다.
    for id in &ids {
        sqlx::query("DELETE FROM clothing_style_genre WHERE clothing_id = ?")
            .bind(id)
            .execute(&pool)
            .await
            .expect("태그 정리");
        clothing_repo::delete_clothing(&pool, "default", id)
            .await
            .expect("정리");
    }

    assert_eq!(saved.len(), uploads.len(), "엣지 옷장 조회에 모두 나온다");
    let get = |name: &str| saved.iter().find(|c| c.name == name).unwrap();
    let skirt = get("블랙 레더 미디 스커트");
    assert!(skirt.material_primary.is_none() && skirt.texture_keywords.is_none());

    // 판별: 가죽 옷은 엣지, 가죽 트리밍 니트·가죽 신발·가방은 아니다.
    assert!(mood_gate::is_leather_garment(skirt));
    assert!(mood_gate::is_edgy_piece(skirt));
    assert!(!mood_gate::is_edgy_piece(get("블랙 레더 트리밍 니트")));
    assert!(!mood_gate::is_edgy_piece(get("블랙 레더 앵클부츠")));
    assert!(!mood_gate::is_edgy_piece(get("블랙 레더 숄더백")));
    let outfit = |names: &[&str]| names.iter().map(|n| get(n)).collect::<Vec<_>>();
    assert_eq!(
        mood_gate::reads_as(
            StyleGenre::Edgy,
            &outfit(&[
                "블랙 기본 탑",
                "블랙 레더 미디 스커트",
                "블랙 레더 앵클부츠"
            ])
        ),
        Some(true)
    );
    assert_eq!(
        mood_gate::reads_as(
            StyleGenre::Edgy,
            &outfit(&[
                "블랙 레더 트리밍 니트",
                "블랙 레더 앵클부츠",
                "블랙 레더 숄더백"
            ])
        ),
        Some(false)
    );

    // 추천의 올블랙 보정: 이름이 니트라고 말하는 옷과는 소재 대비, 소재를 모르는 탑과는 아니다.
    assert!(mood_rules::dark_edgy_look_has_contrast(&outfit(&[
        "블랙 리브드 니트",
        "블랙 레더 미디 스커트"
    ])));
    assert!(!mood_rules::dark_edgy_look_has_contrast(&outfit(&[
        "블랙 기본 탑",
        "블랙 레더 미디 스커트"
    ])));

    // 날씨: 26°C 후보 목록에서 가죽 스커트는 빠지고, 가죽 신발·가방은 남는다.
    let none = std::collections::HashSet::new();
    let sl = build_all_shortlists(
        &saved,
        &ShortlistContext {
            temperature: 26.0,
            situation: Some("일상"),
            current_season: Some("여름"),
            recent_item_ids: &none,
        },
    );
    assert!(!sl.bottoms.iter().any(|c| c.name == "블랙 레더 미디 스커트"));
    assert!(sl.shoes.iter().any(|c| c.name == "블랙 레더 앵클부츠"));

    // 이미지: 소재를 지어내지 않는다 — 소재 칸은 비고, 이름이 가죽을 전한다.
    assert_eq!(skirt.image_material(), None);
}
