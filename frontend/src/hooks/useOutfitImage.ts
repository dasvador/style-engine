import { useEffect, useRef, useState } from 'react';
import { api } from '../api/endpoints';
import { buildImagePrompt } from '../lib/lookbook';

type ImageState = { status: 'loading' } | { status: 'ready'; url: string } | { status: 'failed' };

/**
 * 착장 이미지 생성.
 *
 * 기존 UI 는 카드를 innerHTML 로 그린 뒤 setTimeout 으로 DOM 을 다시 찾아 이미지를 꽂았다.
 * 여기서는 컴포넌트가 자기 상태로 들고 있으므로 DOM 조회도 타이머도 필요 없다.
 * 생성은 수 초가 걸리고 실패해도 카드 나머지는 그대로 보여야 하므로 상태를 셋으로 나눈다.
 */
export function useOutfitImage(
  items: readonly { slot: string; name: string; material?: string | null }[],
  mood: string | null,
): ImageState {
  const [state, setState] = useState<ImageState>({ status: 'loading' });
  const alive = useRef(true);

  // 같은 착장으로 두 번 요청하지 않도록 프롬프트 문자열을 키로 쓴다.
  const prompt = buildImagePrompt(items);

  useEffect(() => {
    alive.current = true;
    setState({ status: 'loading' });

    if (!prompt) {
      setState({ status: 'failed' });
      return;
    }

    void (async () => {
      try {
        const r = await api.chat.image({ items: prompt, mood });
        if (!alive.current) return;
        setState(r.image_url ? { status: 'ready', url: r.image_url } : { status: 'failed' });
      } catch {
        if (alive.current) setState({ status: 'failed' });
      }
    })();

    return () => {
      alive.current = false;
    };
  }, [prompt, mood]);

  return state;
}
