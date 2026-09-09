import { useMemo, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api } from '../api/endpoints';
import { errorMessage } from '../api/client';
import type { Clothing, Gender, MultiModeRecommendation, Region, StyleMood, WeatherResponse } from '../types/api';
import { WeatherBar } from '../components/WeatherBar';
import { RegionBar } from '../components/RegionBar';
import { RecommendationCard } from '../components/RecommendationCard';
import { ErrorMessage, Loading } from '../components/Status';

interface Props {
  clothes: Clothing[];
  weather: WeatherResponse | null;
  weatherLoading: boolean;
  weatherError: string | null;
  region: Region | null;
  moods: StyleMood[];
  gender: Gender;
  onGenderChange: (g: Gender) => void;
  selectedMood: string | null;
  onMoodChange: (key: string) => void;
  onOpenRegion: () => void;
  onOpenAddPanel: () => void;
}

export function HomePage({
  clothes,
  weather,
  weatherLoading,
  weatherError,
  region,
  moods,
  gender,
  onGenderChange,
  selectedMood,
  onMoodChange,
  onOpenRegion,
  onOpenAddPanel,
}: Props) {
  const navigate = useNavigate();
  const [rec, setRec] = useState<MultiModeRecommendation | null>(null);
  const [recLoading, setRecLoading] = useState(false);
  const [recError, setRecError] = useState<string | null>(null);
  const recSectionRef = useRef<HTMLDivElement>(null);

  const selectedMoodDescription =
    moods.find((m) => m.mood_key === selectedMood)?.description ?? null;

  const summary = useMemo(() => {
    const roles: Record<string, number> = {};
    for (const c of clothes) {
      const r = c.role ?? '미분류';
      roles[r] = (roles[r] ?? 0) + 1;
    }
    const base = roles['베이스'] ?? 0;
    const accent = (roles['포인트'] ?? 0) + (roles['약한포인트'] ?? 0);
    const structural = roles['구조템'] ?? 0;

    const warnings: string[] = [];
    if (clothes.length >= 5) {
      if (base === 0) warnings.push('베이스 역할 아이템이 없어요. 무채색 기본 아이템을 추가해 보세요.');
      if (accent === 0) warnings.push('포인트 역할 아이템이 없어요. 포인트 아이템을 추가해 보세요.');
      if (structural === 0) warnings.push('구조템이 없어요. 전체 실루엣을 잡아주는 아이템을 추가해 보세요.');
      if (base > 0 && accent > 0 && accent > base * 2) {
        warnings.push('포인트가 베이스보다 많아요. 기본 아이템을 더 추가하면 좋겠어요.');
      }
    }
    return { total: clothes.length, base, accent, warnings };
  }, [clothes]);

  const loadRecommendation = async () => {
    setRecLoading(true);
    setRecError(null);
    try {
      const r = await api.recommendation.multi({
        occasion: '일상',
        gender,
        style_mood: selectedMood,
      });
      setRec(r);
    } catch (err) {
      setRecError(errorMessage(err));
    } finally {
      setRecLoading(false);
    }
  };

  const loadAndScroll = async () => {
    await loadRecommendation();
    recSectionRef.current?.scrollIntoView({ behavior: 'smooth' });
  };

  return (
    <>
      <header className="masthead">
        <div className="wordmark">Wardrobe Edit</div>
        <p className="wordmark-sub">오늘의 옷장을 편집합니다</p>
      </header>

      <section className="section">
        <WeatherBar
          weather={weather}
          loading={weatherLoading}
          error={weatherError}
          onOpenRegion={onOpenRegion}
        />
        <RegionBar region={region} onClick={onOpenRegion} />
      </section>

      <section className="section">
        <div className="pick-group">
          <span className="eyebrow">For whom</span>
          <div className="gender-tabs" role="group" aria-label="성별 선택">
            <button
              className={`gender-btn${gender === 'female' ? ' active' : ''}`}
              aria-pressed={gender === 'female'}
              onClick={() => onGenderChange('female')}
            >
              여성
            </button>
            <button
              className={`gender-btn${gender === 'male' ? ' active' : ''}`}
              aria-pressed={gender === 'male'}
              onClick={() => onGenderChange('male')}
            >
              남성
            </button>
          </div>
        </div>

        <div className="pick-group">
          <span className="eyebrow">Style</span>
          <div className="mood-chips" role="group" aria-label="스타일 장르 선택">
            {moods.map((m) => (
              <button
                key={m.mood_key}
                className={`mood-chip-btn${selectedMood === m.mood_key ? ' active' : ''}`}
                aria-pressed={selectedMood === m.mood_key}
                onClick={() => onMoodChange(m.mood_key)}
              >
                {m.mood_label}
              </button>
            ))}
          </div>
          {/* 이름만으로는 장르 간 차이를 알기 어렵다. 고른 장르의 설명을 한 줄 보여준다.
              문구는 DB(style_mood.description)에 있어 배포 없이 고칠 수 있다. */}
          {selectedMoodDescription && <p className="mood-note">{selectedMoodDescription}</p>}
        </div>
      </section>

      <section className="section cta-group">
        <button className="cta-primary" onClick={() => void loadAndScroll()}>
          오늘의 스타일 제안 보기
          <svg
            className="cta-arrow"
            width="18" height="18" viewBox="0 0 24 24"
            fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true"
          >
            <path d="M4 12h15" />
            <path d="M13 6l6 6-6 6" />
          </svg>
        </button>
        <button className="cta-secondary" onClick={() => navigate('/evaluate')}>
          내 코디 평가하기
        </button>
      </section>

      {summary.total > 0 && (
        <section className="section wardrobe-note">
          <span className="eyebrow">My wardrobe</span>
          <div className="wardrobe-count">
            {summary.total}
            <em>pieces</em>
          </div>
          <div className="wardrobe-breakdown">
            베이스 {summary.base} · 포인트 {summary.accent}
          </div>

          {summary.warnings.map((w) => (
            <p className="note-warning" key={w}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true">
                <path d="M12 9v4" />
                <path d="M12 17h.01" />
                <path d="M10.3 3.9L2.4 18a2 2 0 001.7 3h15.8a2 2 0 001.7-3L13.7 3.9a2 2 0 00-3.4 0z" />
              </svg>
              <span>{w}</span>
            </p>
          ))}

          <div className="wardrobe-links">
            <button className="link-action" onClick={() => navigate('/wardrobe')}>
              옷장 보기 →
            </button>
            <button
              className="link-action"
              onClick={() => {
                navigate('/wardrobe');
                onOpenAddPanel();
              }}
            >
              + 새 아이템
            </button>
          </div>
        </section>
      )}

      <section ref={recSectionRef} className="section">
        <div className="section-head">
          <h2 className="section-title">오늘의 제안</h2>
        </div>

        {recLoading && <Loading>오늘의 조합을 고르는 중...</Loading>}
        {recError && <ErrorMessage>추천을 불러올 수 없습니다: {recError}</ErrorMessage>}
        {!recLoading && !recError && rec && rec.modes.length === 0 && (
          <ErrorMessage>추천 결과가 없습니다</ErrorMessage>
        )}
        {!recLoading && !recError && !rec && (
          <p className="prose">
            위의 제안 보기를 누르면 오늘 날씨와 고른 장르에 맞춰 옷장에서 조합을 골라 드려요.
          </p>
        )}
        {!recLoading && !recError && rec && rec.modes.length > 0 && (
          <>
            <p className="caption" style={{ marginBottom: 14 }}>
              {rec.weather_summary}
            </p>
            {rec.modes.map((m) => (
              <RecommendationCard key={m.mode} mode={m} selectedMood={selectedMood} />
            ))}
          </>
        )}
      </section>
    </>
  );
}
