import { useEffect, useRef, useState } from 'react';
import { api } from '../api/endpoints';
import { buildImagePrompt } from '../lib/lookbook';

type ImageState = { status: 'loading' } | { status: 'ready'; url: string } | { status: 'failed' };

type ImageItems = readonly { slot: string; name: string; material?: string | null }[];

/**
 * 이번 세션에서 이미 받아 온 착장 이미지.
 *
 * 룩북은 카드를 넘겨 가며 이전 코디를 다시 본다. 캐시가 없으면 되돌아갈 때마다
 * 요청이 다시 나가고, 서버 캐시에 걸리더라도 왕복과 로딩 깜빡임이 남는다.
 * 키는 프롬프트 + 무드 — 서버가 `prompt_hash` 를 만드는 기준과 같다.
 */
const imageCache = new Map<string, string>();

/**
 * 생성 중인 요청.
 *
 * 카드를 쌓을 때 미리 받기가 돌고, 동시에 보고 있는 카드는 자기 훅으로도 요청한다.
 * 둘이 같은 착장을 노리면 서버 캐시에 아직 아무것도 없으므로 둘 다 통과해 이미지가
 * 두 번 생성된다 — 장당 비용이 그대로 두 배가 된다. 같은 키의 요청은 하나로 묶는다.
 */
const inflight = new Map<string, Promise<string | null>>();

/**
 * 캐시가 바뀐 것을 화면에 알리는 장치.
 *
 * 썸네일은 `cachedOutfitImage` 로 캐시를 읽기만 했다. 그런데 이미지가 도착해
 * 캐시에 들어가는 순간 다시 그려지는 것은 그 이미지를 띄운 카드 하나뿐이라,
 * 썸네일은 빈 채로 남았다 — 카드를 넘겨 부모가 다시 그려져야 그제야 나타났다.
 * 캐시를 구독 가능한 저장소로 만들어 들어오는 즉시 반영한다.
 */
const listeners = new Set<() => void>();
let cacheVersion = 0;

function publish(key: string, url: string) {
  imageCache.set(key, url);
  cacheVersion += 1;
  for (const fn of listeners) fn();
}

export function subscribeOutfitImages(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function outfitImageVersion(): number {
  return cacheVersion;
}

function cacheKey(items: ImageItems, mood: string | null): string {
  return `${mood ?? ''}::${buildImagePrompt(items)}`;
}

/** 캐시 → 진행 중인 요청 → 새 요청 순으로 하나의 이미지를 가져온다. */
function loadImage(key: string, prompt: string, mood: string | null): Promise<string | null> {
  const hit = imageCache.get(key);
  if (hit) return Promise.resolve(hit);

  const running = inflight.get(key);
  if (running) return running;

  const task = api.chat
    .image({ items: prompt, mood })
    .then((r) => {
      if (r.image_url) publish(key, r.image_url);
      return r.image_url ?? null;
    })
    .catch(() => null)
    .finally(() => {
      inflight.delete(key);
    });

  inflight.set(key, task);
  return task;
}

/**
 * 아직 보지 않은 코디의 이미지를 미리 받아 둔다.
 *
 * 룩북은 썸네일을 나란히 놓고 고르는 화면이다. 보고 있는 카드만 생성하면 넘겨
 * 보기 전까지 썸네일이 비어 있어서 비교 자체가 안 된다. 그래서 카드가 쌓일 때
 * 함께 받아 둔다.
 */
export async function prefetchOutfitImage(items: ImageItems, mood: string | null): Promise<void> {
  const prompt = buildImagePrompt(items);
  if (!prompt) return;
  await loadImage(cacheKey(items, mood), prompt, mood);
}

/** 썸네일처럼 요청을 새로 내지 않고 이미 받은 것만 쓰고 싶을 때. */
export function cachedOutfitImage(items: ImageItems, mood: string | null): string | null {
  return imageCache.get(cacheKey(items, mood)) ?? null;
}

/**
 * 착장 이미지 생성.
 *
 * 생성은 수십 초가 걸리고 실패해도 카드 나머지는 그대로 보여야 하므로 상태를 셋으로
 * 나눈다.
 */
export function useOutfitImage(items: ImageItems, mood: string | null): ImageState {
  const [state, setState] = useState<ImageState>({ status: 'loading' });
  const alive = useRef(true);

  const prompt = buildImagePrompt(items);
  const key = cacheKey(items, mood);

  useEffect(() => {
    alive.current = true;

    if (!prompt) {
      setState({ status: 'failed' });
      return;
    }

    // 이미 받아 둔 이미지면 깜빡임 없이 바로 보여준다.
    const hit = imageCache.get(key);
    if (hit) {
      setState({ status: 'ready', url: hit });
      return;
    }

    setState({ status: 'loading' });
    void loadImage(key, prompt, mood).then((url) => {
      if (!alive.current) return;
      setState(url ? { status: 'ready', url } : { status: 'failed' });
    });

    return () => {
      alive.current = false;
    };
  }, [key, prompt, mood]);

  return state;
}
