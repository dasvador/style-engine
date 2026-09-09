import type { Region } from '../types/api';

export function RegionBar({ region, onClick }: { region: Region | null; onClick: () => void }) {
  return (
    <div className="region-bar" onClick={onClick}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
        <path d="M12 2C8.13 2 5 5.13 5 9c0 5.25 7 13 7 13s7-7.75 7-13c0-3.87-3.13-7-7-7z" />
        <circle cx="12" cy="9" r="2.5" />
      </svg>
      <span>{region ? `${region.name} (${region.latitude}, ${region.longitude})` : '지역 설정하기'}</span>
    </div>
  );
}
