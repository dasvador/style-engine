import { useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api } from '../api/endpoints';
import { errorMessage } from '../api/client';
import type { Clothing, Thickness } from '../types/api';
import { ScreenHeader } from '../components/ScreenHeader';
import { EmptyState, ErrorMessage } from '../components/Status';
import { getRoleChipClass } from '../lib/lookbook';

/** `heather_gray` 처럼 저장된 색상값을 읽을 수 있게 다듬는다. 값 자체는 건드리지 않는다. */
function humanizeColor(color: string | null): string | null {
  return color ? color.replace(/_/g, ' ') : null;
}

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
      <ScreenHeader title="내 옷장" sub={`${items.length}벌`} />

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
          icon="Wardrobe"
          text={
            clothes.length === 0
              ? '아직 등록한 옷이 없습니다. 첫 아이템을 더하면 옷장 편집이 시작됩니다.'
              : '이 조건에 맞는 아이템이 없습니다. 위의 분류를 바꿔 보세요.'
          }
        />
      ) : (
        <div className="items-grid">
          {items.map((c) => (
            <button className="item-card" key={c.id} onClick={() => navigate(`/wardrobe/${c.id}`)}>
              {c.image_url?.startsWith('data:image/') ? (
                <img className="item-thumb" src={c.image_url} alt="" />
              ) : (
                /* 이미지가 없어도 자리를 지켜 그리드가 무너지지 않게 한다. */
                <div className="item-thumb-empty" aria-hidden="true">
                  <span>{c.category}</span>
                </div>
              )}
              <span className="item-card-name">{c.name}</span>
              {/* 태그를 늘어놓지 않는다. 역할만 칩으로, 나머지는 한 줄 메타로. */}
              <span className="item-card-meta">
                {[humanizeColor(c.color), c.tone].filter(Boolean).join(' · ') || c.category}
              </span>
              {c.role && (
                <span className="item-card-tags">
                  <span className={`chip ${getRoleChipClass(c.role)}`}>{c.role}</span>
                </span>
              )}
            </button>
          ))}
        </div>
      )}

      <AddPanel open={addPanelOpen} onClose={() => setAddPanelOpen(false)} onAdded={onReload} />

      <button className="fab" onClick={() => setAddPanelOpen(!addPanelOpen)}>
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
          <path d="M12 5v14M5 12h14" />
        </svg>
        새 아이템
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
  const [confirm, setConfirm] = useState<Clothing | null>(null);
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
      setImageData(null);
      // 바로 닫지 않는다. 분석이 제안한 장르를 사용자가 확인·수정한 뒤에 닫는다 —
      // 틀린 장르로 들어가면 그 장르의 추천이 통째로 어긋난다.
      setConfirm(created);
      await onAdded();
    } catch (err) {
      setStatus({ ok: false, text: `분석 실패: ${errorMessage(err)}` });
    } finally {
      setUploading(false);
    }
  };

  if (confirm) {
    return (
      <GenreConfirm
        item={confirm}
        onDone={async () => {
          setConfirm(null);
          onClose();
          await onAdded();
        }}
      />
    );
  }

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
        <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="var(--ink-faint)" strokeWidth="1.4">
          <rect x="3" y="3" width="18" height="18" rx="2" />
          <circle cx="8.5" cy="8.5" r="1.5" />
          <path d="M21 15l-5-5L5 21" />
        </svg>
        <p className="upload-hint">클릭하거나 이미지를 드래그해 사진을 올려 주세요</p>
        {imageData && <img src={imageData} className="upload-preview" alt="미리보기" />}
      </label>

      {imageData && (
        <div style={{ textAlign: 'center', marginTop: 12 }}>
          <button className="btn btn-success btn-lg" onClick={() => void upload()} disabled={uploading}>
            {uploading ? (
              <>
                <span className="spinner" /> 사진을 분석하는 중...
              </>
            ) : (
              '사진으로 등록하기'
            )}
          </button>
        </div>
      )}

      {status && (
        <div className={status.ok ? 'msg' : 'msg msg-error'} style={status.ok ? { color: 'var(--olive)' } : undefined}>
          {status.text}
        </div>
      )}
    </div>
  );
}


