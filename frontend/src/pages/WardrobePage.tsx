import { useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api } from '../api/endpoints';
import { errorMessage } from '../api/client';
import type { Clothing, Thickness } from '../types/api';
import { ScreenHeader } from '../components/ScreenHeader';
import { EmptyState, ErrorMessage } from '../components/Status';
import { getRoleChipClass } from '../lib/lookbook';

const CATEGORY_FILTERS = ['전체', '상의', '하의', '아우터', '신발', '가방'];
const ROLE_FILTERS = ['전체', '베이스', '포인트', '약한포인트', '연결템', '구조템'];

interface Props {
  clothes: Clothing[];
  onReload: () => Promise<void>;
  addPanelOpen: boolean;
  setAddPanelOpen: (open: boolean) => void;
}

export function WardrobePage({ clothes, onReload, addPanelOpen, setAddPanelOpen }: Props) {
  const navigate = useNavigate();
  const [category, setCategory] = useState('전체');
  const [role, setRole] = useState('전체');

  const items = useMemo(
    () =>
      clothes.filter(
        (c) => (category === '전체' || c.category === category) && (role === '전체' || c.role === role),
      ),
    [clothes, category, role],
  );

  return (
    <>
      <ScreenHeader title="옷장" sub={`${items.length}벌`} />

      <div className="filter-bar">
        {CATEGORY_FILTERS.map((c) => (
          <button
            key={c}
            className={`filter-chip${c === category ? ' active' : ''}`}
            onClick={() => setCategory(c)}
          >
            {c}
          </button>
        ))}
      </div>
      <div className="filter-bar">
        {ROLE_FILTERS.map((r) => (
          <button key={r} className={`filter-chip${r === role ? ' active' : ''}`} onClick={() => setRole(r)}>
            {r}
          </button>
        ))}
      </div>

      {items.length === 0 ? (
        <EmptyState
          icon="👕"
          text={clothes.length === 0 ? '옷장이 비어있어요. 옷을 등록해보세요!' : '해당 조건의 아이템이 없습니다.'}
        />
      ) : (
        <div className="items-grid">
          {items.map((c) => (
            <div className="item-card" key={c.id} onClick={() => navigate(`/wardrobe/${c.id}`)}>
              {c.image_url?.startsWith('data:image/') && (
                <img
                  src={c.image_url}
                  alt=""
                  style={{ width: '100%', height: 80, objectFit: 'cover', borderRadius: 8, marginBottom: 8 }}
                />
              )}
              <div className="item-card-name">{c.name}</div>
              <div className="item-card-tags">
                {c.role && <span className={`chip ${getRoleChipClass(c.role)}`}>{c.role}</span>}
                {c.tone && <span className="chip chip-neutral">{c.tone}</span>}
                {c.style && <span className="chip chip-neutral">{c.style}</span>}
              </div>
              {c.seasons.length > 0 && (
                <div className="item-card-seasons">
                  {c.seasons.map((s) => (
                    <span className="chip chip-neutral" style={{ fontSize: '0.65rem' }} key={s}>
                      {s}
                    </span>
                  ))}
                </div>
              )}
            </div>
          ))}
        </div>
      )}

      <AddPanel open={addPanelOpen} onClose={() => setAddPanelOpen(false)} onAdded={onReload} />

      <button className="fab" onClick={() => setAddPanelOpen(!addPanelOpen)}>
        +
      </button>
    </>
  );
}

/** 옷 등록 패널 — 직접 입력 / 사진 분석 두 탭. */
function AddPanel({
  open,
  onClose,
  onAdded,
}: {
  open: boolean;
  onClose: () => void;
  onAdded: () => Promise<void>;
}) {
  const [tab, setTab] = useState<'manual' | 'image'>('manual');

  return (
    <div className={`add-panel${open ? ' open' : ''}`}>
      <div className="add-panel-header">
        <span className="add-panel-title">옷 등록하기</span>
        <button className="add-panel-close" onClick={onClose}>
          ×
        </button>
      </div>
      <div className="tab-toggle">
        <button
          className={`tab-toggle-item${tab === 'manual' ? ' active' : ''}`}
          onClick={() => setTab('manual')}
        >
          직접 입력
        </button>
        <button className={`tab-toggle-item${tab === 'image' ? ' active' : ''}`} onClick={() => setTab('image')}>
          사진 분석
        </button>
      </div>

      {tab === 'manual' ? (
        <ManualForm onClose={onClose} onAdded={onAdded} />
      ) : (
        <ImageUpload onClose={onClose} onAdded={onAdded} />
      )}
    </div>
  );
}

