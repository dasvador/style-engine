import type { Region } from '../types/api';

/**
 * 지역 변경 진입점. 좌표는 여기서 읽을 일이 없어 설정 시트로 옮겼고,
 * 여기에는 지역명만 남긴다.
 */
export function RegionBar({ region, onClick }: { region: Region | null; onClick: () => void }) {
  return (
    <button className="region-line" onClick={onClick}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true">
        <path d="M12 2C8.13 2 5 5.13 5 9c0 5.25 7 13 7 13s7-7.75 7-13c0-3.87-3.13-7-7-7z" />
        <circle cx="12" cy="9" r="2.5" />
      </svg>
      <span>{region ? `${region.name} 지역 변경` : '지역 설정하기'}</span>
    </button>
  );
}
