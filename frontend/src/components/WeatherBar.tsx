import type { WeatherResponse } from '../types/api';
import { getWeatherEmoji } from '../lib/lookbook';
import { Loading } from './Status';

interface Props {
  weather: WeatherResponse | null;
  loading: boolean;
  error: string | null;
  onOpenRegion: () => void;
}

/**
 * 날씨는 코디를 정하는 정보 중 하나일 뿐이라, 화면을 차지하는 배너가 아니라
 * 지면 맨 윗줄의 작은 정보 행으로 둔다. 지역과 기온을 먼저 읽히게 하고
 * 습도처럼 덜 중요한 값은 아래 줄로 내린다.
 */
export function WeatherBar({ weather, loading, error, onOpenRegion }: Props) {
  if (loading) {
    return <Loading>날씨를 확인하는 중...</Loading>;
  }

  if (error || !weather) {
    return (
      <button className="region-line" onClick={onOpenRegion}>
        날씨를 불러올 수 없습니다. 지역을 설정해 주세요.
      </button>
    );
  }

  const desc = weather.current.weather_description || '';
  return (
    <div>
      <div className="weather-line">
        <span className="weather-place">{weather.region_name}</span>
        <span className="weather-temp">{weather.current.temperature}°</span>
        <span className="weather-emoji" aria-hidden="true">
          {getWeatherEmoji(desc)}
        </span>
      </div>
      {desc && <div className="weather-desc">{desc}</div>}
      <div className="weather-meta">습도 {weather.current.humidity}%</div>
    </div>
  );
}

export { Loading };
