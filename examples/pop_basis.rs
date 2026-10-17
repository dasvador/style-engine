//! 실제 추천 카드가 왜 팝인지(또는 아닌지) 앱과 같은 판정 함수로 적는다. 실측 기록용.
//!
//!     echo "<기온>\t<id,id,..>" | cargo run --example pop_basis
//!
//! 한 줄에 카드 하나. 옷(상의·하의·아우터)만 본다 — 가방·신발 색은 근거가 아니다. 12°C 아래에서
//! 아우터가 있으면 코트 속 상의는 보이지 않는다고 본다(`mood_gate::reads_as_at`). 출력은 탭으로
//! 나눈 `팝 여부 / 근거 / 보이는 옷(색 계열)`.
use std::io::BufRead;

use sqlx::mysql::MySqlPoolOptions;
use style_engine::db::clothing_repo;
use style_engine::models::style_vocab::StyleGenre;
use style_engine::services::mood_gate;
use style_engine::services::mood_palette::colour_family;
use style_engine::services::serving_ranker::CLOSED_COAT_BELOW_C;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("DATABASE_URL")?)
        .await?;
    let all = clothing_repo::list_clothing_filtered(&pool, None, None).await?;
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let Some((t, ids)) = line.split_once('\t') else {
            continue;
        };
        let t: f64 = t.parse()?;
        let outfit: Vec<_> = ids
            .split(',')
            .filter_map(|id| all.iter().find(|c| c.id == id))
            .collect();
        let has_outer = outfit.iter().any(|c| c.category == "아우터");
        let visible: Vec<_> = outfit
            .iter()
            .filter(|c| matches!(c.category.as_str(), "상의" | "하의" | "아우터"))
            .filter(|c| !(c.category == "상의" && has_outer && t < CLOSED_COAT_BELOW_C))
            .collect();
        let mut basis = Vec::new();
        for c in &visible {
            if mood_gate::is_pop_piece(c) {
                let kind = if c.category == "아우터" {
                    "배색 아우터"
                } else {
                    "배색·그래픽 옷"
                };
                basis.push(format!("{kind}: {}", c.name));
            }
        }
        let mut families: Vec<&str> = visible
            .iter()
            .filter(|c| mood_gate::is_vivid_colour(c))
            .filter_map(|c| colour_family(c))
            .collect();
        families.sort();
        families.dedup();
        if families.len() >= 2 {
            basis.push(format!("두 색 계열: {}", families.join("+")));
        }
        if let Some(o) = visible.iter().find(|c| c.category == "아우터")
            && !mood_gate::is_pop_piece(o)
            && mood_gate::is_vivid_colour(o)
        {
            basis.push(format!(
                "컬러 아우터: {}({})",
                o.name,
                colour_family(o).unwrap_or("-")
            ));
        }
        let reads = mood_gate::reads_as_at(StyleGenre::Pop, &outfit, Some(t)) == Some(true);
        let shown: Vec<String> = visible
            .iter()
            .map(|c| {
                let f = if mood_gate::is_vivid_colour(c) {
                    colour_family(c).unwrap_or("-")
                } else {
                    "기본"
                };
                format!("{}({f})", c.name)
            })
            .collect();
        println!(
            "{}\t{}\t{}",
            if reads { "팝" } else { "팝 아님" },
            if basis.is_empty() {
                "-".into()
            } else {
                basis.join(" · ")
            },
            shown.join(" / ")
        );
    }
    Ok(())
}
