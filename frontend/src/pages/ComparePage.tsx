import { useEffect, useRef, useState } from 'react';
import { useLocation, useNavigate, useParams } from 'react-router-dom';
import { api } from '../api/endpoints';
import { ApiError, errorMessage } from '../api/client';
import type {
  Clothing,
  Comparison,
  ComparisonChoice,
  ComparisonItem,
  ComparisonReason,
  ComparisonVote,
  StyleMood,
} from '../types/api';
import { ScreenHeader } from '../components/ScreenHeader';
import { Loading } from '../components/Status';
import { swatchesFor } from '../lib/colorSwatch';

/** 이유 목록. A·B 를 고르면 "고른 이유", 둘 다 아쉬우면 "아쉬운 점" 으로 읽힌다. */
const REASONS: { code: ComparisonReason; label: string }[] = [
  { code: 'color_combo', label: '색 조합' },
  { code: 'silhouette', label: '실루엣' },
  { code: 'personal_taste', label: '내 취향' },
  { code: 'practicality', label: '실용성/착용 편의' },
  { code: 'weather_fit', label: '날씨 적합성' },
];

const COMMENT_MAX = 200;

/** 두 코디를 같은 순서로 늘어놓는다 — 위에서 아래로 입는 순서. */
const CATEGORY_ORDER = ['아우터', '상의', '하의', '신발', '가방'];

function sortItems(items: ComparisonItem[]): ComparisonItem[] {
  const rank = (c: string) => {
    const i = CATEGORY_ORDER.indexOf(c);
    return i < 0 ? CATEGORY_ORDER.length : i;
  };
  return [...items].sort((a, b) => rank(a.category) - rank(b.category));
}

function choiceSummary(choice: ComparisonChoice): string {
  if (choice === 'a') return 'A 코디를 골랐어요.';
  if (choice === 'b') return 'B 코디를 골랐어요.';
  return '둘 다 아쉽다고 남겼어요.';
}

interface Props {
  clothes: Clothing[];
  moods: StyleMood[];
}

type LoadState =
  | { phase: 'loading' }
  | { phase: 'missing' }
  | { phase: 'error'; message: string }
  | { phase: 'ready'; comparison: Comparison };

/**
 * 홈 추천에서 나온 두 코디 중 더 끌리는 쪽을 고른다.
 *
 * 비교할 코디는 서버가 그 추천 기록에서 꺼내고, 좌우도 서버가 처음 한 번 정한다. 화면은
 * 추천 id 만 보낸다. 비교를 여는 순간 두 코디를 보여 줬다는 기록(노출)은 서버에 남는다.
 * 고르는 것은 선택이다 — 건너뛰면 선택 결과(응답)만 남지 않는다.
 *
 * 생성 이미지(모델 얼굴·배경)는 쓰지 않는다. 옷 이름·카테고리·색으로 두 코디를 똑같은
 * 모양으로 보여 주고, 옷장에 그 옷의 사진이 그대로 있을 때만 작은 사진을 곁들인다.
 */
export function ComparePage({ clothes, moods }: Props) {
  const navigate = useNavigate();
  const { id } = useParams<{ id: string }>();
  const batchId = (useLocation().state as { batchId?: string } | null)?.batchId;

  const [load, setLoad] = useState<LoadState>({ phase: 'loading' });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let alive = true;
    // 비교 id 로 들어오면 늘 서버에서 다시 읽는다. 브라우저 기록에 담아 둔 비교를 쓰면
    // 새로고침 때 저장 전 모습이 되살아난다.
    if (!id && !batchId) {
      setLoad({ phase: 'missing' });
      return;
    }
    setLoad({ phase: 'loading' });
    void (async () => {
      try {
        if (id) {
          const c = await api.comparisons.get(id);
          if (alive) setLoad({ phase: 'ready', comparison: c });
        } else if (batchId) {
          // 여는 순간 서버가 노출을 남긴다. 주소를 비교 id 로 바꿔 새로고침해도 같은 비교가 열린다.
          const c = await api.comparisons.open(batchId);
          if (!alive) return;
          navigate(`/compare/${c.id}`, { replace: true });
        }
      } catch (err) {
        if (alive) setLoad({ phase: 'error', message: errorMessage(err) });
      }
    })();
    return () => {
      alive = false;
    };
  }, [id, batchId, attempt, navigate]);

  return (
    <>
      <ScreenHeader title="둘 중 고르기" sub="이번 추천에서 나온 두 코디예요" />

      {load.phase === 'loading' && <Loading>비교할 코디를 준비하는 중...</Loading>}

      {load.phase === 'missing' && (
        <div className="compare-status">
          <p className="prose">홈에서 코디를 만든 뒤 ‘둘 중 고르기’를 눌러 주세요.</p>
          <button className="btn btn-outline btn-sm" onClick={() => navigate('/')}>
            홈으로
          </button>
        </div>
      )}

      {load.phase === 'error' && (
        <div className="compare-status" role="alert">
          <p className="msg msg-error">비교를 열 수 없어요: {load.message}</p>
          <div className="compare-status-actions">
            <button className="btn btn-outline btn-sm" onClick={() => setAttempt((n) => n + 1)}>
              다시 시도
            </button>
            <button className="link-action" onClick={() => navigate('/')}>
              홈으로
            </button>
          </div>
        </div>
      )}

      {load.phase === 'ready' && (
        <CompareForm
          key={load.comparison.id}
          comparison={load.comparison}
          clothes={clothes}
          moods={moods}
          onDone={() => navigate('/')}
          // 이미 저장된 선택을 서버에서 다시 읽는다.
          onReload={() => setAttempt((n) => n + 1)}
        />
      )}
    </>
  );
}

