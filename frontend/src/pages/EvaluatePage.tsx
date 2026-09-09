import { useEffect, useRef, useState } from 'react';
import { useLocation } from 'react-router-dom';
import { api } from '../api/endpoints';
import { errorMessage } from '../api/client';
import type { Clothing, EvaluateBody, EvaluateResponse } from '../types/api';
import { ScreenHeader } from '../components/ScreenHeader';
import { ErrorMessage } from '../components/Status';
import { scoreClass } from '../lib/lookbook';

/** 평가 슬롯 정의. key 는 요청 본문의 필드명과 같다. */
const SLOTS = [
  { key: 'top', label: '상의', category: '상의' },
  { key: 'bottom', label: '하의', category: '하의' },
  { key: 'outer', label: '아우터', category: '아우터' },
  { key: 'shoes', label: '신발', category: '신발' },
  { key: 'bag', label: '가방', category: '가방' },
] as const;

type SlotKey = (typeof SLOTS)[number]['key'];
type Selection = Partial<Record<SlotKey, string>>;

/** 상세 화면에서 '이 옷으로 평가'로 넘어올 때 전달되는 상태. */
export interface EvaluatePrefill {
  slot: SlotKey;
  id: string;
}

export function EvaluatePage({ clothes }: { clothes: Clothing[] }) {
  const location = useLocation();
  const prefill = (location.state as { prefill?: EvaluatePrefill } | null)?.prefill;

  const [selection, setSelection] = useState<Selection>({});
  const [result, setResult] = useState<EvaluateResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const resultRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (prefill) {
      setSelection((prev) => ({ ...prev, [prefill.slot]: prefill.id }));
    }
  }, [prefill]);

  useEffect(() => {
    if (result) resultRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }, [result]);

  const evaluate = async () => {
    const body: EvaluateBody = {};
    let filled = 0;
    for (const s of SLOTS) {
      const v = selection[s.key];
      if (v) {
        body[s.key] = v;
        filled += 1;
      }
    }

    if (filled < 2) {
      setResult(null);
      setError('최소 2개 이상의 아이템을 선택해주세요.');
      return;
    }

    setLoading(true);
    setError(null);
    setResult(null);
    try {
      setResult(await api.outfit.evaluate(body));
    } catch (err) {
      setError(`평가 실패: ${errorMessage(err)}`);
    } finally {
      setLoading(false);
    }
  };

  return (
    <>
      <ScreenHeader title="코디 평가" sub="입은 옷의 조합을 살펴봅니다" />

      <div className="card">
        {SLOTS.map((s) => {
          const picked = clothes.find((c) => c.id === selection[s.key]);
          const thumb = picked?.image_url?.startsWith('data:image/') ? picked.image_url : null;
          return (
            <div className={`slot-row${picked ? ' filled' : ''}`} key={s.key}>
              {/* 고른 옷이 있으면 썸네일로, 없으면 슬롯 자리를 비워 두지 않는다. */}
              {thumb ? (
                <img className="slot-thumb" src={thumb} alt="" />
              ) : (
                <div className="slot-thumb-empty" aria-hidden="true">
                  {s.label}
                </div>
              )}
              <div className="slot-body">
                <div className="slot-label">{s.label}</div>
                <select
                  className="slot-select"
                  aria-label={`${s.label} 선택`}
                  value={selection[s.key] ?? ''}
                  onChange={(e) => setSelection((prev) => ({ ...prev, [s.key]: e.target.value }))}
                >
                  <option value="">선택 안함</option>
                  {clothes
                    .filter((c) => c.category === s.category)
                    .map((c) => (
                      <option value={c.id} key={c.id}>
                        {c.name}
                      </option>
                    ))}
                </select>
              </div>
            </div>
          );
        })}

        <button
          className="btn btn-primary btn-lg"
          style={{ marginTop: 18 }}
          onClick={() => void evaluate()}
          disabled={loading}
        >
          {loading ? (
            <>
              <span className="spinner" /> 평가 중...
            </>
          ) : (
            <>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                <path d="M9 12l2 2 4-4" />
                <circle cx="12" cy="12" r="10" />
              </svg>
              평가하기
            </>
          )}
        </button>
      </div>

      <div ref={resultRef}>
        {error && (
          <div className="card">
            <ErrorMessage>{error}</ErrorMessage>
          </div>
        )}
        {result && <EvaluationResult result={result} />}
      </div>
    </>
  );
}

function EvaluationResult({ result }: { result: EvaluateResponse }) {
  return (
    <>
      <div className="verdict">
        <div className="verdict-score">
          {result.score}
          <em>/100</em>
        </div>
        <div className={`verdict-label ${scoreClass(result.score)}`}>{result.verdict_label}</div>
        <div className="verdict-rule" />
        <p className="eval-summary">{result.summary}</p>
      </div>

      {result.strengths.length > 0 && (
        <div className="card">
          <div className="card-title">잘 어울리는 점</div>
          {result.strengths.map((s, i) => (
            <div className="strength-item" key={`${s.rule}-${i}`}>
              <span className="strength-icon">✔</span>
              <div>
                <strong>{s.rule}</strong>
                <br />
                <span className="item-detail-text">{s.detail}</span>
              </div>
            </div>
          ))}
        </div>
      )}

      {result.problems.length > 0 && (
        <div className="card">
          <div className="card-title">아쉬운 점</div>
          {result.problems.map((p, i) => (
            <div className="problem-item" key={`${p.code}-${i}`}>
              <span className="problem-icon">⚠</span>
              <div style={{ flex: 1 }}>
                <strong>{p.rule}</strong>
                <br />
                <span className="item-detail-text">{p.detail}</span>
              </div>
              <span className="problem-deduction">-{p.deduction}</span>
            </div>
          ))}
        </div>
      )}

      {result.suggestions.length > 0 && (
        <div className="card">
          <div className="card-title">이렇게 바꿔 보세요</div>
          {result.suggestions.map((s, i) => (
            <div className="suggestion-card" key={`${s.type}-${i}`}>
              <div className="suggestion-type">{s.type}</div>
              <div className="suggestion-reason">{s.reason}</div>
              {s.recommended_examples.length > 0 && (
                <div className="suggestion-examples">
                  {s.recommended_examples.map((ex) => (
                    <span className="chip chip-neutral" key={ex}>
                      {ex}
                    </span>
                  ))}
                </div>
              )}
            </div>
          ))}
        </div>
      )}

      {result.explanation && (
        <div className="card">
          <div className="card-title">에디터 노트</div>
          {/* LLM 생성 문자열 — 텍스트로만 렌더링한다. */}
          <div className="explanation-block">{result.explanation}</div>
        </div>
      )}
    </>
  );
}
