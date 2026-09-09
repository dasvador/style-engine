import { useCallback, useEffect, useRef, useState } from 'react';
import { api } from '../api/endpoints';
import { errorMessage } from '../api/client';
import type { Clothing } from '../types/api';

/**
 * 옷장 목록.
 *
 * 홈(요약), 옷장, 평가(슬롯 선택), 상세가 모두 같은 목록을 본다. 화면마다 따로 읽으면
 * 등록·삭제 후 서로 어긋나므로 App 에서 한 번 읽어 내려보낸다.
 */
export function useClothes() {
  const [items, setItems] = useState<Clothing[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      const list = await api.clothes.list();
      if (alive.current) {
        setItems(list);
        setError(null);
      }
    } catch (err) {
      // 기존 UI 도 실패 시 빈 옷장으로 계속 진행했다.
      if (alive.current) {
        setItems([]);
        setError(errorMessage(err));
      }
    } finally {
      if (alive.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  return { items, loading, error, reload };
}
