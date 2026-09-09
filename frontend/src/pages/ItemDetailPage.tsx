import { useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { api } from '../api/endpoints';
import { errorMessage } from '../api/client';
import type { Clothing } from '../types/api';
import { ErrorMessage } from '../components/Status';
import { THICKNESS_LABEL, getRoleExplanation } from '../lib/lookbook';

/** 카테고리 → 평가 화면 슬롯. '이 옷으로 평가'에 쓴다. */
const CATEGORY_TO_SLOT: Record<string, 'top' | 'bottom' | 'outer' | 'shoes' | 'bag'> = {
  상의: 'top',
  하의: 'bottom',
  아우터: 'outer',
  신발: 'shoes',
  가방: 'bag',
};

export function ItemDetailPage({ clothes, onReload }: { clothes: Clothing[]; onReload: () => Promise<void> }) {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const item = clothes.find((c) => c.id === id);

  if (!item) {
    return (
      <>
        <DetailHeader title="" onBack={() => navigate('/wardrobe')} onDelete={null} />
        <ErrorMessage>아이템을 찾을 수 없습니다.</ErrorMessage>
      </>
    );
  }

  const remove = async () => {
    if (!window.confirm('정말 삭제하시겠습니까?')) return;
    setDeleting(true);
    setError(null);
    try {
      await api.clothes.remove(item.id);
      await onReload();
      navigate('/wardrobe');
    } catch (err) {
      setError(`삭제 실패: ${errorMessage(err)}`);
      setDeleting(false);
    }
  };

  const goToEvaluate = () => {
    const slot = CATEGORY_TO_SLOT[item.category];
    navigate('/evaluate', slot ? { state: { prefill: { slot, id: item.id } } } : undefined);
  };

  const basicRows: [string, string][] = [
    ['카테고리', item.category],
    ['색상', item.color ?? '-'],
    ['두께', THICKNESS_LABEL[item.thickness] ?? item.thickness],
    ['계절', item.seasons.length ? item.seasons.join(', ') : '-'],
  ];

  const tagRows: [string, string | null][] = [
    ['역할', item.role],
    ['톤', item.tone],
    ['채도', item.saturation],
    ['색온도', item.color_temperature],
    ['활용도', item.versatility != null ? `${item.versatility}/5` : null],
    ['존재감', item.statement_level != null ? `${item.statement_level}/5` : null],
    ['격식', item.formality_level != null ? `${item.formality_level}/5` : null],
    ['스타일', item.style],
    ['무게감', item.weight],
  ];

  return (
    <>
      <DetailHeader title={item.name} onBack={() => navigate('/wardrobe')} onDelete={deleting ? null : remove} />

      {error && <ErrorMessage>{error}</ErrorMessage>}

      {item.image_url?.startsWith('data:image/') && (
        <img className="detail-image" src={item.image_url} alt={item.name} />
      )}

      <div className="card">
        <div className="card-title">기본 정보</div>
        {basicRows.map(([label, value]) => (
          <div className="detail-row" key={label}>
            <span className="detail-label">{label}</span>
            <span className="detail-value">{value}</span>
          </div>
        ))}
      </div>

      <div className="card">
        <div className="card-title">스타일 분석</div>
        {item.role && (
          <div className="role-explain" style={{ marginBottom: 12 }}>
            {getRoleExplanation(item.role)}
          </div>
        )}
        {tagRows.map(([label, value]) =>
          value ? (
            <div className="detail-row" key={label}>
              <span className="detail-label">{label}</span>
              <span className="detail-value">{value}</span>
            </div>
          ) : null,
        )}
        {item.texture_worlds.length > 0 && (
          <div className="detail-row">
            <span className="detail-label">텍스처</span>
            <span className="detail-value">
              {item.texture_worlds.map((tw) => (
                <span className="chip chip-neutral" style={{ marginRight: 4 }} key={tw}>
                  {tw}
                </span>
              ))}
            </span>
          </div>
        )}
      </div>

      <button className="btn btn-primary btn-lg" style={{ marginBottom: 12 }} onClick={goToEvaluate}>
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
          <path d="M9 12l2 2 4-4" />
          <circle cx="12" cy="12" r="10" />
        </svg>
        이 옷으로 코디 평가
      </button>
    </>
  );
}

function DetailHeader({
  title,
  onBack,
  onDelete,
}: {
  title: string;
  onBack: () => void;
  onDelete: (() => void) | null;
}) {
  return (
    <div className="detail-header">
      <button className="detail-back" onClick={onBack}>
        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <path d="M15 18l-6-6 6-6" />
        </svg>
      </button>
      <span className="detail-title">{title}</span>
      {onDelete && (
        <button className="btn btn-danger btn-sm" onClick={() => void onDelete()}>
          삭제
        </button>
      )}
    </div>
  );
}
