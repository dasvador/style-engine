use serde::{Deserialize, Serialize};

/// Response DTO for weather endpoint
#[derive(Debug, Serialize)]
pub struct WeatherResponse {
    pub region_name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub current: CurrentWeather,
}

#[derive(Debug, Serialize)]
pub struct CurrentWeather {
    pub temperature: f64,
    pub apparent_temperature: f64,
    pub humidity: f64,
    pub wind_speed: f64,
    pub weather_code: i32,
    pub weather_description: String,
}

// ─── KMA 초단기실황 API response ───

#[derive(Debug, Deserialize)]
pub struct KmaResponse {
    pub response: KmaResponseInner,
}

#[derive(Debug, Deserialize)]
pub struct KmaResponseInner {
    pub header: KmaHeader,
    pub body: KmaBody,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KmaHeader {
    pub result_code: String,
    pub result_msg: String,
}

#[derive(Debug, Deserialize)]
pub struct KmaBody {
    pub items: KmaItems,
}

#[derive(Debug, Deserialize)]
pub struct KmaItems {
    pub item: Vec<KmaItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KmaItem {
    pub category: String,
    pub obsr_value: String,
}

// ─── Open-Meteo API response (fallback) ───

#[derive(Debug, Deserialize)]
pub struct OpenMeteoResponse {
    pub current: OpenMeteoCurrent,
}

#[derive(Debug, Deserialize)]
pub struct OpenMeteoCurrent {
    pub temperature_2m: f64,
    pub apparent_temperature: f64,
    pub relative_humidity_2m: f64,
    pub wind_speed_10m: f64,
    pub weather_code: i32,
}
