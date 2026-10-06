import { useEffect, useMemo, useRef, useState } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { api } from '../api/endpoints';
import { errorMessage } from '../api/client';
import type { Clothing, Gender, Look, Region, StyleMood, WeatherResponse } from '../types/api';
import { WeatherBar } from '../components/WeatherBar';
import { RegionBar } from '../components/RegionBar';
import { LookbookStack } from '../components/LookbookStack';
import { prefetchOutfitImage } from '../hooks/useOutfitImage';
import { buildImagePrompt } from '../lib/lookbook';
import { outfitToImageItems } from '../lib/lookbook';
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

/** 저장해 둔 코디의 시각. 오늘 만든 것과 지난 것을 구분한다. */
function formatSavedAt(iso: string): string {
  const d = new Date(iso.includes('Z') || iso.includes('+') ? iso : `${iso}Z`);
  if (Number.isNaN(d.getTime())) return '';
  const today = new Date();
  const sameDay =
    d.getFullYear() === today.getFullYear() &&
    d.getMonth() === today.getMonth() &&
    d.getDate() === today.getDate();
  const time = `${d.getHours()}시 ${String(d.getMinutes()).padStart(2, '0')}분`;
  return sameDay ? time : `${d.getMonth() + 1}월 ${d.getDate()}일 ${time}`;
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
  // 내 룩북에서 카드를 눌러 들어오면 그 코디를 바로 띄운다.
  const openLookId = (useLocation().state as { lookId?: string } | null)?.lookId ?? null;
  // 생성한 코디를 덮어쓰지 않고 쌓는다. 이미 비용을 들여 만든 이미지를 매번
  // 버리지 않고, 사용자가 둘을 나란히 두고 고를 수 있게 하기 위해서다.
  const [looks, setLooks] = useState<Look[]>([]);
  const [activeLook, setActiveLook] = useState(0);
  // 중복 판별은 최신 목록을 봐야 하는데, 저장·미리받기가 비동기로 돌아 setState 의
  // prev 만으로는 바깥에서 added 를 쓸 수 없다. 현재 목록을 ref 로 같이 들고 있는다.
  const looksRef = useRef<Look[]>([]);
  looksRef.current = looks;

  // 지난번에 만든 코디를 다시 불러온다. 한 장에 비용을 들여 만든 것이라
  // 새로고침했다고 버리지 않는다. 이미지는 저장해 둔 프롬프트로 캐시에서 돌아온다.
  useEffect(() => {
    void (async () => {
      try {
        // 서버는 최신순으로 준다. 화면은 만든 순서대로 쌓이므로 뒤집는다.
        const saved = (await api.lookbook.list()).slice().reverse();
        if (saved.length === 0) return;
        // 룩북에서 특정 코디를 눌러 들어왔으면 그 자리로 맞춘다.
        if (openLookId) {
          const at = saved.findIndex((v) => v.id === openLookId);
          if (at >= 0) setActiveLook(at);
        }
        setLooks(
          saved.map((v) => ({
            key: v.id,
            sig: `${v.mood_key ?? ''}|${v.outfit_json.map((o) => o.name).join('|')}`,
            savedId: v.id,
            title: v.title,
            meta: [formatSavedAt(v.created_at), v.weather_summary].filter(Boolean).join(' · '),
            moodKey: v.mood_key,
            outfit: v.outfit_json,
            reason: v.reason ?? '',
            recommendation: v.recommendation ?? '',
            liked: v.liked,
            worn: v.worn,
          })),
        );
      } catch {
        // 못 불러와도 새로 만들면 된다 — 화면을 막지는 않는다.
      }
    })();
  }, [openLookId]);
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

  const moodLabel = moods.find((m) => m.mood_key === selectedMood)?.mood_label ?? '전체';

  const loadRecommendation = async () => {
    setRecLoading(true);
    setRecError(null);
    try {
      const r = await api.recommendation.multi({
        occasion: '일상',
        gender,
        style_mood: selectedMood,
      });
      const stamp = new Date();
      const time = `${stamp.getHours()}시 ${String(stamp.getMinutes()).padStart(2, '0')}분`;
      const fresh: Look[] = r.modes.map((m, i) => ({
        // 키는 무조건 유일해야 한다. 예전에는 착장 구성을 키로 썼는데, 한 번의
        // 응답 안에서 두 모드가 같은 착장을 내면 키가 겹쳐 React 가 노드를 제대로
        // 지우지 못했다 — 6장인데 썸네일이 7개 뜨고 번호가 중복됐다.
        key: `${Date.now()}-${i}-${Math.random().toString(36).slice(2, 8)}`,
        sig: `${selectedMood ?? ''}|${m.outfit.map((o) => o.name).join('|')}`,
        title: m.mode_label,
        meta: `${time} · ${moodLabel} · ${r.weather_summary}`,
        moodKey: selectedMood,
        outfit: m.outfit,
        reason: m.reason,
        recommendation: m.recommendation,
        liked: false,
        worn: false,
      }));

      // 이전 목록과의 중복뿐 아니라 이번 응답 안의 중복도 걸러야 한다 —
      // 세 모드가 같은 착장을 낼 수 있다.
      const seen = new Set(looksRef.current.map((l) => l.sig));
      const added: Look[] = [];
      for (const l of fresh) {
        if (seen.has(l.sig)) continue;
        seen.add(l.sig);
        added.push(l);
      }
      if (added.length > 0) {
        // 새 코디는 뒤에 붙인다.
        //
        // 앞에 넣으면 먼저 만든 코디가 뒤로 밀려, 보고 있던 카드가 매번 다른 번호로
        // 옮겨 간다. 룩북은 앞에서부터 채워지는 편이 읽기 쉽다 — 01번은 계속 01번이다.
        // 대신 새로 만든 첫 장으로 바로 넘겨 늘어난 것이 눈에 보이게 한다.
        const firstNew = looksRef.current.length;
        setLooks((prev) => [...prev, ...added]);
        setActiveLook(firstNew);
      }

      // 새로 쌓인 카드의 이미지를 미리 받아 둔다. 썸네일이 비어 있으면 비교할 수가
      // 없기 때문이다. 한꺼번에 몰아 보내지 않고 하나씩 — 생성이 수십 초씩 걸린다.
      // 저장이 먼저다. 이미지 생성은 한 장에 수십 초가 걸리는데, 그 뒤에 저장을
      // 두면 두 번째·세 번째 코디는 1분 넘게 저장되지 않는다. 그 사이에 화면을
      // 벗어나면 비용을 들여 만든 코디가 그냥 사라진다. DB 쓰기는 금방이므로
      // 전부 저장한 뒤에 이미지를 받는다.
      void (async () => {
        await Promise.all(
          added.map(async (l) => {
            try {
              const saved = await api.lookbook.save({
                mood_key: l.moodKey,
                title: l.title,
                weather_summary: r.weather_summary,
                reason: l.reason,
                recommendation: l.recommendation,
                image_prompt: buildImagePrompt(outfitToImageItems(l.outfit)),
                outfit_json: l.outfit,
              });
              setLooks((prev) =>
                prev.map((x) => (x.key === l.key ? { ...x, savedId: saved.id } : x)),
              );
            } catch {
              // 저장에 실패해도 화면은 그대로 쓴다.
            }
          }),
        );

        // 이미지는 하나씩 — 한꺼번에 몰아 보내면 생성이 서로 밀린다.
        for (const l of added) {
          // 받아 온 이미지는 캐시가 구독자에게 알리므로 여기서 다시 그릴 필요가 없다.
          await prefetchOutfitImage(outfitToImageItems(l.outfit), l.moodKey);
        }
      })();
    } catch (err) {
      setRecError(errorMessage(err));
    } finally {
      setRecLoading(false);
    }
  };

  /** 취향 신호. 카드를 넘겨 본 것과 구분해, 버튼을 눌렀을 때만 보낸다. */
  const sendFeedback = async (look: Look, type: 'like' | 'worn') => {
    const slot = (category: string) => look.outfit.find((o) => o.category === category)?.name;
    setLooks((prev) =>
      prev.map((l) =>
        l.key === look.key ? { ...l, liked: type === 'like' || l.liked, worn: type === 'worn' || l.worn } : l,
      ),
    );
    try {
      if (look.savedId) {
        await api.lookbook.mark(look.savedId, {
          liked: type === 'like' || undefined,
          worn: type === 'worn' || undefined,
        });
      }
      await api.feedback({
        feedback_type: type,
        reasons: [],
        inner_name: slot('상의'),
        outer_name: slot('아우터'),
        bottom_name: slot('하의'),
        shoes_name: slot('신발'),
        bag_name: slot('가방'),
      });
    } catch {
      // 취향 기록 실패로 화면을 되돌리지는 않는다 — 다음 추천에 덜 반영될 뿐이다.
    }
  };

  // 특정 코디를 열러 들어왔으면 그 영역까지 데려다준다.
  useEffect(() => {
    if (!openLookId || looks.length === 0) return;
    recSectionRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }, [openLookId, looks.length]);

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
        <button className="cta-primary" onClick={() => void loadAndScroll()} disabled={recLoading}>
          {/* 한 번 생성하면 모드 셋(오늘의 추천·다른 조합·안 입은 옷)이 함께 쌓인다.
              "한 장 더" 라고 적으면 세 장이 늘어나는 것과 어긋난다. */}
          {recLoading ? '코디를 고르는 중…' : looks.length === 0 ? '오늘의 룩북 만들기' : '코디 더 만들기'}
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
        {recLoading && <Loading>오늘의 조합을 고르는 중...</Loading>}
        {recError && <ErrorMessage>추천을 불러올 수 없습니다: {recError}</ErrorMessage>}

        {!recLoading && !recError && looks.length === 0 && (
          <>
            <div className="section-head">
              <h2 className="section-title">오늘의 제안</h2>
            </div>
            <p className="prose">
              위의 룩북 만들기를 누르면 오늘 날씨와 고른 장르에 맞춰 옷장에서 조합을 골라
              드려요. 만든 코디는 여기에 쌓여서, 나중에 다시 넘겨 보며 고를 수 있어요.
            </p>
          </>
        )}

        {looks.length > 0 && (
          <div className="section-head" style={{ marginBottom: 0 }}>
            <span />
            <button className="link-action" onClick={() => navigate('/lookbook')}>
              내 룩북 →
            </button>
          </div>
        )}

        {looks.length > 0 && (
          <LookbookStack
            looks={looks}
            activeIndex={Math.min(activeLook, looks.length - 1)}
            onSelect={setActiveLook}
            onLike={(l) => void sendFeedback(l, 'like')}
            onWearToday={(l) => void sendFeedback(l, 'worn')}
          />
        )}
      </section>

    </>
  );
}
