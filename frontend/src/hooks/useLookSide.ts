import { useEffect, useRef, useState } from 'react';
import { api } from '../api/endpoints';
import { errorMessage } from '../api/client';
import type { SideStatus, SideView } from '../types/api';

/** 만드는 중일 때 상태를 다시 묻는 간격과 횟수(약 4분 — 서버 쪽 호출 상한보다 길다). */
const SIDE_POLL_MS = 3000;
const SIDE_POLL_MAX = 80;

const SIDE_MESSAGES: Partial<Record<SideStatus, string>> = {
  failed: '옆모습을 만들지 못했어요.',
  blocked: '이 사진은 옆모습을 만들 수 없어요.',
  budget: '오늘 사진 예산을 다 써서 지금은 만들 수 없어요.',
};

export type AngleView = 'original' | 'side';

export interface LookSide {
  view: AngleView;
  /** 지금 옆모습을 보여 줄 수 있으면 그 경로. 원본을 보는 중이면 null. */
  shownUrl: string | null;
  making: boolean;
  error: string | null;
  canRetry: boolean;
  ready: boolean;
  showOriginal: () => void;
  showSide: () => void;
}

/**
 * 저장된 룩 한 장의 원본/90도 옆모습 상태. 룩북 카드와 홈 상세가 같이 쓴다.
 *
 * 옆모습은 명시적으로 누를 때만 서버가 한 장 만든다(계정·원본 사진별로 저장돼 이미 있으면 다시
 * 만들지 않는다). 만드는 동안과 실패해도 원본은 그대로 보인다. 룩이 바뀌면 부르는 쪽이 `key` 로
 * 새로 마운트해야 한다 — 다른 코디의 상태가 섞이지 않게 상태를 룩 하나에만 묶는다.
 */
export function useLookSide(
  lookId: string,
  initial: { status: SideStatus | null; imagePath: string | null },
  initialView: AngleView = 'original',
): LookSide {
  const initialReady = Boolean(initial.imagePath);
  const [view, setView] = useState<AngleView>(initialReady ? initialView : 'original');
  const [side, setSide] = useState<SideView>({
    status: initialReady ? 'ready' : (initial.status ?? 'none'),
    image_path: initial.imagePath,
    retryable: true,
  });
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const settle = (r: SideView) => {
    setSide(r);
    if (r.status === 'ready' && r.image_path) {
      setError(null);
      setView('side');
    } else if (r.status !== 'pending') {
      setError(SIDE_MESSAGES[r.status] ?? null);
      setView('original');
    }
  };

  /** 다른 탭·다른 클릭이 만드는 중이면 끝날 때까지 상태만 본다. */
  const waitForSide = async () => {
    for (let i = 0; i < SIDE_POLL_MAX; i++) {
      await new Promise((r) => setTimeout(r, SIDE_POLL_MS));
      if (!alive.current) return;
      const r = await api.lookbook.side(lookId);
      if (!alive.current) return;
      if (r.status !== 'pending') {
        settle(r);
        return;
      }
    }
    if (alive.current) {
      setError('옆모습이 아직 준비되지 않았어요. 잠시 뒤 다시 눌러 주세요.');
      setSide((s) => ({ ...s, status: 'none' }));
      setView('original');
    }
  };

  const showSide = async () => {
    if (side.status === 'ready' && side.image_path) {
      setView('side');
      return;
    }
    if (side.status === 'pending' && view === 'side') return;
    setError(null);
    setView('side');
    setSide((s) => ({ ...s, status: 'pending' }));
    try {
      const r = await api.lookbook.makeSide(lookId);
      if (!alive.current) return;
      if (r.status === 'pending') await waitForSide();
      else settle(r);
    } catch (err) {
      if (!alive.current) return;
      setError(errorMessage(err));
      setSide((s) => ({ ...s, status: 'failed', retryable: true }));
      setView('original');
    }
  };

  const ready = side.status === 'ready' && Boolean(side.image_path);
  return {
    view,
    shownUrl: view === 'side' && ready ? side.image_path : null,
    making: view === 'side' && side.status === 'pending',
    error,
    canRetry: side.status === 'failed' && side.retryable,
    ready,
    showOriginal: () => setView('original'),
    showSide: () => void showSide(),
  };
}
