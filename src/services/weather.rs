use anyhow::Context;

use crate::models::weather::{OpenMeteoResponse, CurrentWeather};

pub async fn fetch_weather(
    client: &reqwest::Client,
    latitude: f64,
    longitude: f64,
) -> anyhow::Result<CurrentWeather> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,apparent_temperature,relative_humidity_2m,wind_speed_10m,weather_code",
        latitude, longitude
    );

    let resp: OpenMeteoResponse = client
        .get(&url)
        .send()
        .await
        .context("Failed to call Open-Meteo API")?
        .json()
        .await
        .context("Failed to parse Open-Meteo response")?;

    let c = resp.current;
    Ok(CurrentWeather {
        temperature: c.temperature_2m,
        apparent_temperature: c.apparent_temperature,
        humidity: c.relative_humidity_2m,
        wind_speed: c.wind_speed_10m,
        weather_code: c.weather_code,
        weather_description: weather_code_to_description(c.weather_code),
    })
}

fn weather_code_to_description(code: i32) -> String {
    match code {
        0 => "맑음",
        1 => "대체로 맑음",
        2 => "부분적으로 흐림",
        3 => "흐림",
        45 | 48 => "안개",
        51 | 53 | 55 => "이슬비",
        61 | 63 | 65 => "비",
        66 | 67 => "진눈깨비",
        71 | 73 | 75 => "눈",
        77 => "싸락눈",
        80 | 81 | 82 => "소나기",
        85 | 86 => "눈보라",
        95 => "뇌우",
        96 | 99 => "우박 동반 뇌우",
        _ => "알 수 없음",
    }
    .to_string()
}
