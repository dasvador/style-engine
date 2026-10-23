// 선택(투표) 계약. Rust `services::outfit_comparison` 의 Choice·ReasonCode·validate_vote 와 같은
// 값과 규칙을 쓴다 — 데모에서 쌓은 응답을 나중에 같은 모양으로 읽을 수 있게.

/** 선택지. `skip`(건너뛰기)은 선택이 아니다 — 아무것도 저장하지 않으므로 여기 없다. */
export const CHOICES = /** @type {const} */ (['a', 'b', 'neither']);

/** 이유 코드. 순서는 Rust enum 선언 순서(= 정렬 순서)와 같다. */
export const REASONS = /** @type {const} */ ([
  'color_combo',
  'silhouette',
  'personal_taste',
  'practicality',
  'weather_fit',
]);

export const REASON_LABELS = {
  color_combo: '색 조합',
  silhouette: '실루엣',
  personal_taste: '내 취향',
  practicality: '실용성/착용 편의',
  weather_fit: '날씨 적합성',
};

/** 메모 최대 글자 수(Rust `COMMENT_MAX_CHARS`, 표의 VARCHAR(200)). */
export const COMMENT_MAX_CHARS = 200;
/** 정리 전 원문 상한. 공백·제어 문자로 부풀린 큰 입력을 정리 전에 끊는다. */
export const COMMENT_RAW_MAX_CHARS = 1000;

export class InvalidVote extends Error {}

/** A·B 는 고른 쪽의 좋은 점, 둘 다 아쉬움은 두 코디의 아쉬운 점이다. */
export function reasonPolarity(choice) {
  return choice === 'neither' ? 'both_lacking' : 'chosen';
}

/**
 * 검증·정규화. 이유는 정렬·중복 제거, 메모는 줄바꿈 말고 제어 문자를 버리고 앞뒤 공백을 뗀다.
 * 빈 메모는 null. 스키마(zod)를 거친 값이 오지만, 저장소를 바로 부르는 경우도 같은 검사를 한다.
 */
export function validateVote({ choice, reasons = [], comment = null }) {
  if (!CHOICES.includes(choice)) throw new InvalidVote('choice 는 a, b, neither 중 하나여야 해요.');
  if (!Array.isArray(reasons) || reasons.length > REASONS.length * 2) {
    throw new InvalidVote('reasons 형식이 올바르지 않아요.');
  }
  for (const r of reasons) {
    if (!REASONS.includes(r)) throw new InvalidVote(`알 수 없는 이유 코드예요: ${String(r).slice(0, 40)}`);
  }
  const sorted = REASONS.filter((r) => reasons.includes(r));

  let cleanedComment = null;
  if (comment !== null && comment !== undefined) {
    if (typeof comment !== 'string') throw new InvalidVote('comment 는 글자여야 해요.');
    if ([...comment].length > COMMENT_RAW_MAX_CHARS) {
      throw new InvalidVote(`메모는 ${COMMENT_MAX_CHARS}자 이내로 적어 주세요.`);
    }
    // eslint-disable-next-line no-control-regex
    const trimmed = comment.replace(/[\u0000-\u0009\u000b-\u001f\u007f-\u009f]/g, '').trim();
    if ([...trimmed].length > COMMENT_MAX_CHARS) {
      throw new InvalidVote(`메모는 ${COMMENT_MAX_CHARS}자 이내로 적어 주세요.`);
    }
    cleanedComment = trimmed === '' ? null : trimmed;
  }
  return { choice, reasons: sorted, comment: cleanedComment };
}

export function sameVote(a, b) {
  return (
    a.choice === b.choice &&
    a.comment === b.comment &&
    a.reasons.length === b.reasons.length &&
    a.reasons.every((r, i) => r === b.reasons[i])
  );
}
