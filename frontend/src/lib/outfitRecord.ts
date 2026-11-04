import { ApiError } from '../api/client';
import type { WearReaction, WearStatus } from '../types/api';

export const WEAR_STATUS_LABEL: Record<WearStatus, string> = {
  planned: '입을 계획',
  worn: '입음',
  cancelled: '취소',
};

export const REACTION_LABEL: Record<WearReaction, string> = {
  comfortable: '편했어요',
  uncomfortable: '불편했어요',
  too_cold: '추웠어요',
  too_hot: '더웠어요',
  liked: '마음에 들었어요',
  disliked: '아쉬웠어요',
};

/**
 * 쓰기가 서버에 반영됐는지 알 수 없는 실패인가. 연결이 응답 전에 끊겼거나(0), 서버 오류거나, 200 인데 본문을
 * 읽지 못한 경우다. 이때는 "저장 안 됨" 이라고 말하지 않고 서버 기록을 다시 읽어 확인한다.
 */
export function isUncertainWrite(err: unknown): boolean {
  return !(err instanceof ApiError) || err.status === 0 || err.status >= 500;
}

/**
 * 기록 API 실패를 사용자 문장으로. 서버의 `error` 는 한국어 문장이지만, 요청 모양 오류(422)와 서버 오류는
 * 기술적인 원문일 수 있어 고정 문장으로 바꾼다.
 */
export function outfitErrorMessage(err: unknown): string {
  if (isUncertainWrite(err)) return '응답을 받지 못해 저장됐는지 알 수 없어요. 기록 화면의 최신 내용을 확인해 주세요.';
  if (err instanceof ApiError && err.status !== 422) return err.message;
  return '요청을 처리하지 못했어요. 화면을 새로고침한 뒤 다시 시도해 주세요.';
}

/** 서울 기준 오늘(YYYY-MM-DD). 서버가 날짜를 정하지 않은 계획과 같은 기준이다. */
export function seoulToday(): string {
  return new Date().toLocaleDateString('en-CA', { timeZone: 'Asia/Seoul' });
}

/** 구성이 같은 코디인지 가르는 키. 옷 id 가 하나라도 없으면(예전 룩) 비교할 수 없다. */
export function outfitIdKey(ids: (string | undefined)[]): string | null {
  if (ids.length === 0 || ids.some((id) => !id)) return null;
  return [...ids].sort().join('|');
}
