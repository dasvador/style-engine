use std::collections::HashMap;

use anyhow::{anyhow, Context};
use chrono::{FixedOffset, Timelike, Utc};

use crate::models::weather::{CurrentWeather, KmaResponse, OpenMeteoResponse};

/// Fetch weather: KMA primary, Open-Meteo fallback
pub async fn fetch_weather(
    client: &reqwest::Client,
    kma_api_key: &str,
    latitude: f64,
    longitude: f64,
) -> anyhow::Result<CurrentWeather> {
    if !kma_api_key.is_empty() {
        match fetch_from_kma(client, kma_api_key, latitude, longitude).await {
            Ok(w) => return Ok(w),
            Err(e) => {
                tracing::warn!("KMA API failed, falling back to Open-Meteo: {e}");
            }
        }
    } else {
        tracing::warn!("KMA_API_KEY not set, using Open-Meteo fallback");
    }

    fetch_from_open_meteo(client, latitude, longitude).await
}

// ─── KMA 초단기실황 ───

async fn fetch_from_kma(
    client: &reqwest::Client,
    api_key: &str,
    latitude: f64,
    longitude: f64,
) -> anyhow::Result<CurrentWeather> {
    let (nx, ny) = latlon_to_grid(latitude, longitude);

    let kst = FixedOffset::east_opt(9 * 3600).unwrap();
    let now = Utc::now().with_timezone(&kst);

    // 초단기실황: 매시 40분 이후 해당 정시 데이터 제공
    let (base_date, base_time) = if now.minute() >= 40 {
        (now.format("%Y%m%d").to_string(), format!("{:02}00", now.hour()))
    } else {
        let prev = now - chrono::Duration::hours(1);
        (prev.format("%Y%m%d").to_string(), format!("{:02}00", prev.hour()))
    };

    // KMA API 키는 Encoding 버전을 그대로 URL에 삽입
    let url = format!(
        "https://apis.data.go.kr/1360000/VilageFcstInfoService_2.0/getUltraSrtNcst\
         ?serviceKey={}&numOfRows=10&pageNo=1&dataType=JSON\
         &base_date={}&base_time={}&nx={}&ny={}",
        api_key, base_date, base_time, nx, ny
    );

    let resp: KmaResponse = client
        .get(&url)
        .send()
        .await
        .context("KMA API call failed")?
        .json()
        .await
        .context("KMA response parse failed")?;

    if resp.response.header.result_code != "00" {
        return Err(anyhow!(
            "KMA API error: {} ({})",
            resp.response.header.result_msg,
            resp.response.header.result_code
        ));
    }

    let mut data: HashMap<String, f64> = HashMap::new();
    for item in &resp.response.body.items.item {
        if let Ok(val) = item.obsr_value.parse::<f64>() {
            data.insert(item.category.clone(), val);
        }
    }

    let temperature = *data.get("T1H").ok_or_else(|| anyhow!("T1H (기온) not found in KMA response"))?;
    let humidity = *data.get("REH").unwrap_or(&0.0);
    let wind_speed = *data.get("WSD").unwrap_or(&0.0);
    let pty = *data.get("PTY").unwrap_or(&0.0) as i32;

    Ok(CurrentWeather {
        temperature,
        apparent_temperature: calc_apparent_temperature(temperature, wind_speed),
        humidity,
        wind_speed,
        weather_code: pty_to_weather_code(pty),
        weather_description: pty_to_description(pty),
    })
}

// ─── Open-Meteo fallback ───

async fn fetch_from_open_meteo(
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

// ─── Grid conversion (Lambert Conformal Conic) ───

const RE: f64 = 6371.00877 / 5.0;
const SLAT1_DEG: f64 = 30.0;
const SLAT2_DEG: f64 = 60.0;
const OLON_DEG: f64 = 126.0;
const OLAT_DEG: f64 = 38.0;
const XO: f64 = 43.0;
const YO: f64 = 136.0;

fn latlon_to_grid(lat: f64, lon: f64) -> (i32, i32) {
    let degrad = std::f64::consts::PI / 180.0;

    let slat1 = SLAT1_DEG * degrad;
    let slat2 = SLAT2_DEG * degrad;
    let olon = OLON_DEG * degrad;
    let olat = OLAT_DEG * degrad;

    let sn = {
        let t = (std::f64::consts::FRAC_PI_4 + slat2 * 0.5).tan()
            / (std::f64::consts::FRAC_PI_4 + slat1 * 0.5).tan();
        (slat1.cos() / slat2.cos()).ln() / t.ln()
    };

    let sf = {
        let t = (std::f64::consts::FRAC_PI_4 + slat1 * 0.5).tan();
        t.powf(sn) * slat1.cos() / sn
    };

    let ro = {
        let t = (std::f64::consts::FRAC_PI_4 + olat * 0.5).tan();
        RE * sf / t.powf(sn)
    };

    let ra = {
        let t = (std::f64::consts::FRAC_PI_4 + lat * degrad * 0.5).tan();
        RE * sf / t.powf(sn)
    };

    let mut theta = lon * degrad - olon;
    if theta > std::f64::consts::PI {
        theta -= 2.0 * std::f64::consts::PI;
    }
    if theta < -std::f64::consts::PI {
        theta += 2.0 * std::f64::consts::PI;
    }
    theta *= sn;

    let nx = (ra * theta.sin() + XO + 0.5) as i32;
    let ny = (ro - ra * theta.cos() + YO + 0.5) as i32;

    (nx, ny)
}

// ─── Apparent temperature ───

fn calc_apparent_temperature(temp: f64, wind_speed_ms: f64) -> f64 {
    let wind_kmh = wind_speed_ms * 3.6;

    if temp <= 10.0 && wind_kmh > 4.8 {
        // Wind chill (Environment Canada formula)
        13.12 + 0.6215 * temp - 11.37 * wind_kmh.powf(0.16)
            + 0.3965 * temp * wind_kmh.powf(0.16)
    } else {
        temp
    }
}

// ─── Weather code mappings ───

fn pty_to_weather_code(pty: i32) -> i32 {
    match pty {
        1 => 61, // 비
        2 => 66, // 비/눈
        3 => 71, // 눈
        5 => 51, // 빗방울
        6 => 66, // 빗방울눈날림
        7 => 77, // 눈날림
        _ => 0,  // 없음
    }
}

fn pty_to_description(pty: i32) -> String {
    match pty {
        1 => "비",
        2 => "비/눈",
        3 => "눈",
        5 => "빗방울",
        6 => "빗방울눈날림",
        7 => "눈날림",
        _ => "강수 없음",
    }
    .to_string()
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