/** 화면에 보여줄 장르 목록. 값은 서버의 표준 식별자와 같아야 한다. */
const GENRE_CHOICES: { key: string; label: string }[] = [
  { key: 'minimal', label: '미니멀' },
  { key: 'classic', label: '클래식' },
  { key: 'romantic', label: '로맨틱' },
  { key: 'modern_chic', label: '모던 시크' },
  { key: 'bohemian', label: '보헤미안' },
  { key: 'street', label: '스트리트' },
  { key: 'mannish', label: '매니시' },
  { key: 'sporty_casual', label: '스포티 캐주얼' },
  { key: 'amekaji', label: '아메카지' },
  { key: 'preppy', label: '프레피' },
  { key: 'workwear', label: '워크웨어' },
  { key: 'outdoor_casual', label: '아웃도어 캐주얼' },
];

const GENDER_CHOICES: { key: string; label: string }[] = [
  { key: 'female', label: '여성' },
  { key: 'male', label: '남성' },
  { key: 'unisex', label: '공용' },
];

/**
 * 분석이 제안한 장르를 확인·수정하는 단계.
 *
 * 한 벌이 여러 장르에 들어갈 수 있어 복수 선택이다 — 옥스퍼드 셔츠는 클래식이면서
 * 아메카지이고 프레피다. 하나만 고르게 하면 나머지 장르에서 그 옷이 영영 후보에
 * 들어가지 않는다.
 */
function GenreConfirm({ item, onDone }: { item: Clothing; onDone: () => Promise<void> }) {
  const [genres, setGenres] = useState<string[]>(item.style_genres ?? []);
  const [gender, setGender] = useState<string>(item.gender ?? 'unisex');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const toggle = (key: string) =>
    setGenres((prev) => (prev.includes(key) ? prev.filter((g) => g !== key) : [...prev, key]));

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      await api.clothes.setGenres(item.id, { style_genres: genres, gender });
      await onDone();
    } catch (err) {
      setError(`저장 실패: ${errorMessage(err)}`);
      setSaving(false);
    }
  };

  return (
    <div>
      <p className="prose" style={{ marginBottom: 16 }}>
        <strong>{item.name}</strong> 을(를) 등록했습니다. 분석이 제안한 분류를 확인해 주세요.
      </p>

      <div className="pick-group">
        <span className="eyebrow">누구의 옷</span>
        <div className="gender-tabs" role="group" aria-label="성별 선택">
          {GENDER_CHOICES.map((g) => (
            <button
              key={g.key}
              className={`gender-btn${gender === g.key ? ' active' : ''}`}
              aria-pressed={gender === g.key}
              onClick={() => setGender(g.key)}
            >
              {g.label}
            </button>
          ))}
        </div>
      </div>

      <div className="pick-group">
        <span className="eyebrow">어울리는 장르 (여러 개 가능)</span>
        <div className="mood-chips" role="group" aria-label="장르 선택">
          {GENRE_CHOICES.map((g) => (
            <button
              key={g.key}
              className={`mood-chip-btn${genres.includes(g.key) ? ' active' : ''}`}
              aria-pressed={genres.includes(g.key)}
              onClick={() => toggle(g.key)}
            >
              {g.label}
            </button>
          ))}
        </div>
        <p className="mood-note">
          {genres.length === 0
            ? '하나도 고르지 않으면 옷의 속성으로 어울리는 장르를 추정합니다.'
            : `${genres.length}개 장르의 추천 후보에 들어갑니다.`}
        </p>
      </div>

      {error && <ErrorMessage>{error}</ErrorMessage>}

      <button
        className="btn btn-primary btn-lg"
        style={{ marginTop: 18 }}
        onClick={() => void save()}
        disabled={saving}
      >
        {saving ? '저장 중...' : '확인'}
      </button>
    </div>
  );
}
