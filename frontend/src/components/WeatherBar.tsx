import type { WeatherResponse } from '../types/api';
import { getWeatherEmoji } from '../lib/lookbook';
import { Loading } from './Status';

interface Props {
  weather: WeatherResponse | null;
  loading: boolean;
  error: string | null;
  onOpenRegion: () => void;
}

export function WeatherBar({ weather, loading, error, onOpenRegion }: Props) {
  if (loading) {
    return (
      <div className="msg msg-loading" style={{ marginBottom: 12 }}>
        <span className="spinner" /> 날씨 불러오는 중...
      </div>
    );
  }

  if (error || !weather) {
    return (
      <span className="msg msg-error" style={{ cursor: 'pointer' }} onClick={onOpenRegion}>
        날씨를 불러올 수 없습니다. 지역을 설정해 주세요.
      </span>
    );
  }

  const desc = weather.current.weather_description || '';
  return (
    <div className="weather-bar">
      <span className="weather-icon">{getWeatherEmoji(desc)}</span>
      <span className="weather-text">
        {weather.region_name} · {desc} · 습도 {weather.current.humidity}%
      </span>
      <span className="weather-temp">{weather.current.temperature}°C</span>
    </div>
  );
}

export { Loading };
