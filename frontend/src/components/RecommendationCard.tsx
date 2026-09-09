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

      {/* 이미지 생성은 '오늘의 추천'에만 붙인다 — 기존 동작과 동일.
          이미지가 있으면 글보다 먼저 보이도록 구성 목록 위에 둔다. */}
      {mode.mode === 'todays_pick' && mode.outfit.length > 0 && (
        <OutfitImage items={outfitToImageItems(mode.outfit)} mood={selectedMood} />
      )}

      {/* 작은 태그 나열 대신 슬롯과 이름을 나란히 읽는 구성 목록. */}
      {mode.outfit.length > 0 && (
        <ul className="rec-pieces">
          {mode.outfit.map((o, i) => (
            <li className="rec-piece" key={`${o.category}-${o.name}-${i}`}>
              <span className="rec-piece-slot">{o.category}</span>
              <span className="rec-piece-name">{o.name}</span>
            </li>
          ))}
        </ul>
      )}

      <p className="mode-reason">{mode.reason}</p>

      <button className="mode-toggle" onClick={() => setOpen((v) => !v)}>
        {open ? '접기 ▴' : '자세히 ▾'}
      </button>

      <div className={`mode-detail${open ? ' open' : ''}`}>
        {mode.outfit.map((o, i) => (
          <div className="mode-detail-item" key={`${o.name}-${i}`}>
            {o.image_url?.startsWith('data:image/') && (
              <img className="mode-detail-thumb" src={o.image_url} alt="" />
            )}
            <div>
              <div className="item-card-name">{o.name}</div>
              <div className="item-card-meta">
                {o.category} · {o.reason}
              </div>
            </div>
          </div>
        ))}

        <p className="mode-reason">{mode.recommendation}</p>

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
