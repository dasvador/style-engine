import { useState } from 'react';
import type { ModeRecommendation } from '../types/api';
import { MODE_ICON, outfitToImageItems } from '../lib/lookbook';
import { OutfitImage } from './OutfitImage';

interface Props {
  mode: ModeRecommendation;
  selectedMood: string | null;
}

/** 추천 3모드 중 한 장. 상세는 접힌 채로 시작한다 (기존 동작과 동일). */
export function RecommendationCard({ mode, selectedMood }: Props) {
  const [open, setOpen] = useState(false);
  const icon = MODE_ICON[mode.mode] ?? MODE_ICON.todays_pick;
  const sd = mode.scoring_detail;

  return (
    <div className="mode-card" data-mode={mode.mode}>
      <div className="mode-header">
        <div className="mode-header-left">
          <span className="mode-icon">{icon}</span>
          <span className="mode-label">{mode.mode_label}</span>
        </div>
      </div>

      <div className="mode-subtitle">{mode.mode_description}</div>

      {mode.outfit.length > 0 && (
        <div className="rec-items">
          {mode.outfit.map((o, i) => (
            <span className="chip" key={`${o.category}-${o.name}-${i}`}>
              {o.category}: {o.name}
            </span>
          ))}
        </div>
      )}

      {/* 이미지 생성은 '오늘의 추천'에만 붙인다 — 기존 동작과 동일 */}
      {mode.mode === 'todays_pick' && mode.outfit.length > 0 && (
        <OutfitImage items={outfitToImageItems(mode.outfit)} mood={selectedMood} />
      )}

      <div className="mode-reason">{mode.reason}</div>

      <button className="mode-toggle" onClick={() => setOpen((v) => !v)}>
        {open ? '접기 ▴' : '자세히 ▾'}
      </button>

      <div className={`mode-detail${open ? ' open' : ''}`}>
        {mode.outfit.map((o, i) => (
          <div key={`${o.name}-${i}`} style={{ display: 'flex', gap: 10, alignItems: 'center', marginBottom: 8 }}>
            {o.image_url?.startsWith('data:image/') && (
              <img
                src={o.image_url}
                alt=""
                style={{ width: 40, height: 40, borderRadius: 6, objectFit: 'cover' }}
              />
            )}
            <div>
              <div style={{ fontWeight: 600, fontSize: '0.85rem' }}>{o.name}</div>
              <div style={{ fontSize: '0.75rem', color: 'var(--gray-500)' }}>
                {o.category} · {o.reason}
              </div>
            </div>
          </div>
        ))}

        <div style={{ fontSize: '0.8rem', color: 'var(--gray-600)', marginTop: 8 }}>{mode.recommendation}</div>

        {sd && (
          <div className="scoring-bar">
            <span>스타일 {sd.style_score}</span>
            {sd.recency_penalty > 0 && <span>반복 -{sd.recency_penalty}</span>}
            {sd.diversity_bonus > 0 && <span>다양성 +{sd.diversity_bonus}</span>}
            {sd.dormant_bonus > 0 && <span>부활 +{sd.dormant_bonus}</span>}
            <span>→ {sd.final_score}</span>
          </div>
        )}

        {mode.tips.length > 0 && (
          <div className="rec-tip">
            <span>💡</span>
            <span>{mode.tips[0]}</span>
          </div>
        )}
      </div>
    </div>
  );
}