function ManualForm({ onClose, onAdded }: { onClose: () => void; onAdded: () => Promise<void> }) {
  const [name, setName] = useState('');
  const [category, setCategory] = useState('');
  const [thickness, setThickness] = useState<Thickness>('medium');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setSaving(true);
    setError(null);
    try {
      await api.clothes.create({ name, category, thickness });
      setName('');
      setCategory('');
      setThickness('medium');
      onClose();
      await onAdded();
    } catch (err) {
      setError(`추가 실패: ${errorMessage(err)}`);
    } finally {
      setSaving(false);
    }
  };

  return (
    <form onSubmit={submit}>
      {error && <ErrorMessage>{error}</ErrorMessage>}
      <div className="form-group">
        <label className="form-label">이름</label>
        <input
          className="form-input"
          type="text"
          placeholder="예: 네이비 옥스포드 셔츠"
          value={name}
          onChange={(e) => setName(e.target.value)}
          required
        />
      </div>
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8 }}>
        <div className="form-group">
          <label className="form-label">카테고리</label>
          <select
            className="form-select"
            value={category}
            onChange={(e) => setCategory(e.target.value)}
            required
          >
            <option value="">선택</option>
            <option value="상의">상의</option>
            <option value="하의">하의</option>
            <option value="아우터">아우터</option>
            <option value="신발">신발</option>
            <option value="가방">가방</option>
          </select>
        </div>
        <div className="form-group">
          <label className="form-label">두께</label>
          <select
            className="form-select"
            value={thickness}
            onChange={(e) => setThickness(e.target.value as Thickness)}
          >
            <option value="medium">보통</option>
            <option value="thin">얇은</option>
            <option value="thick">두꺼운</option>
          </select>
        </div>
      </div>
      <button type="submit" className="btn btn-primary btn-lg" disabled={saving}>
        {saving ? '등록 중...' : '등록하기'}
      </button>
    </form>
  );
}

function ImageUpload({ onClose, onAdded }: { onClose: () => void; onAdded: () => Promise<void> }) {
  const [imageData, setImageData] = useState<string | null>(null);
  const [uploading, setUploading] = useState(false);
  const [status, setStatus] = useState<{ ok: boolean; text: string } | null>(null);
  const [dragging, setDragging] = useState(false);

  const readFile = (file: File) => {
    const reader = new FileReader();
    reader.onload = (e) => {
      const result = e.target?.result;
      if (typeof result === 'string') {
        setImageData(result);
        setStatus(null);
      }
    };
    reader.readAsDataURL(file);
  };

  const upload = async () => {
    if (!imageData) return;
    setUploading(true);
    setStatus(null);
    try {
      const created = await api.clothes.upload({ image_data: imageData });
      setStatus({ ok: true, text: `${created.name} (${created.category}) 등록 완료!` });
      setImageData(null);
      onClose();
      await onAdded();
    } catch (err) {
      setStatus({ ok: false, text: `분석 실패: ${errorMessage(err)}` });
    } finally {
      setUploading(false);
    }
  };

  return (
    <div>
      <label
        className={`upload-area${dragging ? ' dragging' : ''}`}
        onDragOver={(e) => {
          e.preventDefault();
          setDragging(true);
        }}
        onDragLeave={() => setDragging(false)}
        onDrop={(e) => {
          e.preventDefault();
          setDragging(false);
          const file = e.dataTransfer.files[0];
          if (file?.type.startsWith('image/')) readFile(file);
        }}
      >
        <input
          type="file"
          accept="image/*"
          style={{ display: 'none' }}
          onChange={(e) => {
            const file = e.target.files?.[0];
            if (file) readFile(file);
          }}
        />
        <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="var(--gray-300)" strokeWidth="1.5">
          <rect x="3" y="3" width="18" height="18" rx="2" />
          <circle cx="8.5" cy="8.5" r="1.5" />
          <path d="M21 15l-5-5L5 21" />
        </svg>
        <p style={{ color: 'var(--gray-400)', fontSize: '0.85rem', marginTop: 8 }}>
          클릭하거나 이미지를 드래그하여 업로드
        </p>
        {imageData && <img src={imageData} className="upload-preview" alt="미리보기" />}
      </label>

      {imageData && (
        <div style={{ textAlign: 'center', marginTop: 12 }}>
          <button className="btn btn-success btn-lg" onClick={() => void upload()} disabled={uploading}>
            {uploading ? (
              <>
                <span className="spinner" /> AI가 분석 중...
              </>
            ) : (
              'AI로 분석하여 등록'
            )}
          </button>
        </div>
      )}

      {status && (
        <div className={status.ok ? 'msg' : 'msg msg-error'} style={status.ok ? { color: 'var(--success)' } : undefined}>
          {status.text}
        </div>
      )}
    </div>
  );
}
