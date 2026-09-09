import { useEffect, useState } from 'react';
import { api } from '../api/endpoints';
import { errorMessage } from '../api/client';
import type { Region } from '../types/api';
import { ErrorMessage } from './Status';

interface Props {
  open: boolean;
  region: Region | null;
  onClose: () => void;
  onSaved: () => void;
}

/** 지역 설정 시트. 저장 후 날씨를 다시 읽도록 상위에 알린다. */
export function RegionModal({ open, region, onClose, onSaved }: Props) {
  const [name, setName] = useState('');
  const [lat, setLat] = useState('');
  const [lon, setLon] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setName(region?.name ?? '');
    setLat(region ? String(region.latitude) : '');
    setLon(region ? String(region.longitude) : '');
    setError(null);
  }, [open, region]);

  if (!open) return null;

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    const latitude = Number(lat);
    const longitude = Number(lon);
    if (!Number.isFinite(latitude) || !Number.isFinite(longitude)) {
      setError('위도와 경도는 숫자여야 합니다.');
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await api.region.upsert({ name, latitude, longitude });
      onSaved();
      onClose();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="modal-overlay open" onClick={onClose}>
      <div className="modal-sheet" onClick={(e) => e.stopPropagation()}>
        <div className="modal-handle" />
        <h3 className="modal-title">지역 설정</h3>
        {/* 좌표는 홈 화면에서 빼고 여기로 옮겼다. 바꿀 때만 필요한 값이다. */}
        {region ? (
          <p className="modal-current">
            현재 {region.name} · {region.latitude}, {region.longitude}
          </p>
        ) : (
          <p className="modal-current">날씨를 받아올 지역을 정해 주세요.</p>
        )}
        {error && <ErrorMessage>{error}</ErrorMessage>}
        <form onSubmit={submit}>
          <div className="form-group">
            <label className="form-label">지역명</label>
            <input
              className="form-input"
              type="text"
              placeholder="예: 서울"
              value={name}
              onChange={(e) => setName(e.target.value)}
              required
            />
          </div>
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8 }}>
            <div className="form-group">
              <label className="form-label">위도</label>
              <input
                className="form-input"
                type="text"
                placeholder="37.5665"
                value={lat}
                onChange={(e) => setLat(e.target.value)}
                required
              />
            </div>
            <div className="form-group">
              <label className="form-label">경도</label>
              <input
                className="form-input"
                type="text"
                placeholder="126.978"
                value={lon}
                onChange={(e) => setLon(e.target.value)}
                required
              />
            </div>
          </div>
          <div className="modal-actions">
            <button type="button" className="btn btn-outline" onClick={onClose}>
              취소
            </button>
            <button type="submit" className="btn btn-primary" disabled={saving}>
              {saving ? '저장 중...' : '저장'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
