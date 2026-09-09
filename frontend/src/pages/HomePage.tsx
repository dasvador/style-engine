import { useMemo, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api } from '../api/endpoints';
import { errorMessage } from '../api/client';
import type { Clothing, Gender, MultiModeRecommendation, Region, StyleMood, WeatherResponse } from '../types/api';
import { ScreenHeader } from '../components/ScreenHeader';
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
      <ScreenHeader title="오늘뭐입지" />

      <WeatherBar
        weather={weather}
        loading={weatherLoading}
        error={weatherError}
        onOpenRegion={onOpenRegion}
      />
      <RegionBar region={region} onClick={onOpenRegion} />

      <div className="card" style={{ padding: '14px 16px' }}>
        <div style={{ display: 'flex', gap: 8, marginBottom: 10 }}>
          <button
            className={`gender-btn${gender === 'male' ? ' active' : ''}`}
            onClick={() => onGenderChange('male')}
          >
            남성
          </button>
          <button
            className={`gender-btn${gender === 'female' ? ' active' : ''}`}
            onClick={() => onGenderChange('female')}
          >
            여성
          </button>
        </div>
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
          {moods.map((m) => (
            <button
              key={m.mood_key}
              className={`mood-chip-btn${selectedMood === m.mood_key ? ' active' : ''}`}
              onClick={() => onMoodChange(m.mood_key)}
            >
              {m.mood_label}
            </button>
          ))}
        </div>
        {/* 이름만으로는 장르 간 차이를 알기 어렵다. 고른 장르의 설명을 한 줄 보여준다.
            문구는 DB(style_mood.description)에 있어 배포 없이 고칠 수 있다. */}
        {selectedMoodDescription && (
          <div
            style={{
              marginTop: 8,
              fontSize: '0.78rem',
              lineHeight: 1.5,
              color: 'var(--gray-500)',
            }}
          >
            {selectedMoodDescription}
          </div>
        )}
      </div>

      <div className="cta-group">
        <button className="cta-primary" onClick={() => navigate('/evaluate')}>
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
            <path d="M9 12l2 2 4-4" />
            <circle cx="12" cy="12" r="10" />
          </svg>
          지금 입은 코디 평가하기
        </button>
        <button className="cta-secondary" onClick={() => void loadAndScroll()}>
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z" />
          </svg>
          오늘 추천 보기
        </button>
      </div>

      {summary.total > 0 && (
        <div className="card">
          <div className="card-title">내 옷장 요약</div>
          <div className="summary-grid">
            <div className="summary-item">
              <div className="summary-val">{summary.total}</div>
              <div className="summary-lbl">전체</div>
            </div>
            <div className="summary-item">
              <div className="summary-val">{summary.base}</div>
              <div className="summary-lbl">베이스</div>
            </div>
            <div className="summary-item">
              <div className="summary-val">{summary.accent}</div>
              <div className="summary-lbl">포인트</div>
            </div>
          </div>
          <div style={{ marginTop: 8 }}>
            {summary.warnings.map((w) => (
              <div className="warning-banner" key={w}>
                <span>⚠️</span>
                <span>{w}</span>
              </div>
            ))}
          </div>
          <div className="quick-actions">
            <button
              className="btn btn-outline btn-sm"
              onClick={() => {
                navigate('/wardrobe');
                onOpenAddPanel();
              }}
            >
              옷 등록
            </button>
            <button className="btn btn-outline btn-sm" onClick={() => navigate('/wardrobe')}>
              옷장 보기
            </button>
          </div>
        </div>
      )}

      <div ref={recSectionRef}>
        <div className="card-title" style={{ marginTop: 8 }}>
          AI 추천
        </div>
        {recLoading && <Loading>AI가 코디를 고민 중...</Loading>}
        {recError && <ErrorMessage>추천을 불러올 수 없습니다: {recError}</ErrorMessage>}
        {!recLoading && !recError && rec && rec.modes.length === 0 && (
          <ErrorMessage>추천 결과가 없습니다</ErrorMessage>
        )}
        {!recLoading && !recError && rec && rec.modes.length > 0 && (
          <>
            <div style={{ fontSize: '0.82rem', color: 'var(--gray-500)', marginBottom: 10 }}>
              {rec.weather_summary}
            </div>
            {rec.modes.map((m) => (
              <RecommendationCard key={m.mode} mode={m} selectedMood={selectedMood} />
            ))}
          </>
        )}
      </div>
    </>
  );
}