function CompareForm({
  comparison,
  clothes,
  moods,
  onDone,
  onReload,
}: {
  comparison: Comparison;
  clothes: Clothing[];
  moods: StyleMood[];
  onDone: () => void;
  onReload: () => void;
}) {
  const [choice, setChoice] = useState<ComparisonChoice | null>(null);
  const [reasons, setReasons] = useState<ComparisonReason[]>([]);
  const [comment, setComment] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [conflict, setConflict] = useState(false);
  const [saved, setSaved] = useState<ComparisonVote | null>(comparison.vote);
  const resultRef = useRef<HTMLHeadingElement>(null);

  useEffect(() => {
    if (saved) resultRef.current?.focus();
  }, [saved]);

  const ctx = comparison.context;
  const moodLabel = moods.find((m) => m.mood_key === ctx.mood_key)?.mood_label;
  const contextLine = [
    moodLabel,
    ctx.temperature != null ? `${ctx.temperature}°C` : null,
    ctx.weather_description || null,
  ]
    .filter(Boolean)
    .join(' · ');

  const pick = (next: ComparisonChoice) => {
    // A·B 와 '둘 다 아쉬움' 은 이유의 뜻이 반대다. 방향이 바뀌면 고른 이유를 비운다.
    const polarity = (c: ComparisonChoice | null) => (c === 'neither' ? 'neg' : c ? 'pos' : null);
    if (polarity(next) !== polarity(choice)) setReasons([]);
    setChoice(next);
    setSubmitError(null);
  };

  const toggleReason = (code: ComparisonReason) =>
    setReasons((prev) => (prev.includes(code) ? prev.filter((r) => r !== code) : [...prev, code]));

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!choice || submitting) return;
    setSubmitting(true);
    setSubmitError(null);
    try {
      const trimmed = comment.trim();
      const res = await api.comparisons.vote(comparison.id, {
        choice,
        reasons,
        ...(trimmed ? { comment: trimmed } : {}),
      });
      setSaved(res.vote);
    } catch (err) {
      if (err instanceof ApiError && err.status === 409) {
        setConflict(true);
      } else {
        setSubmitError(errorMessage(err));
      }
    } finally {
      setSubmitting(false);
    }
  };

  if (saved) {
    const labels = REASONS.filter((r) => saved.reasons.includes(r.code)).map((r) => r.label);
    return (
      <section className="compare-result" aria-live="polite">
        <h2 className="section-title" tabIndex={-1} ref={resultRef}>
          고마워요, 저장했어요
        </h2>
        <p className="prose">
          {choiceSummary(saved.choice)}
          {labels.length > 0 &&
            ` ${saved.choice === 'neither' ? '아쉬운 점' : '이유'}: ${labels.join(', ')}.`}
        </p>
        {saved.comment && <p className="compare-result-comment">“{saved.comment}”</p>}
        <div className="compare-grid compare-grid-static">
          {comparison.options.map((o) => (
            <OptionCard
              key={o.slot}
              slot={o.slot}
              items={o.items}
              clothes={clothes}
              chosen={saved.choice === o.slot}
            />
          ))}
        </div>
        <button className="btn btn-primary btn-lg" onClick={onDone}>
          홈으로
        </button>
      </section>
    );
  }

  const negative = choice === 'neither';

  return (
    <form className="compare-form" onSubmit={(e) => void submit(e)}>
      {contextLine && <p className="compare-context">{contextLine}</p>}
      <p className="compare-note">
        정답은 없어요. 지금 내 취향에 더 가까운 쪽을 골라 주세요. 고른 기록은 내 취향 메모로만
        남아요.
      </p>

      <fieldset className="compare-choice">
        <legend className="visually-hidden">더 마음에 드는 코디</legend>
        <div className="compare-grid">
          {comparison.options.map((o) => (
            <label
              key={o.slot}
              className={`compare-option${choice === o.slot ? ' selected' : ''}`}
            >
              <input
                type="radio"
                name="choice"
                value={o.slot}
                checked={choice === o.slot}
                onChange={() => pick(o.slot)}
                className="visually-hidden"
                aria-label={`${o.slot.toUpperCase()} 코디가 더 좋아요`}
              />
              <OptionCard slot={o.slot} items={o.items} clothes={clothes} chosen={choice === o.slot} />
              <span className="compare-option-cta" aria-hidden="true">
                {choice === o.slot ? `${o.slot.toUpperCase()} 선택됨 ✓` : `${o.slot.toUpperCase()}가 더 좋아요`}
              </span>
            </label>
          ))}
        </div>
        <label className={`compare-neither${negative ? ' selected' : ''}`}>
          <input
            type="radio"
            name="choice"
            value="neither"
            checked={negative}
            onChange={() => pick('neither')}
          />
          <span>둘 다 아쉬워요</span>
        </label>
      </fieldset>

      {choice && (
        <>
          <fieldset className="compare-reasons">
            <legend className="compare-legend">
              {negative ? '둘 다 아쉬운 점 (선택)' : `${choice.toUpperCase()}를 고른 이유 (선택)`}
            </legend>
            <div className="compare-reason-list">
              {REASONS.map((r) => (
                <label
                  key={r.code}
                  className={`compare-reason${reasons.includes(r.code) ? ' on' : ''}`}
                >
                  <input
                    type="checkbox"
                    checked={reasons.includes(r.code)}
                    onChange={() => toggleReason(r.code)}
                  />
                  <span>{r.label}</span>
                </label>
              ))}
            </div>
          </fieldset>

          <div className="compare-comment">
            <label className="compare-legend" htmlFor="compare-comment">
              한 줄 메모 (선택)
            </label>
            <textarea
              id="compare-comment"
              className="form-input"
              rows={2}
              maxLength={COMMENT_MAX}
              value={comment}
              onChange={(e) => setComment(e.target.value)}
              placeholder={negative ? '예: 둘 다 오늘 날씨엔 무거워 보여요' : '예: 바지 핏이 더 편해 보여요'}
              aria-describedby="compare-comment-count"
            />
            <span id="compare-comment-count" className="compare-count">
              {comment.length}/{COMMENT_MAX}
            </span>
          </div>
        </>
      )}

      {conflict && (
        <div className="compare-status" role="alert">
          <p className="msg msg-error">이 비교에는 이미 다른 선택이 저장돼 있어요. 처음 고른 것이 남아요.</p>
          <button type="button" className="btn btn-outline btn-sm" onClick={onReload}>
            저장된 선택 보기
          </button>
        </div>
      )}
      {submitError && (
        <p className="msg msg-error" role="alert">
          저장하지 못했어요: {submitError} 다시 눌러 주세요.
        </p>
      )}

      <div className="compare-actions">
        <button type="submit" className="btn btn-primary btn-lg" disabled={!choice || submitting}>
          {submitting ? '저장하는 중…' : submitError ? '다시 저장하기' : '선택 저장'}
        </button>
        <button type="button" className="link-action" onClick={onDone}>
          건너뛰기
        </button>
        <p className="compare-skip-note">건너뛰면 선택 결과는 저장되지 않아요.</p>
      </div>
    </form>
  );
}

