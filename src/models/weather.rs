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

/// Open-Meteo API response shape
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
