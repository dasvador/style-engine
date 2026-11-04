import { useEffect, useState } from 'react';
import { api } from '../api/endpoints';

type State = { status: 'loading' } | { status: 'ready'; url: string } | { status: 'failed' };

/**
 * 보호 경로(`/api/images/{file}`)의 사진을 인증 헤더와 함께 읽어 화면에 쓸 주소로 돌려준다.
 * 화면을 떠나거나 파일이 바뀌면 요청을 끊고 blob 주소를 해제한다. 이미지를 새로 만들지 않는다.
 */
export function useProtectedImage(file: string | null): State {
  const [state, setState] = useState<State>({ status: 'loading' });

  useEffect(() => {
    if (!file) return;
    const abort = new AbortController();
    let url: string | null = null;
    setState({ status: 'loading' });
    void api.images
      .blob(file, abort.signal)
      .then((blob) => {
        if (abort.signal.aborted) return;
        url = URL.createObjectURL(blob);
        setState({ status: 'ready', url });
      })
      .catch(() => {
        if (!abort.signal.aborted) setState({ status: 'failed' });
      });
    return () => {
      abort.abort();
      if (url) URL.revokeObjectURL(url);
    };
  }, [file]);

  return state;
}
