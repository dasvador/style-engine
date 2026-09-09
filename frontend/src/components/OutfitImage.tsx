import { useOutfitImage } from '../hooks/useOutfitImage';

interface Props {
  items: readonly { slot: string; name: string; material?: string | null }[];
  mood: string | null;
}

/** 룩북/추천 카드의 생성 이미지 자리. 로딩·실패 상태를 자체적으로 표시한다. */
export function OutfitImage({ items, mood }: Props) {
  const state = useOutfitImage(items, mood);

  return (
    <div className="lookbook-img-wrap">
      {state.status === 'loading' && (
        <div style={{ color: '#b8b0a6', fontSize: '0.7rem', padding: '40px 0', letterSpacing: 1, textAlign: 'center' }}>
          GENERATING LOOK...
        </div>
      )}
      {state.status === 'failed' && (
        <div style={{ color: '#c4a882', fontSize: '0.7rem', padding: '20px 0', textAlign: 'center' }}>
          이미지를 불러올 수 없습니다
        </div>
      )}
      {state.status === 'ready' && (
        <img src={state.url} alt="추천 착장" style={{ width: '100%', borderRadius: 12, objectFit: 'contain' }} />
      )}
    </div>
  );
}
