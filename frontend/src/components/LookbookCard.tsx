import { useState } from 'react';
import { api } from '../api/endpoints';
import type { ChatItem, CurrentWeather, FeedbackBody, FeedbackType } from '../types/api';
import {
  DISLIKE_MESSAGES,
  DISLIKE_REASONS,
  LIKE_MESSAGES,
  LIKE_REASONS,
  REASON_LABELS,
  cleanStyleNote,
  extractSilhouetteTags,
  extractTextureTags,
  generateLookbookTitle,
  generateMoodSubtitle,
} from '../lib/lookbook';
import { OutfitImage } from './OutfitImage';

interface Props {
  items: ChatItem[];
  reply: string;
  weather: CurrentWeather | null;
  selectedMood: string | null;
}

const SLOT_ORDER = ['inner', 'outer', 'bottom', 'shoes', 'bag'] as const;
const SLOT_LABEL: Record<string, string> = {
  inner: 'BASE',
  outer: 'LAYER',
  bottom: 'BOTTOM',
  shoes: 'FOOTWEAR',
  bag: 'CARRY',
};


type FeedbackState =
  | { phase: 'idle' }
  | { phase: 'choosing'; type: FeedbackType }
  | { phase: 'done'; text: string };

export function LookbookCard({ items, reply, weather, selectedMood }: Props) {
  const [feedback, setFeedback] = useState<FeedbackState>({ phase: 'idle' });

  const title = generateLookbookTitle(items);
  const moodSub = generateMoodSubtitle(items);
  const note = cleanStyleNote(reply);
  const texTags = extractTextureTags(items);
  const silTags = extractSilhouetteTags(items);

  const submitFeedback = async (type: FeedbackType, reasons: string[]) => {
    // 슬롯별로 정해진 필드에 담는다. switch 로 쓰면 필드명이 타입으로 검사된다.
    const body: FeedbackBody = { feedback_type: type, reasons };
    for (const it of items) {
      switch (it.slot) {
        case 'inner':
          body.inner_name = it.name;
          break;
        case 'outer':
          body.outer_name = it.name;
          break;
        case 'bottom':
          body.bottom_name = it.name;
          break;
        case 'shoes':
          body.shoes_name = it.name;
          break;
        case 'bag':
          body.bag_name = it.name;
          break;
        default:
          break;
      }
    }

    try {
      await api.feedback(body);
    } catch {
      // 피드백 저장 실패가 화면을 막을 이유는 없다. 기존 UI 도 콘솔에만 남겼다.
    }

    const label = type === 'like' ? '👍' : '👎';
    const pool = type === 'like' ? LIKE_MESSAGES : DISLIKE_MESSAGES;
    const msg = pool[Math.floor(Math.random() * pool.length)] ?? pool[0]!;
    const reasonText = reasons.map((r) => REASON_LABELS[r] ?? r).join(', ');
    setFeedback({ phase: 'done', text: `${label} ${msg}${reasonText ? ` — ${reasonText}` : ''}` });
  };

  return (
    <div className="lookbook-card">
      <div className="lookbook-header">
        <div className="lookbook-title">{title}</div>
        <div className="lookbook-mood-subtitle">{moodSub}</div>
        {weather && (
          <div className="lookbook-subtitle" style={{ marginTop: 4 }}>
            {weather.temperature}°C · {weather.weather_description || ''}
          </div>
        )}
      </div>

      <OutfitImage items={items} mood={selectedMood} />

      <div className="lookbook-items">
        {SLOT_ORDER.map((slot) => {
          const it = items.find((i) => i.slot === slot);
          if (!it) return null;
          const notOwned = it.owned === false;
          return (
            <div key={slot} className={notOwned ? 'lookbook-item not-owned' : 'lookbook-item'}>
              <div className="lookbook-item-slot">{SLOT_LABEL[slot] ?? slot}</div>
              <div className="lookbook-item-name">{it.name}</div>
              {notOwned && <div className="lookbook-item-tag">wish</div>}
            </div>
          );
        })}
      </div>

      {note && (
        <div className="lookbook-note">
          <div className="lookbook-note-title">Style Note</div>
          {/* LLM 이 만든 문자열이므로 텍스트로만 렌더링한다. 줄바꿈만 살린다. */}
          <div className="lookbook-note-text" style={{ whiteSpace: 'pre-line' }}>
            {note}
          </div>
        </div>
      )}

      {(texTags.length > 0 || silTags.length > 0) && (
        <div className="lookbook-tags">
          {texTags.map((t) => (
            <span className="lookbook-tag texture" key={`tex-${t}`}>
              {t}
            </span>
          ))}
          {silTags.map((t) => (
            <span className="lookbook-tag silhouette" key={`sil-${t}`}>
              {t}
            </span>
          ))}
        </div>
      )}

      <div className="lookbook-fb">
        {feedback.phase === 'idle' && (
          <>
            <button className="lookbook-action-btn" onClick={() => setFeedback({ phase: 'choosing', type: 'like' })}>
              좋아요
            </button>
            <button
              className="lookbook-action-btn"
              onClick={() => setFeedback({ phase: 'choosing', type: 'dislike' })}
            >
              아쉬워요
            </button>
          </>
        )}

        {feedback.phase === 'choosing' && (
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4, marginTop: 4 }}>
            {(feedback.type === 'like' ? LIKE_REASONS : DISLIKE_REASONS).map((r) => (
              <button
                key={r}
                className="chat-fb-btn"
                style={{ fontSize: '0.75rem' }}
                onClick={() => void submitFeedback(feedback.type, [r])}
              >
                {REASON_LABELS[r] ?? r}
              </button>
            ))}
            <button
              className="chat-fb-btn"
              style={{ fontSize: '0.75rem' }}
              onClick={() => void submitFeedback(feedback.type, [])}
            >
              그냥 {feedback.type === 'like' ? '👍' : '👎'}
            </button>
          </div>
        )}

        {feedback.phase === 'done' && <span className="chat-fb-done">{feedback.text}</span>}
      </div>
    </div>
  );
}
