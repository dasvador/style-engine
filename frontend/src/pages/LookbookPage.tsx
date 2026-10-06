import { useEffect, useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api } from '../api/endpoints';
import { errorMessage } from '../api/client';
import type { SavedLook } from '../types/api';
import { ScreenHeader } from '../components/ScreenHeader';
import { EmptyState, ErrorMessage, Loading } from '../components/Status';

const FILTERS = [
  { key: 'all', label: '전체' },
  { key: 'liked', label: '마음에 들어요' },
  { key: 'worn', label: '오늘 입을래요' },
] as const;

type FilterKey = (typeof FILTERS)[number]['key'];

/**
 * 모아 둔 코디.
 *
 * 홈의 룩북은 "지금 고르는" 화면이라 만든 순서대로 넘겨 본다. 여기는 "지난 것을
 * 돌아보는" 화면이라 최신순 격자로 둔다. 이미지는 저장해 둔 프롬프트로 요청하므로
 * 서버 캐시에 걸려 새로 만들지 않는다.
 */
export function LookbookPage() {
  const navigate = useNavigate();
  const [looks, setLooks] = useState<SavedLook[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<FilterKey>('all');

  useEffect(() => {
    void (async () => {
      try {
        // 서버가 이미 만들어 둔 그림의 경로까지 함께 준다. 여기서 이미지를
        // 요청하지 않는다 — 모아 보는 화면을 여는 것만으로 생성이 돌면 안 된다.
        setLooks(await api.lookbook.list());
      } catch (err) {
        setError(errorMessage(err));
        setLooks([]);
      }
    })();
  }, []);

  const shown = useMemo(() => {
    if (!looks) return [];
    if (filter === 'liked') return looks.filter((l) => l.liked);
    if (filter === 'worn') return looks.filter((l) => l.worn);
    return looks;
  }, [looks, filter]);

  const counts = useMemo(
    () => ({
      all: looks?.length ?? 0,
      liked: looks?.filter((l) => l.liked).length ?? 0,
      worn: looks?.filter((l) => l.worn).length ?? 0,
    }),
    [looks],
  );

  return (
    <>
      <ScreenHeader title="내 룩북" sub={looks ? `${counts.all}장` : undefined} />

      {error && <ErrorMessage>{error}</ErrorMessage>}
      {looks === null && <Loading>모아 둔 코디를 불러오는 중...</Loading>}

      {looks !== null && counts.all === 0 && (
        <EmptyState
          icon="Lookbook"
          text="아직 만든 코디가 없습니다. 홈에서 오늘의 룩북을 만들면 여기에 쌓입니다."
        />
      )}

      {counts.all > 0 && (
        <>
          <div className="filter-bar">
            {FILTERS.map((f) => (
              <button
                key={f.key}
                className={`filter-chip${filter === f.key ? ' active' : ''}`}
                onClick={() => setFilter(f.key)}
              >
                {f.label} {counts[f.key]}
              </button>
            ))}
          </div>

          {shown.length === 0 ? (
            <EmptyState icon="Lookbook" text="이 조건에 맞는 코디가 없습니다." />
          ) : (
            <div className="saved-grid">
              {shown.map((l) => (
                <LookCard key={l.id} look={l} onOpen={() => navigate('/')} />
              ))}
            </div>
          )}
        </>
      )}
    </>
  );
}

function LookCard({ look, onOpen }: { look: SavedLook; onOpen: () => void }) {
  const url = look.image_path;
  return (
    <button className="saved-card" onClick={onOpen}>
      {url ? (
        <img className="saved-thumb" src={url} alt="" />
      ) : (
        <span className="saved-thumb-empty">사진 없음</span>
      )}
      <span className="saved-card-title">{look.title}</span>
      <span className="saved-card-meta">{formatDate(look.created_at)}</span>
      <span className="saved-card-items">
        {look.outfit_json.map((o) => o.name).slice(0, 2).join(' · ')}
      </span>
      {(look.liked || look.worn) && (
        <span className="saved-card-marks">
          {look.liked && <i className="mark-liked" title="마음에 들어요" />}
          {look.worn && <i className="mark-worn" title="오늘 입을래요" />}
        </span>
      )}
    </button>
  );
}

function formatDate(iso: string): string {
  const d = new Date(iso.includes('Z') || iso.includes('+') ? iso : `${iso}Z`);
  if (Number.isNaN(d.getTime())) return '';
  return `${d.getMonth() + 1}월 ${d.getDate()}일`;
}
