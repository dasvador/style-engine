import { useMemo, useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { api } from '../api/endpoints';
import { useAsync } from '../hooks/useAsync';
import { REACTION_LABEL, WEAR_STATUS_LABEL, outfitErrorMessage } from '../lib/outfitRecord';
import type { WearHistory, WearReaction, WearRecord, WearRecordUpdate, WearStatus } from '../types/api';
import { ScreenHeader } from '../components/ScreenHeader';
import { EmptyState, ErrorMessage, Loading } from '../components/Status';

const FILTERS: { key: WearStatus | 'all'; label: string }[] = [
  { key: 'all', label: '전체' },
  { key: 'planned', label: '입을 계획' },
  { key: 'worn', label: '입음' },
  { key: 'cancelled', label: '취소' },
];

/**
 * 입을 계획과 실제로 입은 기록.
 *
 * 계획은 착용이 아니다. '입었어요' 를 눌러야 입은 기록이 되고, 그 기록만 다음 추천에 쓰인다.
 * 룩북의 옛 '오늘 입을래요' 표시는 날짜가 없어 여기로 옮기지 않았다.
 */
export function WearPage() {
  const navigate = useNavigate();
  const history = useAsync<WearHistory>(() => api.outfits.history({ limit: 100 }), []);
  const [filter, setFilter] = useState<WearStatus | 'all'>('all');
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const records = useMemo(() => history.data?.records ?? [], [history.data]);
  const shown = filter === 'all' ? records : records.filter((r) => r.status === filter);

  /** 기록을 바꾸고 다시 읽는다. 실패해도 다시 읽어 그사이 바뀐 값을 보여 준다. */
  const run = async (record: WearRecord, action: () => Promise<unknown>, done: string) => {
    setBusy(record.record_id);
    setMessage(null);
    try {
      await action();
      setMessage(done);
    } catch (err) {
      setMessage(outfitErrorMessage(err));
    } finally {
      setBusy(null);
      await history.reload();
    }
  };

  const update = (record: WearRecord, change: Omit<WearRecordUpdate, 'record_id' | 'expected_revision'>, done: string) =>
    run(
      record,
      () => api.outfits.update({ record_id: record.record_id, expected_revision: record.revision, ...change }),
      done,
    );

  return (
    <>
      <ScreenHeader title="코디 기록" sub={history.data ? `${records.length}건` : undefined} />
      <p className="wear-note">
        '오늘 입을래요' 는 입을 계획으로 남아요. 실제로 입었다면 '입었어요' 를 눌러 주세요. 입은 기록만 다음 추천에 조금
        반영돼요.
      </p>

      {history.loading && !history.data && <Loading>기록을 불러오는 중...</Loading>}
      {history.error && <ErrorMessage onRetry={() => void history.reload()}>{history.error}</ErrorMessage>}
      {message && (
        <div className="msg msg-loading" role="status">
          {message}
        </div>
      )}

      {history.data && records.length === 0 && (
        <>
          <EmptyState icon="Record" text="아직 기록이 없어요. 홈에서 코디를 고르고 '오늘 입을래요' 를 눌러 보세요." />
          <button className="btn btn-secondary btn-lg" onClick={() => navigate('/')}>
            홈으로
          </button>
        </>
      )}

      {records.length > 0 && (
        <div className="filter-bar">
          {FILTERS.map((f) => (
            <button
              key={f.key}
              className={`filter-chip${filter === f.key ? ' active' : ''}`}
              onClick={() => setFilter(f.key)}
            >
              {f.label}
            </button>
          ))}
        </div>
      )}

      {shown.map((r) => (
        <RecordCard
          key={`${r.record_id}-${r.revision}`}
          record={r}
          today={history.data?.today ?? ''}
          disabled={busy !== null}
          onConfirm={(date) =>
            run(
              r,
              () => api.outfits.confirm(r.record_id, r.revision, date ? { date } : {}),
              '입은 기록으로 남겼어요.',
            )
          }
          onCancel={() => update(r, { action: 'cancel' }, '입을 계획을 취소했어요.')}
          onUndo={() => update(r, { action: 'undo' }, '입은 기록을 되돌렸어요.')}
          onEdit={(reaction, comment) =>
            update(r, { action: 'edit', reaction, comment }, '기록을 고쳤어요.')
          }
        />
      ))}
      {history.data?.has_more && <p className="wear-note">오래된 기록은 더 있어요. 최근 100건만 보여 드려요.</p>}
    </>
  );
}

function RecordCard({
  record,
  today,
  disabled,
  onConfirm,
  onCancel,
  onUndo,
  onEdit,
}: {
  record: WearRecord;
  today: string;
  disabled: boolean;
  onConfirm: (date?: string) => void;
  onCancel: () => void;
  onUndo: () => void;
  onEdit: (reaction: WearReaction | null, comment: string | null) => void;
}) {
  const [reaction, setReaction] = useState<WearReaction | ''>(record.reaction ?? '');
  const [comment, setComment] = useState(record.comment ?? '');
  const future = record.date > today;
  const changed = reaction !== (record.reaction ?? '') || comment.trim() !== (record.comment ?? '');

  return (
    <article className={`wear-card ${record.status}`}>
      <header className="wear-card-head">
        <strong>
          {record.date}
          {record.date === today ? ' · 오늘' : ''}
        </strong>
        <span className={`wear-status ${record.status}`}>{WEAR_STATUS_LABEL[record.status]}</span>
      </header>

      <ul className="rec-pieces">
        {record.items.map((i) => (
          <li className="rec-piece" key={i.id}>
            <span className="rec-piece-slot">{i.category}</span>
            <span className="rec-piece-name">{i.name}</span>
          </li>
        ))}
      </ul>

      {(record.occasion || record.temperature !== null) && (
        <p className="wear-note">
          {[record.occasion, record.temperature !== null ? `${record.temperature}°C 기준` : null]
            .filter(Boolean)
            .join(' · ')}
        </p>
      )}

      {record.status !== 'worn' && record.comment && <p className="wear-note">메모: {record.comment}</p>}

      {record.status === 'worn' && (
        <div className="wear-edit">
          <select
            value={reaction}
            aria-label="입어 본 반응"
            onChange={(e) => setReaction(e.target.value as WearReaction | '')}
          >
            <option value="">반응 없음</option>
            {Object.entries(REACTION_LABEL).map(([k, label]) => (
              <option key={k} value={k}>
                {label}
              </option>
            ))}
          </select>
          <input
            value={comment}
            maxLength={200}
            placeholder="한 줄 메모"
            aria-label="한 줄 메모"
            onChange={(e) => setComment(e.target.value)}
          />
          <button
            className="btn btn-secondary"
            disabled={disabled || !changed}
            onClick={() => onEdit(reaction || null, comment.trim() || null)}
          >
            저장
          </button>
        </div>
      )}

      <Link className="link-action wear-open" to={`/outfits/${encodeURIComponent(record.record_id)}`}>
        코디 보기 →
      </Link>

      <div className="wear-actions">
        {record.status === 'planned' && (
          <>
            <button className="btn btn-primary" disabled={disabled} onClick={() => onConfirm(future ? today : undefined)}>
              {future ? '오늘 입었어요' : '입었어요'}
            </button>
            <button className="btn btn-secondary" disabled={disabled} onClick={onCancel}>
              계획 취소
            </button>
          </>
        )}
        {record.status === 'worn' && (
          <button className="btn btn-secondary" disabled={disabled} onClick={onUndo}>
            입은 기록 되돌리기
          </button>
        )}
      </div>
    </article>
  );
}
