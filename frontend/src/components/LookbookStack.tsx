import { useRef, useState, useSyncExternalStore } from 'react';
import type { Look } from '../types/api';
import { outfitToImageItems } from '../lib/lookbook';
import {
  cachedOutfitImage,
  outfitImageVersion,
  subscribeOutfitImages,
} from '../hooks/useOutfitImage';
import { OutfitImage } from './OutfitImage';

interface Props {
  looks: Look[];
  activeIndex: number;
  onSelect: (index: number) => void;
  onLike: (look: Look) => void;
  onWearToday: (look: Look) => void;
}

/**
 * 만들어진 코디가 쌓이는 룩북.
 *
 * 예전에는 추천을 다시 받으면 이전 결과를 덮어썼다. 이미 비용을 들여 만든 이미지가
 * 매번 사라졌고, 사용자는 두 코디를 나란히 두고 고를 수가 없었다. 그래서 생성한
 * 코디를 배열로 쌓고 좌우로 넘겨 보게 한다.
 *
 * 카드를 넘기는 것과 마음에 든다는 것은 구분한다 — 넘겨 보는 건 그냥 크게 보려는
 * 행동일 수 있다. 취향 신호는 아래의 두 버튼에서만 나온다.
 */
export function LookbookStack({ looks, activeIndex, onSelect, onLike, onWearToday }: Props) {
  // 이미지가 캐시에 들어오는 즉시 썸네일을 다시 그린다.
  useSyncExternalStore(subscribeOutfitImages, outfitImageVersion, outfitImageVersion);

  const active = looks[activeIndex];
  const touchStartX = useRef<number | null>(null);
  const [slideFrom, setSlideFrom] = useState<'left' | 'right' | null>(null);

  if (!active) return null;

  const go = (next: number) => {
    if (next < 0 || next >= looks.length || next === activeIndex) return;
    setSlideFrom(next > activeIndex ? 'right' : 'left');
    onSelect(next);
  };

  // 뒤에 두 장만 비쳐 보이게 한다. 더 쌓여도 깊이감은 두 장이면 충분하다.
  const behind = Math.min(2, looks.length - 1 - activeIndex);

  return (
    <section className="lookbook-stack">
      <header className="lookbook-stack-head">
        <span className="eyebrow" style={{ marginBottom: 0 }}>
          오늘의 룩북
        </span>
        <span className="lookbook-count">{looks.length} LOOKS</span>
      </header>

      <div
        className="lookbook-deck"
        onTouchStart={(e) => {
          touchStartX.current = e.touches[0]?.clientX ?? null;
        }}
        onTouchEnd={(e) => {
          const start = touchStartX.current;
          touchStartX.current = null;
          if (start === null) return;
          const dx = (e.changedTouches[0]?.clientX ?? start) - start;
          // 손가락이 살짝 흔들린 것과 넘기려는 동작을 가른다.
          if (Math.abs(dx) < 40) return;
          go(dx < 0 ? activeIndex + 1 : activeIndex - 1);
        }}
      >
        {/* 뒤에 쌓인 카드. 내용은 없고 깊이만 만든다. */}
        {Array.from({ length: behind }, (_, i) => (
          <div className="lookbook-behind" data-depth={i + 1} key={`behind-${i}`} aria-hidden="true" />
        ))}

        <article
          className={`lookbook-face${slideFrom ? ` slide-${slideFrom}` : ''}`}
          key={active.key}
          onAnimationEnd={() => setSlideFrom(null)}
        >
          {active.liked && (
            <span className="lookbook-bookmark" title="마음에 들어요">
              <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                <path d="M6 2h12a1 1 0 011 1v19l-7-4.5L5 22V3a1 1 0 011-1z" />
              </svg>
            </span>
          )}
          <OutfitImage items={outfitToImageItems(active.outfit)} mood={active.moodKey} />
          <h3 className="lookbook-face-title">{active.title}</h3>
          <p className="lookbook-face-meta">{active.meta}</p>
        </article>

        {looks.length > 1 && (
          <>
            <button
              className="lookbook-arrow prev"
              onClick={() => go(activeIndex - 1)}
              disabled={activeIndex === 0}
              aria-label="이전 코디"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6">
                <path d="M15 18l-6-6 6-6" />
              </svg>
            </button>
            <button
              className="lookbook-arrow next"
              onClick={() => go(activeIndex + 1)}
              disabled={activeIndex === looks.length - 1}
              aria-label="다음 코디"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6">
                <path d="M9 6l6 6-6 6" />
              </svg>
            </button>
          </>
        )}
      </div>

      <div className="lookbook-pager">
        {String(activeIndex + 1).padStart(2, '0')} / {String(looks.length).padStart(2, '0')}
      </div>

      {looks.length > 1 && (
        <div className="lookbook-thumbs" role="tablist" aria-label="코디 목록">
          {looks.map((l, i) => {
            // 이미 받아 둔 이미지만 쓴다. 썸네일 때문에 생성 요청을 내지 않는다.
            const url = cachedOutfitImage(outfitToImageItems(l.outfit), l.moodKey);
            return (
              <button
                key={l.key}
                role="tab"
                aria-selected={i === activeIndex}
                className={`lookbook-thumb${i === activeIndex ? ' active' : ''}`}
                onClick={() => go(i)}
                title={l.title}
              >
                {url ? <img src={url} alt="" /> : <span>{String(i + 1).padStart(2, '0')}</span>}
                {l.liked && <i className="lookbook-thumb-mark" aria-hidden="true" />}
              </button>
            );
          })}
        </div>
      )}

      <div className="lookbook-actions">
        <button
          className={`chip-action${active.liked ? ' on' : ''}`}
          onClick={() => onLike(active)}
          disabled={active.liked}
        >
          {active.liked ? '마음에 들어요 ✓' : '마음에 들어요'}
        </button>
        <button
          className={`chip-action${active.worn ? ' on' : ''}`}
          onClick={() => onWearToday(active)}
          disabled={active.worn}
        >
          {active.worn ? '오늘 입을래요 ✓' : '오늘 입을래요'}
        </button>
      </div>

      <ul className="rec-pieces">
        {active.outfit.map((o, i) => (
          <li className="rec-piece" key={`${o.category}-${o.name}-${i}`}>
            <span className="rec-piece-slot">{o.category}</span>
            <span className="rec-piece-name">{o.name}</span>
          </li>
        ))}
      </ul>

      <p className="mode-reason">{active.reason}</p>

      <Details look={active} />
    </section>
  );
}

/** 자세한 설명은 접어 둔다 — 카드에는 사진과 짧은 이름만 남긴다. */
function Details({ look }: { look: Look }) {
  const [open, setOpen] = useState(false);
  if (!look.recommendation) return null;
  return (
    <>
      <button className="mode-toggle" onClick={() => setOpen((v) => !v)}>
        {open ? '접기 ▴' : '자세히 ▾'}
      </button>
      {open && <p className="mode-reason">{look.recommendation}</p>}
    </>
  );
}