/** 한쪽 코디. A·B 가 완전히 같은 모양이어야 한다 — 자리 말고는 차이를 두지 않는다. */
function OptionCard({
  slot,
  items,
  clothes,
  chosen,
}: {
  slot: 'a' | 'b';
  items: ComparisonItem[];
  clothes: Clothing[];
  chosen: boolean;
}) {
  return (
    <article className={`compare-card${chosen ? ' chosen' : ''}`} aria-label={`코디 ${slot.toUpperCase()}`}>
      <h3 className="compare-card-head">{slot.toUpperCase()}</h3>
      <ul className="compare-items">
        {sortItems(items).map((item) => (
          <ItemRow key={item.id} item={item} clothes={clothes} />
        ))}
      </ul>
    </article>
  );
}

function ItemRow({ item, clothes }: { item: ComparisonItem; clothes: Clothing[] }) {
  // 옷장 사진은 그 옷이 그때와 같은 이름으로 남아 있을 때만 쓴다 — 바뀐 옷의 사진을
  // 그때 보여 준 코디처럼 내세우지 않는다.
  const current = item.has_image ? clothes.find((c) => c.id === item.id) : undefined;
  const thumb =
    current && current.name === item.name && current.image_url?.startsWith('data:image/')
      ? current.image_url
      : null;
  const dots = swatchesFor(item.color);
  return (
    <li className="compare-item">
      {thumb ? (
        <img className="compare-thumb" src={thumb} alt="" />
      ) : (
        <span className="compare-thumb compare-thumb-empty" aria-hidden="true">
          {dots.length > 0
            ? dots.map((hex, i) => <i key={i} className="compare-dot" style={{ background: hex }} />)
            : item.category.slice(0, 2)}
        </span>
      )}
      <span className="compare-item-text">
        <span className="compare-item-slot">{item.category}</span>
        <span className="compare-item-name">{item.name}</span>
        {item.color && <span className="compare-item-color">{item.color}</span>}
      </span>
    </li>
  );
}
