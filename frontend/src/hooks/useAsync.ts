import { useCallback, useEffect, useRef, useState } from 'react';
import { errorMessage } from '../api/client';

/** 로딩·오류·데이터 세 상태를 한 번에 다루기 위한 최소 hook. */
export interface AsyncState<T> {
  data: T | null;
  loading: boolean;
  error: string | null;
}

/**
 * 마운트 시 한 번 실행하고 `reload()` 로 다시 부를 수 있는 비동기 로더.
 *
 * 서버 상태 라이브러리를 쓰지 않는 이유: 지금 화면들은 각자 필요한 시점에 한 번 읽고,
 * 변경 후 명시적으로 다시 읽는 수준이라 캐시 무효화 문제가 없다.
 */
export function useAsync<T>(fn: () => Promise<T>, deps: readonly unknown[] = []): AsyncState<T> & {
  reload: () => Promise<void>;
  setData: (value: T) => void;
} {
  const [data, setData] = useState<T | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // 언마운트 후 setState 를 막는다.
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const fnRef = useRef(fn);
  fnRef.current = fn;

  const run = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const value = await fnRef.current();
      if (alive.current) setData(value);
    } catch (err) {
      if (alive.current) setError(errorMessage(err));
    } finally {
      if (alive.current) setLoading(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);

  useEffect(() => {
    void run();
  }, [run]);

  return { data, loading, error, reload: run, setData };
}
