//! 실측 기록용: 카드·초안의 룩 시그니처(`services::look_signature`)를 앱과 같은 함수로 적는다.
//!
//!     printf '<기온>\t<id 또는 이름>|<…>\n' | cargo run --example look_card
//!
//! 한 줄에 코디 하나. 항목은 id 로 먼저 찾고, 없으면 옷장에서 정확히 같은 이름(초안 기록용)으로
//! 찾는다. 출력: `시그니처 \t 보이는 색 있음 \t 하의 이름`.
use std::io::BufRead;

use sqlx::mysql::MySqlPoolOptions;
use style_engine::db::clothing_repo;
use style_engine::services::look_signature;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("DATABASE_URL")?)
        .await?;
    let all = clothing_repo::list_clothing_filtered(&pool, None, None).await?;
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let Some((t, keys)) = line.split_once('\t') else {
            continue;
        };
        let t: f64 = t.parse()?;
        let items: Vec<_> = keys
            .split('|')
            .filter_map(|k| {
                all.iter()
                    .find(|c| c.id == k)
                    .or_else(|| all.iter().find(|c| c.name == k))
            })
            .collect();
        let by = |cat: &str| items.iter().copied().find(|c| c.category == cat);
        match look_signature::signature(by("상의"), by("하의"), by("아우터"), Some(t)) {
            Some(s) => println!(
                "{}\t{}\t{}",
                s.label(),
                !s.achromatic(),
                by("하의").map(|c| c.name.as_str()).unwrap_or("-")
            ),
            None => println!("-\tfalse\t-"),
        }
    }
    Ok(())
}
