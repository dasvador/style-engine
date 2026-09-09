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
      {/* 룩북은 남성 장르를 골라도 여성 모델로 그려진다. 이건 제약이 아니라 이
          기능의 의도라서, 결과만 내놓지 말고 무엇을 하고 있는지 한 줄로 밝힌다.
          (프롬프트 쪽 설명은 src/routes/chat.rs :: build_image_prompt) */}
      {state.status !== 'failed' && (
        <p
          style={{
            margin: '8px 2px 0',
            fontSize: '0.72rem',
            lineHeight: 1.5,
            color: 'var(--gray-500)',
          }}
        >
          남성복과 여성복의 경계를 두지 않고, 선택한 아이템을 여성 모델의 새로운 룩으로 구성합니다.
        </p>
      )}
    </div>
  );
}
